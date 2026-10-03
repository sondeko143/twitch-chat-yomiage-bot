use crate::feed::{self, FeedHub};
use crate::{
    api,
    store::{SharedStore, Store},
};
use anyhow::bail;
use log::warn;
use std::{convert::Infallible, path::Path, time::Duration};

fn format_chatters_line(
    now: chrono::NaiveDateTime,
    chatters: &api::Chatters,
    channel_name: &str,
    username: &str,
) -> String {
    let mut users: Vec<&str> = chatters
        .data
        .iter()
        .map(|c| c.user_login.as_str())
        .filter(|name| *name != channel_name && *name != username)
        .collect();
    users.sort_unstable();
    format!("{},{}", now.format("%Y-%m-%d %H:%M:%S"), users.join(","))
}

async fn with_token_refresh<T, F, Fut>(
    store: &mut Store,
    client_id: &str,
    client_secret: &str,
    mut call: F,
) -> anyhow::Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let mut refreshed = false;
    loop {
        match call(store.access_token().to_string()).await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) && !refreshed {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                    refreshed = true;
                } else {
                    bail!(err);
                }
            }
        }
    }
}

/// `with_token_refresh` の共有ストア版。`read-chat` 内では yomiage の再接続処理と
/// トークンの持ち主が同じなので、ここで独自にストアを開き直さず mutex 越しに使う。
///
/// 401 でも、呼び出しに使ったトークンから既に差し替わっていれば（他方が更新済み）
/// 追加のリフレッシュはせず、そのトークンで 1 回だけ再試行する。リフレッシュは
/// 最大 1 回（ADR-0021）。
async fn with_shared_token_refresh<T, F, Fut>(
    store: &SharedStore,
    client_id: &str,
    client_secret: &str,
    mut call: F,
) -> anyhow::Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let mut refreshed = false;
    loop {
        let used = store.lock().await.access_token().to_string();
        match call(used.clone()).await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) && !refreshed {
                    warn!("refresh token: {}", err);
                    let mut guard = store.lock().await;
                    if guard.access_token() == used {
                        guard.update_tokens(client_id, client_secret).await?;
                    }
                    refreshed = true;
                } else {
                    bail!(err);
                }
            }
        }
    }
}

/// 視聴者一覧を `interval` ごとに取得してハブへ送る。終わらない（呼び出し側が abort する）。
///
/// 取得の成功・失敗はハブの status に反映する。失敗してもループは続き、次の周期で
/// 再試行する。トークンは `store` を yomiage と共有する。
pub async fn chatters_poll_loop(
    hub: FeedHub,
    store: SharedStore,
    channel_user_id: String,
    user_id: String,
    interval: Duration,
    client_id: String,
    client_secret: String,
) -> Infallible {
    let (client_id, channel_user_id, user_id) = (&client_id, &channel_user_id, &user_id);
    poll_chatters(&hub, interval, || async {
        let res =
            with_shared_token_refresh(&store, client_id, &client_secret, |token| async move {
                api::get_chatters(channel_user_id, user_id, &token, client_id).await
            })
            .await?;
        Ok(res
            .data
            .into_iter()
            .map(|c| feed::Chatter {
                login: c.user_login,
                display_name: c.user_name,
            })
            .collect())
    })
    .await
}

async fn poll_chatters<F, Fut>(hub: &FeedHub, interval: Duration, mut fetch: F) -> Infallible
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<feed::Chatter>>>,
{
    let mut ticker = tokio::time::interval(interval.max(Duration::from_millis(1)));
    // 取得が間隔より長引いても、取りこぼした分をまとめて発火しない。
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let now = chrono::Utc::now();
        match fetch().await {
            Ok(chatters) => {
                hub.send_chatters(feed::Chatters {
                    fetched_at: now,
                    chatters,
                });
                hub.update_status(|s| {
                    s.chatters_last_success = Some(now);
                    s.chatters_last_error = None;
                });
            }
            Err(err) => {
                warn!("chatters poll failed: {:#}", err);
                hub.update_status(|s| s.chatters_last_error = Some(format!("{err:#}")));
            }
        }
    }
}

/// 配信チャンネル ID を引くときの試行回数（初回を含む）。
const CHANNEL_ID_ATTEMPTS: u32 = 3;
/// 1 回目の再試行までの間隔。以降は倍にする。
const CHANNEL_ID_RETRY_INITIAL_BACKOFF: Duration = Duration::from_secs(2);

/// 一時的な失敗（通信エラー・5xx）なら間隔を空けて再試行する。4xx と応答の解釈失敗は
/// 再試行しない（401 のトークン更新は呼び出し側の `with_token_refresh` が担う）。
async fn retry_transient<T, F, Fut>(mut call: F) -> Result<T, reqwest::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let mut attempt = 1;
    let mut backoff = CHANNEL_ID_RETRY_INITIAL_BACKOFF;
    loop {
        match call().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < CHANNEL_ID_ATTEMPTS && is_transient(&err) => {
                warn!(
                    "transient error (attempt {}/{}): {}; retry in {}s",
                    attempt,
                    CHANNEL_ID_ATTEMPTS,
                    err,
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
                backoff = backoff.saturating_mul(2);
            }
            Err(err) => return Err(err),
        }
    }
}

/// 再試行に値する一時的な失敗か。
///
/// 想定する一時的なエラーの種別: connect（接続できない）、timeout（応答が時間内に
/// 来ない）、request（送出・経路の失敗）、body（本文の受信中の失敗）。5xx も一時的。
/// decode（応答の解釈失敗）と builder（要求の組み立て失敗）は、繰り返しても直らない。
fn is_transient(err: &reqwest::Error) -> bool {
    match err.status() {
        Some(status) => status.is_server_error(),
        None => !err.is_decode() && !err.is_builder(),
    }
}

/// Get Users を、一時的な失敗（タイムアウトを含む）は再試行しつつ呼ぶ。
/// 各試行は `api::get_user_from` のタイムアウトで打ち切られる。
async fn get_user_with_retry(
    url: &str,
    login: &str,
    access_token: &str,
    client_id: &str,
) -> Result<api::User, reqwest::Error> {
    retry_transient(|| {
        api::get_user_from(url, api::REQUEST_TIMEOUT, login, access_token, client_id)
    })
    .await
}

fn first_user_id(user: api::User) -> anyhow::Result<String> {
    match user.data.into_iter().next() {
        Some(user) => Ok(user.id),
        None => bail!("channel not found"),
    }
}

/// 配信チャンネルの ID。設定の `channel` と login が一致するキャッシュがあれば API を
/// 呼ばずにそれを使い、無ければ Helix で引いてストアへ保存する（ADR-0027）。
pub(crate) async fn resolve_channel_user_id(
    store: &mut Store,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    resolve_channel_user_id_from(
        api::TWITCH_USERS_API_URL,
        store,
        channel_name,
        client_id,
        client_secret,
    )
    .await
}

/// [`resolve_channel_user_id`] の Get Users の接続先を差し替えられる版（テスト用の継ぎ目）。
async fn resolve_channel_user_id_from(
    users_url: &str,
    store: &mut Store,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    if let Some(id) = store.cached_channel_id(channel_name) {
        return Ok(id.to_string());
    }
    let channel_user = with_token_refresh(store, client_id, client_secret, |token| async move {
        get_user_with_retry(users_url, channel_name, &token, client_id).await
    })
    .await?;
    let id = first_user_id(channel_user)?;
    store.save_channel_id(channel_name, &id)?;
    Ok(id)
}

/// [`resolve_channel_user_id`] の共有ストア版。Helix の呼び出し（再試行の待ちを含む）の
/// 間はストアのロックを持たない。
pub(crate) async fn resolve_shared_channel_user_id(
    store: &SharedStore,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    resolve_shared_channel_user_id_from(
        api::TWITCH_USERS_API_URL,
        store,
        channel_name,
        client_id,
        client_secret,
    )
    .await
}

/// [`resolve_shared_channel_user_id`] の Get Users の接続先を差し替えられる版（テスト用の継ぎ目）。
async fn resolve_shared_channel_user_id_from(
    users_url: &str,
    store: &SharedStore,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    let cached = store
        .lock()
        .await
        .cached_channel_id(channel_name)
        .map(str::to_string);
    if let Some(id) = cached {
        return Ok(id);
    }
    let channel_user =
        with_shared_token_refresh(store, client_id, client_secret, |token| async move {
            get_user_with_retry(users_url, channel_name, &token, client_id).await
        })
        .await?;
    let id = first_user_id(channel_user)?;
    store.lock().await.save_channel_id(channel_name, &id)?;
    Ok(id)
}

async fn chatters_tick(
    store: &mut Store,
    channel_user_id: &str,
    user_id: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let res = with_token_refresh(store, client_id, client_secret, |token| async move {
        api::get_chatters(channel_user_id, user_id, &token, client_id).await
    })
    .await?;
    let now = chrono::Local::now().naive_local();
    println!(
        "{}",
        format_chatters_line(now, &res, channel_name, username)
    );
    Ok(())
}

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
    interval: Option<Duration>,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let user_id = store.user_id(username, client_id).await?;
    let channel_user_id =
        resolve_channel_user_id(&mut store, channel_name, client_id, client_secret).await?;

    let Some(period) = interval else {
        return chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await;
    };

    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // このループは break しない: 唯一の脱出経路は `?` による Err 伝播で、正常終了は
    // 呼び出し元の select!（Ctrl+C アーム）が担う。break を足すと Ctrl+C 無しで
    // 常駐が「成功」する経路が生まれるので注意。
    loop {
        ticker.tick().await;
        chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await?;
    }
}

pub async fn show_user_info(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let channel_user =
        with_token_refresh(&mut store, client_id, client_secret, |token| async move {
            api::get_user(username, &token, client_id).await
        })
        .await?;
    if channel_user.data.is_empty() {
        bail!("channel not found");
    }
    println!("{:?}", channel_user);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        format_chatters_line, get_user_with_retry, poll_chatters, resolve_channel_user_id_from,
        resolve_shared_channel_user_id_from, retry_transient, with_shared_token_refresh,
    };
    use crate::api::{Chatter, Chatters};
    use crate::feed::{self, FeedHub};
    use crate::store::{SharedStore, Store};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn feed_chatter(login: &str) -> feed::Chatter {
        feed::Chatter {
            login: login.to_string(),
            display_name: login.to_string(),
        }
    }

    /// `fetch` の結果を順に返す。尽きたら Ok(空)。
    fn scripted(
        results: Vec<anyhow::Result<Vec<feed::Chatter>>>,
        calls: Arc<AtomicUsize>,
    ) -> impl FnMut() -> std::future::Ready<anyhow::Result<Vec<feed::Chatter>>> {
        let results = Arc::new(Mutex::new(results.into_iter()));
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(results.lock().unwrap().next().unwrap_or(Ok(Vec::new())))
        }
    }

    fn chatter(login: &str) -> Chatter {
        Chatter {
            user_id: format!("{login}-id"),
            user_login: login.to_string(),
            user_name: login.to_string(),
        }
    }

    fn at(h: u32, mi: u32, s: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 9)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    #[test]
    fn formats_timestamp_then_logins_sorted_ascending() {
        let chatters = Chatters {
            data: vec![chatter("zoe"), chatter("alice"), chatter("bob")],
        };

        let line = format_chatters_line(at(12, 34, 56), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 12:34:56,alice,bob,zoe");
    }

    #[test]
    fn excludes_channel_owner_and_bot_account() {
        let chatters = Chatters {
            data: vec![chatter("mychannel"), chatter("mybot"), chatter("viewer")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,viewer");
    }

    #[test]
    fn keeps_trailing_comma_when_every_chatter_is_excluded() {
        let chatters = Chatters {
            data: vec![chatter("mybot")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,");
    }

    #[tokio::test(start_paused = true)]
    async fn poll_success_sends_chatters_and_marks_last_success() {
        let hub = FeedHub::new(10);
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch = scripted(vec![Ok(vec![feed_chatter("a"), feed_chatter("b")])], calls);
        let task = tokio::spawn({
            let hub = hub.clone();
            async move { poll_chatters(&hub, Duration::from_secs(60), fetch).await }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        let snap = hub.subscribe().0;
        let logins: Vec<_> = snap
            .chatters
            .as_ref()
            .expect("chatters を送るはず")
            .chatters
            .iter()
            .map(|c| c.login.as_str())
            .collect();
        assert_eq!(logins, ["a", "b"]);
        assert!(snap.status.chatters_last_success.is_some());
        assert_eq!(snap.status.chatters_last_error, None);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn poll_failure_reports_reason_keeps_looping_and_recovers() {
        let hub = FeedHub::new(10);
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch = scripted(
            vec![Err(anyhow::anyhow!("boom")), Ok(vec![feed_chatter("a")])],
            Arc::clone(&calls),
        );
        let task = tokio::spawn({
            let hub = hub.clone();
            async move { poll_chatters(&hub, Duration::from_secs(60), fetch).await }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;
        let snap = hub.subscribe().0;
        assert_eq!(snap.status.chatters_last_error.as_deref(), Some("boom"));
        assert_eq!(snap.status.chatters_last_success, None);
        assert!(snap.chatters.is_none());
        assert!(!task.is_finished());

        tokio::time::sleep(Duration::from_secs(60)).await;
        let snap = hub.subscribe().0;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "次の周期で再試行するはず");
        assert!(snap.status.chatters_last_success.is_some());
        assert_eq!(snap.status.chatters_last_error, None);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn poll_does_not_burst_to_catch_up_after_a_slow_fetch() {
        let hub = FeedHub::new(10);
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch = {
            let calls = Arc::clone(&calls);
            move || {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n == 0 {
                        // 3 周期分以上かかる最初の取得。
                        tokio::time::sleep(Duration::from_millis(350)).await;
                    }
                    Ok(Vec::new())
                }
            }
        };
        let task = tokio::spawn({
            let hub = hub.clone();
            async move { poll_chatters(&hub, Duration::from_millis(100), fetch).await }
        });

        // 遅い取得は 0..350ms。直後の 350ms に 1 回、次は 450ms。
        tokio::time::sleep(Duration::from_millis(400)).await;

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "遅れた周期をまとめて発火してはいけない"
        );
        task.abort();
    }

    /// `/?t=<token>` が `old` のときだけ 401 を返す HTTP モック。
    async fn serve_401_for_old() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut conn, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut seen = Vec::new();
                    let mut byte = [0u8; 1];
                    while conn.read_exact(&mut byte).await.is_ok() {
                        seen.push(byte[0]);
                        if seen.ends_with(b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&seen).into_owned();
                            let res: &[u8] = if head.contains("t=old") {
                                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"
                            } else {
                                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                            };
                            if conn.write_all(res).await.is_err() {
                                return;
                            }
                            seen.clear();
                        }
                    }
                });
            }
        });
        format!("http://{addr}/")
    }

    fn shared_store(dir: &std::path::Path) -> SharedStore {
        crate::store::save_tokens(dir, "data.json", "old".into(), "refresh".into()).unwrap();
        Arc::new(tokio::sync::Mutex::new(
            Store::new(dir, "data.json").unwrap(),
        ))
    }

    #[tokio::test]
    async fn shared_refresh_reuses_a_token_the_other_owner_already_refreshed() {
        let url = serve_401_for_old().await;
        let dir = tempfile::tempdir().unwrap();
        let store = shared_store(dir.path());
        let used: Arc<Mutex<Vec<String>>> = Arc::default();

        let out = with_shared_token_refresh(&store, "cid", "secret", |token| {
            let (url, store, used) = (url.clone(), Arc::clone(&store), Arc::clone(&used));
            async move {
                used.lock().unwrap().push(token.clone());
                if used.lock().unwrap().len() == 1 {
                    // 取得の最中に read-chat 側がトークンを更新した状況。
                    store.lock().await.replace_access_token_for_test("new");
                }
                reqwest::get(format!("{url}?t={token}"))
                    .await?
                    .error_for_status()
                    .map(|_| ())
            }
        })
        .await;

        // Twitch へのリフレッシュ（ネットワーク）に行かず、更新済みトークンで成功する。
        out.expect("更新済みトークンで再試行して成功するはず");
        assert_eq!(*used.lock().unwrap(), ["old", "new"]);
    }

    /// `status` の HTTP 応答から作った reqwest のエラー。
    fn http_error(status: u16) -> reqwest::Error {
        reqwest::Response::from(
            axum::http::Response::builder()
                .status(status)
                .body(String::new())
                .unwrap(),
        )
        .error_for_status()
        .unwrap_err()
    }

    /// 呼ばれた回数を数えつつ、`results` を順に返す呼び出し。尽きたら Ok(0)。
    fn scripted_call(
        results: Vec<Result<u32, reqwest::Error>>,
        calls: Arc<AtomicUsize>,
    ) -> impl FnMut() -> std::future::Ready<Result<u32, reqwest::Error>> {
        let mut results = results.into_iter();
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(results.next().unwrap_or(Ok(0)))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn transient_server_errors_are_retried_with_spacing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let start = tokio::time::Instant::now();

        let got = retry_transient(scripted_call(
            vec![Err(http_error(503)), Err(http_error(500)), Ok(7)],
            Arc::clone(&calls),
        ))
        .await;

        assert_eq!(got.unwrap(), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(
            start.elapsed() >= Duration::from_secs(2 + 4),
            "間隔を空けて再試行するはず: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_three_attempts() {
        let calls = Arc::new(AtomicUsize::new(0));

        let got = retry_transient(scripted_call(
            vec![
                Err(http_error(502)),
                Err(http_error(502)),
                Err(http_error(502)),
                Ok(1),
            ],
            Arc::clone(&calls),
        ))
        .await;

        assert_eq!(got.unwrap_err().status().map(|s| s.as_u16()), Some(502));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn client_errors_are_not_retried() {
        for status in [400, 401, 403, 404] {
            let calls = Arc::new(AtomicUsize::new(0));
            let got = retry_transient(scripted_call(
                vec![Err(http_error(status)), Ok(1)],
                Arc::clone(&calls),
            ))
            .await;
            assert_eq!(got.unwrap_err().status().map(|s| s.as_u16()), Some(status));
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{status}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_bodies_are_not_retried() {
        let decode_error = reqwest::Response::from(
            axum::http::Response::builder()
                .status(200)
                .body("not json".to_string())
                .unwrap(),
        )
        .json::<serde_json::Value>()
        .await
        .unwrap_err();
        let calls = Arc::new(AtomicUsize::new(0));

        let got = retry_transient(scripted_call(
            vec![Err(decode_error), Ok(1)],
            Arc::clone(&calls),
        ))
        .await;

        assert!(got.unwrap_err().is_decode());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// 応答を返さずに接続を切るサーバ。通信エラー（応答なし）を起こす。
    async fn serve_hang_up() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut conn, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = conn.read(&mut buf).await;
                drop(conn);
            }
        });
        format!("http://{addr}/")
    }

    #[tokio::test(start_paused = true)]
    async fn network_errors_are_retried() {
        let url = serve_hang_up().await;
        let calls = Arc::new(AtomicUsize::new(0));

        let got = retry_transient(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            let url = url.clone();
            async move { reqwest::get(url).await.map(|_| ()) }
        })
        .await;

        let err = got.unwrap_err();
        assert_eq!(err.status(), None, "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// 受け付けるだけで応答しないサーバ。受け付けた接続数を数える。
    async fn serve_never_respond() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        tokio::spawn({
            let accepted = Arc::clone(&accepted);
            async move {
                let mut held = Vec::new();
                while let Ok((conn, _)) = listener.accept().await {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    held.push(conn);
                }
            }
        });
        (format!("http://{addr}/"), accepted)
    }

    #[tokio::test(start_paused = true)]
    async fn get_user_timeouts_are_retried_within_three_attempts() {
        let (url, accepted) = serve_never_respond().await;

        let got = get_user_with_retry(&url, "login", "tok", "cid").await;

        let err = got.unwrap_err();
        assert!(err.is_timeout(), "{err:?}");
        assert_eq!(accepted.load(Ordering::SeqCst), 3);
    }

    /// キャッシュがあれば Helix を呼ばない。回帰してもループバックの閉じたポートへ向かう
    /// だけで、外部へは出ない（再試行の待ちは仮想時間で即座に進む）。
    #[tokio::test(start_paused = true)]
    async fn cached_channel_id_is_used_without_calling_the_api() {
        // 何も待ち受けていないループバックのポート。到達すれば接続拒否になる。
        let unreachable = "http://127.0.0.1:1/";
        let dir = tempfile::tempdir().unwrap();
        let store = shared_store(dir.path());
        store.lock().await.save_channel_id("mychan", "C1").unwrap();

        let shared =
            resolve_shared_channel_user_id_from(unreachable, &store, "MyChan", "cid", "secret")
                .await
                .unwrap();
        let owned = resolve_channel_user_id_from(
            unreachable,
            &mut *store.lock().await,
            "mychan",
            "cid",
            "secret",
        )
        .await
        .unwrap();

        assert_eq!(shared, "C1");
        assert_eq!(owned, "C1");
    }

    #[tokio::test]
    async fn shared_refresh_gives_up_after_a_single_retry() {
        let url = serve_401_for_old().await;
        let dir = tempfile::tempdir().unwrap();
        let store = shared_store(dir.path());
        let calls = Arc::new(AtomicUsize::new(0));

        let out = with_shared_token_refresh(&store, "cid", "secret", |_token| {
            let (url, store, calls) = (url.clone(), Arc::clone(&store), Arc::clone(&calls));
            async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    store
                        .lock()
                        .await
                        .replace_access_token_for_test("still-bad");
                }
                // 常に 401 になる要求。
                reqwest::get(format!("{url}?t=old"))
                    .await?
                    .error_for_status()
                    .map(|_| ())
            }
        })
        .await;

        assert!(out.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "再試行は 1 回だけ");
    }
}
