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
    const ADDR_STR: &'static str = "127.0.0.1:9001";
    tokio::spawn(async move {
        let mut mock = MockCommanderService::new();
        mock.expect_process_command()
            .returning(|_| Ok(tonic::Response::new(Response { result: true })));
        let addr = ADDR_STR.parse().unwrap();
        build(mock).serve(addr).await.unwrap();
    });

    let result = process_command(
        format!("http://{ADDR_STR}").as_str(),
        &vec![String::from("o:/tts")],
        String::from(""),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.result, true)
}
