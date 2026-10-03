//! `read-chat` から `monitor` へのローカル配信（メッセージ型・ハブ・通知の要約）。

mod hub;
mod link;
mod message;
mod summary;

pub use hub::{FeedHub, FeedRecvError};
pub use link::{Link, LinkStatus};
pub use message::{
    ChatLine, Chatter, Chatters, ConnectionState, FeedDecodeError, FeedMessage, NotificationLine,
    Snapshot, Status, PROTOCOL_VERSION,
};

// 型名・関数名ではまだどこからも参照していない公開 API。参照し始めたら外す。
#[allow(unused_imports)]
pub use hub::FeedReceiver;
#[allow(unused_imports)]
pub use summary::summarize;
