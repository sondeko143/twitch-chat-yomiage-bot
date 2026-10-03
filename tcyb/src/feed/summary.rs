//! 通知の種別と event から、表示用の 1 行要約を作る。

use serde_json::Value;

/// 通知の表示用 1 行要約。follow / raid / chat.notification は人が読める文にし、
/// それ以外は種別名を含む 1 行にする。
pub fn summarize(subscription_type: &str, event: &Value) -> String {
    let line = match subscription_type {
        "channel.follow" => format!("{} さんがフォローしました", text(event, "user_name")),
        "channel.raid" => format!(
            "{} さんが {} 人でレイドしました",
            text(event, "from_broadcaster_user_name"),
            text(event, "viewers")
        ),
        "channel.chat.notification" => chat_notification(event),
        other => format!("{other} の通知"),
    };
    one_line(&line)
}

/// `channel.chat.notification` の要約。`notice_type` ごとの詳細は同名のオブジェクトにある。
fn chat_notification(event: &Value) -> String {
    let notice_type = text(event, "notice_type");
    let detail = event.get(notice_type.as_str()).unwrap_or(&Value::Null);
    let chatter = if event.get("chatter_is_anonymous") == Some(&Value::Bool(true)) {
        "匿名".to_string()
    } else {
        text(event, "chatter_user_name")
    };
    match notice_type.as_str() {
        "sub" => format!("{chatter} さんがサブスクしました ({})", plan(detail)),
        "resub" => format!(
            "{chatter} さんが {} か月目のサブスクを継続しました ({})",
            text(detail, "cumulative_months"),
            plan(detail)
        ),
        "sub_gift" => format!(
            "{chatter} さんが {} さんにサブスクをギフトしました ({})",
            text(detail, "recipient_user_name"),
            plan(detail)
        ),
        "community_sub_gift" => format!(
            "{chatter} さんがサブスクを {} 個ギフトしました ({})",
            text(detail, "total"),
            plan(detail)
        ),
        "raid" => format!(
            "{} さんが {} 人でレイドしました",
            text(detail, "user_name"),
            text(detail, "viewer_count")
        ),
        "announcement" => format!("{chatter} さんのアナウンス"),
        "bits_badge_tier" => format!(
            "{chatter} さんが Bits バッジ {} に到達しました",
            text(detail, "tier")
        ),
        _ => match event.get("system_message").and_then(Value::as_str) {
            Some(m) if !m.trim().is_empty() => m.to_string(),
            _ => format!("channel.chat.notification ({notice_type}): {chatter}"),
        },
    }
}

/// サブスクの種類。Prime なら `Prime`、それ以外は `sub_tier`（"1000" 等）から `Tier N`。
fn plan(detail: &Value) -> String {
    if detail.get("is_prime") == Some(&Value::Bool(true)) {
        return "Prime".to_string();
    }
    match detail.get("sub_tier").and_then(Value::as_str) {
        Some("1000") => "Tier 1".to_string(),
        Some("2000") => "Tier 2".to_string(),
        Some("3000") => "Tier 3".to_string(),
        Some(other) => other.to_string(),
        None => "?".to_string(),
    }
}

/// フィールドの表示用文字列。文字列はそのまま、それ以外は JSON 表記、無ければ `?`。
fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "?".to_string(),
        Some(other) => other.to_string(),
    }
}

/// 表示が崩れないよう、改行を空白にして 1 行にする。
fn one_line(s: &str) -> String {
    s.split(['\r', '\n'])
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn follow() {
        let s = summarize("channel.follow", &json!({"user_name": "太郎"}));
        assert_eq!(s, "太郎 さんがフォローしました");
    }

    #[test]
    fn raid() {
        let s = summarize(
            "channel.raid",
            &json!({"from_broadcaster_user_name": "花子", "viewers": 12}),
        );
        assert_eq!(s, "花子 さんが 12 人でレイドしました");
    }

    fn chat_notification(notice_type: &str, extra: Value) -> String {
        let mut event = json!({
            "chatter_user_name": "太郎",
            "chatter_is_anonymous": false,
            "notice_type": notice_type,
            "system_message": "system says hi",
        });
        event[notice_type] = extra;
        summarize("channel.chat.notification", &event)
    }

    #[test]
    fn chat_notification_known_notice_types() {
        assert_eq!(
            chat_notification("sub", json!({"sub_tier": "1000", "is_prime": false})),
            "太郎 さんがサブスクしました (Tier 1)"
        );
        assert_eq!(
            chat_notification("sub", json!({"sub_tier": "1000", "is_prime": true})),
            "太郎 さんがサブスクしました (Prime)"
        );
        assert_eq!(
            chat_notification("resub", json!({"sub_tier": "2000", "cumulative_months": 7})),
            "太郎 さんが 7 か月目のサブスクを継続しました (Tier 2)"
        );
        assert_eq!(
            chat_notification(
                "sub_gift",
                json!({"sub_tier": "1000", "recipient_user_name": "花子"})
            ),
            "太郎 さんが 花子 さんにサブスクをギフトしました (Tier 1)"
        );
        assert_eq!(
            chat_notification(
                "community_sub_gift",
                json!({"sub_tier": "3000", "total": 5})
            ),
            "太郎 さんがサブスクを 5 個ギフトしました (Tier 3)"
        );
        assert_eq!(
            chat_notification("raid", json!({"user_name": "次郎", "viewer_count": 30})),
            "次郎 さんが 30 人でレイドしました"
        );
        assert_eq!(
            chat_notification("announcement", json!({"color": "BLUE"})),
            "太郎 さんのアナウンス"
        );
        assert_eq!(
            chat_notification("bits_badge_tier", json!({"tier": 1000})),
            "太郎 さんが Bits バッジ 1000 に到達しました"
        );
    }

    #[test]
    fn chat_notification_anonymous_chatter() {
        let event = json!({
            "chatter_user_name": "ananonymousgifter",
            "chatter_is_anonymous": true,
            "notice_type": "sub_gift",
            "sub_gift": {"sub_tier": "1000", "recipient_user_name": "花子"},
        });
        assert_eq!(
            summarize("channel.chat.notification", &event),
            "匿名 さんが 花子 さんにサブスクをギフトしました (Tier 1)"
        );
    }

    #[test]
    fn chat_notification_unknown_notice_type_uses_system_message() {
        assert_eq!(
            chat_notification("charity_donation", json!({})),
            "system says hi"
        );
    }

    #[test]
    fn chat_notification_unknown_notice_type_without_system_message() {
        let event = json!({"chatter_user_name": "太郎", "notice_type": "brand_new"});
        assert_eq!(
            summarize("channel.chat.notification", &event),
            "channel.chat.notification (brand_new): 太郎"
        );
    }

    #[test]
    fn unknown_subscription_type_contains_type_name() {
        let s = summarize("channel.cheer", &json!({"user_name": "太郎", "bits": 100}));
        assert!(s.contains("channel.cheer"), "{s}");
        assert!(!s.contains('\n'), "{s}");
    }

    #[test]
    fn missing_fields_still_yield_one_line() {
        for t in [
            "channel.follow",
            "channel.raid",
            "channel.chat.notification",
            "x.y",
        ] {
            let s = summarize(t, &Value::Null);
            assert!(!s.is_empty(), "{t}");
            assert!(!s.contains('\n'), "{t}: {s}");
        }
    }
}
