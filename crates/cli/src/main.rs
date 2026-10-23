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
    /// Run the artifact host
    Artifacts,
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
    Run {
        #[arg(long, env = "MYCELIUM_CONTROL_PLANE")]
        control_plane: Option<String>,
    },
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();

    match cli.command {
        Command::Control { command: ControlCommand::Serve } => mycelium_control::serve().await,
        Command::Artifacts => mycelium_artifacts::serve().await,
        Command::Node { command: NodeCommand::Join { url, join_token_file } } => {
            mycelium_node::join(mycelium_node::JoinOptions { control_plane: url, join_token_file, config_path: None }).await
        }
        Command::Node { command: NodeCommand::Run { control_plane } } => {
            if let Some(url) = control_plane.filter(|_| !mycelium_node::config::config_path().exists())
            {
                mycelium_node::join(mycelium_node::JoinOptions { control_plane: url, join_token_file: None, config_path: None }).await?;
            }

            mycelium_node::run().await
        }
        Command::Node { command: NodeCommand::Status } => mycelium_node::status().await,
        Command::Keeper { dir, command } => mycelium_keeper::run(dir, command).await,
        Command::Version => {
            println!("{VERSION}");

            Ok(())
        }
    }
}
