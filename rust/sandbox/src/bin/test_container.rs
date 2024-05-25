use ansi_term::Color;
use corust_sandbox::container::{
    CargoCommand, ContainerFactory, ContainerMessage, ContainerRunRet, ExecuteCommand, TargetType,
};
use log::Level;
use std::io::Write;
use std::sync::Arc;

const MAX_CONCURRENT_CONTAINERS: usize = 10;

#[tokio::main]
async fn main() {
    // Initialize env logger
    env_logger::builder()
        .format(|buf, record| {
            let level = match record.level() {
                Level::Error => Color::Red.paint("ERROR"),
                Level::Warn => Color::Yellow.paint("WARN"),
                Level::Info => Color::Green.paint("INFO"),
                Level::Debug => Color::Blue.paint("DEBUG"),
                Level::Trace => Color::Purple.paint("TRACE"),
            };

            writeln!(
                buf,
                "[{} {}:{}] {}",
                level,
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .init();
    let container_factory = Arc::new(ContainerFactory::new(MAX_CONCURRENT_CONTAINERS));
    let container = container_factory.create_container().await.unwrap();
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
