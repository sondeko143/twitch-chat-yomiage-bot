use axum::http::{HeaderMap, HeaderValue};
use const_format::formatcp;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

lazy_static! {
    /// プロセス全体で使い回す HTTP クライアント。呼び出しごとに新規生成すると
    /// DNS+TLS ハンドシェイクを払い直すため、コネクションプール/keep-alive を
    /// 共有して再利用する。
    static ref HTTP_CLIENT: reqwest::Client = build_http_client(TWITCH_API_HOST);
}

/// 共有 HTTP クライアントを組む。`retry_host` は張り直しを許すホスト。
///
/// プールに残った keep-alive 接続は、こちらが次に使う前にサーバ/経路側から
/// 破棄されうる（`show-chatters --interval` のように tick 間で接続が寝る使い方だと
/// 顕著）。死んだ接続へ書いた結果は RST（Windows なら os error 10054）か応答前の
/// EOF として返るが、hyper がリクエストを取り戻せるのは「送出前に落ちた」場合だけ
/// なので、この経路は自動で張り直されない。reqwest 既定のリトライも HTTP/2 の
/// GOAWAY/REFUSED_STREAM 専用で、`http2` feature 無しの本ビルドでは働かない。
fn build_http_client<H>(retry_host: H) -> reqwest::Client
where
    H: for<'a> PartialEq<&'a str> + Send + Sync + 'static,
{
    reqwest::Client::builder()
        .retry(
            reqwest::retry::for_host(retry_host)
                .max_retries_per_request(1)
                .classify_fn(|req_rep| {
                    // 応答が返らなかった GET だけを 1 回張り直す。GET は副作用が無く
                    // 2 度届いても害が無い一方、応答が無い以上呼び出し元は何も得て
                    // いない。POST（ban / eventsub 購読）は二重適用になるので対象外。
                    let retryable =
                        *req_rep.method() == reqwest::Method::GET && req_rep.status().is_none();
                    if retryable {
                        req_rep.retryable()
                    } else {
                        req_rep.success()
                    }
                }),
        )
        .build()
        .expect("HTTP クライアントの構築に失敗した")
}

const TWITCH_API_HOST: &str = "api.twitch.tv";
const TWITCH_USERS_API_URL: &str = formatcp!("https://{}/helix/users", TWITCH_API_HOST);
const TWITCH_BANS_API_URL: &str = formatcp!("https://{}/helix/moderation/bans", TWITCH_API_HOST);
const TWITCH_CHATTERS_API_URL: &str = formatcp!("https://{}/helix/chat/chatters", TWITCH_API_HOST);
const TWITCH_FOLLOWED_API_URL: &str =
    formatcp!("https://{}/helix/channels/followed", TWITCH_API_HOST);
const TWITCH_SUB_EVENT_API_URL: &str =
    formatcp!("https://{}/helix/eventsub/subscriptions", TWITCH_API_HOST);
const TWITCH_ID_HOST: &str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);
pub const TWITCH_OAUTH2_AUTHZ_URL: &str = formatcp!("https://{}/oauth2/authorize", TWITCH_ID_HOST);

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub data: Vec<UserData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserData {
    pub id: String,
    pub login: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub broadcaster_type: String,
    pub description: String,
    pub email: Option<String>,
    pub created_at: String,
}

pub async fn get_user(
    username: &str,
    access_token: &str,
    client_id: &str,
) -> Result<User, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let res: User = HTTP_CLIENT
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
    let res: RefreshToken = HTTP_CLIENT
        .post(TWITCH_OAUTH2_TOKEN_URL)
        .timeout(std::time::Duration::from_secs(30))
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
    let res: AccessToken = HTTP_CLIENT
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
    let res = HTTP_CLIENT
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
    let res: Chatters = HTTP_CLIENT
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
pub struct Pagination {
    pub cursor: Option<String>,
}
#[derive(Deserialize, Serialize)]
pub struct Followeds {
    pub data: Vec<Followed>,
    pub pagination: Pagination,
}

#[derive(Serialize, Deserialize)]
pub struct Followed {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub broadcaster_name: String,
}

pub async fn get_followed(
    user_id: &str,
    first: &i64,
    after: &str,
    access_token: &str,
    client_id: &str,
) -> Result<Followeds, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let first_s = first.to_string();
    let queries = match after.is_empty() {
        true => vec![("user_id", user_id), ("first", first_s.as_str())],
        false => vec![
            ("user_id", user_id),
            ("first", first_s.as_str()),
            ("after", after),
        ],
    };
    let res: Followeds = HTTP_CLIENT
        .get(TWITCH_FOLLOWED_API_URL)
        .headers(headers)
        .query(&queries)
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
    let res = HTTP_CLIENT
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

#[cfg(test)]
mod tests {
    use super::build_http_client;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const OK_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";

    /// リクエストのヘッダ終端まで読み進める。EOF に当たったら false。
    async fn read_request(conn: &mut TcpStream) -> bool {
        let mut seen: Vec<u8> = Vec::new();
        let mut byte = [0u8; 1];
        while conn.read_exact(&mut byte).await.is_ok() {
            seen.push(byte[0]);
            if seen.ends_with(b"\r\n\r\n") {
                return true;
            }
        }
        false
    }

    /// `kill_nth` 番目（0 始まり）のリクエストにだけ応答せず接続を切る HTTP サーバ。
    /// ピアが接続を捨てた状態を、接続の再利用有無に依らず再現する。
    async fn serve(listener: TcpListener, kill_nth: usize) {
        let seen = Arc::new(AtomicUsize::new(0));
        while let Ok((mut conn, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                while read_request(&mut conn).await {
                    if seen.fetch_add(1, Ordering::SeqCst) == kill_nth {
                        return;
                    }
                    if conn.write_all(OK_RESPONSE).await.is_err() {
                        return;
                    }
                }
            });
        }
    }

    /// テストがハングしないよう、全ての待ちに上限を付ける。
    async fn within<F: std::future::Future>(what: &str, fut: F) -> F::Output {
        match tokio::time::timeout(Duration::from_secs(5), fut).await {
            Ok(v) => v,
            Err(_) => panic!("{what} が 5 秒以内に決着しなかった"),
        }
    }

    #[tokio::test]
    async fn retries_a_get_whose_connection_the_peer_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _server = tokio::spawn(serve(listener, 1));
        let client = build_http_client(addr.ip().to_string());
        let url = format!("http://{addr}/");

        // ボディまで読み切らないと接続がプールへ戻らない。
        let first = within("1 回目の GET", client.get(&url).send())
            .await
            .expect("1 回目は成功するはず");
        assert!(first.status().is_success());
        within("1 回目のボディ読み", first.text()).await.unwrap();

        let second = within("2 回目の GET", client.get(&url).send()).await;

        assert!(
            second.is_ok(),
            "接続を切られた GET は張り直して成功するはず: {:?}",
            second.err()
        );
    }

    #[tokio::test]
    async fn does_not_retry_a_post_whose_connection_the_peer_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _server = tokio::spawn(serve(listener, 0));
        let client = build_http_client(addr.ip().to_string());

        let sent = within(
            "POST",
            client.post(format!("http://{addr}/")).body("{}").send(),
        )
        .await;

        assert!(
            sent.is_err(),
            "副作用のある POST は張り直さずエラーを返すはず"
        );
    }
}
