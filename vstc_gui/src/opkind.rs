use serde::{Deserialize, Serialize};
use std::fmt;
use vstreamer_protos::Operation;

/// Local, serializable mirror of proto `Operation` (which lacks `Serialize`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    Transcribe,
    Translate,
    Subtitle,
    #[default]
    Tts,
    Vc,
    Playback,
    Pause,
    Resume,
    Reload,
    SetFilters,
    Ping,
    Forward,
}

impl OpKind {
    /// Every variant, for populating the command dropdown.
    pub const ALL: [OpKind; 12] = [
        OpKind::Transcribe,
        OpKind::Translate,
        OpKind::Subtitle,
        OpKind::Tts,
        OpKind::Vc,
        OpKind::Playback,
        OpKind::Pause,
        OpKind::Resume,
        OpKind::Reload,
        OpKind::SetFilters,
        OpKind::Ping,
        OpKind::Forward,
    ];

    /// Convert to the proto enum used on the wire.
    pub fn to_proto(self) -> Operation {
        match self {
            OpKind::Transcribe => Operation::Transcribe,
            OpKind::Translate => Operation::Translate,
            OpKind::Subtitle => Operation::Subtitle,
            OpKind::Tts => Operation::Tts,
            OpKind::Vc => Operation::Vc,
            OpKind::Playback => Operation::Playback,
            OpKind::Pause => Operation::Pause,
            OpKind::Resume => Operation::Resume,
            OpKind::Reload => Operation::Reload,
            OpKind::SetFilters => Operation::SetFilters,
            OpKind::Ping => Operation::Ping,
            OpKind::Forward => Operation::Forward,
        }
    }

    /// Human-facing dropdown label.
    pub fn label(self) -> &'static str {
        match self {
            OpKind::Transcribe => "文字起こし (transcribe)",
            OpKind::Translate => "翻訳 (translate)",
            OpKind::Subtitle => "字幕 (subtitle)",
            OpKind::Tts => "読み上げ (tts)",
            OpKind::Vc => "声質変換 (vc)",
            OpKind::Playback => "再生 (playback)",
            OpKind::Pause => "一時停止 (pause)",
            OpKind::Resume => "再開 (resume)",
            OpKind::Reload => "リロード (reload)",
            OpKind::SetFilters => "フィルタ設定 (set_filters)",
            OpKind::Ping => "ping",
            OpKind::Forward => "転送 (forward)",
        }
    }
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_proto_maps_each_variant() {
        let pairs = [
            (OpKind::Transcribe, Operation::Transcribe),
            (OpKind::Translate, Operation::Translate),
            (OpKind::Subtitle, Operation::Subtitle),
            (OpKind::Tts, Operation::Tts),
            (OpKind::Vc, Operation::Vc),
            (OpKind::Playback, Operation::Playback),
            (OpKind::Pause, Operation::Pause),
            (OpKind::Resume, Operation::Resume),
            (OpKind::Reload, Operation::Reload),
            (OpKind::SetFilters, Operation::SetFilters),
            (OpKind::Ping, Operation::Ping),
            (OpKind::Forward, Operation::Forward),
        ];
        // Guard: the pair table must cover every variant, so no arm escapes the check.
        assert_eq!(pairs.len(), OpKind::ALL.len());
        for (kind, op) in pairs {
            assert_eq!(kind.to_proto(), op);
        }
    }

    #[test]
    fn all_has_twelve_unique_variants() {
        assert_eq!(OpKind::ALL.len(), 12);
    }

    #[test]
    fn label_is_non_empty_for_all() {
        for op in OpKind::ALL {
            assert!(!op.label().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        for op in OpKind::ALL {
            let json = serde_json::to_string(&op).expect("serialize");
            let back: OpKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, op);
        }
    }
}
