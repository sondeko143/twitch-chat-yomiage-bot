//! 画面の描画。左にコメント欄（上）と通知欄（下）、右に視聴者欄、最下部に接続状態行と
//! 操作説明（警告があればそちら）を出す。

use super::state::{LinkState, MonitorState, Pane};
use crate::feed::{ChatLine, Chatter, ConnectionState, NotificationLine, Status};
use chrono::{DateTime, Local, Utc};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::str::FromStr;

const HELP: &str = "q: 終了  Tab: 欄の切替  ↑↓ PgUp PgDn: スクロール  End: 最新へ追従";
const SEPARATOR: &str = " | ";
/// 新規視聴者の印。
const NEW_MARK: &str = "★";

/// 画面全体を描く。各欄の表示行数を `state` へ記録する。
pub fn draw(frame: &mut Frame, state: &mut MonitorState) {
    let [main, status_area, footer_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(75), Constraint::Percentage(25)]).areas(main);
    let [chat_area, notification_area] =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)]).areas(left);

    let chat_title = format!("コメント ({})", state.chats().len());
    draw_pane(
        frame,
        state,
        Pane::Chat,
        chat_area,
        chat_title,
        |s, top, n| s.chats().iter().skip(top).take(n).map(chat_line).collect(),
    );
    let notification_title = format!("通知 ({})", state.notifications().len());
    draw_pane(
        frame,
        state,
        Pane::Notifications,
        notification_area,
        notification_title,
        |s, top, n| {
            s.notifications()
                .iter()
                .skip(top)
                .take(n)
                .map(notification_line)
                .collect()
        },
    );
    let chatters_title = match state.chatter_count() {
        Some(count) => format!("視聴者 ({count})"),
        None => "視聴者 (未取得)".to_string(),
    };
    draw_pane(
        frame,
        state,
        Pane::Chatters,
        right,
        chatters_title,
        |s, top, n| {
            s.chatters()
                .unwrap_or_default()
                .into_iter()
                .skip(top)
                .take(n)
                .map(|(c, is_new)| chatter_line(c, is_new))
                .collect()
        },
    );

    frame.render_widget(Paragraph::new(status_line(state)), status_area);
    frame.render_widget(Paragraph::new(footer_line(state)), footer_area);
}

/// 枠付きの 1 欄を描く。`rows(state, top, height)` が表示する行を返す。
fn draw_pane(
    frame: &mut Frame,
    state: &mut MonitorState,
    pane: Pane,
    area: Rect,
    title: String,
    rows: impl FnOnce(&MonitorState, usize, usize) -> Vec<Line<'static>>,
) {
    let block = Block::bordered();
    let height = usize::from(block.inner(area).height);
    state.set_viewport(pane, height);

    let mut title = title;
    if pane != Pane::Chatters && state.is_scrolled(pane) {
        title.push_str(" [スクロール中: End で最新へ]");
    }
    let border = if state.focus() == pane {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new()
    };
    let block = block.title(title).border_style(border);
    let lines = rows(state, state.top(pane), height);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn local_time(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M:%S").to_string()
}

fn dim(text: String) -> Span<'static> {
    Span::styled(text, Style::new().fg(Color::DarkGray))
}

fn chat_line(chat: &ChatLine) -> Line<'static> {
    let name_style = chat
        .color
        .as_deref()
        .and_then(|c| Color::from_str(c).ok())
        .map_or_else(Style::new, |c| Style::new().fg(c))
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        dim(format!("{} ", local_time(chat.received_at))),
        Span::styled(chat.display_name.clone(), name_style),
        Span::raw(": "),
        Span::raw(chat.text.clone()),
    ])
}

fn notification_line(n: &NotificationLine) -> Line<'static> {
    Line::from(vec![
        dim(format!("{} ", local_time(n.received_at))),
        Span::raw(n.summary.clone()),
    ])
}

fn chatter_line(chatter: &Chatter, is_new: bool) -> Line<'static> {
    let mut name = chatter.display_name.clone();
    if !chatter.display_name.eq_ignore_ascii_case(&chatter.login) {
        name.push_str(&format!(" ({})", chatter.login));
    }
    if is_new {
        let style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled(NEW_MARK, style),
            Span::raw(" "),
            Span::styled(name, style),
        ])
    } else {
        Line::from(format!("  {name}"))
    }
}

fn colored(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

/// 接続状態行: `read-chat` との接続、IRC / EventSub、視聴者一覧の取得状況。
fn status_line(state: &MonitorState) -> Line<'static> {
    let mut spans = vec![Span::raw("read-chat: ")];
    let mut reason = None;
    match state.link() {
        LinkState::Connected => spans.push(colored("接続", Color::Green)),
        LinkState::Connecting => spans.push(colored("接続試行中", Color::Yellow)),
        LinkState::Disconnected { error, retry_in } => {
            spans.push(colored(
                format!("未接続（{:.1}秒後に再接続）", retry_in.as_secs_f64()),
                Color::Red,
            ));
            reason = Some(error.clone());
        }
    }
    let known = (*state.link() == LinkState::Connected).then(|| state.status());
    spans.push(Span::raw(format!("{SEPARATOR}IRC: ")));
    spans.push(connection_span(known.map(|s| s.irc)));
    spans.push(Span::raw(format!("{SEPARATOR}EventSub: ")));
    spans.push(connection_span(known.map(|s| s.eventsub)));
    spans.push(Span::raw(format!("{SEPARATOR}視聴者一覧: ")));
    spans.extend(chatters_status_spans(known));
    if let Some(reason) = reason {
        spans.push(Span::raw(SEPARATOR));
        spans.push(dim(reason));
    }
    Line::from(spans)
}

/// `None` は `read-chat` に繋がっておらず分からないとき。
fn connection_span(state: Option<ConnectionState>) -> Span<'static> {
    match state {
        Some(ConnectionState::Connected) => colored("接続", Color::Green),
        Some(ConnectionState::Connecting) => colored("接続待ち", Color::Yellow),
        Some(ConnectionState::Disconnected) => colored("切断", Color::Red),
        None => dim("不明".into()),
    }
}

fn chatters_status_spans(status: Option<&Status>) -> Vec<Span<'static>> {
    let Some(status) = status else {
        return vec![dim("不明".into())];
    };
    let last_success = status
        .chatters_last_success
        .map(|at| format!("{} 更新", local_time(at)));
    match (&status.chatters_last_error, last_success) {
        (Some(error), last) => {
            let mut spans = vec![colored(format!("取得失敗（{error}）"), Color::Red)];
            if let Some(last) = last {
                spans.push(Span::raw(format!(" 最終: {last}")));
            }
            spans
        }
        (None, Some(last)) => vec![Span::raw(last)],
        (None, None) => vec![dim("未取得".into())],
    }
}

/// 最下行: 警告があれば警告、無ければ操作説明。
fn footer_line(state: &MonitorState) -> Line<'static> {
    match state.warning() {
        Some(warning) => Line::from(Span::styled(
            format!("⚠ {warning}"),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        None => Line::from(dim(HELP.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::{ConnectionState, FeedMessage, Snapshot, Status};
    use crate::monitor::state::tests::{at, chat, chatters, notification};
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::text::Span;
    use ratatui::Terminal;
    use std::time::Duration;

    fn render(state: &mut MonitorState, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, state)).unwrap();
        terminal
    }

    /// 画面を行ごとの文字列にする。全角文字の後ろの継続セルは飛ばす。
    fn lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                let mut line = String::new();
                let mut skip = 0;
                for x in 0..area.width {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    let symbol = buffer[(x, y)].symbol();
                    skip = Span::raw(symbol).width().saturating_sub(1);
                    line.push_str(symbol);
                }
                line
            })
            .collect()
    }

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        lines(terminal).join("\n")
    }

    fn connected_state() -> MonitorState {
        let mut state = MonitorState::new(100);
        state.set_link(LinkState::Connected);
        state.apply(FeedMessage::Snapshot(Snapshot {
            chats: vec![chat(1), chat(2)],
            notifications: vec![notification(3)],
            chatters: Some(chatters(&["alice", "bob"])),
            status: Status {
                irc: ConnectionState::Connected,
                eventsub: ConnectionState::Disconnected,
                chatters_last_success: Some(at(100)),
                chatters_last_error: None,
            },
        }));
        state
    }

    #[test]
    fn shows_panes_with_contents_and_viewer_count() {
        let mut state = connected_state();
        let terminal = render(&mut state, 100, 20);
        let screen = screen(&terminal);
        for expected in [
            "コメント",
            "ユーザー1",
            "コメント2",
            "通知",
            "通知3",
            "視聴者 (2)",
            "alice",
            "bob",
        ] {
            assert!(screen.contains(expected), "{expected:?} not in\n{screen}");
        }
    }

    #[test]
    fn status_line_shows_link_irc_eventsub_and_chatters_update() {
        let mut state = connected_state();
        let terminal = render(&mut state, 120, 20);
        let status = lines(&terminal)[18].clone();
        assert!(status.contains("read-chat: 接続"), "{status}");
        assert!(status.contains("IRC: 接続"), "{status}");
        assert!(status.contains("EventSub: 切断"), "{status}");
        assert!(status.contains("視聴者一覧:"), "{status}");
        assert!(status.contains("更新"), "{status}");
    }

    #[test]
    fn status_line_shows_chatters_failure() {
        let mut state = connected_state();
        state.apply(FeedMessage::Status(Status {
            chatters_last_error: Some("401 Unauthorized".into()),
            ..state.status().clone()
        }));
        let terminal = render(&mut state, 140, 20);
        let status = lines(&terminal)[18].clone();
        assert!(status.contains("取得失敗"), "{status}");
        assert!(status.contains("401 Unauthorized"), "{status}");
    }

    #[test]
    fn status_line_shows_disconnected_while_retrying() {
        let mut state = MonitorState::new(10);
        state.set_link(LinkState::Disconnected {
            error: "connection refused".into(),
            retry_in: Duration::from_secs(2),
        });
        let terminal = render(&mut state, 140, 20);
        let status = lines(&terminal)[18].clone();
        assert!(status.contains("read-chat: 未接続"), "{status}");
        assert!(status.contains("2.0秒後に再接続"), "{status}");
        assert!(status.contains("connection refused"), "{status}");
        assert!(status.contains("IRC: 不明"), "{status}");
    }

    #[test]
    fn new_viewers_are_marked() {
        let mut state = connected_state();
        state.apply(FeedMessage::Chatters(chatters(&["alice", "bob", "carol"])));
        let terminal = render(&mut state, 100, 20);
        let screen = lines(&terminal);
        let carol = screen.iter().find(|l| l.contains("carol")).unwrap();
        let alice = screen.iter().find(|l| l.contains("alice")).unwrap();
        assert!(carol.contains("★"), "{carol}");
        assert!(!alice.contains("★"), "{alice}");
        assert!(screen.iter().any(|l| l.contains("視聴者 (3)")));
    }

    #[test]
    fn new_viewers_are_colored() {
        let mut state = connected_state();
        state.apply(FeedMessage::Chatters(chatters(&["alice", "carol"])));
        let terminal = render(&mut state, 100, 20);
        let buffer = terminal.backend().buffer();
        let lines = lines(&terminal);
        let y = lines.iter().position(|l| l.contains("carol")).unwrap();
        let star_x = (0..buffer.area.width)
            .find(|&x| buffer[(x, y as u16)].symbol() == "★")
            .unwrap();
        assert_eq!(buffer[(star_x, y as u16)].fg, Color::Green);
    }

    #[test]
    fn incompatible_message_warning_is_shown() {
        let mut state = connected_state();
        state.apply_text(r#"{"v":2,"kind":"chat"}"#);
        let terminal = render(&mut state, 140, 20);
        let last = lines(&terminal)[19].clone();
        assert!(last.contains("非互換"), "{last}");
    }

    #[test]
    fn help_line_lists_keys() {
        let mut state = connected_state();
        let terminal = render(&mut state, 120, 20);
        let last = lines(&terminal)[19].clone();
        for key in ["q", "Tab", "PgUp", "End"] {
            assert!(last.contains(key), "{key} not in {last}");
        }
    }

    #[test]
    fn chat_pane_follows_latest_and_records_viewport() {
        let mut state = MonitorState::new(100);
        state.apply(FeedMessage::Snapshot(Snapshot {
            chats: (1..=30).map(chat).collect(),
            ..Snapshot::default()
        }));
        let terminal = render(&mut state, 100, 20);
        let screen = screen(&terminal);
        assert!(screen.contains("コメント30"), "{screen}");
        assert!(!screen.contains("コメント1 "), "{screen}");
        // 記録した表示行数でページ送りできる
        state.scroll_pages(-1);
        assert!(state.top(Pane::Chat) < 30 - 1);
    }

    #[test]
    fn scrolled_pane_stays_put_and_says_so() {
        let mut state = MonitorState::new(100);
        state.apply(FeedMessage::Snapshot(Snapshot {
            chats: (1..=30).map(chat).collect(),
            ..Snapshot::default()
        }));
        render(&mut state, 100, 20);
        state.scroll_pages(-100);
        state.apply(FeedMessage::Chat(chat(31)));
        let terminal = render(&mut state, 100, 20);
        let screen = screen(&terminal);
        assert!(screen.contains("コメント1 "), "{screen}");
        assert!(!screen.contains("コメント31"), "{screen}");
        assert!(screen.contains("スクロール中"), "{screen}");
    }

    #[test]
    fn focused_pane_border_is_highlighted() {
        let mut state = connected_state();
        state.focus_next(); // 通知欄
        let terminal = render(&mut state, 100, 20);
        let buffer = terminal.backend().buffer();
        let lines = lines(&terminal);
        let y = lines.iter().position(|l| l.contains("通知 (")).unwrap();
        assert_eq!(buffer[(0, y as u16)].fg, Color::Yellow);
        assert_ne!(buffer[(0, 0)].fg, Color::Yellow);
    }

    #[test]
    fn tiny_terminal_does_not_panic() {
        let mut state = connected_state();
        render(&mut state, 5, 3);
        render(&mut state, 1, 1);
    }
}
