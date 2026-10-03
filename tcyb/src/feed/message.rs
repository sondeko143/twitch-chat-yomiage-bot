//! 配信口で流すメッセージの型。送信側（`read-chat`）と受信側（`monitor`）で共有する。
//!
//! JSON では `{"v":1,"kind":"chat", ...}` の形になる。`kind` は `snapshot` / `chat` /
//! `notification` / `chatters` / `status` のいずれか。

use super::summary::summarize;
use crate::eventsub::NotificationEvent;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// プロトコル版。全メッセージの `v` に入る。
pub const PROTOCOL_VERSION: u32 = 1;

/// 配信口で流すメッセージ。直列化すると `v`（[`PROTOCOL_VERSION`]）と `kind` を持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "Envelope", try_from = "Envelope")]
pub enum FeedMessage {
    Snapshot(Snapshot),
    Chat(ChatLine),
    Notification(NotificationLine),
    Chatters(Chatters),
    Status(Status),
}

/// チャット 1 件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatLine {
    pub received_at: DateTime<Utc>,
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// EventSub の通知 1 件。`summary` は表示用の 1 行要約、`event` は元の JSON。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationLine {
    pub received_at: DateTime<Utc>,
    pub subscription_type: String,
    pub summary: String,
    pub event: Value,
}

impl From<NotificationEvent> for NotificationLine {
    fn from(n: NotificationEvent) -> Self {
        let summary = summarize(&n.subscription_type, &n.event);
        Self {
            received_at: n.received_at,
            subscription_type: n.subscription_type,
            summary,
            event: n.event,
        }
    }
}

/// 視聴者一覧（全量）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chatters {
    pub fetched_at: DateTime<Utc>,
    pub chatters: Vec<Chatter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chatter {
    pub login: String,
    pub display_name: String,
}

/// 接続の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Connecting,
    Connected,
    Disconnected,
}

/// IRC / EventSub の接続状態と、視聴者一覧の取得状況。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Status {
    pub irc: ConnectionState,
    pub eventsub: ConnectionState,
    /// 視聴者一覧の最終成功時刻。
    #[serde(default)]
    pub chatters_last_success: Option<DateTime<Utc>>,
    /// 視聴者一覧の直近の失敗理由。
    #[serde(default)]
    pub chatters_last_error: Option<String>,
}

/// 購読開始時・取りこぼし後に送る現在の状態。`chats` / `notifications` は古い順。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub chats: Vec<ChatLine>,
    pub notifications: Vec<NotificationLine>,
    #[serde(default)]
    pub chatters: Option<Chatters>,
    pub status: Status,
}

/// 受信した JSON を解釈できなかった理由。
#[derive(Debug, Error)]
pub enum FeedDecodeError {
    /// `v` が [`PROTOCOL_VERSION`] と異なる（非互換）。
    #[error("incompatible feed protocol version {0} (expected {PROTOCOL_VERSION})")]
    Incompatible(u64),
    #[error("malformed feed message: {0}")]
    Malformed(#[from] serde_json::Error),
}

impl FeedMessage {
    /// JSON テキストフレームへ直列化する。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("FeedMessage is always serializable")
    }

    /// JSON テキストフレームを解釈する。版が違えば [`FeedDecodeError::Incompatible`]。
    #[allow(dead_code)] // 受信側（monitor）の task で使う
    pub fn from_json(text: &str) -> Result<Self, FeedDecodeError> {
        #[derive(Deserialize)]
        struct VersionOnly {
            v: u64,
        }
        let VersionOnly { v } = serde_json::from_str(text)?;
        if v != u64::from(PROTOCOL_VERSION) {
            return Err(FeedDecodeError::Incompatible(v));
        }
        Ok(serde_json::from_str(text)?)
    }
}

/// 直列化の形。`v` と、`kind` で区別する本体を平坦に並べる。
#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    #[serde(flatten)]
    body: Body,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Body {
    Snapshot(Snapshot),
    Chat(ChatLine),
    Notification(NotificationLine),
    Chatters(Chatters),
    Status(Status),
}

impl From<FeedMessage> for Envelope {
    fn from(m: FeedMessage) -> Self {
        let body = match m {
            FeedMessage::Snapshot(x) => Body::Snapshot(x),
            FeedMessage::Chat(x) => Body::Chat(x),
            FeedMessage::Notification(x) => Body::Notification(x),
            FeedMessage::Chatters(x) => Body::Chatters(x),
            FeedMessage::Status(x) => Body::Status(x),
        };
        Self {
            v: PROTOCOL_VERSION,
            body,
        }
    }
}

impl TryFrom<Envelope> for FeedMessage {
    type Error = FeedDecodeError;

    fn try_from(e: Envelope) -> Result<Self, Self::Error> {
        if e.v != PROTOCOL_VERSION {
            return Err(FeedDecodeError::Incompatible(u64::from(e.v)));
        }
        Ok(match e.body {
            Body::Snapshot(x) => Self::Snapshot(x),
            Body::Chat(x) => Self::Chat(x),
            Body::Notification(x) => Self::Notification(x),
            Body::Chatters(x) => Self::Chatters(x),
            Body::Status(x) => Self::Status(x),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 123_456_789).unwrap()
    }

    fn chat() -> ChatLine {
        ChatLine {
            received_at: at(1),
            user_login: "taro".into(),
            display_name: "太郎".into(),
            text: "こんにちは".into(),
            color: Some("#FF0000".into()),
        }
    }

    fn notification() -> NotificationLine {
        NotificationLine {
            received_at: at(2),
            subscription_type: "channel.follow".into(),
            summary: "太郎 さんがフォローしました".into(),
            event: json!({"user_name": "太郎", "nested": {"n": 3}}),
        }
    }

    fn chatters() -> Chatters {
        Chatters {
            fetched_at: at(3),
            chatters: vec![Chatter {
                login: "taro".into(),
                display_name: "太郎".into(),
            }],
        }
    }

    fn status() -> Status {
        Status {
            irc: ConnectionState::Connected,
            eventsub: ConnectionState::Disconnected,
            chatters_last_success: Some(at(4)),
            chatters_last_error: Some("401".into()),
        }
    }

    fn all_messages() -> Vec<(FeedMessage, &'static str)> {
        vec![
            (
                FeedMessage::Snapshot(Snapshot {
                    chats: vec![chat()],
                    notifications: vec![notification()],
                    chatters: Some(chatters()),
                    status: status(),
                }),
                "snapshot",
            ),
            (FeedMessage::Snapshot(Snapshot::default()), "snapshot"),
            (FeedMessage::Chat(chat()), "chat"),
            (
                FeedMessage::Chat(ChatLine {
                    color: None,
                    ..chat()
                }),
                "chat",
            ),
            (FeedMessage::Notification(notification()), "notification"),
            (FeedMessage::Chatters(chatters()), "chatters"),
            (FeedMessage::Status(status()), "status"),
            (FeedMessage::Status(Status::default()), "status"),
        ]
    }

    #[test]
    fn every_message_round_trips_with_version_and_kind() {
        for (msg, kind) in all_messages() {
            let text = msg.to_json();
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["v"], json!(1), "{text}");
            assert_eq!(value["kind"], json!(kind), "{text}");
            assert_eq!(FeedMessage::from_json(&text).unwrap(), msg, "{text}");
            assert_eq!(serde_json::from_str::<FeedMessage>(&text).unwrap(), msg);
        }
    }

    #[test]
    fn chat_fields_are_flat_next_to_kind() {
        let value: Value = serde_json::from_str(&FeedMessage::Chat(chat()).to_json()).unwrap();
        assert_eq!(value["user_login"], json!("taro"));
        assert_eq!(value["display_name"], json!("太郎"));
        assert_eq!(value["text"], json!("こんにちは"));
        assert_eq!(value["color"], json!("#FF0000"));
    }

    #[test]
    fn other_version_is_reported_as_incompatible() {
        let text = r#"{"v":2,"kind":"chat"}"#;
        assert!(matches!(
            FeedMessage::from_json(text),
            Err(FeedDecodeError::Incompatible(2))
        ));
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            FeedMessage::from_json("not json"),
            Err(FeedDecodeError::Malformed(_))
        ));
        assert!(matches!(
            FeedMessage::from_json(r#"{"v":1,"kind":"unknown"}"#),
            Err(FeedDecodeError::Malformed(_))
        ));
    }

    #[test]
    fn notification_line_is_built_from_event_with_summary() {
        let event = NotificationEvent {
            subscription_type: "channel.follow".into(),
            event: json!({"user_name": "太郎"}),
            received_at: at(5),
        };
        let line = NotificationLine::from(event.clone());
        assert_eq!(line.received_at, event.received_at);
        assert_eq!(line.subscription_type, "channel.follow");
        assert_eq!(line.event, event.event);
        assert_eq!(line.summary, summarize("channel.follow", &event.event));
    }
}
