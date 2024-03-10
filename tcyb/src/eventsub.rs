use crate::api::sub_event;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use serde::Deserialize;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

#[derive(Error, Debug)]
pub enum EventSubError {
    #[error("connection error")]
    MessageConnectionError,
    #[error("session reconnect")]
    SessionReconnect { reconnect_url: String },
    #[error(transparent)]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
}

pub async fn sub_event_client_loop(
    url: Url,
    access_token: String,
    user_id: String,
    client_id: String,
    address: String,
    operations: Vec<String>,
    greeting_template: String,
    timeout_sec: u64,
) -> Result<(), EventSubError> {
    info!("connect event sub");
    let (mut ws_stream, _) = connect_async(url).await?;
    while let Ok(Some(msg)) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_sec),
        ws_stream.next(),
    )
    .await
    {
        let msg = msg?;
        if let Err(e) = process_message(
            &mut ws_stream,
            msg,
            &address,
            &operations,
            &user_id,
            &access_token,
            &client_id,
            &greeting_template,
        )
        .await
        {
            match e {
                MessageError::SessionReconnect { reconnect_url } => {
                    warn!("session reconnect {}: try to reconnect.", reconnect_url);
                    return Err(EventSubError::SessionReconnect { reconnect_url });
                }
                MessageError::ConnectionError(e) => {
                    warn!("connection error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::SerializeError(e) => {
                    warn!("msg serialization error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::RequestError(e) => {
                    warn!("msg request error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::VstcError(e) => {
                    warn!("vstc error {}: ignore it.", e);
                }
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct EventSubMessage {
    metadata: Metadata,
    payload: Payload,
}

#[derive(Deserialize)]
struct Metadata {
    message_type: String,
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Payload {
    session: Option<Session>,
    event: Option<Event>,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    reconnect_url: Option<String>,
}

#[derive(Deserialize)]
struct Event {
    user_name: String,
}

#[derive(Error, Debug)]
enum MessageError {
    #[error("session reconnect")]
    SessionReconnect { reconnect_url: String },
    #[error(transparent)]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
    #[error(transparent)]
    SerializeError(#[from] serde_json::Error),
    #[error(transparent)]
    RequestError(#[from] reqwest::Error),
    #[error(transparent)]
    VstcError(#[from] vstc::VstcError),
}

async fn process_message(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg: Message,
    address: &str,
    operations: &[String],
    user_id: &str,
    access_token: &str,
    client_id: &str,
    greeting_template: &str,
) -> Result<(), MessageError> {
    if msg.is_ping() {
        debug!("ping");
        let data = msg.into_data();
        let item = Message::Pong(data);
        ws_stream.send(item).await?;
        Ok(())
    } else if msg.is_text() || msg.is_binary() {
        let msg_str = msg.into_text()?;
        let event_msg: EventSubMessage = serde_json::from_str(&msg_str)?;
        match event_msg.metadata.message_type.as_str() {
            "session_welcome" => {
                let session_id = match event_msg.payload.session {
                    Some(s) => s.id,
                    None => String::from(""),
                };
                info!("session welcome {}", session_id);
                sub_event(user_id, session_id.as_str(), access_token, client_id).await?;
                Ok(())
            }
            "session_reconnect" => {
                let reconnect_url = match event_msg.payload.session {
                    Some(s) => s.reconnect_url.unwrap_or(String::from("")),
                    None => String::from(""),
                };
                info!("reconnect to {}", reconnect_url);
                Err(MessageError::SessionReconnect { reconnect_url })
            }
            "notification" => match event_msg.metadata.subscription_type {
                Some(s) => match s.as_str() {
                    "channel.follow" => {
                        let user_name = match event_msg.payload.event {
                            Some(e) => e.user_name,
                            None => String::from("Unknown user"),
                        };
                        info!("received follow notification {}", user_name);
                        send_greeting_message_to_speak(
                            user_name.as_str(),
                            address,
                            operations,
                            greeting_template,
                        )
                        .await?;
                        Ok(())
                    }
                    _ => {
                        info!("received {}", msg_str);
                        Ok(())
                    }
                },
                None => Ok(()),
            },
            _ => {
                debug!("received {}", msg_str);
                Ok(())
            }
        }
    } else {
        Ok(())
    }
}

async fn send_greeting_message_to_speak(
    user_name: &str,
    uri: &str,
    operations: &[String],
    text_template: &str,
) -> Result<(), vstc::VstcError> {
    let greeting = text_template.replace("user_name", user_name);
    vstc::process_command(uri, operations, greeting, None, None, None).await?;
    Ok(())
}
