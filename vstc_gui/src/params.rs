use crate::catalog::{spec, ParamKind};
use crate::state::PipelineStep;
use std::collections::HashMap;
use vstreamer_protos::{Operation, OperationRoute};

/// Validate a single non-empty raw value against its declared kind.
pub fn validate_param(kind: ParamKind, raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    match kind {
        ParamKind::Text => Ok(()),
        ParamKind::Int => raw
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| format!("整数を入力してください: '{raw}'")),
        ParamKind::Float => raw
            .parse::<f64>()
            .map(|_| ())
            .map_err(|_| format!("数値を入力してください: '{raw}'")),
        ParamKind::Enum(allowed) => {
            if allowed.contains(&raw) {
                Ok(())
            } else {
                Err(format!(
                    "{} のいずれかを入力してください: '{}'",
                    allowed.join("/"),
                    raw
                ))
            }
        }
    }
}

/// Build the proto `queries` map for one step from its non-empty params.
/// Unknown keys (not in the catalog) are passed through verbatim.
pub fn build_queries(step: &PipelineStep) -> Result<HashMap<String, String>, Vec<String>> {
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    for (key, raw) in &step.params {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match spec(key) {
            Some(s) => match validate_param(s.kind, trimmed) {
                Ok(()) => {
                    out.insert(key.clone(), trimmed.to_string());
                }
                Err(e) => errors.push(format!("{}: {}", s.label, e)),
            },
            None => {
                out.insert(key.clone(), trimmed.to_string());
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Build all proto routes from the pipeline steps. Errors are prefixed by step number.
pub fn build_routes(steps: &[PipelineStep]) -> Result<Vec<OperationRoute>, Vec<String>> {
    let mut routes = Vec::new();
    let mut errors = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        if !step.enabled {
            // Disabled steps become FORWARD: the data passes through this hop
            // unchanged, preserving the pipeline's routing (the remotes and their
            // order). Skipping instead would collapse the chain and change the route.
            // FORWARD ignores params, so a disabled step is never validated.
            routes.push(OperationRoute {
                operation: Operation::Forward.into(),
                remote: step.remote.trim().to_string(),
                queries: HashMap::new(),
            });
            continue;
        }
        match build_queries(step) {
            Ok(queries) => routes.push(OperationRoute {
                operation: step.op.to_proto().into(),
                remote: step.remote.trim().to_string(),
                queries,
            }),
            Err(errs) => {
                for e in errs {
                    errors.push(format!("ステップ {}: {}", idx + 1, e));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(routes)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opkind::OpKind;

    fn step_with(params: &[(&str, &str)]) -> PipelineStep {
        let mut s = PipelineStep::default();
        for (k, v) in params {
            s.params.insert((*k).to_string(), (*v).to_string());
        }
        s
    }

    #[test]
    fn validate_int_rejects_non_numeric() {
        assert!(validate_param(ParamKind::Int, "abc").is_err());
        assert!(validate_param(ParamKind::Int, "3").is_ok());
    }

    #[test]
    fn validate_float_accepts_decimal() {
        assert!(validate_param(ParamKind::Float, "1.1").is_ok());
        assert!(validate_param(ParamKind::Float, "x").is_err());
    }

    #[test]
    fn validate_enum_checks_allowed() {
        assert!(validate_param(ParamKind::Enum(&["s", "n"]), "s").is_ok());
        assert!(validate_param(ParamKind::Enum(&["s", "n"]), "x").is_err());
    }

    #[test]
    fn build_queries_skips_empty_and_keeps_filled() {
        let step = step_with(&[("spd", "1.1"), ("i", ""), ("pit", "  ")]);
        let q = build_queries(&step).expect("ok");
        assert_eq!(q.get("spd").map(String::as_str), Some("1.1"));
        assert!(!q.contains_key("i"));
        assert!(!q.contains_key("pit"));
    }

    #[test]
    fn build_queries_reports_type_error() {
        let step = step_with(&[("i", "abc")]);
        assert!(build_queries(&step).is_err());
    }

    #[test]
    fn build_routes_sets_operation_remote_queries() {
        let mut step = step_with(&[("spd", "1.1")]);
        step.op = OpKind::Tts;
        step.remote = "//localhost:8080".to_string();
        let routes = build_routes(&[step]).expect("ok");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].operation, vstreamer_protos::Operation::Tts as i32);
        assert_eq!(routes[0].remote, "//localhost:8080");
        assert_eq!(
            routes[0].queries.get("spd").map(String::as_str),
            Some("1.1")
        );
    }

    #[test]
    fn build_routes_propagates_error_with_step_index() {
        let step = step_with(&[("i", "x")]);
        let err = build_routes(&[step]).expect_err("err");
        assert!(err[0].contains("ステップ 1"));
    }

    #[test]
    fn build_routes_replaces_disabled_step_with_forward() {
        let mut enabled = step_with(&[("spd", "1.1")]);
        enabled.op = OpKind::Tts;
        let mut disabled = step_with(&[("t", "en")]);
        disabled.op = OpKind::Translate;
        disabled.remote = "//host:9".to_string();
        disabled.enabled = false;
        let routes = build_routes(&[disabled, enabled]).expect("ok");
        // Route topology is preserved: both hops present, in order.
        assert_eq!(routes.len(), 2);
        // Disabled translate -> FORWARD with its remote kept and no queries.
        assert_eq!(
            routes[0].operation,
            vstreamer_protos::Operation::Forward as i32
        );
        assert_eq!(routes[0].remote, "//host:9");
        assert!(routes[0].queries.is_empty());
        // Enabled step keeps its real operation.
        assert_eq!(routes[1].operation, vstreamer_protos::Operation::Tts as i32);
    }

    #[test]
    fn build_routes_disabled_step_with_invalid_params_does_not_error() {
        // A disabled step becomes FORWARD and is never validated, so invalid
        // params must not block the send.
        let mut disabled = step_with(&[("i", "abc")]);
        disabled.enabled = false;
        let routes = build_routes(&[disabled]).expect("disabled invalid step must not error");
        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].operation,
            vstreamer_protos::Operation::Forward as i32
        );
    }
}
