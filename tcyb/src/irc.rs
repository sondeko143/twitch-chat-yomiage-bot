use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

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
) -> Result<(), ChatError> {
    let mut ws_stream = connect_and_authorize(&url, &access_token, &username, &channel).await?;
    while let Ok(Some(msg)) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_sec),
        ws_stream.next(),
    )
    .await
    {
        let msg = msg?;
        if let Err(e) = process_message(&mut ws_stream, msg, &address, &operations).await {
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
    Ok(())
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
) -> Result<(), MessageError> {
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
                send_chat_message_to_speak(chat_msg.as_str(), address, operations).await?;
                Ok(())
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
    if CHAT_MSG_PTN.is_match(msg_str) {
        if let Some(caps) = CHAT_MSG_PTN.captures(msg_str) {
            return IrcMessage {
                kind: IrcMessageKind::Chat,
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
    #[test]
    fn parse_smile_emoji_message() {
        let message = parse_message(
            ":testuser!somthing@something.tmi.twitch.tv PRIVMSG #somechannel :hello :)",
        );
        assert_eq!(message.chat_msg.unwrap().as_str(), "hello :)");
    }
}
