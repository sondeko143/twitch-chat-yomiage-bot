//! This is the vstreamer-tool's client library

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

use core::fmt;
use vstreamer_protos::{
    commander_client::CommanderClient, Command, Operation, OperationChain, OperationRoute,
    Response, Sound,
};

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
) -> Result<Response, Box<dyn std::error::Error>> {
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

#[derive(Debug, Clone)]
struct OpConvertError {
    op_str: String,
}
impl fmt::Display for OpConvertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid operation string {}", self.op_str)
    }
}
impl std::error::Error for OpConvertError {}

fn convert_to_operation(op_str: &str) -> Result<Operation, OpConvertError> {
    match op_str {
        "transl" | "translate" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "play" | "playback" => Ok(Operation::Playback),
        "sub" | "subtitle" => Ok(Operation::Subtitle),
        "vc" => Ok(Operation::Vc),
        "reload" => Ok(Operation::Reload),
        "pause" => Ok(Operation::Pause),
        "resume" => Ok(Operation::Resume),
        _ => Err(OpConvertError {
            op_str: String::from(op_str),
        }),
    }
}
