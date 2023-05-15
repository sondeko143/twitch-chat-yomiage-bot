use axum::http::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

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
        .get("https://api.twitch.tv/helix/users")
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
        .post("https://id.twitch.tv/oauth2/token")
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
    code: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(String, String), reqwest::Error> {
    let res: AccessToken = reqwest::Client::new()
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("code", code),
            ("redirect_uri", "http://localhost:8000/callback"),
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
struct Ban {
    data: BanData,
}

#[derive(Serialize, Deserialize)]
struct BanData {
    user_id: String,
    reason: String,
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
            user_id: banned_id.to_string(),
            reason: "bot".to_string(),
        },
    };
    let res = reqwest::Client::new()
        .post("https://api.twitch.tv/helix/moderation/bans")
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
