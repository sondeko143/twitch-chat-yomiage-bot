//! This is the vstreamer-tool's client library

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]

use core::fmt;
use tonic;
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
    operations: &Vec<String>,
    text: String,
    sound: Option<Sound>,
    file_path: Option<String>,
    filters: Option<Vec<String>>,
) -> Result<Response, Box<dyn std::error::Error>> {
    let dst = uri.to_string();
    let mut channel = CommanderClient::connect(dst).await?;
    let op_routes = operations
        .iter()
        .map(convert_to_operation)
        .filter(|o| o.is_ok())
        .map(|o| OperationRoute {
            operation: o.unwrap().into(),
            remote: "".into(),
        })
        .collect::<Vec<_>>();
    let c = tonic::Request::new(Command {
        chains: vec![OperationChain {
            operations: op_routes,
        }],
        text: String::from(text),
        sound,
        file_path: file_path.unwrap_or_default(),
        filters: filters.unwrap_or_default(),
    });
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}

#[derive(Debug, Clone)]
struct ConvertError;
impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid operation string")
    }
}
impl std::error::Error for ConvertError {}

fn convert_to_operation(op_str: &String) -> Result<Operation, ConvertError> {
    return match op_str.as_str() {
        "translate" => Ok(Operation::Translate),
        "transl" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "playback" => Ok(Operation::Playback),
        "play" => Ok(Operation::Playback),
        "subtitle" => Ok(Operation::Subtitle),
        "sub" => Ok(Operation::Subtitle),
        "reload" => Ok(Operation::Reload),
        "pause" => Ok(Operation::Pause),
        "resume" => Ok(Operation::Resume),
        _ => Err(ConvertError),
    };
}
