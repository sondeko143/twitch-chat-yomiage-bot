//! This is the vstreamer-tool's client library

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

use std::collections::HashMap;

use thiserror::Error;
use url::Url;
use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operand, Operation, OperationChain, OperationRoute,
    Response, Sound,
};

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
    TransportError(#[from] tonic::transport::Error),

    /// Send error
    #[error(transparent)]
    StatusError(#[from] tonic::Status),

    /// Operation parse error
    #[error(transparent)]
    UrlError(#[from] url::ParseError),
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
    let dst = uri.to_string();
    let mut channel = CommanderClient::connect(dst).await?;
    let op_routes: Result<Vec<_>, _> = operations
        .iter()
        .map(String::as_ref)
        .map(convert_to_operation)
        .collect();
    let operand = Operand {
        text,
        sound,
        file_path: file_path.unwrap_or_default(),
        filters: filters.unwrap_or_default(),
    };
    let c = tonic::Request::new(Command {
        chains: vec![OperationChain {
            operations: op_routes?,
        }],
        operand: Some(operand),
    });
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}

fn convert_to_operation(op_str: &str) -> Result<OperationRoute, VstcError> {
    let parsed = Url::parse(op_str)?;
    let hash_query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    let operation = match parsed.path().strip_prefix('/').unwrap_or_default() {
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
    fn convert_without_host() {
        let result = convert_to_operation("o:/transl?t=en&s=ja").unwrap();
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }

    #[test]
    fn convert_with_host() {
        let result = convert_to_operation("o://localhost:8080/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:8080");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");

        let result = convert_to_operation("https://localhost/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:443");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }
}
