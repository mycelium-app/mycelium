use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

const VERSION: &str = match option_env!("MYCELIUM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(name = "mycelium", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the control plane
    Control {
        #[command(subcommand)]
        command: ControlCommand,
    },
    /// Join this machine to a control plane and run the node daemon
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Internal: own one agent's stdio for one conversation
    Keeper {
        #[arg(long)]
        dir: PathBuf,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Print the version
    Version,
}

#[derive(Subcommand)]
enum ControlCommand {
    Serve,
}

#[derive(Subcommand)]
enum NodeCommand {
    Join {
        url: String,
        #[arg(long, env = "MYCELIUM_JOIN_TOKEN_FILE")]
        join_token_file: Option<PathBuf>,
    },
    Run,
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Control { command: ControlCommand::Serve } => mycelium_control::serve().await,
        Command::Node { command: NodeCommand::Join { url, join_token_file } } => {
            mycelium_node::join(mycelium_node::JoinOptions { control_plane: url, join_token_file }).await
        }
        Command::Node { command: NodeCommand::Run } => mycelium_node::run().await,
        Command::Node { command: NodeCommand::Status } => mycelium_node::status().await,
        Command::Keeper { dir, command } => mycelium_keeper::run(dir, command).await,
        Command::Version => {
            println!("{VERSION}");

            Ok(())
        }
    }
}
