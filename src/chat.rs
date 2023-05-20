use crate::api::get_tokens_by_refresh;
use crate::DBStore;
use core::fmt;
use futures_util::{SinkExt, StreamExt};
use jfs::Store;
use log::{info, warn};
use regex::Regex;
use std::path::PathBuf;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;
use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operation, OperationChain, OperationRoute,
};

const CONNECT_ADDR: &str = "wss://irc-ws.chat.twitch.tv:443";
const IRC_TIMEOUT_SECS: u64 = 60 * 10;

pub async fn read_chat(
    db_dir: &PathBuf,
    db_name: &str,
    username: &str,
    channel: &str,
    client_id: &str,
    client_secret: &str,
    operations: Vec<String>,
    address: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let chat_msg_pat = Regex::new(
        r":(?P<user>.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(?P<channel>.+) :(?P<chat_msg>.+)",
    )
    .unwrap();
    let login_failed_pat =
        Regex::new(r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*").unwrap();
    let ping_pat = Regex::new(r"PING :tmi\.twitch\.tv").unwrap();
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
            if msg.is_text() || msg.is_binary() {
                let msg_str = msg.into_text()?;
                if chat_msg_pat.is_match(&msg_str) {
                    let caps = chat_msg_pat.captures(&msg_str).unwrap();
                    info!(
                        "{:?} says {:?} in #{:?}",
                        &caps["user"], &caps["chat_msg"], &caps["channel"]
                    );
                    match send_chat_message_to_read(&caps["chat_msg"], address, &operations).await {
                        Ok(_) => (),
                        Err(err) => warn!("{:?}", err),
                    }
                } else if login_failed_pat.is_match(&msg_str) {
                    info!("expired.");
                    let (access_token, refresh_token) =
                        get_tokens_by_refresh(&obj.refresh_token, client_id, client_secret).await?;
                    let updated_obj = DBStore {
                        access_token,
                        refresh_token,
                        ..obj
                    };
                    db.save_with_id(&updated_obj, db_name)?;
                    break;
                } else if ping_pat.is_match(&msg_str) {
                    info!("respond to ping");
                    match ws_stream
                        .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
                        .await
                    {
                        Ok(_) => (),
                        Err(err) => {
                            warn!("{}", err);
                            break;
                        }
                    }
                } else {
                    info!("{}", msg_str);
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

async fn send_chat_message_to_read(
    chat_msg: &str,
    uri: &str,
    operations: &Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dst = uri.to_string();
    let mut channel = CommanderClient::connect(dst).await?;
    let op_routes = operations
        .iter()
        .map(convert_to_operation)
        .filter(|o| o.is_ok())
        .map(|o| OperationRoute {
            operation: o.unwrap().into(),
            remote: "".into(),
        })
        .collect::<Vec<_>>();
    let c = tonic::Request::new(Command {
        chains: vec![OperationChain {
            operations: op_routes,
        }],
        text: String::from(chat_msg),
        ..Command::default()
    });
    channel.process_command(c).await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct ConvertError;
impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid operation string")
    }
}
impl std::error::Error for ConvertError {}

fn convert_to_operation(op_str: &String) -> Result<Operation, ConvertError> {
    return match op_str.as_str() {
        "translate" => Ok(Operation::Translate),
        "transl" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "playback" => Ok(Operation::Playback),
        "play" => Ok(Operation::Playback),
        "subtitle" => Ok(Operation::Subtitle),
        "sub" => Ok(Operation::Subtitle),
        _ => Err(ConvertError),
    };
}
