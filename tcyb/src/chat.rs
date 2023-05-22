use crate::api::get_tokens_by_refresh;
use crate::DBStore;
use futures_util::{SinkExt, StreamExt};
use jfs::Store;
use log::{info, warn};
use regex::Regex;
use std::path::PathBuf;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;
use vstc;

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
    vstc::process_command(uri, operations, chat_msg.to_string(), None, None, None).await?;
    Ok(())
}
