use crate::api::get_tokens_by_refresh;
use crate::DBStore;
use futures_util::{SinkExt, StreamExt};
use jfs::Store;
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use std::path::Path;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

const CONNECT_ADDR: &str = "wss://irc-ws.chat.twitch.tv:443";
const IRC_TIMEOUT_SECS: u64 = 60 * 10;

#[allow(clippy::too_many_arguments)]
pub async fn read_chat(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    channel: &str,
    client_id: &str,
    client_secret: &str,
    operations: &[String],
    address: &str,
) -> anyhow::Result<()> {
    let url = url::Url::parse(CONNECT_ADDR)?;
    let db = Store::new(db_dir)?;
    loop {
        let obj = db.get::<DBStore>(db_name)?;
        let mut ws_stream =
            match connect_and_authorize(&url, &obj.access_token, username, channel).await {
                Ok(s) => s,
                Err(_) => continue,
            };
        while let Ok(Some(msg)) = tokio::time::timeout(
            std::time::Duration::from_secs(IRC_TIMEOUT_SECS),
            ws_stream.next(),
        )
        .await
        {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    warn!("{}", e);
                    break;
                }
            };
            if let Err(e) = process_message(msg, address, operations, &mut ws_stream).await {
                match e {
                    ChatError::LoginFailed => {
                        warn!("login failed: try to refresh token and reconnect.");
                        let (access_token, refresh_token) =
                            get_tokens_by_refresh(&obj.refresh_token, client_id, client_secret)
                                .await?;
                        let updated_obj = DBStore {
                            access_token,
                            refresh_token,
                            ..obj
                        };
                        db.save_with_id(&updated_obj, db_name)?;
                        break;
                    }
                    ChatError::ConnectionError(e) => {
                        warn!("connection error {}: try to reconnect.", e);
                        break;
                    }
                    ChatError::VstcError(e) => {
                        warn!("vstc error {}: ignore it.", e);
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
    Ok(ws_stream)
}

#[derive(Error, Debug)]
enum ChatError {
    #[error("login failed")]
    LoginFailed,
    #[error(transparent)]
    VstcError(#[from] vstc::VstcError),
    #[error(transparent)]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
}

async fn process_message(
    msg: Message,
    address: &str,
    operations: &[String],
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<(), ChatError> {
    if msg.is_text() || msg.is_binary() {
        let msg_str = msg.into_text()?;
        let irc_message = parse_message(&msg_str);
        match irc_message.kind {
            IrcMessageKind::Chat => {
                let chat_msg = irc_message.chat_msg.unwrap_or_default();
                info!(
                    "{:?} says {:?} in #{:?}",
                    irc_message.user.unwrap_or_default().as_str(),
                    chat_msg.as_str(),
                    irc_message.channel.unwrap_or_default().as_str(),
                );
                send_chat_message_to_read(chat_msg.as_str(), address, operations).await?;
                Ok(())
            }
            IrcMessageKind::LoginFailed => Err(ChatError::LoginFailed),
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

async fn send_chat_message_to_read(
    chat_msg: &str,
    uri: &str,
    operations: &[String],
) -> Result<(), vstc::VstcError> {
    vstc::process_command(uri, operations, chat_msg.to_string(), None, None, None).await?;
    Ok(())
}

enum IrcMessageKind {
    Chat,
    LoginFailed,
    Ping,
    Unknown,
}
impl Default for IrcMessageKind {
    fn default() -> Self {
        IrcMessageKind::Unknown
    }
}

#[derive(Default)]
struct IrcMessage {
    kind: IrcMessageKind,
    chat_msg: Option<String>,
    user: Option<String>,
    channel: Option<String>,
}

fn parse_message(msg_str: &str) -> IrcMessage {
    lazy_static! {
        static ref CHAT_MSG_PTN: Regex = Regex::new(
            r":(?P<user>.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(?P<channel>[^:]+) :(?P<chat_msg>.+)"
        )
        .unwrap();
        static ref LOGIN_FAILED_PTN: Regex =
            Regex::new(r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*").unwrap();
        static ref PING_PTN: Regex = Regex::new(r"PING :tmi\.twitch\.tv").unwrap();
    }
    if CHAT_MSG_PTN.is_match(&msg_str) {
        if let Some(caps) = CHAT_MSG_PTN.captures(&msg_str) {
            return IrcMessage {
                kind: IrcMessageKind::Chat,
                chat_msg: Some(caps["chat_msg"].into()),
                channel: Some(caps["channel"].into()),
                user: Some(caps["user"].into()),
            };
        }
    } else if LOGIN_FAILED_PTN.is_match(&msg_str) {
        return IrcMessage {
            kind: IrcMessageKind::LoginFailed,
            ..Default::default()
        };
    } else if PING_PTN.is_match(&msg_str) {
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
    #[test]
    fn parse_smile_emoji_message() {
        let message = parse_message(
            ":testuser!somthing@something.tmi.twitch.tv PRIVMSG #somechannel :hello :)",
        );
        assert_eq!(message.chat_msg.unwrap().as_str(), "hello :)");
    }
}
