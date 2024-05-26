use corust_sandbox::{
    container::{
        CargoCommand, ContainerFactory, ContainerMessage, ContainerRunRet, DockerBackend,
        ExecuteCommand, TargetType,
    },
    init_logger,
};
use env_logger::Target;
use std::sync::Arc;

const MAX_CONCURRENT_CONTAINERS: usize = 10;

#[tokio::main]
async fn main() {
    // Initialize env logger
    init_logger(Target::Stderr);
    let backend = DockerBackend::new();
    let container_factory = Arc::new(ContainerFactory::new(MAX_CONCURRENT_CONTAINERS));
    let container = container_factory.create_container(backend).await.unwrap();
    let ContainerRunRet {
        mut child,
        io_component,
    } = match container.run().await {
        Err(e) => panic!("{}", e),
        Ok(io_component) => io_component,
    };

    log::info!("Container started");

    // Send a message to the container to test
    let execute_command = ExecuteCommand::new(
        "fn main() { println!(\"Hello world!\"); }".to_string(),
        TargetType::Binary,
        CargoCommand::Run,
    );
    let message = ContainerMessage::Execute(execute_command);
    log::debug!("Sending container message: {:?}", message);
    io_component.child_stdin_tx.send(message).await.unwrap();

    // Wait for all tasks to complete
    let exit_code = child.wait().await.expect("Failed to wait on child");
    log::info!("Container exited with: {:?}", exit_code);
}
