//! This is the vstreamer-tool's client library

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tonic::transport::Channel;
use url::Url;
use uuid::Uuid;
use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operand, Operation, OperationChain, OperationRoute,
    Response, Sound,
};

const CONNECT_TIMEOUT_SECS: u64 = 5;
const RPC_TIMEOUT_SECS: u64 = 10;

/// Scheme prefix that turns a scheme-less route string into an absolute URL.
/// It carries no meaning of its own: `Url::parse` only accepts absolute URLs.
const ROUTE_SCHEME: &str = "o:";

/// All possible errors returned by this library.
#[derive(Error, Debug)]
pub enum VstcError {
    /// Invalid operation string given
    #[error("invalid operation string {op_str:?}")]
    OpConvertError {
        /// given parameter
        op_str: String,
    },

    /// Connection error
    #[error(transparent)]
    TransportError(Box<tonic::transport::Error>),

    /// Send error
    #[error(transparent)]
    StatusError(Box<tonic::Status>),

    /// Operation parse error
    #[error(transparent)]
    UrlError(#[from] url::ParseError),
}

impl From<tonic::transport::Error> for VstcError {
    fn from(value: tonic::transport::Error) -> Self {
        Self::TransportError(Box::new(value))
    }
}

impl From<tonic::Status> for VstcError {
    fn from(value: tonic::Status) -> Self {
        Self::StatusError(Box::new(value))
    }
}

/// Send the command to the channel.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
/// * The given operations' strings can not convert to.
pub async fn process_command(
    uri: &str,
    operations: &[String],
    text: String,
    sound: Option<Sound>,
    file_path: Option<String>,
    filters: Option<Vec<String>>,
) -> Result<Response, VstcError> {
    let routes: Result<Vec<_>, _> = operations
        .iter()
        .map(String::as_ref)
        .map(parse_route)
        .collect();
    process_chains_with_operand(
        uri,
        vec![routes?],
        RouteOperand {
            text,
            sound,
            file_path: file_path.unwrap_or_default(),
            filters: filters.unwrap_or_default(),
        },
    )
    .await
}

/// Optional [`Operand`] fields for [`process_routes_with_operand`].
///
/// `trace_id` and `origin_ts` are generated inside this crate, so callers never
/// fill them in. Every field has a `Default`, letting a caller set only what its
/// operation needs — e.g. only `file_path` for `Reload`.
#[derive(Debug, Default, Clone)]
pub struct RouteOperand {
    /// Text payload.
    pub text: String,
    /// Path of the config file to act on (used by `Reload`).
    pub file_path: String,
    /// Filter names (used by `SetFilters`).
    pub filters: Vec<String>,
    /// Raw sound payload.
    pub sound: Option<Sound>,
}

/// Build a proto `Operand` from the caller-visible fields, stamping a fresh
/// trace id and the current origin timestamp.
fn build_operand(operand: RouteOperand) -> Operand {
    Operand {
        text: operand.text,
        sound: operand.sound,
        file_path: operand.file_path,
        filters: operand.filters,
        trace_id: Uuid::new_v4().to_string(),
        origin_ts: unix_timestamp_secs(),
    }
}

/// Wrap the given chains into a `Command` carrying a single shared `operand`.
///
/// Every chain sees the same input and the same trace id (ADR-0018).
fn build_command(chains: Vec<Vec<OperationRoute>>, operand: RouteOperand) -> Command {
    Command {
        chains: chains
            .into_iter()
            .map(|operations| OperationChain { operations })
            .collect(),
        operand: Some(build_operand(operand)),
    }
}

/// Connect to `uri` with this crate's connect/RPC timeouts applied.
async fn connect(uri: &str) -> Result<CommanderClient<Channel>, VstcError> {
    let endpoint = tonic::transport::Endpoint::new(uri.to_string())?
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS));
    Ok(CommanderClient::connect(endpoint).await?)
}

/// Send pre-built operation routes with a text operand to the channel.
///
/// Unlike [`process_command`], this takes already-structured [`OperationRoute`]
/// values instead of URL-style operation strings, so callers (e.g. a GUI) that
/// already have separated destination/command/parameter fields don't round-trip
/// through string parsing.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_routes(
    uri: &str,
    routes: Vec<OperationRoute>,
    text: String,
) -> Result<Response, VstcError> {
    process_routes_with_operand(
        uri,
        routes,
        RouteOperand {
            text,
            ..RouteOperand::default()
        },
    )
    .await
}

/// Send pre-built operation routes together with the operand fields in
/// `operand`.
///
/// [`process_routes`] covers the text-only case; use this when an operation
/// needs another operand field, such as `Reload` needing `file_path`.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_routes_with_operand(
    uri: &str,
    routes: Vec<OperationRoute>,
    operand: RouteOperand,
) -> Result<Response, VstcError> {
    process_chains_with_operand(uri, vec![routes], operand).await
}

/// Send several operation chains together, sharing one operand.
///
/// [`process_routes_with_operand`] covers the single-chain case. Use this when
/// one input fans out to several destinations: the chains travel in a single
/// `Command`, so the server sees one request and every chain shares the same
/// `trace_id` (ADR-0018).
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_chains_with_operand(
    uri: &str,
    chains: Vec<Vec<OperationRoute>>,
    operand: RouteOperand,
) -> Result<Response, VstcError> {
    let mut channel = connect(uri).await?;
    let c = tonic::Request::new(build_command(chains, operand));
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}

/// Current wall-clock time as fractional seconds since the Unix epoch.
///
/// Used as the telemetry origin timestamp. Returns `0.0` if the system clock is
/// set before the Unix epoch, so command sending never fails on a clock error.
fn unix_timestamp_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// True when `s` already starts with a URL scheme (`scheme:`).
///
/// RFC 3986 spells a scheme as an ASCII letter followed by letters, digits,
/// `+`, `-` or `.`, terminated by `:`. Checked by hand so this crate does not
/// grow a regex dependency. Guarding on the whole prefix (not just "there is a
/// colon") keeps a query value like `?u=http://x` from being mistaken for one.
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let mut chars = s[..colon].chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Make `op_str` absolute so `Url::parse` accepts it (ADR-0018).
///
/// * `//host:port/op?q` gains `o:`
/// * `scheme:...` is left alone, which covers the historical `o:/op` form
/// * anything else gains `o:/`, so `op?q` becomes `o:/op?q`
fn normalize_op_str(op_str: &str) -> Cow<'_, str> {
    if has_scheme(op_str) {
        Cow::Borrowed(op_str)
    } else if op_str.starts_with("//") {
        Cow::Owned(format!("{ROUTE_SCHEME}{op_str}"))
    } else {
        Cow::Owned(format!("{ROUTE_SCHEME}/{op_str}"))
    }
}

/// Parse one route string into a proto [`OperationRoute`].
///
/// Accepts `//host:port/op?query`, a bare `op?query`, and any absolute URL
/// (including the historical `o:/op` form). Public so a caller can validate a
/// route before storing it, without sending anything (ADR-0018).
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * The normalized string is not a parsable URL.
/// * The path names an operation this crate does not know.
pub fn parse_route(op_str: &str) -> Result<OperationRoute, VstcError> {
    let normalized = normalize_op_str(op_str);
    let parsed = Url::parse(&normalized)?;
    let hash_query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    let operation = match parsed.path().strip_prefix('/').unwrap_or_default() {
        "transc" | "transcribe" => Ok(Operation::Transcribe),
        "transl" | "translate" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "play" | "playback" => Ok(Operation::Playback),
        "sub" | "subtitle" => Ok(Operation::Subtitle),
        "vc" => Ok(Operation::Vc),
        "reload" => Ok(Operation::Reload),
        "pause" => Ok(Operation::Pause),
        "resume" => Ok(Operation::Resume),
        "forward" | "fwd" => Ok(Operation::Forward),
        _ => Err(VstcError::OpConvertError {
            op_str: String::from(op_str),
        }),
    };
    let remote = match parsed.host_str() {
        Some(host) => format!(
            "//{}{}",
            host,
            match parsed.port_or_known_default() {
                Some(port) => format!(":{port}"),
                None => String::new(),
            }
        ),
        None => String::new(),
    };
    Ok(OperationRoute {
        operation: operation?.into(),
        remote,
        queries: hash_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_command_wraps_routes_in_single_chain() {
        let routes = vec![OperationRoute {
            operation: Operation::Tts as i32,
            remote: String::new(),
            queries: HashMap::new(),
        }];
        let cmd = build_command(
            vec![routes.clone()],
            RouteOperand {
                text: "hello".to_string(),
                ..RouteOperand::default()
            },
        );
        assert_eq!(cmd.chains.len(), 1);
        assert_eq!(cmd.chains[0].operations, routes);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hello");
        assert!(operand.sound.is_none());
        assert!(!operand.trace_id.is_empty());
    }

    #[test]
    fn build_command_carries_optional_operand_fields() {
        let cmd = build_command(
            vec![Vec::new()],
            RouteOperand {
                file_path: "conf.yml".to_string(),
                filters: vec!["a".to_string()],
                ..RouteOperand::default()
            },
        );
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.file_path, "conf.yml");
        assert_eq!(operand.filters, vec!["a".to_string()]);
        assert!(operand.text.is_empty());
        assert!(operand.origin_ts > 0.0);
    }

    #[test]
    fn build_command_keeps_each_chain_separate() {
        let route = |op: Operation, remote: &str| OperationRoute {
            operation: op as i32,
            remote: remote.to_string(),
            queries: HashMap::new(),
        };
        let first = vec![route(Operation::Transcribe, "//h1:1")];
        let second = vec![
            route(Operation::Translate, ""),
            route(Operation::Subtitle, "//h2:2"),
        ];
        let cmd = build_command(vec![first.clone(), second.clone()], RouteOperand::default());
        assert_eq!(cmd.chains.len(), 2);
        assert_eq!(cmd.chains[0].operations, first, "chain order must be kept");
        assert_eq!(cmd.chains[1].operations, second);
    }

    #[test]
    fn build_command_shares_one_operand_across_chains() {
        // ADR-0018: N 本のチェーンは 1 つの入力を共有する。trace_id も 1 つ。
        let cmd = build_command(
            vec![Vec::new(), Vec::new(), Vec::new()],
            RouteOperand {
                text: "hi".to_string(),
                ..RouteOperand::default()
            },
        );
        assert_eq!(cmd.chains.len(), 3);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hi");
        assert!(!operand.trace_id.is_empty());
    }

    #[test]
    fn convert_without_host() {
        let result = parse_route("o:/transl?t=en&s=ja").unwrap();
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }

    #[test]
    fn convert_with_host() {
        let result = parse_route("o://localhost:8080/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:8080");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");

        let result = parse_route("https://localhost/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:443");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }

    #[test]
    fn parse_route_accepts_the_scheme_less_remote_form() {
        let route = parse_route("//localhost:8081/transc").expect("scheme-less remote form");
        assert_eq!(route.operation, Operation::Transcribe as i32);
        assert_eq!(route.remote, "//localhost:8081");
        assert!(route.queries.is_empty());
    }

    #[test]
    fn parse_route_accepts_the_scheme_less_bare_form() {
        let route = parse_route("transl?t=en").expect("scheme-less bare form");
        assert_eq!(route.operation, Operation::Translate as i32);
        assert!(route.remote.is_empty(), "no host means no remote");
        assert_eq!(route.queries["t"], "en");
    }

    #[test]
    fn parse_route_still_accepts_the_historical_scheme_form() {
        // ADR-0018 の正規化は受け付ける入力の拡張であって置き換えではない。
        let route = parse_route("o:/tts?spd=1.1").expect("o: form");
        assert_eq!(route.operation, Operation::Tts as i32);
        assert!(route.remote.is_empty());
        assert_eq!(route.queries["spd"], "1.1");

        let route = parse_route("o://localhost:8080/transl?t=en").expect("o:// form");
        assert_eq!(route.remote, "//localhost:8080");
        assert_eq!(route.queries["t"], "en");
    }

    #[test]
    fn parse_route_maps_both_transcribe_aliases() {
        for s in ["transc", "transcribe"] {
            let route = parse_route(s).unwrap_or_else(|e| panic!("{s} should parse: {e}"));
            assert_eq!(route.operation, Operation::Transcribe as i32, "for {s}");
        }
    }

    #[test]
    fn parse_route_does_not_mistake_a_query_value_for_a_scheme() {
        // 'transl?u=http://x' にはコロンがあるが、その手前は scheme として不正。
        // scheme 判定を「最初のコロンがあれば scheme」で済ませると、この文字列が
        // 絶対 URL 扱いのまま Url::parse に渡り、operation が空になって落ちる。
        let route = parse_route("transl?u=http://x").expect("query value containing a colon");
        assert_eq!(route.operation, Operation::Translate as i32);
        assert_eq!(route.queries["u"], "http://x");
    }

    #[test]
    fn parse_route_rejects_an_unknown_operation() {
        let err = parse_route("//localhost:8081/nope").expect_err("unknown operation");
        assert!(
            matches!(err, VstcError::OpConvertError { .. }),
            "should be an operation-conversion error, got {err:?}"
        );
    }
}
