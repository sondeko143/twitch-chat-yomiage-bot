//! 配信メッセージのハブ。直近の chat / notification をリングバッファで保持し、
//! 新着を購読者へ broadcast する。

use super::message::{ChatLine, Chatters, FeedMessage, NotificationLine, Snapshot, Status};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::broadcast;

/// 購読者 1 人あたりの未読の上限の下限。保持件数が小さくても短いバーストは取りこぼさない。
const MIN_CHANNEL_CAPACITY: usize = 64;

/// 送信者と購読者が共有するハブ。`Clone` は同じハブへのハンドルを複製する。
///
/// 送信操作は購読者の有無や受信の遅れに関係なく、待たずに戻る。
#[derive(Clone)]
pub struct FeedHub {
    inner: Arc<Inner>,
}

struct Inner {
    history_size: usize,
    state: Mutex<State>,
    tx: broadcast::Sender<FeedMessage>,
}

#[derive(Default)]
struct State {
    chats: VecDeque<ChatLine>,
    notifications: VecDeque<NotificationLine>,
    chatters: Option<Chatters>,
    status: Status,
}

/// 購読者の受信口。
pub struct FeedReceiver {
    hub: FeedHub,
    rx: broadcast::Receiver<FeedMessage>,
}

/// 受信口から新着を受け取れなかった理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedRecvError {
    /// 受信が遅れて、この件数を取りこぼした。[`FeedReceiver::resync`] で取り直す。
    Lagged(u64),
}

impl FeedHub {
    /// chat / notification をそれぞれ `history_size` 件まで保持するハブを作る。
    pub fn new(history_size: usize) -> Self {
        let (tx, _) = broadcast::channel(history_size.max(MIN_CHANNEL_CAPACITY));
        Self {
            inner: Arc::new(Inner {
                history_size,
                state: Mutex::new(State::default()),
                tx,
            }),
        }
    }

    pub fn send_chat(&self, chat: ChatLine) {
        let mut state = self.lock();
        push_bounded(&mut state.chats, chat.clone(), self.inner.history_size);
        self.broadcast(FeedMessage::Chat(chat));
    }

    pub fn send_notification(&self, notification: NotificationLine) {
        let mut state = self.lock();
        push_bounded(
            &mut state.notifications,
            notification.clone(),
            self.inner.history_size,
        );
        self.broadcast(FeedMessage::Notification(notification));
    }

    pub fn send_chatters(&self, chatters: Chatters) {
        let mut state = self.lock();
        state.chatters = Some(chatters.clone());
        self.broadcast(FeedMessage::Chatters(chatters));
    }

    pub fn send_status(&self, status: Status) {
        self.update_status(|s| *s = status);
    }

    /// 現在の status を書き換えて送る。複数のループがそれぞれの欄だけを更新するとき用。
    pub fn update_status(&self, f: impl FnOnce(&mut Status)) {
        let mut state = self.lock();
        f(&mut state.status);
        self.broadcast(FeedMessage::Status(state.status.clone()));
    }

    /// 購読を始める。返すスナップショットと受信口の間で、並行する送信は欠けも重複もしない。
    pub fn subscribe(&self) -> (Snapshot, FeedReceiver) {
        // 送信は保持の更新と broadcast を同じロックの中で行う。ここでも同じロックの中で
        // スナップショットと受信口を作るので、各送信はどちらか一方にだけ現れる。
        let state = self.lock();
        let rx = self.inner.tx.subscribe();
        let snapshot = state.snapshot();
        drop(state);
        (
            snapshot,
            FeedReceiver {
                hub: self.clone(),
                rx,
            },
        )
    }

    /// 購読者がいなくても、受信が止まっていても待たない（broadcast は古い未読を上書きする）。
    fn broadcast(&self, msg: FeedMessage) {
        // 購読者が 0 人のときの Err は捨ててよい
        let _ = self.inner.tx.send(msg);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // 保持データは送信ごとに整合が取れているので、毒化しても使い続けてよい
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            chats: self.chats.iter().cloned().collect(),
            notifications: self.notifications.iter().cloned().collect(),
            chatters: self.chatters.clone(),
            status: self.status.clone(),
        }
    }
}

fn push_bounded<T>(buf: &mut VecDeque<T>, item: T, cap: usize) {
    buf.push_back(item);
    while buf.len() > cap {
        buf.pop_front();
    }
}

impl FeedReceiver {
    /// 次の新着を待つ。取りこぼしたら [`FeedRecvError::Lagged`] を返す。
    pub async fn recv(&mut self) -> Result<FeedMessage, FeedRecvError> {
        match self.rx.recv().await {
            Ok(msg) => Ok(msg),
            Err(broadcast::error::RecvError::Lagged(n)) => Err(FeedRecvError::Lagged(n)),
            // 受信口がハブを持っているので送信側は閉じない
            Err(broadcast::error::RecvError::Closed) => {
                unreachable!("FeedReceiver keeps the hub (and its sender) alive")
            }
        }
    }

    /// スナップショットを取り直し、受信口をその時点からの新着に付け替える。
    pub fn resync(&mut self) -> Snapshot {
        let (snapshot, fresh) = self.hub.subscribe();
        self.rx = fresh.rx;
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::message::{Chatter, ConnectionState};
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::time::Duration;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn chat(n: i64) -> ChatLine {
        ChatLine {
            received_at: at(n),
            user_login: format!("u{n}"),
            display_name: format!("U{n}"),
            text: format!("msg {n}"),
            color: None,
        }
    }

    fn notification(n: i64) -> NotificationLine {
        NotificationLine {
            received_at: at(n),
            subscription_type: "channel.follow".into(),
            summary: format!("n{n}"),
            event: json!({"n": n}),
        }
    }

    fn chatters(n: i64) -> Chatters {
        Chatters {
            fetched_at: at(n),
            chatters: vec![Chatter {
                login: format!("u{n}"),
                display_name: format!("U{n}"),
            }],
        }
    }

    fn status_irc(state: ConnectionState) -> Status {
        Status {
            irc: state,
            ..Status::default()
        }
    }

    #[test]
    fn snapshot_contains_everything_sent_before_subscribing() {
        let hub = FeedHub::new(10);
        hub.send_chat(chat(1));
        hub.send_notification(notification(2));
        hub.send_chat(chat(3));
        hub.send_chatters(chatters(4));
        hub.send_chatters(chatters(5));
        hub.send_status(status_irc(ConnectionState::Disconnected));
        hub.send_status(status_irc(ConnectionState::Connected));

        let (snap, _rx) = hub.subscribe();
        assert_eq!(snap.chats, vec![chat(1), chat(3)]);
        assert_eq!(snap.notifications, vec![notification(2)]);
        assert_eq!(snap.chatters, Some(chatters(5)));
        assert_eq!(snap.status, status_irc(ConnectionState::Connected));
    }

    #[test]
    fn empty_hub_snapshot() {
        let (snap, _rx) = FeedHub::new(10).subscribe();
        assert_eq!(snap, Snapshot::default());
    }

    #[test]
    fn history_drops_oldest_beyond_history_size() {
        let hub = FeedHub::new(3);
        for n in 1..=5 {
            hub.send_chat(chat(n));
            hub.send_notification(notification(n + 100));
        }
        let (snap, _rx) = hub.subscribe();
        assert_eq!(snap.chats, vec![chat(3), chat(4), chat(5)]);
        assert_eq!(
            snap.notifications,
            vec![notification(103), notification(104), notification(105)]
        );
    }

    #[test]
    fn zero_history_size_keeps_nothing_but_still_broadcasts() {
        let hub = FeedHub::new(0);
        let (_, mut rx) = hub.subscribe();
        hub.send_chat(chat(1));
        assert!(hub.subscribe().0.chats.is_empty());
        let got = rx.rx.try_recv().unwrap();
        assert_eq!(got, FeedMessage::Chat(chat(1)));
    }

    #[test]
    fn update_status_changes_only_the_given_fields() {
        let hub = FeedHub::new(10);
        hub.update_status(|s| s.irc = ConnectionState::Connected);
        hub.update_status(|s| s.eventsub = ConnectionState::Disconnected);
        let (snap, _rx) = hub.subscribe();
        assert_eq!(snap.status.irc, ConnectionState::Connected);
        assert_eq!(snap.status.eventsub, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn subscriber_receives_new_messages_in_order() {
        let hub = FeedHub::new(10);
        hub.send_chat(chat(1));
        let (_, mut rx) = hub.subscribe();
        hub.send_chat(chat(2));
        hub.send_notification(notification(3));
        hub.send_chatters(chatters(4));
        hub.update_status(|s| s.irc = ConnectionState::Connected);

        assert_eq!(rx.recv().await, Ok(FeedMessage::Chat(chat(2))));
        assert_eq!(
            rx.recv().await,
            Ok(FeedMessage::Notification(notification(3)))
        );
        assert_eq!(rx.recv().await, Ok(FeedMessage::Chatters(chatters(4))));
        assert_eq!(
            rx.recv().await,
            Ok(FeedMessage::Status(status_irc(ConnectionState::Connected)))
        );
    }

    #[test]
    fn send_does_not_block_without_subscribers() {
        let hub = FeedHub::new(5);
        for n in 0..10_000 {
            hub.send_chat(chat(n));
        }
        assert_eq!(hub.subscribe().0.chats.len(), 5);
    }

    #[tokio::test]
    async fn stalled_subscriber_does_not_block_and_detects_lag_then_resyncs() {
        let hub = FeedHub::new(5);
        let (_, mut rx) = hub.subscribe();
        // 受信口を読まないまま、チャネル容量を大きく超えて送る
        let sender = hub.clone();
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                for n in 0..10_000 {
                    sender.send_chat(chat(n));
                }
            }),
        )
        .await
        .expect("send must not block on a stalled subscriber")
        .unwrap();

        assert!(matches!(rx.recv().await, Err(FeedRecvError::Lagged(_))));

        let snap = rx.resync();
        assert_eq!(
            snap.chats,
            (9_995..10_000).map(chat).collect::<Vec<_>>(),
            "resync returns the latest history"
        );
        // 付け替え後は、その時点以降の新着だけが届く
        hub.send_chat(chat(20_000));
        assert_eq!(rx.recv().await, Ok(FeedMessage::Chat(chat(20_000))));
        assert!(rx.rx.is_empty());
    }

    #[test]
    fn concurrent_sends_are_neither_lost_nor_duplicated_across_snapshot_and_receiver() {
        const PER_SENDER: i64 = 2_000;
        const SENDERS: i64 = 4;
        // 保持件数を十分大きくして、スナップショット + 受信口で全件を数えられるようにする
        let hub = FeedHub::new((PER_SENDER * SENDERS) as usize);
        let handles: Vec<_> = (0..SENDERS)
            .map(|s| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    for i in 0..PER_SENDER {
                        hub.send_chat(chat(s * PER_SENDER + i));
                    }
                })
            })
            .collect();
        // 送信の最中に購読を始める
        std::thread::yield_now();
        let (snap, mut rx) = hub.subscribe();
        for h in handles {
            h.join().unwrap();
        }

        let mut seen: Vec<i64> = snap
            .chats
            .iter()
            .map(|c| c.received_at.timestamp())
            .collect();
        while let Ok(msg) = rx.rx.try_recv() {
            match msg {
                FeedMessage::Chat(c) => seen.push(c.received_at.timestamp()),
                other => panic!("unexpected {other:?}"),
            }
        }
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "no duplicates");
        assert_eq!(
            seen,
            (0..PER_SENDER * SENDERS).collect::<Vec<_>>(),
            "no loss"
        );
    }
}
