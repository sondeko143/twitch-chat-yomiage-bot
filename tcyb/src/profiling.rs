//! 起動レイテンシ計測の足回り。
//! 詳細: docs/superpowers/specs/2026-06-25-startup-latency-profiling-design.md
//!
//! スパン計装は常時コンパイルされるが、実際の計測サブスクライバと重い依存は
//! `profiling` feature 下でのみ有効化される。本体コードは init/mark_ready/
//! wait_for_shutdown の 3 関数だけを使い、feature の有無を意識しない。

/// 計測対象の接続系統。両方が ready になったら起動完了とみなす。
#[derive(Clone, Copy, Debug)]
pub enum Component {
    Irc,
    Event,
}

#[cfg(feature = "profiling")]
mod imp {
    use super::Component;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    /// IRC / EventSub の ready 状態を集約し、両方揃ったら Notify で通知する。
    pub struct ReadyTracker {
        irc: AtomicBool,
        event: AtomicBool,
        notify: Notify,
    }

    impl ReadyTracker {
        pub const fn new() -> Self {
            Self {
                irc: AtomicBool::new(false),
                event: AtomicBool::new(false),
                notify: Notify::const_new(),
            }
        }

        pub fn mark(&self, c: Component) {
            match c {
                Component::Irc => self.irc.store(true, Ordering::SeqCst),
                Component::Event => self.event.store(true, Ordering::SeqCst),
            }
            if self.irc.load(Ordering::SeqCst) && self.event.load(Ordering::SeqCst) {
                // wait 側がまだ待っていなくても、Notify が permit を保持するので取りこぼさない。
                self.notify.notify_one();
            }
        }

        pub async fn wait(&self) {
            self.notify.notified().await;
        }
    }

    static TRACKER: ReadyTracker = ReadyTracker::new();

    pub fn mark_ready(c: Component) {
        TRACKER.mark(c);
    }

    pub async fn wait_for_shutdown() {
        TRACKER.wait().await;
    }

    /// drop でトレースをフラッシュするガード。main 末尾まで保持する。
    pub struct ProfileGuard {
        _chrome: tracing_chrome::FlushGuard,
        _flame: tracing_flame::FlushGuard<std::io::BufWriter<std::fs::File>>,
    }

    pub fn init() -> ProfileGuard {
        use tracing_subscriber::prelude::*;

        std::fs::create_dir_all("target/profile").expect("create target/profile dir");

        let (chrome_layer, chrome_guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file("target/profile/trace.json")
            .build();
        let (flame_layer, flame_guard) =
            tracing_flame::FlameLayer::with_file("target/profile/tracing.folded")
                .expect("open target/profile/tracing.folded");

        tracing_subscriber::registry()
            .with(chrome_layer)
            .with(flame_layer)
            .init();

        ProfileGuard {
            _chrome: chrome_guard,
            _flame: flame_guard,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Component, ReadyTracker};
        use std::time::Duration;

        #[tokio::test]
        async fn wait_pends_until_both_marked() {
            let t = ReadyTracker::new();
            t.mark(Component::Irc);
            // IRC だけ ready → wait は完了してはならない
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_err(), "wait should still pend with only Irc marked");
        }

        #[tokio::test]
        async fn wait_completes_when_both_marked() {
            let t = ReadyTracker::new();
            t.mark(Component::Irc);
            t.mark(Component::Event);
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_ok(), "wait should complete when both marked");
        }

        #[tokio::test]
        async fn mark_order_does_not_matter() {
            let t = ReadyTracker::new();
            t.mark(Component::Event);
            t.mark(Component::Irc);
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_ok(), "order of marks should not matter");
        }
    }
}

#[cfg(not(feature = "profiling"))]
mod imp {
    use super::Component;

    /// 非 profiling ビルドの no-op ガード（Drop なし・フィールドなし）。
    pub struct ProfileGuard;

    pub fn init() -> ProfileGuard {
        ProfileGuard
    }

    #[inline]
    pub fn mark_ready(_c: Component) {}

    pub async fn wait_for_shutdown() {
        // 非 profiling では決して完了しない（select 分岐を発火させない）。
        std::future::pending::<()>().await
    }
}

pub use imp::{init, mark_ready, wait_for_shutdown, ProfileGuard};
