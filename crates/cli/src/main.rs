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
    /// Internal: carry one SSH connection to a node through the control plane
    SshProxy {
        node: String,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long)]
        control_plane: Option<String>,
    },
    /// Print the version
    Version,
}

#[derive(Subcommand)]
enum ControlCommand {
    Serve,
    /// Re-encrypt every stored secret under the current secret key, generating a new key first when none is set in the environment
    RotateSecrets,
}

#[derive(Subcommand)]
enum NodeCommand {
    Join {
        url: String,
        /// A single-use join token from Add an outpost, instead of a device code
        #[arg(long = "token", env = "MYCELIUM_JOIN_TOKEN", hide_env_values = true)]
        join_token: Option<String>,
        #[arg(long, env = "MYCELIUM_JOIN_TOKEN_FILE")]
        join_token_file: Option<PathBuf>,
        /// The name this node shows in the node list, instead of the machine's hostname
        #[arg(long)]
        name: Option<String>,
    },
    /// Print the Kubernetes manifests for an outpost: one node in one namespace of a cluster
    Outpost {
        name: String,
        #[arg(long, default_value = "mycelium")]
        namespace: String,
        #[arg(long)]
        storage_class: Option<String>,
        /// The control plane the outpost joins on first start; needs --token
        #[arg(long, env = "MYCELIUM_CONTROL_PLANE", requires = "join_token")]
        control_plane: Option<String>,
        /// A join token from Add an outpost, stored in a Secret that the node joins with
        #[arg(long = "token", env = "MYCELIUM_JOIN_TOKEN", hide_env_values = true, requires = "control_plane")]
        join_token: Option<String>,
    },
    Run {
        #[arg(long, env = "MYCELIUM_CONTROL_PLANE")]
        control_plane: Option<String>,
        #[arg(long = "token", env = "MYCELIUM_JOIN_TOKEN", hide_env_values = true)]
        join_token: Option<String>,
    },
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();

    match cli.command {
        Command::Control { command: ControlCommand::Serve } => mycelium_control::serve().await,
        Command::Control { command: ControlCommand::RotateSecrets } => mycelium_control::rotate_secrets().await,
        Command::Artifacts => mycelium_artifacts::serve().await,
        Command::Node { command: NodeCommand::Join { url, join_token, join_token_file, name } } => {
            mycelium_node::join(mycelium_node::JoinOptions { control_plane: url, join_token, join_token_file, name, config_path: None }).await
        }
        Command::Node { command: NodeCommand::Outpost { name, namespace, storage_class, control_plane, join_token } } => {
            let join = control_plane.zip(join_token).map(|(control_plane, token)| mycelium_node::manifests::OutpostJoin { control_plane, token });
            let manifests = mycelium_node::manifests::Outpost { name, namespace, storage_class: storage_class.filter(|class| !class.is_empty()), join }.manifests()?;
            print!("{manifests}");

            Ok(())
        }
        Command::Node { command: NodeCommand::Run { control_plane, join_token } } => {
            if let Some(url) = control_plane.filter(|_| !mycelium_node::config::config_path().exists())
            {
                mycelium_node::join(mycelium_node::JoinOptions { control_plane: url, join_token, join_token_file: None, name: None, config_path: None }).await?;
            }

            mycelium_node::run().await
        }
        Command::Node { command: NodeCommand::Status } => mycelium_node::status().await,
        Command::Keeper { dir, command } => mycelium_keeper::run(dir, command).await,
        Command::SshProxy { node, token_file, control_plane } => mycelium_node::ssh_proxy::run(node, token_file, control_plane).await,
        Command::Version => {
            println!("{VERSION}");

            Ok(())
        }
    }
}
