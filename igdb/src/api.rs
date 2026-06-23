use const_format::formatcp;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

const IGDB_API_HOST: &str = "api.igdb.com";
const IGDB_GAME_API_URL: &str = formatcp!("https://{}/v4/games", IGDB_API_HOST);
const IGDB_COVER_API_URL: &str = formatcp!("https://{}/v4/covers", IGDB_API_HOST);
const TWITCH_ID_HOST: &str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);

#[derive(Serialize, Deserialize)]
struct ClientCredentialsToken {
    access_token: String,
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
