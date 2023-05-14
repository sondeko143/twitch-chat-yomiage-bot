use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use jfs::Store;
use log::{error, info, warn};
use regex::Regex;
use std::{fmt::Debug, path::PathBuf};

use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operation, OperationChain, OperationRoute,
};

use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    tungstenite::{Error, Result},
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    ReadChat {},
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
struct AppConfig {
    client_id: String,
    client_secret: String,
    channel: String,
    username: String,
    speech_port: i16,
    operations: Vec<String>,
    db_dir: PathBuf,
    db_name: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct DBStore {
    access_token: String,
    refresh_token: String,
    user_id: String,
}

#[derive(Serialize, Deserialize)]
struct RefreshToken {
    access_token: String,
    refresh_token: String,
}

async fn refresh_token(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, String), reqwest::Error> {
    let res: RefreshToken = reqwest::Client::new()
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("client_secret", client_secret),
        ])
        .send()
        .await?
        .json()
        .await?;

    Ok((res.access_token, res.refresh_token))
}

async fn send_chat_message_to_read(chat_msg: &str, uri: String) -> Result<()> {
    let mut channel = match CommanderClient::connect(uri).await {
        Ok(channel) => channel,
        Err(error) => {
            warn!("{:?}", error.to_string());
            return Ok(());
        }
    };
    let c = tonic::Request::new(Command {
        chains: vec![OperationChain {
            operations: vec![
                OperationRoute {
                    operation: Operation::Translate.into(),
                    remote: "".into(),
                },
                OperationRoute {
                    operation: Operation::Tts.into(),
                    remote: "".into(),
                },
                OperationRoute {
                    operation: Operation::Playback.into(),
                    remote: "".into(),
                },
            ],
        }],
        text: String::from(chat_msg),
        ..Command::default()
    });
    let _ = match channel.process_command(c).await {
        Ok(response) => response,
        Err(status) => {
            warn!("{:?}", status.message());
            return Ok(());
        }
    };
    Ok(())
}

async fn read_chat(
    db: &Store,
    db_name: &str,
    username: &str,
    channel: &str,
    client_id: &str,
    client_secret: &str,
    port: i16,
) -> Result<()> {
    let chat_msg_pat = Regex::new(
        r":(?P<user>.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(?P<channel>.+) :(?P<chat_msg>.+)",
    )
    .unwrap();
    let login_failed_pat =
        Regex::new(r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*").unwrap();
    let connect_addr = "wss://irc-ws.chat.twitch.tv:443";
    let url = url::Url::parse(connect_addr).unwrap();
    loop {
        let (mut ws_stream, _) = connect_async(&url).await.unwrap();
        let obj = db.get::<DBStore>(db_name).unwrap();
        info!("authorizing...");
        ws_stream
            .send(Message::Text(format!("PASS oauth:{}", obj.access_token)))
            .await?;
        ws_stream
            .send(Message::Text(format!("NICK {}", username)))
            .await?;
        ws_stream
            .send(Message::Text(format!("JOIN #{}", channel)))
            .await?;
        while let Some(msg) = ws_stream.next().await {
            let msg = msg?;
            if msg.is_text() || msg.is_binary() {
                let msg_str = msg.into_text().unwrap();
                if chat_msg_pat.is_match(&msg_str) {
                    let caps = chat_msg_pat.captures(&msg_str).unwrap();
                    info!(
                        "{:?} says {:?} in #{:?}",
                        &caps["user"], &caps["chat_msg"], &caps["channel"]
                    );
                    let dest = format!("http://localhost:{}", port);
                    send_chat_message_to_read(&caps["chat_msg"], dest).await?;
                } else if login_failed_pat.is_match(&msg_str) {
                    info!("expired.");
                    let (access_token, refresh_token) =
                        refresh_token(&obj.refresh_token, client_id, client_secret)
                            .await
                            .unwrap();
                    let updated_obj = DBStore {
                        access_token: access_token,
                        refresh_token: refresh_token,
                        ..obj.clone()
                    };
                    db.save_with_id(&updated_obj, db_name).unwrap();
                    break;
                } else {
                    info!("{}", msg_str);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    let config = config::Config::builder()
        .add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        )
        .build()
        .unwrap();
    let app_config: AppConfig = config.try_deserialize().unwrap();
    let args = Cli::parse();

    let db_cfg = jfs::Config {
        single: false,
        pretty: false,
        indent: 2,
    };
    let db = Store::new_with_cfg(app_config.db_dir, db_cfg).unwrap();

    match &args.command {
        Some(Commands::ReadChat {}) => {
            if let Err(e) = read_chat(
                &db,
                &app_config.db_name,
                &app_config.username,
                &app_config.channel,
                &app_config.client_id,
                &app_config.client_secret,
                app_config.speech_port,
            )
            .await
            {
                match e {
                    Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
                    err => error!("failed: {}", err),
                }
            }
        }
        None => {}
    }
}
