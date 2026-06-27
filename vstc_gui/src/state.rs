use crate::opkind::OpKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Default for a step's `enabled` flag: new and legacy (pre-`enabled`) steps
/// are enabled, so adding the field never silently disables a user's pipeline.
fn default_enabled() -> bool {
    true
}

/// One pipeline step (maps to a proto `OperationRoute`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub op: OpKind,
    pub remote: String,
    /// Raw text buffers keyed by param key; empty / missing means unset.
    pub params: BTreeMap<String, String>,
    /// Whether this step is sent. Disabled steps are skipped (not validated,
    /// not sent), letting the user keep a step configured but temporarily off.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Default for PipelineStep {
    fn default() -> Self {
        Self {
            op: OpKind::default(),
            remote: String::new(),
            params: BTreeMap::new(),
            enabled: default_enabled(),
        }
    }
}

/// Persisted UI state (serialized via eframe Storage, per OS user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub host: String,
    pub port: u16,
    pub text: String,
    pub steps: Vec<PipelineStep>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
            text: String::new(),
            steps: vec![PipelineStep::default()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_one_step_and_localhost() {
        let s = AppState::default();
        assert_eq!(s.host, "localhost");
        assert_eq!(s.port, 8080);
        assert_eq!(s.steps.len(), 1);
    }

    #[test]
    fn pipeline_step_default_is_enabled() {
        assert!(PipelineStep::default().enabled);
    }

    #[test]
    fn deserialize_without_enabled_field_defaults_true() {
        // Simulates state persisted before the `enabled` field existed: it must
        // load as enabled so an upgrade never silently disables a user's steps.
        let json = r#"{"op":"Tts","remote":"","params":{}}"#;
        let step: PipelineStep = serde_json::from_str(json).expect("deserialize legacy step");
        assert!(step.enabled);
    }

    #[test]
    fn pipeline_step_enabled_false_round_trips() {
        let step = PipelineStep {
            enabled: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&step).expect("serialize");
        let back: PipelineStep = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.enabled);
    }

    #[test]
    fn appstate_serde_round_trip() {
        let mut step = PipelineStep {
            op: OpKind::Tts,
            ..Default::default()
        };
        step.remote = "//localhost:8080".to_string();
        step.params.insert("spd".to_string(), "1.1".to_string());
        let state = AppState {
            host: "h".to_string(),
            port: 1234,
            text: "hi".to_string(),
            steps: vec![step],
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: AppState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.host, "h");
        assert_eq!(back.port, 1234);
        assert_eq!(back.text, "hi");
        assert_eq!(back.steps.len(), 1);
        assert_eq!(back.steps[0].op, OpKind::Tts);
        assert_eq!(back.steps[0].remote, "//localhost:8080");
        assert_eq!(
            back.steps[0].params.get("spd").map(String::as_str),
            Some("1.1")
        );
    }
}
