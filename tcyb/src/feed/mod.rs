//! `read-chat` から `monitor` へのローカル配信（メッセージ型・ハブ・通知の要約）。

mod hub;
mod link;
mod message;
mod summary;

pub use hub::{FeedHub, FeedRecvError};
pub use link::{Link, LinkStatus};
pub use message::{ChatLine, Chatter, Chatters, FeedMessage, NotificationLine};

// 受信側（monitor）だけが使う公開 API。monitor の task で使い始めたら外す。
#[allow(unused_imports)]
pub use hub::FeedReceiver;
#[allow(unused_imports)]
pub use message::ConnectionState;
pub use message::{FeedDecodeError, Snapshot, Status, PROTOCOL_VERSION};
#[allow(unused_imports)]
pub use summary::summarize;
