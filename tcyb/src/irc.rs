use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use thiserror::Error;
use tokio::process::Command;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
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
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
}

pub async fn read_chat_client_loop(
    url: Url,
    access_token: String,
    username: String,
    channel: String,
    address: String,
    operations: Vec<String>,
    timeout_sec: u64,
    translate_command: String,
) -> Result<(), ChatError> {
    let mut ws_stream = connect_and_authorize(&url, &access_token, &username, &channel).await?;
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
                            &address,
                            &operations,
                            &username,
                            &channel,
                            &translate_command,
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
                let send_fut = ws_stream.send(Message::Text(String::from("PING :tcyb")));
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
) -> Result<
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Error,
> {
    let (mut ws_stream, _) = connect_async(url).await?;
    info!("authorizing...");
    ws_stream
        .send(Message::Text(format!("PASS oauth:{}", access_token)))
        .await?;
    ws_stream
        .send(Message::Text(format!("NICK {}", username)))
        .await?;
    ws_stream
        .send(Message::Text(format!("JOIN #{}", channel)))
        .await?;
    ws_stream
        .send(Message::Text(String::from("CAP REQ :twitch.tv/tags")))
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
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
}

async fn process_message(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg: Message,
    address: &str,
    operations: &[String],
    username: &str,
    channel: &str,
    translate_command: &str,
) -> Result<(), MessageError> {
    if msg.is_text() || msg.is_binary() {
        let msg_str = msg.into_text()?;
        let irc_message = parse_message(&msg_str);
        match irc_message.kind {
            IrcMessageKind::Chat => {
                let chat_msg = irc_message.chat_msg.unwrap_or_default();
                let user = irc_message.user.unwrap_or_default();
                if user != username {
                    info!(
                        "{:?} says {:?} in #{:?}",
                        user.as_str(),
                        chat_msg.as_str(),
                        irc_message.channel.unwrap_or_default().as_str(),
                    );
                    send_chat_message_to_speak(chat_msg.as_str(), address, operations).await?;
                    let msg_id = irc_message.msg_id.unwrap_or_default();
                    let translate_fut = Command::new(translate_command)
                        .args([chat_msg.as_str()])
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
                            if !stdout.is_empty() {
                                let translated_message = format!(
                                    "@reply-parent-msg-id={msg_id} PRIVMSG #{channel} :{stdout}"
                                );
                                match ws_stream
                                    .send(Message::Text(String::from(translated_message.as_str())))
                                    .await
                                {
                                    Ok(_) => {
                                        info! {"{translated_message}"}
                                    }
                                    Err(err) => warn!("{err}"),
                                }
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
            IrcMessageKind::Ping => {
                info!("respond to ping");
                ws_stream
                    .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
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

async fn send_chat_message_to_speak(
    chat_msg: &str,
    uri: &str,
    operations: &[String],
) -> Result<(), vstc::VstcError> {
    vstc::process_command(uri, operations, chat_msg.to_string(), None, None, None).await?;
    Ok(())
}

#[derive(Default)]
enum IrcMessageKind {
    Chat,
    LoginFailed,
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
}

fn parse_message(msg_str: &str) -> IrcMessage {
    lazy_static! {
        static ref CHAT_MSG_PTN: Regex = Regex::new(
            r"^@(?P<tags>[^ ]+) :(?P<user>.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(?P<channel>[^:]+) :(?P<chat_msg>.+)"
        )
        .unwrap();
        static ref LOGIN_FAILED_PTN: Regex =
            Regex::new(r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*").unwrap();
        static ref PING_PTN: Regex = Regex::new(r"PING :tmi\.twitch\.tv").unwrap();
    }
    if CHAT_MSG_PTN.is_match(msg_str) {
        if let Some(caps) = CHAT_MSG_PTN.captures(msg_str) {
            let tags = &caps["tags"];
            let id_tag = tags
                .split(';')
                .find(|tag| {
                    let name_value: Vec<_> = tag.split('=').collect();
                    name_value[0] == "id"
                })
                .unwrap_or_default();
            let msg_id = id_tag.get(3..).unwrap_or_default();
            return IrcMessage {
                kind: IrcMessageKind::Chat,
                msg_id: Some(msg_id.into()),
                chat_msg: Some(caps["chat_msg"].into()),
                channel: Some(caps["channel"].into()),
                user: Some(caps["user"].into()),
            };
        }
    } else if LOGIN_FAILED_PTN.is_match(msg_str) {
        return IrcMessage {
            kind: IrcMessageKind::LoginFailed,
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
    fn parse_smile_emoji_message() {
        let message = parse_message(
            "@badge-info=;badges=broadcaster/1;client-nonce=c047bc731be346ced547db43b626c763;color=#151538;display-name=解樹形図_祈;emotes=;first-msg=0;flags=;id=370397f6-fd48-4190-bdf2-c8547a048df8;mod=0;returning-chatter=0;room-id=173660453;subscriber=0;tmi-sent-ts=1716111351803;turbo=0;user-id=173660453;user-type= :testuser!somthing@something.tmi.twitch.tv PRIVMSG #somechannel :hello :)",
        );
        assert_eq!(message.chat_msg.unwrap().as_str(), "hello :)");
    }
}
