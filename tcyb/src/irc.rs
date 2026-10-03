use crate::feed::{ChatLine, FeedHub, Link, LinkStatus};
use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use thiserror::Error;
use tokio::process::Command;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::Instrument;
use url::Url;

const TRANSLATE_TIMEOUT_SECS: u64 = 10;
const PING_INTERVAL_SECS: u64 = 60;
const PING_SEND_TIMEOUT_SECS: u64 = 5;

#[derive(Error, Debug)]
pub enum ChatError {
    #[error("login failed")]
    LoginFailed,
    #[error("connection error")]
    MessageConnectionError,
    #[error(transparent)]
    ConnectionError(Box<tokio_tungstenite::tungstenite::Error>),
}

impl From<tokio_tungstenite::tungstenite::Error> for ChatError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::ConnectionError(Box::new(e))
    }
}

/// 受けたチャットの行き先（読み上げ・翻訳返信・配信口）。
#[derive(Clone)]
pub struct ChatOutputs {
    pub speech_address: String,
    pub operations: Vec<String>,
    pub translate_command: String,
    /// 配信口のハブ。`None` なら配信しない。
    pub hub: Option<FeedHub>,
}

pub async fn read_chat_client_loop(
    url: Url,
    access_token: String,
    username: String,
    channel: String,
    outputs: ChatOutputs,
    timeout_sec: u64,
) -> Result<(), ChatError> {
    // このループが終わる（abort を含む）と disconnected になる
    let status = outputs
        .hub
        .clone()
        .map(|hub| LinkStatus::connecting(hub, Link::Irc));
    // connected にするのはログイン完了の応答（001）を受けたとき（process_message）
    let mut ws_stream = connect_and_authorize(&url, &access_token, &username, &channel).await?;
    crate::profiling::mark_ready(crate::profiling::Component::Irc);
    let idle_timeout = std::time::Duration::from_secs(timeout_sec);
    let mut ping_interval =
        tokio::time::interval(std::time::Duration::from_secs(PING_INTERVAL_SECS));
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping_interval.tick().await;
    let mut last_received = tokio::time::Instant::now();
    loop {
        let elapsed = last_received.elapsed();
        if elapsed >= idle_timeout {
            warn!("irc idle timeout after {}s, reconnect", timeout_sec);
            return Ok(());
        }
        let remaining = idle_timeout - elapsed;
        tokio::select! {
            res = tokio::time::timeout(remaining, ws_stream.next()) => {
                match res {
                    Ok(Some(msg_res)) => {
                        last_received = tokio::time::Instant::now();
                        let msg = msg_res?;
                        if let Err(e) = process_message(
                            &mut ws_stream,
                            msg,
                            &outputs,
                            &username,
                            &channel,
                            status.as_ref(),
                        )
                        .await
                        {
                            match e {
                                MessageError::LoginFailed => {
                                    return Err(ChatError::LoginFailed);
                                }
                                MessageError::ConnectionError(_) => {
                                    return Err(ChatError::MessageConnectionError);
                                }
                                MessageError::VstcError(e) => {
                                    warn!("vstc error {}: ignore it.", e);
                                }
                            }
                        }
                    }
                    Ok(None) => return Ok(()),
                    Err(_) => {
                        warn!("irc idle timeout after {}s, reconnect", timeout_sec);
                        return Ok(());
                    }
                }
            }
            _ = ping_interval.tick() => {
                let send_fut = ws_stream.send(Message::Text("PING :tcyb".into()));
                match tokio::time::timeout(
                    std::time::Duration::from_secs(PING_SEND_TIMEOUT_SECS),
                    send_fut,
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        warn!("irc ping send failed: {}", e);
                        return Err(e.into());
                    }
                    Err(_) => {
                        warn!("irc ping send timed out, reconnect");
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn connect_and_authorize(
    url: &Url,
    access_token: &str,
    username: &str,
    channel: &str,
) -> Result<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, ChatError> {
    let (mut ws_stream, _) = connect_async(url.as_str())
        .instrument(tracing::info_span!("irc_connect"))
        .await?;
    info!("authorizing...");
    async {
        ws_stream
            .send(Message::Text(format!("PASS oauth:{}", access_token).into()))
            .await?;
        ws_stream
            .send(Message::Text(format!("NICK {}", username).into()))
            .await?;
        ws_stream
            .send(Message::Text(format!("JOIN #{}", channel).into()))
            .await?;
        ws_stream
            .send(Message::Text("CAP REQ :twitch.tv/tags".into()))
            .await?;
        Ok::<(), tokio_tungstenite::tungstenite::Error>(())
    }
    .instrument(tracing::info_span!("irc_auth"))
    .await?;
    Ok(ws_stream)
}

#[derive(Error, Debug)]
enum MessageError {
    #[error("login failed")]
    LoginFailed,
    #[error(transparent)]
    VstcError(#[from] vstc::VstcError),
    #[error(transparent)]
    ConnectionError(Box<tokio_tungstenite::tungstenite::Error>),
}

impl From<tokio_tungstenite::tungstenite::Error> for MessageError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::ConnectionError(Box::new(e))
    }
}

async fn process_message(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg: Message,
    outputs: &ChatOutputs,
    username: &str,
    channel: &str,
    status: Option<&LinkStatus>,
) -> Result<(), MessageError> {
    if msg.is_text() || msg.is_binary() {
        let msg_str = msg.into_text()?;
        let irc_message = parse_message(&msg_str);
        match irc_message.kind {
            IrcMessageKind::Chat => {
                // 読み上げより先に、待たずに配信する
                publish_chat(outputs.hub.as_ref(), &irc_message);
                let chat_msg = irc_message.chat_msg.unwrap_or_default();
                let user = irc_message.user.unwrap_or_default();
                if user != username {
                    info!(
                        "{:?} says {:?} in #{:?}",
                        user.as_str(),
                        chat_msg.as_str(),
                        irc_message.channel.unwrap_or_default().as_str(),
                    );
                    send_chat_message_to_speak(
                        chat_msg.as_str(),
                        &outputs.speech_address,
                        &outputs.operations,
                    )
                    .await?;
                    let msg_id = irc_message.msg_id.unwrap_or_default();
                    let (cleaned, emotes) =
                        split_message_emotes(&chat_msg, &irc_message.emote_ranges);
                    let emote_suffix = emotes.join(" ");

                    if cleaned.is_empty() {
                        // emote のみのメッセージ: 翻訳をスキップし emote だけ返信する。
                        if !emote_suffix.is_empty() {
                            send_reply(ws_stream, &msg_id, channel, &emote_suffix).await;
                        }
                        return Ok(());
                    }

                    let translate_fut = Command::new(&outputs.translate_command)
                        .args([cleaned.as_str()])
                        .kill_on_drop(true)
                        .output();
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(TRANSLATE_TIMEOUT_SECS),
                        translate_fut,
                    )
                    .await
                    {
                        Ok(Ok(output)) => {
                            let stdout = match std::str::from_utf8(&output.stdout) {
                                Ok(val) => val,
                                Err(err) => {
                                    warn!("{err}");
                                    ""
                                }
                            };
                            info!("{stdout}");
                            if let Some(body) = translated_reply_body(stdout, &emote_suffix) {
                                send_reply(ws_stream, &msg_id, channel, &body).await;
                            }
                        }
                        Ok(Err(err)) => {
                            warn!("{err}");
                        }
                        Err(_) => {
                            warn!(
                                "translate command timed out after {}s, killed child",
                                TRANSLATE_TIMEOUT_SECS
                            );
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }
            IrcMessageKind::LoginFailed => Err(MessageError::LoginFailed),
            IrcMessageKind::Welcome => {
                info!("{}", msg_str);
                if let Some(status) = status {
                    status.connected();
                }
                Ok(())
            }
            IrcMessageKind::Ping => {
                info!("respond to ping");
                ws_stream
                    .send(Message::Text("PONG :tmi.twitch.tv".into()))
                    .await?;
                Ok(())
            }
            _ => {
                info!("{}", msg_str);
                Ok(())
            }
        }
    } else {
        Ok(())
    }
}

/// 受けたチャットを配信口用の 1 行にする。チャット以外は `None`。
fn chat_line(message: &IrcMessage, received_at: chrono::DateTime<chrono::Utc>) -> Option<ChatLine> {
    let IrcMessageKind::Chat = message.kind else {
        return None;
    };
    let user_login = message.user.clone().unwrap_or_default();
    let display_name = match message.display_name.as_deref() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => user_login.clone(),
    };
    Some(ChatLine {
        received_at,
        user_login,
        display_name,
        text: message.chat_msg.clone().unwrap_or_default(),
        color: message.color.clone().filter(|c| !c.is_empty()),
    })
}

/// チャットをハブへ送る。ハブの送信は購読者の有無や遅れに関係なく待たない。
fn publish_chat(hub: Option<&FeedHub>, message: &IrcMessage) {
    if let Some(hub) = hub {
        if let Some(line) = chat_line(message, chrono::Utc::now()) {
            hub.send_chat(line);
        }
    }
}

async fn send_chat_message_to_speak(
    chat_msg: &str,
    uri: &str,
    operations: &[String],
) -> Result<(), vstc::VstcError> {
    vstc::process_command(uri, operations, chat_msg.to_string(), None, None, None).await?;
    Ok(())
}

async fn send_reply(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg_id: &str,
    channel: &str,
    body: &str,
) {
    let reply = format!("@reply-parent-msg-id={msg_id} PRIVMSG #{channel} :{body}");
    match ws_stream.send(Message::Text(reply.as_str().into())).await {
        Ok(_) => info!("{reply}"),
        Err(err) => warn!("{err}"),
    }
}

fn parse_emote_ranges(tag_value: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if tag_value.is_empty() {
        return ranges;
    }
    for emote in tag_value.split('/') {
        if let Some((_id, positions)) = emote.split_once(':') {
            for pos in positions.split(',') {
                if let Some((start, end)) = pos.split_once('-') {
                    if let (Ok(s), Ok(e)) = (start.parse::<usize>(), end.parse::<usize>()) {
                        ranges.push((s, e));
                    }
                }
            }
        }
    }
    ranges
}

fn split_message_emotes(chat_msg: &str, ranges: &[(usize, usize)]) -> (String, Vec<String>) {
    if ranges.is_empty() {
        return (chat_msg.to_string(), Vec::new());
    }
    let chars: Vec<char> = chat_msg.chars().collect();
    // Sort by start position so emotes are collected in text-appearance order
    // ("出現順"), independent of how ranges are grouped in the emotes tag
    // (Twitch groups ranges by emote id, not by text position).
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort_by_key(|&(start, _)| start);

    let mut emotes: Vec<String> = Vec::new();
    let mut covered = vec![false; chars.len()];
    for &(start, end) in &sorted {
        if start > end || end >= chars.len() {
            continue;
        }
        let emote: String = chars[start..=end].iter().collect();
        if !emotes.contains(&emote) {
            emotes.push(emote);
        }
        for slot in &mut covered[start..=end] {
            *slot = true;
        }
    }

    let cleaned_raw: String = chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if covered[i] { None } else { Some(*c) })
        .collect();
    let cleaned = cleaned_raw.split_whitespace().collect::<Vec<_>>().join(" ");

    (cleaned, emotes)
}

fn translated_reply_body(translated_stdout: &str, emote_suffix: &str) -> Option<String> {
    let translated = translated_stdout.trim();
    if translated.is_empty() {
        return None;
    }
    if emote_suffix.is_empty() {
        Some(translated.to_string())
    } else {
        Some(format!("{translated} {emote_suffix}"))
    }
}

#[derive(Default)]
enum IrcMessageKind {
    Chat,
    LoginFailed,
    /// ログイン完了の応答（数値応答 `001`）。
    Welcome,
    Ping,
    #[default]
    Unknown,
}

#[derive(Default)]
struct IrcMessage {
    kind: IrcMessageKind,
    msg_id: Option<String>,
    chat_msg: Option<String>,
    user: Option<String>,
    channel: Option<String>,
    emote_ranges: Vec<(usize, usize)>,
    display_name: Option<String>,
    color: Option<String>,
}

fn find_tag<'a>(tags: &'a str, name: &str) -> Option<&'a str> {
    tags.split(';').find_map(|tag| {
        let (key, value) = tag.split_once('=')?;
        (key == name).then_some(value)
    })
}

fn parse_message(msg_str: &str) -> IrcMessage {
    lazy_static! {
        static ref CHAT_MSG_PTN: Regex = Regex::new(
            r"^@(?P<tags>[^ ]+) :(?P<user>.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(?P<channel>[^:]+) :(?P<chat_msg>.+)"
        )
        .unwrap();
        static ref LOGIN_FAILED_PTN: Regex =
            Regex::new(r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*").unwrap();
        // 1 フレームに複数行が入るので、どの行の先頭でもよい
        static ref WELCOME_PTN: Regex = Regex::new(r"(?m)^:tmi\.twitch\.tv 001 ").unwrap();
        static ref PING_PTN: Regex = Regex::new(r"PING :tmi\.twitch\.tv").unwrap();
    }
    if CHAT_MSG_PTN.is_match(msg_str) {
        if let Some(caps) = CHAT_MSG_PTN.captures(msg_str) {
            let tags = &caps["tags"];
            let msg_id = find_tag(tags, "id").unwrap_or_default();
            let emote_ranges = parse_emote_ranges(find_tag(tags, "emotes").unwrap_or_default());
            return IrcMessage {
                kind: IrcMessageKind::Chat,
                msg_id: Some(msg_id.into()),
                chat_msg: Some(caps["chat_msg"].into()),
                channel: Some(caps["channel"].into()),
                user: Some(caps["user"].into()),
                emote_ranges,
                display_name: find_tag(tags, "display-name").map(Into::into),
                color: find_tag(tags, "color").map(Into::into),
            };
        }
    } else if LOGIN_FAILED_PTN.is_match(msg_str) {
        return IrcMessage {
            kind: IrcMessageKind::LoginFailed,
            ..Default::default()
        };
    } else if WELCOME_PTN.is_match(msg_str) {
        return IrcMessage {
            kind: IrcMessageKind::Welcome,
            ..Default::default()
        };
    } else if PING_PTN.is_match(msg_str) {
        return IrcMessage {
            kind: IrcMessageKind::Ping,
            ..Default::default()
        };
    }
    IrcMessage::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn translate_command_timeout_kills_long_running_child() {
        use std::time::Duration;
        let start = std::time::Instant::now();
        let fut = Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .kill_on_drop(true)
            .output();
        let res = tokio::time::timeout(Duration::from_secs(2), fut).await;
        let elapsed = start.elapsed();

        assert!(
            res.is_err(),
            "expected outer timeout to fire, but child returned"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "elapsed too long, kill_on_drop may not have worked: {:?}",
            elapsed
        );
    }

    #[test]
    fn find_tag_returns_value() {
        assert_eq!(find_tag("id=abc;mod=0", "id"), Some("abc"));
        assert_eq!(find_tag("id=abc;mod=0", "mod"), Some("0"));
    }

    #[test]
    fn find_tag_missing_returns_none() {
        assert_eq!(find_tag("id=abc;mod=0", "emotes"), None);
    }

    #[test]
    fn find_tag_empty_value() {
        assert_eq!(find_tag("emotes=;id=abc", "emotes"), Some(""));
    }

    #[test]
    fn parse_message_extracts_emote_ranges() {
        let message = parse_message(
            "@badge-info=;badges=;emotes=25:0-4,6-10;id=abc :u!u@u.tmi.twitch.tv PRIVMSG #chan :Kappa Kappa",
        );
        assert_eq!(message.emote_ranges, vec![(0, 4), (6, 10)]);
    }

    #[test]
    fn parse_message_empty_emotes_yields_no_ranges() {
        let message = parse_message(
            "@badge-info=;badges=;emotes=;id=abc :u!u@u.tmi.twitch.tv PRIVMSG #chan :hello",
        );
        assert!(message.emote_ranges.is_empty());
    }

    #[test]
    fn parse_smile_emoji_message() {
        let message = parse_message(
            "@badge-info=;badges=broadcaster/1;client-nonce=c047bc731be346ced547db43b626c763;color=#151538;display-name=解樹形図_祈;emotes=;first-msg=0;flags=;id=370397f6-fd48-4190-bdf2-c8547a048df8;mod=0;returning-chatter=0;room-id=173660453;subscriber=0;tmi-sent-ts=1716111351803;turbo=0;user-id=173660453;user-type= :testuser!somthing@something.tmi.twitch.tv PRIVMSG #somechannel :hello :)",
        );
        assert_eq!(message.chat_msg.unwrap().as_str(), "hello :)");
    }

    #[test]
    fn parse_message_extracts_display_name_and_color() {
        let message = parse_message(
            "@color=#151538;display-name=解樹形図_祈;emotes=;id=abc :testuser!u@u.tmi.twitch.tv PRIVMSG #chan :hello",
        );
        assert_eq!(message.display_name.as_deref(), Some("解樹形図_祈"));
        assert_eq!(message.color.as_deref(), Some("#151538"));
    }

    #[test]
    fn chat_line_uses_tags_for_display_name_and_color() {
        let at = chrono::DateTime::from_timestamp(10, 0).unwrap();
        let message = parse_message(
            "@color=#FF0000;display-name=Taro;id=abc :taro!u@u.tmi.twitch.tv PRIVMSG #chan :hi there",
        );
        assert_eq!(
            chat_line(&message, at),
            Some(ChatLine {
                received_at: at,
                user_login: "taro".into(),
                display_name: "Taro".into(),
                text: "hi there".into(),
                color: Some("#FF0000".into()),
            })
        );
    }

    #[test]
    fn chat_line_falls_back_to_login_and_no_color_when_tags_are_empty() {
        let at = chrono::DateTime::from_timestamp(10, 0).unwrap();
        let message =
            parse_message("@color=;display-name=;id=abc :taro!u@u.tmi.twitch.tv PRIVMSG #chan :hi");
        let line = chat_line(&message, at).unwrap();
        assert_eq!(line.display_name, "taro");
        assert_eq!(line.color, None);
    }

    #[test]
    fn parse_message_recognizes_login_welcome() {
        let message = parse_message(":tmi.twitch.tv 001 bot :Welcome, GLHF!");
        assert!(matches!(message.kind, IrcMessageKind::Welcome));
    }

    /// Twitch はログイン完了の応答（001〜004, 375, 372, 376）を 1 フレームにまとめて送る。
    #[test]
    fn parse_message_recognizes_welcome_inside_a_multi_line_frame() {
        let message = parse_message(
            ":tmi.twitch.tv 001 bot :Welcome, GLHF!\r\n:tmi.twitch.tv 002 bot :Your host is tmi.twitch.tv\r\n:tmi.twitch.tv 376 bot :>\r\n",
        );
        assert!(matches!(message.kind, IrcMessageKind::Welcome));
    }

    #[test]
    fn parse_message_does_not_take_other_numerics_or_failures_as_welcome() {
        for text in [
            ":tmi.twitch.tv 002 bot :Your host is tmi.twitch.tv",
            ":tmi.twitch.tv 376 bot :>",
            ":bot!bot@bot.tmi.twitch.tv JOIN #chan",
            "@id=x :u!u@u.tmi.twitch.tv PRIVMSG #chan :tmi.twitch.tv 001 bot",
        ] {
            assert!(
                !matches!(parse_message(text).kind, IrcMessageKind::Welcome),
                "{text}"
            );
        }
        assert!(matches!(
            parse_message(":tmi.twitch.tv NOTICE * :Login authentication failed").kind,
            IrcMessageKind::LoginFailed
        ));
    }

    #[test]
    fn chat_line_is_none_for_non_chat_messages() {
        let at = chrono::DateTime::from_timestamp(10, 0).unwrap();
        assert_eq!(chat_line(&parse_message("PING :tmi.twitch.tv"), at), None);
    }

    /// 購読者がいない・受信が止まっているハブへの送信が、チャット処理を待たせない。
    #[tokio::test]
    async fn publishing_chat_does_not_wait_for_absent_or_stalled_subscribers() {
        let message =
            parse_message("@display-name=Taro;id=abc :taro!u@u.tmi.twitch.tv PRIVMSG #chan :hi");
        let no_subscriber = FeedHub::new(5);
        let stalled = FeedHub::new(5);
        let (_, _never_read) = stalled.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for _ in 0..10_000 {
                publish_chat(Some(&no_subscriber), &message);
                publish_chat(Some(&stalled), &message);
            }
            publish_chat(None, &message);
        })
        .await
        .expect("publishing must not wait on subscribers");
        assert_eq!(stalled.subscribe().0.chats.len(), 5);
    }

    /// `hub` の IRC 状態が `want` になるまで待つ（上限 5 秒）。
    async fn wait_irc_state(hub: &FeedHub, want: crate::feed::ConnectionState) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while hub.subscribe().0.status.irc != want {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("IRC の状態が {want:?} にならない"));
    }

    /// ローカルの IRC 風 WebSocket サーバに繋ぎ、接続状態とチャットがハブに反映される。
    /// WebSocket が繋がっただけでは `connecting` のままで、ログイン完了の応答（001）を
    /// 受けて初めて `connected` になる。
    #[tokio::test]
    async fn read_chat_loop_reports_status_and_chat_to_hub() {
        use crate::feed::ConnectionState;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (authorized_tx, authorized_rx) = tokio::sync::oneshot::channel::<()>();
        let (welcome_tx, welcome_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            // PASS / NICK / JOIN / CAP を読み捨てる
            for _ in 0..4 {
                ws.next().await.unwrap().unwrap();
            }
            authorized_tx.send(()).unwrap();
            welcome_rx.await.unwrap();
            ws.send(Message::Text(
                ":tmi.twitch.tv 001 bot :Welcome, GLHF!\r\n:tmi.twitch.tv 002 bot :Your host is tmi.twitch.tv\r\n"
                    .into(),
            ))
            .await
            .unwrap();
            release_rx.await.unwrap();
            ws.send(Message::Text(
                "@color=#00FF00;display-name=Hanako;id=x :hanako!h@h.tmi.twitch.tv PRIVMSG #chan :yo"
                    .into(),
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
            // クライアント側が閉じるまで読む
            while let Some(Ok(_)) = ws.next().await {}
        });

        let hub = FeedHub::new(10);
        let client = tokio::spawn(read_chat_client_loop(
            Url::parse(&format!("ws://{addr}")).unwrap(),
            "token".into(),
            "bot".into(),
            "chan".into(),
            ChatOutputs {
                speech_address: "http://127.0.0.1:1".into(),
                operations: vec![],
                translate_command: "tcyb-nonexistent-translate-command".into(),
                hub: Some(hub.clone()),
            },
            30,
        ));

        authorized_rx.await.unwrap();
        // 認証の送信を終えても、応答が来るまでは接続済みとみなさない
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(hub.subscribe().0.status.irc, ConnectionState::Connecting);

        welcome_tx.send(()).unwrap();
        wait_irc_state(&hub, ConnectionState::Connected).await;

        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), client)
            .await
            .expect("loop ends when the server closes")
            .unwrap()
            .unwrap();
        server.await.unwrap();

        let (snap, _) = hub.subscribe();
        assert_eq!(snap.status.irc, ConnectionState::Disconnected);
        assert_eq!(snap.chats.len(), 1);
        assert_eq!(snap.chats[0].user_login, "hanako");
        assert_eq!(snap.chats[0].display_name, "Hanako");
        assert_eq!(snap.chats[0].text, "yo");
        assert_eq!(snap.chats[0].color.as_deref(), Some("#00FF00"));
    }

    #[test]
    fn parse_emote_ranges_empty() {
        assert_eq!(parse_emote_ranges(""), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn parse_emote_ranges_single() {
        assert_eq!(parse_emote_ranges("25:0-4"), vec![(0, 4)]);
    }

    #[test]
    fn parse_emote_ranges_same_emote_multiple() {
        assert_eq!(parse_emote_ranges("25:0-4,12-16"), vec![(0, 4), (12, 16)]);
    }

    #[test]
    fn parse_emote_ranges_multiple_emotes() {
        assert_eq!(
            parse_emote_ranges("25:0-4/1902:6-10"),
            vec![(0, 4), (6, 10)]
        );
    }

    #[test]
    fn split_message_emotes_no_ranges() {
        let (cleaned, emotes) = split_message_emotes("hello world", &[]);
        assert_eq!(cleaned, "hello world");
        assert!(emotes.is_empty());
    }

    #[test]
    fn split_message_emotes_trailing_emote() {
        let (cleaned, emotes) = split_message_emotes("Hello DinoDance", &[(6, 14)]);
        assert_eq!(cleaned, "Hello");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_middle_emote_collapses_space() {
        let (cleaned, emotes) = split_message_emotes("a Kappa b", &[(2, 6)]);
        assert_eq!(cleaned, "a b");
        assert_eq!(emotes, vec!["Kappa".to_string()]);
    }

    #[test]
    fn split_message_emotes_japanese_codepoint() {
        let (cleaned, emotes) = split_message_emotes("こんにちは DinoDance", &[(6, 14)]);
        assert_eq!(cleaned, "こんにちは");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_emote_only() {
        let (cleaned, emotes) = split_message_emotes("DinoDance", &[(0, 8)]);
        assert_eq!(cleaned, "");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_dedup_same_emote() {
        let (cleaned, emotes) = split_message_emotes("Kappa hi Kappa", &[(0, 4), (9, 13)]);
        assert_eq!(cleaned, "hi");
        assert_eq!(emotes, vec!["Kappa".to_string()]);
    }

    #[test]
    fn split_message_emotes_keeps_distinct_emotes() {
        let (cleaned, emotes) = split_message_emotes("hi Kappa PogChamp", &[(3, 7), (9, 16)]);
        assert_eq!(cleaned, "hi");
        assert_eq!(emotes, vec!["Kappa".to_string(), "PogChamp".to_string()]);
    }

    #[test]
    fn split_message_emotes_orders_by_text_position() {
        // Ranges passed unsorted (Kappa range first), but Kappa appears later in
        // the text than PogChamp. Output must follow text position, not input order.
        let (cleaned, emotes) = split_message_emotes("PogChamp hi Kappa", &[(12, 16), (0, 7)]);
        assert_eq!(cleaned, "hi");
        assert_eq!(emotes, vec!["PogChamp".to_string(), "Kappa".to_string()]);
    }

    #[test]
    fn translated_reply_body_empty_stdout_is_none() {
        assert_eq!(translated_reply_body("   \n", "DinoDance"), None);
    }

    #[test]
    fn translated_reply_body_no_emotes() {
        assert_eq!(
            translated_reply_body("hello\n", ""),
            Some("hello".to_string())
        );
    }

    #[test]
    fn translated_reply_body_appends_emotes() {
        assert_eq!(
            translated_reply_body("hello\n", "DinoDance"),
            Some("hello DinoDance".to_string())
        );
    }
}
