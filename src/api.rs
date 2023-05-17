use axum::http::{HeaderMap, HeaderValue};
use const_format::formatcp;
use serde::{Deserialize, Serialize};

const TWITCH_API_HOST: &'static str = "api.twitch.tv";
const TWITCH_USERS_API_URL: &'static str = formatcp!("https://{}/helix/users", TWITCH_API_HOST);
const TWITCH_BANS_API_URL: &'static str =
    formatcp!("https://{}/helix/moderation/bans", TWITCH_API_HOST);
const TWITCH_ID_HOST: &'static str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &'static str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);
pub const TWITCH_OAUTH2_AUTHZ_URL: &'static str =
    formatcp!("https://{}/oauth2/authorize", TWITCH_ID_HOST);

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

fn auth_headers(access_token: &str, client_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.append(
        "Authorization",
        format!("Bearer {access_token}")
            .parse::<HeaderValue>()
            .unwrap(),
    );
    headers.append("Client-Id", client_id.parse::<HeaderValue>().unwrap());
    return headers;
}
