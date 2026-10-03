//! キー入力を操作に対応づける。

use super::state::MonitorState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// キー操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    FocusNext,
    /// 行単位のスクロール（負で上）。
    Scroll(isize),
    /// 画面単位のスクロール（負で上）。
    Page(isize),
    /// 最新への追従に戻る。
    Follow,
}

/// キーイベントを操作に変える。押下以外（Windows で届く離した通知など）は無視する。
pub fn action_for(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    let action = match key.code {
        // raw モードでは Ctrl+C がシグナルにならずキーとして届くので、終了として扱う
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Tab => Action::FocusNext,
        KeyCode::Up => Action::Scroll(-1),
        KeyCode::Down => Action::Scroll(1),
        KeyCode::PageUp => Action::Page(-1),
        KeyCode::PageDown => Action::Page(1),
        KeyCode::End => Action::Follow,
        _ => return None,
    };
    Some(action)
}

/// 終了以外の操作を表示状態へ反映する。
pub fn apply(state: &mut MonitorState, action: Action) {
    match action {
        Action::Quit => {}
        Action::FocusNext => state.focus_next(),
        Action::Scroll(delta) => state.scroll_by(delta),
        Action::Page(pages) => state.scroll_pages(pages),
        Action::Follow => state.follow_latest(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::state::Pane;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn keys_map_to_actions() {
        let cases = [
            (KeyCode::Char('q'), Action::Quit),
            (KeyCode::Tab, Action::FocusNext),
            (KeyCode::Up, Action::Scroll(-1)),
            (KeyCode::Down, Action::Scroll(1)),
            (KeyCode::PageUp, Action::Page(-1)),
            (KeyCode::PageDown, Action::Page(1)),
            (KeyCode::End, Action::Follow),
        ];
        for (code, action) in cases {
            assert_eq!(action_for(press(code)), Some(action), "{code:?}");
        }
        assert_eq!(action_for(press(KeyCode::Char('x'))), None);
    }

    #[test]
    fn ctrl_c_also_quits_because_raw_mode_swallows_the_signal() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_for(key), Some(Action::Quit));
    }

    #[test]
    fn only_presses_are_handled() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut key = press(KeyCode::Char('q'));
            key.kind = kind;
            assert_eq!(action_for(key), None, "{kind:?}");
        }
    }

    #[test]
    fn actions_drive_the_state() {
        let mut state = MonitorState::new(10);
        state.set_viewport(Pane::Chat, 2);
        state.apply(crate::feed::FeedMessage::Snapshot(crate::feed::Snapshot {
            chats: (1..=6).map(crate::monitor::state::tests::chat).collect(),
            ..Default::default()
        }));
        apply(&mut state, Action::Scroll(-1));
        assert_eq!(state.top(Pane::Chat), 3);
        apply(&mut state, Action::Page(-1));
        assert_eq!(state.top(Pane::Chat), 2);
        apply(&mut state, Action::Follow);
        assert!(!state.is_scrolled(Pane::Chat));
        apply(&mut state, Action::FocusNext);
        assert_eq!(state.focus(), Pane::Notifications);
    }
}
