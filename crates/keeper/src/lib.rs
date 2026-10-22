use anyhow::{Result, bail};
use std::path::PathBuf;

pub async fn run(_dir: PathBuf, _command: Vec<String>) -> Result<()> {
    bail!("not built yet")
}
