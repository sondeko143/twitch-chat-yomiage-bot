use crate::api::{get_tokens_by_code, get_tokens_by_refresh, TWITCH_OAUTH2_AUTHZ_URL};
use crate::store::DBStore;
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
use std::borrow::Cow;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::{net::SocketAddr, path::PathBuf};

pub async fn auth_code_grant(
    listen_addr: &str,
    db_dir: &Path,
    db_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let server_t = tokio::spawn(start_server(
        listen_addr.to_socket_addrs()?.next().unwrap(),
        ServerState {
            db_dir: db_dir.to_path_buf(),
            db_name: db_name.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            listen_addr: listen_addr.to_string(),
        },
    ));
    if webbrowser::open(&format!("http://{}", listen_addr)).is_ok() {
        let t = server_t.await?;
        t.err();
    } else {
        server_t.abort();
    }
    Ok(())
}

pub async fn refresh_token_grant(
    db_dir: &Path,
    db_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(db_name)?;
    let (access_token, refresh_token) =
        get_tokens_by_refresh(&obj.refresh_token, client_id, client_secret).await?;
    let updated_obj = DBStore {
        access_token,
        refresh_token,
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
    listen_addr: String,
}

async fn start_server(addr: SocketAddr, state: ServerState) -> Result<(), hyper::Error> {
    let app = Router::new()
        .route("/", get(auth))
        .route("/callback", get(callback))
        .with_state(state);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

async fn auth(State(state): State<ServerState>) -> impl IntoResponse {
    let state_id = &uuid::Uuid::new_v4().to_string();
    let redirect_uri = &format!("http://{}/callback", state.listen_addr);
    let params: Vec<(&str, &str)> = vec![
        ("client_id", &state.client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        (
            "scope",
            "chat:read chat:edit moderator:manage:banned_users channel:moderate moderator:read:chatters moderator:read:followers",
        ),
        ("force_verify", "true"),
        ("state", state_id),
    ];
    let queries = params
        .iter()
        .map(|p| format!("{}={}", p.0, p.1))
        .collect::<Vec<_>>()
        .join("&");

    return Redirect::to(format!("{}?{}", TWITCH_OAUTH2_AUTHZ_URL, queries).as_str())
        .into_response();
}

#[derive(Debug, Deserialize, Default)]
struct Callback {
    code: String,
}

async fn callback(code: Query<Callback>, State(state): State<ServerState>) -> impl IntoResponse {
    match obtain_access_token(
        &format!("http://{}/callback", state.listen_addr),
        &code.code,
        &state.client_id,
        &state.client_secret,
        &state.db_name,
        state.db_dir,
    )
    .await
    {
        Ok(_) => {
            let msg = "tokens updated successfully";
            info!("{}", msg);
            (StatusCode::OK, Cow::from(msg))
        }
        Err(err) => {
            let msg = format!("failed to update tokens: {}", err);
            warn!("{}", msg);
            (StatusCode::INTERNAL_SERVER_ERROR, Cow::from(msg))
        }
    }
}

async fn obtain_access_token(
    redirect_uri: &str,
    code: &str,
    client_id: &str,
    client_secret: &str,
    db_name: &str,
    db_dir: PathBuf,
) -> anyhow::Result<()> {
    let (access_token, refresh_token) =
        get_tokens_by_code(redirect_uri, code, client_id, client_secret).await?;
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(db_name)?;
    let updated_obj = DBStore {
        access_token,
        refresh_token,
        ..obj
    };
    db.save_with_id(&updated_obj, db_name)?;
    Ok(())
}
