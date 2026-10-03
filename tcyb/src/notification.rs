//! 通知の種別ごとの読み上げテンプレートの選択と置換。

use crate::settings::NotificationSpeech;
use serde_json::Value;

/// 通知の種別と event から読み上げ文を作る。一致するテンプレートが無ければ `None`。
///
/// `type` と `notice_type` の両方が一致する要素を、`notice_type` 未指定の要素より優先する。
pub fn render_speech(
    templates: &[NotificationSpeech],
    type_: &str,
    event: &Value,
) -> Option<String> {
    let notice_type = event.get("notice_type").and_then(Value::as_str);
    let of_type = || templates.iter().filter(|t| t.type_ == type_);
    let chosen = of_type()
        .find(|t| t.notice_type.is_some() && t.notice_type.as_deref() == notice_type)
        .or_else(|| of_type().find(|t| t.notice_type.is_none()))?;
    Some(expand(&chosen.template, event))
}

/// テンプレート中の `{a.b}` を event の値に置き換える。`{{` / `}}` は波括弧そのもの。
fn expand(template: &str, event: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == '}' {
                        closed = true;
                        break;
                    }
                    name.push(n);
                }
                if closed {
                    out.push_str(&field_text(event, &name));
                } else {
                    // 閉じ括弧が無い場合は文字どおり出力する
                    out.push('{');
                    out.push_str(&name);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn field_text(event: &Value, path: &str) -> String {
    let found = path.split('.').try_fold(event, |v, key| v.get(key));
    match found {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => {
            log::warn!("notification template field not found: {path}");
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(type_: &str, notice_type: Option<&str>, template: &str) -> NotificationSpeech {
        NotificationSpeech {
            type_: type_.to_string(),
            notice_type: notice_type.map(str::to_string),
            template: template.to_string(),
        }
    }

    #[test]
    fn replaces_same_name_field() {
        let tpl = [t("channel.follow", None, "{user_name} さん、ありがとう")];
        let got = render_speech(&tpl, "channel.follow", &json!({"user_name": "taro"}));
        assert_eq!(got.as_deref(), Some("taro さん、ありがとう"));
    }

    #[test]
    fn replaces_nested_field() {
        let tpl = [t("x", None, "{a.b}!")];
        let got = render_speech(&tpl, "x", &json!({"a": {"b": "deep"}}));
        assert_eq!(got.as_deref(), Some("deep!"));
    }

    #[test]
    fn escapes_braces() {
        let tpl = [t("x", None, "{{{n}}} {{n}}")];
        let got = render_speech(&tpl, "x", &json!({"n": "v"}));
        assert_eq!(got.as_deref(), Some("{v} {n}"));
    }

    #[test]
    fn missing_field_becomes_empty() {
        let tpl = [t("x", None, "[{nope}][{a.nope}][{n.deeper}]")];
        let got = render_speech(&tpl, "x", &json!({"a": {}, "n": "str"}));
        assert_eq!(got.as_deref(), Some("[][][]"));
    }

    #[test]
    fn non_string_values_use_json_notation() {
        let tpl = [t("x", None, "{viewers}/{ok}/{nothing}")];
        let got = render_speech(
            &tpl,
            "x",
            &json!({"viewers": 42, "ok": true, "nothing": null}),
        );
        assert_eq!(got.as_deref(), Some("42/true/null"));
    }

    #[test]
    fn unclosed_brace_is_kept_literally() {
        let tpl = [t("x", None, "a{b")];
        assert_eq!(render_speech(&tpl, "x", &json!({})).as_deref(), Some("a{b"));
    }

    #[test]
    fn exact_notice_type_beats_unspecified() {
        let tpl = [
            t("channel.chat.notification", None, "generic"),
            t("channel.chat.notification", Some("sub"), "sub only"),
        ];
        let got = render_speech(
            &tpl,
            "channel.chat.notification",
            &json!({"notice_type": "sub"}),
        );
        assert_eq!(got.as_deref(), Some("sub only"));
    }

    #[test]
    fn unspecified_notice_type_matches_any() {
        let tpl = [
            t("channel.chat.notification", Some("sub"), "sub only"),
            t("channel.chat.notification", None, "generic"),
        ];
        let got = render_speech(
            &tpl,
            "channel.chat.notification",
            &json!({"notice_type": "raid"}),
        );
        assert_eq!(got.as_deref(), Some("generic"));
        let none = render_speech(&tpl, "channel.chat.notification", &json!({}));
        assert_eq!(none.as_deref(), Some("generic"));
    }

    #[test]
    fn no_match_returns_none() {
        let tpl = [
            t("channel.follow", None, "f"),
            t("channel.chat.notification", Some("sub"), "s"),
        ];
        assert_eq!(render_speech(&tpl, "channel.raid", &json!({})), None);
        assert_eq!(
            render_speech(
                &tpl,
                "channel.chat.notification",
                &json!({"notice_type": "raid"})
            ),
            None
        );
        assert_eq!(render_speech(&[], "channel.follow", &json!({})), None);
    }
}
