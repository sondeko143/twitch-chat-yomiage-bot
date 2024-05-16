use axum::http::{HeaderMap, HeaderValue};
use const_format::formatcp;
use serde::{Deserialize, Serialize};

const TWITCH_API_HOST: &str = "api.twitch.tv";
const IGDB_API_HOST: &str = "api.igdb.com";
const TWITCH_USERS_API_URL: &str = formatcp!("https://{}/helix/users", TWITCH_API_HOST);
const TWITCH_BANS_API_URL: &str = formatcp!("https://{}/helix/moderation/bans", TWITCH_API_HOST);
const TWITCH_CHATTERS_API_URL: &str = formatcp!("https://{}/helix/chat/chatters", TWITCH_API_HOST);
const TWITCH_SUB_EVENT_API_URL: &str =
    formatcp!("https://{}/helix/eventsub/subscriptions", TWITCH_API_HOST);
const TWITCH_ID_HOST: &str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);
pub const TWITCH_OAUTH2_AUTHZ_URL: &str = formatcp!("https://{}/oauth2/authorize", TWITCH_ID_HOST);
const IGDB_GAME_API_URL: &str = formatcp!("https://{}/v4/games", IGDB_API_HOST);
const IGDB_COVER_API_URL: &str = formatcp!("https://{}/v4/covers", IGDB_API_HOST);

#[derive(Serialize, Deserialize)]
pub struct User {
    pub data: Vec<UserData>,
}

#[derive(Serialize, Deserialize)]
pub struct UserData {
    pub id: String,
}

pub async fn get_user(
    username: &str,
    access_token: &str,
    client_id: &str,
) -> Result<User, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let res: User = reqwest::Client::new()
        .get(TWITCH_USERS_API_URL)
        .headers(headers)
        .query(&[("login", username)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

#[derive(Serialize, Deserialize)]
struct RefreshToken {
    access_token: String,
    refresh_token: String,
}

pub async fn get_tokens_by_refresh(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, String), reqwest::Error> {
    let res: RefreshToken = reqwest::Client::new()
        .post(TWITCH_OAUTH2_TOKEN_URL)
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("client_secret", client_secret),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok((res.access_token, res.refresh_token))
}

#[derive(Serialize, Deserialize)]
struct AccessToken {
    access_token: String,
    refresh_token: String,
}

pub async fn get_tokens_by_code(
    redirect_uri: &str,
    code: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, String), reqwest::Error> {
    let res: AccessToken = reqwest::Client::new()
        .post(TWITCH_OAUTH2_TOKEN_URL)
        .form(&[
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("client_secret", client_secret),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok((res.access_token, res.refresh_token))
}

#[derive(Serialize, Deserialize)]
struct ClientCredentialsToken {
    access_token: String,
    expires_in: i64,
    token_type: String,
}

pub async fn get_tokens_by_client_credentials(
    client_id: &str,
    client_secret: &str,
) -> Result<String, reqwest::Error> {
    let res: ClientCredentialsToken = reqwest::Client::new()
        .post(TWITCH_OAUTH2_TOKEN_URL)
        .query(&[
            ("client_id", client_id),
            ("grant_type", "client_credentials"),
            ("client_secret", client_secret),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(res.access_token)
}

#[derive(Deserialize, Serialize)]
struct Ban<'a> {
    #[serde(borrow)]
    data: BanData<'a>,
}

#[derive(Serialize, Deserialize)]
struct BanData<'a> {
    user_id: &'a str,
    reason: &'a str,
}

pub async fn ban_user(
    operator_id: &str,
    banned_id: &str,
    access_token: &str,
    client_id: &str,
) -> Result<String, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let ban = Ban {
        data: BanData {
            user_id: banned_id,
            reason: "bot",
        },
    };
    let res = reqwest::Client::new()
        .post(TWITCH_BANS_API_URL)
        .headers(headers)
        .query(&[
            ("broadcaster_id", operator_id),
            ("moderator_id", operator_id),
        ])
        .json(&ban)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(res)
}

#[derive(Deserialize, Serialize)]
pub struct Chatters {
    pub data: Vec<Chatter>,
}

#[derive(Serialize, Deserialize)]
pub struct Chatter {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
}

pub async fn get_chatters(
    broadcaster_id: &str,
    operator_id: &str,
    access_token: &str,
    client_id: &str,
) -> Result<Chatters, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let res: Chatters = reqwest::Client::new()
        .get(TWITCH_CHATTERS_API_URL)
        .headers(headers)
        .query(&[
            ("broadcaster_id", broadcaster_id),
            ("moderator_id", operator_id),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

#[derive(Deserialize, Serialize)]
struct EventSubSubscription<'a> {
    #[serde(rename = "type")]
    type_: &'a str,
    version: &'a str,
    #[serde(borrow)]
    condition: EventSubCondition<'a>,
    #[serde(borrow)]
    transport: EventSubTransport<'a>,
}

#[derive(Serialize, Deserialize)]
struct EventSubCondition<'a> {
    broadcaster_user_id: &'a str,
    moderator_user_id: &'a str,
}

#[derive(Serialize, Deserialize)]
struct EventSubTransport<'a> {
    method: &'a str,
    session_id: &'a str,
}

pub async fn sub_event(
    operator_id: &str,
    session_id: &str,
    access_token: &str,
    client_id: &str,
) -> Result<String, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let sub = EventSubSubscription {
        type_: "channel.follow",
        version: "2",
        condition: EventSubCondition {
            broadcaster_user_id: operator_id,
            moderator_user_id: operator_id,
        },
        transport: EventSubTransport {
            method: "websocket",
            session_id,
        },
    };
    let res = reqwest::Client::new()
        .post(TWITCH_SUB_EVENT_API_URL)
        .headers(headers)
        .json(&sub)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(res)
}

#[derive(Serialize, Deserialize)]
pub struct SearchGame {
    pub id: i64,
    pub cover: i64,
    pub name: String,
}

pub async fn search_game(
    access_token: &str,
    client_id: &str,
    name: &str,
) -> Result<Vec<SearchGame>, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let query = match name.parse::<i64>() {
        Ok(game_id) => {
            format!("fields id,name,cover; where id = {}; limit 1;", game_id)
        }
        Err(_) => {
            format!(
                "fields id,name,cover; search \"{}\"; where cover != null; limit 1;",
                name
            )
        }
    };
    let res: Vec<SearchGame> = reqwest::Client::new()
        .post(IGDB_GAME_API_URL)
        .headers(headers)
        .body(query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

#[derive(Serialize, Deserialize)]
pub struct Artwork {
    pub game: i64,
    pub image_id: String,
}

pub async fn get_cover(
    access_token: &str,
    client_id: &str,
    games: &[i64],
) -> Result<Vec<Artwork>, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let query = format!(
        "fields *; where game = ({});",
        games
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let res: Vec<Artwork> = reqwest::Client::new()
        .post(IGDB_COVER_API_URL)
        .headers(headers)
        .body(query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

fn auth_headers(access_token: &str, client_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.append(
        "Authorization",
        format!("Bearer {access_token}")
            .parse::<HeaderValue>()
            .unwrap(),
    );
    headers.append("Client-Id", client_id.parse::<HeaderValue>().unwrap());
    headers
}
