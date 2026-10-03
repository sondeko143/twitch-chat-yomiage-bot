//! IRC / EventSub の接続状態をハブの status へ反映するためのガード。

use super::hub::FeedHub;
use super::message::{ConnectionState, Status};

/// どの接続の状態か。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Irc,
    EventSub,
}

impl Link {
    fn field(self, status: &mut Status) -> &mut ConnectionState {
        match self {
            Self::Irc => &mut status.irc,
            Self::EventSub => &mut status.eventsub,
        }
    }
}

/// 接続 1 回分の状態を報告する。作った時点で `connecting`、[`Self::connected`] で
/// `connected`、捨てた時点（正常終了・エラー・abort のいずれでも）で `disconnected` にする。
pub struct LinkStatus {
    hub: FeedHub,
    link: Link,
}

impl LinkStatus {
    pub fn connecting(hub: FeedHub, link: Link) -> Self {
        let this = Self { hub, link };
        this.set(ConnectionState::Connecting);
        this
    }

    pub fn connected(&self) {
        self.set(ConnectionState::Connected);
    }

    /// EventSub の購読処理を終えた。`connected` にし、購読に失敗した種別を報告する
    /// （1 回の更新で両方を反映する）。
    pub fn subscribed(&self, failed_subscriptions: Vec<String>) {
        let link = self.link;
        self.hub.update_status(|s| {
            *link.field(s) = ConnectionState::Connected;
            s.eventsub_failed_subscriptions = failed_subscriptions;
        });
    }

    /// 状態を変える。EventSub のセッションが始まる・終わるときは、前のセッションの
    /// 購読失敗を消す。
    fn set(&self, state: ConnectionState) {
        let link = self.link;
        self.hub.update_status(|s| {
            *link.field(s) = state;
            if link == Link::EventSub && state != ConnectionState::Connected {
                s.eventsub_failed_subscriptions.clear();
            }
        });
    }
}

impl Drop for LinkStatus {
    fn drop(&mut self) {
        self.set(ConnectionState::Disconnected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(hub: &FeedHub) -> (ConnectionState, ConnectionState) {
        let status = hub.subscribe().0.status;
        (status.irc, status.eventsub)
    }

    #[test]
    fn follows_connecting_connected_disconnected_for_its_own_link_only() {
        use ConnectionState::*;
        let hub = FeedHub::new(1);
        hub.update_status(|s| {
            s.irc = Disconnected;
            s.eventsub = Disconnected;
        });

        let guard = LinkStatus::connecting(hub.clone(), Link::EventSub);
        assert_eq!(states(&hub), (Disconnected, Connecting));
        guard.connected();
        assert_eq!(states(&hub), (Disconnected, Connected));
        drop(guard);
        assert_eq!(states(&hub), (Disconnected, Disconnected));

        let guard = LinkStatus::connecting(hub.clone(), Link::Irc);
        guard.connected();
        assert_eq!(states(&hub), (Connected, Disconnected));
    }

    fn failed(hub: &FeedHub) -> Vec<String> {
        hub.subscribe().0.status.eventsub_failed_subscriptions
    }

    #[test]
    fn subscribed_reports_connected_with_the_failed_types_of_this_session_only() {
        let hub = FeedHub::new(1);
        let guard = LinkStatus::connecting(hub.clone(), Link::EventSub);
        guard.subscribed(vec!["channel.follow".into()]);
        assert_eq!(states(&hub).1, ConnectionState::Connected);
        assert_eq!(failed(&hub), ["channel.follow"]);

        // 切れたら今のセッションの失敗は消える
        drop(guard);
        assert_eq!(states(&hub).1, ConnectionState::Disconnected);
        assert!(failed(&hub).is_empty());

        // 新しいセッションは前のセッションの失敗を引き継がない
        hub.update_status(|s| s.eventsub_failed_subscriptions = vec!["stale".into()]);
        let guard = LinkStatus::connecting(hub.clone(), Link::EventSub);
        assert!(failed(&hub).is_empty());
        guard.subscribed(Vec::new());
        assert_eq!(states(&hub).1, ConnectionState::Connected);
        assert!(failed(&hub).is_empty());
    }

    #[test]
    fn irc_link_leaves_eventsub_failures_alone() {
        let hub = FeedHub::new(1);
        let eventsub = LinkStatus::connecting(hub.clone(), Link::EventSub);
        eventsub.subscribed(vec!["channel.raid".into()]);
        let irc = LinkStatus::connecting(hub.clone(), Link::Irc);
        irc.connected();
        drop(irc);
        assert_eq!(failed(&hub), ["channel.raid"]);
    }

    #[tokio::test]
    async fn aborted_task_reports_disconnected() {
        let hub = FeedHub::new(1);
        let guard = LinkStatus::connecting(hub.clone(), Link::Irc);
        guard.connected();
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        task.abort();
        let _ = task.await;
        assert_eq!(states(&hub).0, ConnectionState::Disconnected);
    }
}
