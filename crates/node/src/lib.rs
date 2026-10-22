use anyhow::{Result, bail};
use std::path::PathBuf;

pub struct JoinOptions {
    pub control_plane: String,
    pub join_token_file: Option<PathBuf>,
}

pub async fn join(_options: JoinOptions) -> Result<()> {
    bail!("not built yet")
}

pub async fn run() -> Result<()> {
    bail!("not built yet")
}

pub async fn status() -> Result<()> {
    bail!("not built yet")
}
