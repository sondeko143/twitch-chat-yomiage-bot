use crate::api::{get_tokens_by_code, get_tokens_by_refresh};
use crate::DBStore;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
    Router,
};
use jfs::Store;
use log::{info, warn};
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
use tokio::task::JoinError;

pub async fn auth_code_grant(
    db_dir: &PathBuf,
    db_name: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), JoinError> {
    let server_t = tokio::spawn(start_server(ServerState {
        db_dir: db_dir.clone(),
        db_name: db_name.to_string(),
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    }));
    if webbrowser::open("http://localhost:8000").is_ok() {
        let t = server_t.await?;
        t.err();
    } else {
        server_t.abort();
    }
    Ok(())
}

pub async fn refresh_token_grant(
    db_dir: &PathBuf,
    db_name: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(db_name)?;
    let (access_token, refresh_token) =
        get_tokens_by_refresh(&obj.refresh_token, client_id, client_secret).await?;
    let updated_obj = DBStore {
        access_token: access_token,
        refresh_token: refresh_token,
        ..obj
    };
    db.save_with_id(&updated_obj, db_name)?;
    Ok(())
}

#[derive(Clone)]
struct ServerState {
    db_dir: PathBuf,
    db_name: String,
    client_id: String,
    client_secret: String,
}

async fn start_server(state: ServerState) -> Result<(), hyper::Error> {
    let app = Router::new()
        .route("/", get(auth))
        .route("/callback", get(callback))
        .with_state(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

async fn auth(State(state): State<ServerState>) -> impl IntoResponse {
    let state_id = uuid::Uuid::new_v4().to_string();
    let params: Vec<(&str, &str)> = vec![
        ("client_id", state.client_id.as_str()),
        ("redirect_uri", "http://localhost:8000/callback"),
        ("response_type", "code"),
        (
            "scope",
            "chat:read moderator:manage:banned_users channel:moderate",
        ),
        ("force_verify", "true"),
        ("state", state_id.as_str()),
    ];
    let queries = params
        .iter()
        .map(|p| format!("{}={}", p.0, p.1))
        .collect::<Vec<_>>()
        .join("&");

    return Redirect::to(format!("https://id.twitch.tv/oauth2/authorize?{}", queries).as_str())
        .into_response();
}

#[derive(Debug, Deserialize, Default)]
struct Callback {
    code: String,
}

async fn callback(code: Query<Callback>, State(state): State<ServerState>) -> impl IntoResponse {
    match obtain_access_token(
        &code.code,
        &state.client_id,
        &state.client_secret,
        &state.db_name,
        state.db_dir,
    )
    .await
    {
        Ok(_) => {
            info!("update tokens successfully");
            return StatusCode::OK.into_response();
        }
        Err(err) => {
            warn!("failed to update tokens: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
}

async fn obtain_access_token(
    code: &str,
    client_id: &str,
    client_secret: &str,
    db_name: &str,
    db_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let (access_token, refresh_token) = get_tokens_by_code(code, client_id, client_secret).await?;
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(&db_name)?;
    let updated_obj = DBStore {
        access_token: access_token,
        refresh_token: refresh_token,
        ..obj
    };
    db.save_with_id(&updated_obj, &db_name)?;
    Ok(())
}
