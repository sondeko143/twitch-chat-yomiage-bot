//! `read-chat` から `monitor` へのローカル配信（メッセージ型・ハブ・通知の要約）。

mod hub;
mod message;
mod summary;

pub use hub::{FeedHub, FeedReceiver, FeedRecvError};
pub use message::{
    ChatLine, Chatter, Chatters, ConnectionState, FeedDecodeError, FeedMessage, NotificationLine,
    Snapshot, Status, PROTOCOL_VERSION,
};
pub use summary::summarize;
