//! This is the vstreamer-tool's client library

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

use thiserror::Error;
use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operation, OperationChain, OperationRoute,
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
}

/// Send the command to the channel.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
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
        .map(|r| match r {
            Ok(o) => Ok(OperationRoute {
                operation: o.into(),
                remote: String::new(),
            }),
            Err(e) => Err(e),
        })
        .collect();
    let c = tonic::Request::new(Command {
        chains: vec![OperationChain {
            operations: op_routes?,
        }],
        text,
        sound,
        file_path: file_path.unwrap_or_default(),
        filters: filters.unwrap_or_default(),
    });
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}

fn convert_to_operation(op_str: &str) -> Result<Operation, VstcError> {
    match op_str {
        "transl" | "translate" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "play" | "playback" => Ok(Operation::Playback),
        "sub" | "subtitle" => Ok(Operation::Subtitle),
        "vc" => Ok(Operation::Vc),
        "reload" => Ok(Operation::Reload),
        "pause" => Ok(Operation::Pause),
        "resume" => Ok(Operation::Resume),
        _ => Err(VstcError::OpConvertError {
            op_str: String::from(op_str),
        }),
    }
}
