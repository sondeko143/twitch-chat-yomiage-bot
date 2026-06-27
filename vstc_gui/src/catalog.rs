use vstreamer_protos::Operation;

/// The value type of a parameter, used to render the right input widget and to validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Text,
    Int,
    Float,
    Enum(&'static [&'static str]),
}

/// One known query parameter (key/label/type), mirroring server `Params`.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
}

/// The complete, authoritative parameter set (server `Params` class). Flat: any
/// subset is valid on any route; `relevant_keys` only decides default visibility.
pub const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        key: "t",
        label: "翻訳先言語 (t)",
        kind: ParamKind::Text,
    },
    ParamSpec {
        key: "s",
        label: "翻訳元言語 (s)",
        kind: ParamKind::Text,
    },
    ParamSpec {
        key: "p",
        label: "位置 (p)",
        kind: ParamKind::Enum(&["s", "n"]),
    },
    ParamSpec {
        key: "i",
        label: "話者ID (i)",
        kind: ParamKind::Int,
    },
    ParamSpec {
        key: "v",
        label: "音量 (v)",
        kind: ParamKind::Int,
    },
    ParamSpec {
        key: "spd",
        label: "速度 (spd)",
        kind: ParamKind::Float,
    },
    ParamSpec {
        key: "pit",
        label: "ピッチ (pit)",
        kind: ParamKind::Float,
    },
];

/// Look up a parameter spec by its query key.
pub fn spec(key: &str) -> Option<&'static ParamSpec> {
    PARAMS.iter().find(|p| p.key == key)
}

/// Keys shown by default for a command. Best-effort (README + semantics);
/// all other keys remain available via the "その他" expander.
pub fn relevant_keys(op: Operation) -> &'static [&'static str] {
    match op {
        Operation::Translate => &["t", "s"],
        Operation::Tts => &["i", "spd", "pit"],
        Operation::Playback => &["v"],
        Operation::Subtitle => &["p"],
        Operation::Vc => &["i"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_seven_params() {
        assert_eq!(PARAMS.len(), 7);
    }

    #[test]
    fn spec_lookup_returns_kind() {
        assert_eq!(spec("spd").expect("spd").kind, ParamKind::Float);
        assert_eq!(spec("i").expect("i").kind, ParamKind::Int);
        assert!(matches!(spec("p").expect("p").kind, ParamKind::Enum(_)));
        assert!(spec("unknown").is_none());
    }

    #[test]
    fn relevance_matches_design() {
        assert_eq!(relevant_keys(Operation::Translate), &["t", "s"]);
        assert_eq!(relevant_keys(Operation::Tts), &["i", "spd", "pit"]);
        assert_eq!(relevant_keys(Operation::Playback), &["v"]);
        assert!(relevant_keys(Operation::Ping).is_empty());
    }
}
