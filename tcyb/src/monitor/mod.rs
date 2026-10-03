//! `tcyb monitor`: `read-chat` の配信口に接続して表示する閲覧専用の TUI。
//!
//! 接続は別タスク（[`client::run`]）が受け持ち、出来事をチャネルで UI ループへ渡す。
//! UI ループはキー入力と出来事のどちらでも起き、接続の試行を待たずに再描画する。

mod client;
mod input;
mod state;
mod ui;

use crate::feed_server::FEED_PATH;
use anyhow::Result;
use client::FeedEvent;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use input::Action;
use ratatui::DefaultTerminal;
use state::{LinkState, MonitorState};
use tokio::sync::mpsc;

/// `127.0.0.1:<port>/feed` へ接続して表示する。`q` で終わる。
///
/// 端末は raw モード・代替画面にし、通常終了・エラー・panic のいずれでも元に戻す。
pub async fn run(port: u16, history_size: usize) -> Result<()> {
    let url = format!("ws://127.0.0.1:{port}{FEED_PATH}");
    let (tx, rx) = mpsc::unbounded_channel();
    let client = tokio::spawn(client::run(url, tx));

    // panic フック（端末を戻してから元のフックを呼ぶ）も入る
    let terminal = ratatui::try_init();
    let _restore = RestoreOnDrop;
    let result = match terminal {
        Ok(mut terminal) => event_loop(&mut terminal, rx, MonitorState::new(history_size)).await,
        Err(e) => Err(e.into()),
    };
    client.abort();
    result
}

/// スコープを抜けるとき（エラーで抜けるときも）端末を元に戻す。
struct RestoreOnDrop;

impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    mut feed: mpsc::UnboundedReceiver<FeedEvent>,
    mut state: MonitorState,
) -> Result<()> {
    let mut keys = EventStream::new();
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut state))?;
        tokio::select! {
            event = keys.next() => match event {
                Some(Ok(Event::Key(key))) => match input::action_for(key) {
                    Some(Action::Quit) => return Ok(()),
                    Some(action) => input::apply(&mut state, action),
                    None => {}
                },
                // リサイズ等は再描画だけ
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
            },
            Some(event) = feed.recv() => apply_feed_event(&mut state, event),
        }
    }
}

fn apply_feed_event(state: &mut MonitorState, event: FeedEvent) {
    match event {
        FeedEvent::Connecting => state.set_link(LinkState::Connecting),
        FeedEvent::Connected => state.set_link(LinkState::Connected),
        FeedEvent::Text(text) => state.apply_text(&text),
        FeedEvent::Disconnected { error, retry_in } => {
            state.set_link(LinkState::Disconnected { error, retry_in });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::FeedMessage;
    use std::time::Duration;

    #[test]
    fn feed_events_update_link_and_contents() {
        let mut state = MonitorState::new(10);
        apply_feed_event(&mut state, FeedEvent::Connected);
        assert_eq!(state.link(), &LinkState::Connected);

        let chat = state::tests::chat(1);
        apply_feed_event(
            &mut state,
            FeedEvent::Text(FeedMessage::Chat(chat).to_json()),
        );
        assert_eq!(state.chats().len(), 1);

        let lost = LinkState::Disconnected {
            error: "closed".into(),
            retry_in: Duration::from_millis(500),
        };
        apply_feed_event(
            &mut state,
            FeedEvent::Disconnected {
                error: "closed".into(),
                retry_in: Duration::from_millis(500),
            },
        );
        assert_eq!(state.link(), &lost);
        // 切れても表示内容は残す（再接続時のスナップショットで置き換わる）
        assert_eq!(state.chats().len(), 1);

        apply_feed_event(&mut state, FeedEvent::Connecting);
        assert_eq!(state.link(), &LinkState::Connecting);
    }
}
