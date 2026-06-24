use mockall::mock;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::Request;
use tonic::Status;
use tonic_reflection::pb::FILE_DESCRIPTOR_SET;
use tonic_reflection::server::Builder;
use vstc::*;
use vstreamer_protos::commander_server::Commander;
use vstreamer_protos::commander_server::CommanderServer;
use vstreamer_protos::Command;
use vstreamer_protos::Response;

mock! {
    CommanderService {}

    #[tonic::async_trait]
    impl Commander for CommanderService {
        async fn process_command(
            &self,
            request: Request<Command>,
        ) -> Result<tonic::Response<Response>, Status>;
        async fn sync_process_command(
            &self,
            request: Request<Command>,
        ) -> Result<tonic::Response<Response>, Status>;
    }
}

pub fn build(cmd: impl Commander) -> Router {
    let reflection_service = Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build()
        .expect("reflection service could not build");

    Server::builder()
        .add_service(CommanderServer::new(cmd))
        .add_service(reflection_service)
}

#[tokio::test]
async fn send_minimal() {
    const ADDR_STR: &str = "127.0.0.1:9001";
    tokio::spawn(async move {
        let mut mock = MockCommanderService::new();
        mock.expect_process_command()
            .returning(|_| Ok(tonic::Response::new(Response { result: true })));
        let addr = ADDR_STR.parse().unwrap();
        build(mock).serve(addr).await.unwrap();
    });

    let result = process_command(
        format!("http://{ADDR_STR}").as_str(),
        &[String::from("o:/tts")],
        String::from(""),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(result.result)
}

#[tokio::test]
async fn populates_trace_id_and_origin_ts() {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    const ADDR_STR: &str = "127.0.0.1:9002";
    let (tx, rx) = channel();
    tokio::spawn(async move {
        let mut mock = MockCommanderService::new();
        mock.expect_process_command().returning(move |req| {
            let operand = req.into_inner().operand.expect("operand should be present");
            tx.send((operand.trace_id, operand.origin_ts))
                .expect("test channel should accept the operand");
            Ok(tonic::Response::new(Response { result: true }))
        });
        let addr = ADDR_STR.parse().unwrap();
        build(mock).serve(addr).await.unwrap();
    });

    process_command(
        format!("http://{ADDR_STR}").as_str(),
        &[String::from("o:/tts")],
        String::from("trace test"),
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let (trace_id, origin_ts) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server should have received the operand");
    assert!(!trace_id.is_empty(), "trace_id should be populated");
    assert!(
        origin_ts > 0.0,
        "origin_ts should be a positive unix timestamp, got {origin_ts}"
    );
}

#[tokio::test]
async fn process_command_times_out_when_server_silent() {
    use std::time::Duration;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move {
        let mut conns = Vec::new();
        loop {
            match listener.accept().await {
                Ok((s, _)) => conns.push(s),
                Err(_) => return,
            }
        }
    });

    let start = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        process_command(
            format!("http://{addr}").as_str(),
            &[],
            String::from("hang test"),
            None,
            None,
            None,
        ),
    )
    .await;
    let elapsed = start.elapsed();

    let inner =
        outcome.expect("process_command did not return within 15s — internal timeout not enforced");
    assert!(
        inner.is_err(),
        "expected connect/RPC error against silent server, got Ok"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "took too long, internal timeout likely not active: {:?}",
        elapsed
    );
}
