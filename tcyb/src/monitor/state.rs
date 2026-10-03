//! monitor の表示状態。受信した配信メッセージの反映・新規視聴者の判定・スクロールを持つ。
//! 端末にもネットワークにも依存しない。

use crate::feed::{
    ChatLine, Chatter, Chatters, FeedDecodeError, FeedMessage, NotificationLine, Snapshot, Status,
    PROTOCOL_VERSION,
};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;

/// 欄。`Tab` でこの順に切り替わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Chat,
    Notifications,
    Chatters,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::Chat, Pane::Notifications, Pane::Chatters];

    fn index(self) -> usize {
        match self {
            Pane::Chat => 0,
            Pane::Notifications => 1,
            Pane::Chatters => 2,
        }
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// 追従時に最新（末尾）を見せる欄か。視聴者欄は先頭を見せる。
    fn follows_bottom(self) -> bool {
        !matches!(self, Pane::Chatters)
    }
}

/// `read-chat`（配信口）との接続状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    /// 未接続。`retry_in` 後に再接続を試みる。
    Disconnected {
        error: String,
        retry_in: Duration,
    },
}

/// 1 つの欄の表示位置。`top` が `None` のときは追従（最新を表示）している。
#[derive(Debug, Clone, Copy, Default)]
struct Scroll {
    top: Option<usize>,
    height: usize,
}

/// monitor の表示状態。
pub struct MonitorState {
    history_size: usize,
    chats: VecDeque<ChatLine>,
    notifications: VecDeque<NotificationLine>,
    chatters: Option<Chatters>,
    status: Status,
    link: LinkState,
    warning: Option<String>,
    /// 起動後に最初に受けた視聴者一覧の login。これに無い人を新規として区別する。
    baseline: Option<HashSet<String>>,
    focus: Pane,
    scrolls: [Scroll; 3],
}

impl MonitorState {
    /// コメントと通知をそれぞれ `history_size` 件まで保持する状態を作る。
    pub fn new(history_size: usize) -> Self {
        Self {
            history_size: history_size.max(1),
            chats: VecDeque::new(),
            notifications: VecDeque::new(),
            chatters: None,
            status: Status::default(),
            link: LinkState::Connecting,
            warning: None,
            baseline: None,
            focus: Pane::Chat,
            scrolls: [Scroll::default(); 3],
        }
    }

    pub fn chats(&self) -> &VecDeque<ChatLine> {
        &self.chats
    }

    pub fn notifications(&self) -> &VecDeque<NotificationLine> {
        &self.notifications
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn link(&self) -> &LinkState {
        &self.link
    }

    /// 非互換・解釈不能なメッセージを受けたときの表示文。
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn focus(&self) -> Pane {
        self.focus
    }

    pub fn set_link(&mut self, link: LinkState) {
        self.link = link;
    }

    /// 受信したテキストフレームを解釈して反映する。版が違えば反映せず警告を出す。
    pub fn apply_text(&mut self, text: &str) {
        match FeedMessage::from_json(text) {
            Ok(msg) => self.apply(msg),
            Err(FeedDecodeError::Incompatible(v)) => {
                self.warning = Some(format!(
                    "非互換な配信メッセージを受信しました（v={v}、この monitor は v={PROTOCOL_VERSION}）。read-chat と monitor の版を揃えてください"
                ));
            }
            Err(err @ FeedDecodeError::Malformed(_)) => {
                self.warning = Some(format!("解釈できない配信メッセージを無視しました: {err}"));
            }
        }
    }

    /// 配信メッセージを反映する。`snapshot` は表示内容を置き換え、他は追記・更新する。
    pub fn apply(&mut self, msg: FeedMessage) {
        self.warning = None;
        match msg {
            FeedMessage::Snapshot(snapshot) => self.apply_snapshot(snapshot),
            FeedMessage::Chat(chat) => {
                let evicted = push_bounded(&mut self.chats, chat, self.history_size);
                self.shift_scroll(Pane::Chat, evicted);
            }
            FeedMessage::Notification(n) => {
                let evicted = push_bounded(&mut self.notifications, n, self.history_size);
                self.shift_scroll(Pane::Notifications, evicted);
            }
            FeedMessage::Chatters(chatters) => self.set_chatters(Some(chatters)),
            FeedMessage::Status(status) => self.status = status,
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let Snapshot {
            chats,
            notifications,
            chatters,
            status,
        } = snapshot;
        self.chats = keep_last(chats, self.history_size);
        self.notifications = keep_last(notifications, self.history_size);
        self.set_chatters(chatters);
        self.status = status;
    }

    fn set_chatters(&mut self, chatters: Option<Chatters>) {
        if self.baseline.is_none() {
            if let Some(c) = &chatters {
                self.baseline = Some(c.chatters.iter().map(|c| c.login.clone()).collect());
            }
        }
        self.chatters = chatters;
    }

    /// 視聴者一覧（取得済みなら）。新規の人を先に、それぞれ login の昇順で並べる。
    /// Get Chatters は返す順を保証しないため、ここで並べて更新ごとの並び替わりを防ぐ。
    /// 組の 2 番目は「monitor 起動後に初めて現れた視聴者か」。
    pub fn chatters(&self) -> Option<Vec<(&Chatter, bool)>> {
        let chatters = self.chatters.as_ref()?;
        let mut rows: Vec<(&Chatter, bool)> = chatters
            .chatters
            .iter()
            .map(|c| (c, self.is_new_viewer(&c.login)))
            .collect();
        rows.sort_by(|(a, a_new), (b, b_new)| b_new.cmp(a_new).then_with(|| a.login.cmp(&b.login)));
        Some(rows)
    }

    /// 視聴者一覧の人数（未取得なら `None`）。
    pub fn chatter_count(&self) -> Option<usize> {
        self.chatters.as_ref().map(|c| c.chatters.len())
    }

    fn is_new_viewer(&self, login: &str) -> bool {
        self.baseline
            .as_ref()
            .is_some_and(|baseline| !baseline.contains(login))
    }

    fn len(&self, pane: Pane) -> usize {
        match pane {
            Pane::Chat => self.chats.len(),
            Pane::Notifications => self.notifications.len(),
            Pane::Chatters => self.chatter_count().unwrap_or(0),
        }
    }

    /// 欄の表示行数を記録する（描画時に呼ぶ）。
    pub fn set_viewport(&mut self, pane: Pane, height: usize) {
        self.scrolls[pane.index()].height = height;
    }

    /// 欄の先頭に表示する項目の位置。
    pub fn top(&self, pane: Pane) -> usize {
        let scroll = self.scrolls[pane.index()];
        let max_top = self.len(pane).saturating_sub(scroll.height);
        match scroll.top {
            Some(top) => top.min(max_top),
            None if pane.follows_bottom() => max_top,
            None => 0,
        }
    }

    /// 欄が追従をやめてスクロールしているか。
    pub fn is_scrolled(&self, pane: Pane) -> bool {
        self.scrolls[pane.index()].top.is_some()
    }

    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// フォーカス中の欄を `delta` 行動かす（負で上）。動かした後は追従しないが、
    /// 末尾に追従する欄で一番下まで下げたら追従に戻る。
    pub fn scroll_by(&mut self, delta: isize) {
        let pane = self.focus;
        let current = self.top(pane);
        let max_top = self
            .len(pane)
            .saturating_sub(self.scrolls[pane.index()].height);
        let next = current.saturating_add_signed(delta).min(max_top);
        self.scrolls[pane.index()].top = if pane.follows_bottom() && next >= max_top {
            None
        } else {
            Some(next)
        };
    }

    /// フォーカス中の欄を 1 画面ぶん動かす（`pages` が負で上）。
    pub fn scroll_pages(&mut self, pages: isize) {
        let height = self.scrolls[self.focus.index()].height;
        let page = height.saturating_sub(1).max(1) as isize;
        self.scroll_by(page.saturating_mul(pages));
    }

    /// フォーカス中の欄を最新への追従に戻す。
    pub fn follow_latest(&mut self) {
        self.scrolls[self.focus.index()].top = None;
    }

    /// 先頭から `evicted` 件消えたとき、スクロール中なら同じ内容が見え続けるよう位置をずらす。
    fn shift_scroll(&mut self, pane: Pane, evicted: usize) {
        if let Some(top) = &mut self.scrolls[pane.index()].top {
            *top = top.saturating_sub(evicted);
        }
    }
}

/// 末尾に追加し、`cap` を超えた分を先頭から捨てる。捨てた件数を返す。
fn push_bounded<T>(buf: &mut VecDeque<T>, item: T, cap: usize) -> usize {
    buf.push_back(item);
    let evicted = buf.len().saturating_sub(cap);
    buf.drain(..evicted);
    evicted
}

/// 古い順の列から末尾 `cap` 件を残す。
fn keep_last<T>(items: Vec<T>, cap: usize) -> VecDeque<T> {
    let mut buf = VecDeque::from(items);
    let excess = buf.len().saturating_sub(cap);
    buf.drain(..excess);
    buf
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::feed::ConnectionState;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    pub fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    pub fn chat(n: i64) -> ChatLine {
        ChatLine {
            received_at: at(n),
            user_login: format!("user{n}"),
            display_name: format!("ユーザー{n}"),
            text: format!("コメント{n}"),
            color: None,
        }
    }

    pub fn notification(n: i64) -> NotificationLine {
        NotificationLine {
            received_at: at(n),
            subscription_type: "channel.follow".into(),
            summary: format!("通知{n}"),
            event: json!({}),
        }
    }

    pub fn chatters(logins: &[&str]) -> Chatters {
        Chatters {
            fetched_at: at(100),
            chatters: logins
                .iter()
                .map(|l| Chatter {
                    login: (*l).into(),
                    display_name: format!("{l}さん"),
                })
                .collect(),
        }
    }

    fn texts(state: &MonitorState) -> Vec<String> {
        state.chats().iter().map(|c| c.text.clone()).collect()
    }

    fn new_logins(state: &MonitorState) -> Vec<String> {
        state
            .chatters()
            .unwrap()
            .into_iter()
            .filter(|(_, new)| *new)
            .map(|(c, _)| c.login.clone())
            .collect()
    }

    fn snapshot_with_chats(range: std::ops::RangeInclusive<i64>) -> FeedMessage {
        FeedMessage::Snapshot(Snapshot {
            chats: range.map(chat).collect(),
            ..Snapshot::default()
        })
    }

    #[test]
    fn snapshot_replaces_contents_and_later_messages_are_appended() {
        let mut state = MonitorState::new(10);
        state.apply(FeedMessage::Chat(chat(1)));
        state.apply(FeedMessage::Notification(notification(1)));

        let status = Status {
            irc: ConnectionState::Connected,
            ..Status::default()
        };
        state.apply(FeedMessage::Snapshot(Snapshot {
            chats: vec![chat(2), chat(3)],
            notifications: vec![],
            chatters: Some(chatters(&["a"])),
            status: status.clone(),
        }));
        assert_eq!(texts(&state), ["コメント2", "コメント3"]);
        assert!(state.notifications().is_empty());
        assert_eq!(state.status(), &status);
        assert_eq!(state.chatter_count(), Some(1));

        state.apply(FeedMessage::Chat(chat(4)));
        state.apply(FeedMessage::Notification(notification(5)));
        assert_eq!(texts(&state), ["コメント2", "コメント3", "コメント4"]);
        assert_eq!(state.notifications()[0].summary, "通知5");
    }

    #[test]
    fn history_is_bounded() {
        let mut state = MonitorState::new(3);
        state.apply(snapshot_with_chats(1..=5));
        assert_eq!(texts(&state), ["コメント3", "コメント4", "コメント5"]);
        state.apply(FeedMessage::Chat(chat(6)));
        assert_eq!(texts(&state), ["コメント4", "コメント5", "コメント6"]);
    }

    #[test]
    fn status_message_updates_status() {
        let mut state = MonitorState::new(3);
        let status = Status {
            eventsub: ConnectionState::Disconnected,
            chatters_last_error: Some("401".into()),
            ..Status::default()
        };
        state.apply(FeedMessage::Status(status.clone()));
        assert_eq!(state.status(), &status);
    }

    #[test]
    fn incompatible_version_is_reported_and_not_applied() {
        let mut state = MonitorState::new(3);
        state.apply(FeedMessage::Chat(chat(1)));
        let mut value: serde_json::Value =
            serde_json::from_str(&FeedMessage::Chat(chat(2)).to_json()).unwrap();
        value["v"] = json!(2);

        state.apply_text(&value.to_string());
        assert_eq!(texts(&state), ["コメント1"]);
        let warning = state.warning().unwrap();
        assert!(warning.contains("非互換"), "{warning}");
        assert!(warning.contains("v=2"), "{warning}");

        state.apply_text(&FeedMessage::Chat(chat(3)).to_json());
        assert_eq!(texts(&state), ["コメント1", "コメント3"]);
        assert_eq!(state.warning(), None);
    }

    #[test]
    fn malformed_text_is_ignored_with_warning() {
        let mut state = MonitorState::new(3);
        state.apply_text("not json");
        assert!(state.chats().is_empty());
        assert!(state.warning().is_some());
    }

    #[test]
    fn viewers_in_the_first_list_are_not_new_and_later_arrivals_are() {
        let mut state = MonitorState::new(3);
        state.apply(FeedMessage::Chatters(chatters(&["a", "b"])));
        assert!(new_logins(&state).is_empty());

        state.apply(FeedMessage::Chatters(chatters(&["a", "c", "b", "d"])));
        assert_eq!(new_logins(&state), ["c", "d"]);
        // 新規の人が先に並ぶ
        let order: Vec<_> = state
            .chatters()
            .unwrap()
            .into_iter()
            .map(|(c, _)| c.login.clone())
            .collect();
        assert_eq!(order, ["c", "d", "a", "b"]);
    }

    #[test]
    fn viewers_are_ordered_by_login_within_each_group_regardless_of_api_order() {
        let order = |state: &MonitorState| -> Vec<String> {
            state
                .chatters()
                .unwrap()
                .into_iter()
                .map(|(c, _)| c.login.clone())
                .collect()
        };
        let mut state = MonitorState::new(3);
        state.apply(FeedMessage::Chatters(chatters(&["b", "a"])));
        assert_eq!(order(&state), ["a", "b"]);

        state.apply(FeedMessage::Chatters(chatters(&["d", "b", "c", "a"])));
        assert_eq!(order(&state), ["c", "d", "a", "b"]);

        // 同じ顔ぶれなら API の返す順が変わっても並びは変わらない
        state.apply(FeedMessage::Chatters(chatters(&["a", "c", "b", "d"])));
        assert_eq!(order(&state), ["c", "d", "a", "b"]);
    }

    #[test]
    fn first_list_may_come_in_a_snapshot_and_survives_reconnect_snapshots() {
        let mut state = MonitorState::new(3);
        // 最初のスナップショットには一覧がまだ無い
        state.apply(FeedMessage::Snapshot(Snapshot::default()));
        assert_eq!(state.chatter_count(), None);

        state.apply(FeedMessage::Snapshot(Snapshot {
            chatters: Some(chatters(&["a"])),
            ..Snapshot::default()
        }));
        assert!(new_logins(&state).is_empty());

        // read-chat 再起動後のスナップショットでも起動時の一覧を基準にする
        state.apply(FeedMessage::Snapshot(Snapshot {
            chatters: Some(chatters(&["a", "b"])),
            ..Snapshot::default()
        }));
        assert_eq!(new_logins(&state), ["b"]);
    }

    #[test]
    fn follows_latest_by_default() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chat, 3);
        state.apply(snapshot_with_chats(1..=5));
        assert_eq!(state.top(Pane::Chat), 2);
        state.apply(FeedMessage::Chat(chat(6)));
        assert_eq!(state.top(Pane::Chat), 3);
        assert!(!state.is_scrolled(Pane::Chat));
    }

    #[test]
    fn scrolled_view_does_not_move_on_new_messages_until_end() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chat, 3);
        state.apply(snapshot_with_chats(1..=10));
        state.scroll_by(-2);
        assert_eq!(state.top(Pane::Chat), 5);
        assert!(state.is_scrolled(Pane::Chat));

        state.apply(FeedMessage::Chat(chat(11)));
        assert_eq!(state.top(Pane::Chat), 5);

        state.follow_latest();
        assert_eq!(state.top(Pane::Chat), 8);
        assert!(!state.is_scrolled(Pane::Chat));
    }

    #[test]
    fn scrolled_view_keeps_showing_the_same_items_when_old_ones_are_dropped() {
        let mut state = MonitorState::new(5);
        state.set_viewport(Pane::Chat, 2);
        state.apply(snapshot_with_chats(1..=5));
        state.scroll_by(-1); // 先頭は コメント3
        assert_eq!(state.chats()[state.top(Pane::Chat)].text, "コメント3");
        state.apply(FeedMessage::Chat(chat(6)));
        assert_eq!(state.chats()[state.top(Pane::Chat)].text, "コメント3");
    }

    #[test]
    fn scrolling_is_clamped_and_pages_by_viewport() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chat, 4);
        state.apply(snapshot_with_chats(1..=20));
        state.scroll_pages(-1);
        assert_eq!(state.top(Pane::Chat), 13);
        state.scroll_pages(-100);
        assert_eq!(state.top(Pane::Chat), 0);
        state.scroll_by(1000);
        assert_eq!(state.top(Pane::Chat), 16);
        // 一番下まで下げたら追従に戻る
        assert!(!state.is_scrolled(Pane::Chat));
    }

    #[test]
    fn scrolling_down_while_following_keeps_following() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chat, 4);
        state.apply(snapshot_with_chats(1..=20));
        state.scroll_by(1);
        assert!(!state.is_scrolled(Pane::Chat));
        state.scroll_pages(1);
        assert!(!state.is_scrolled(Pane::Chat));
        state.apply(FeedMessage::Chat(chat(21)));
        assert_eq!(state.top(Pane::Chat), 17);
        // 上げてから途中まで下げた間は追従しない
        state.scroll_by(-5);
        state.scroll_by(2);
        assert!(state.is_scrolled(Pane::Chat));
        state.scroll_by(100);
        assert!(!state.is_scrolled(Pane::Chat));
    }

    #[test]
    fn tab_switches_focus_and_scroll_is_per_pane() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chat, 2);
        state.set_viewport(Pane::Notifications, 2);
        state.apply(FeedMessage::Snapshot(Snapshot {
            chats: (1..=5).map(chat).collect(),
            notifications: (1..=5).map(notification).collect(),
            ..Snapshot::default()
        }));
        assert_eq!(state.focus(), Pane::Chat);
        state.focus_next();
        assert_eq!(state.focus(), Pane::Notifications);
        state.scroll_by(-1);
        assert_eq!(state.top(Pane::Notifications), 2);
        assert_eq!(state.top(Pane::Chat), 3);
        state.focus_next();
        assert_eq!(state.focus(), Pane::Chatters);
        state.focus_next();
        assert_eq!(state.focus(), Pane::Chat);
    }

    #[test]
    fn chatters_pane_shows_the_head_when_following() {
        let mut state = MonitorState::new(100);
        state.set_viewport(Pane::Chatters, 2);
        state.apply(FeedMessage::Chatters(chatters(&["a", "b", "c", "d"])));
        assert_eq!(state.top(Pane::Chatters), 0);
    }

    #[test]
    fn link_state_is_kept() {
        let mut state = MonitorState::new(3);
        assert_eq!(state.link(), &LinkState::Connecting);
        state.set_link(LinkState::Connected);
        assert_eq!(state.link(), &LinkState::Connected);
    }
}
