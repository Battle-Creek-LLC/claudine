mod cli;
mod config;
mod docker;
mod project;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Build => docker::cmd_build(),
        Command::Init { .. } => anyhow::bail!("not implemented yet"),
        Command::Run { .. } => anyhow::bail!("not implemented yet"),
        Command::Shell { .. } => anyhow::bail!("not implemented yet"),
        Command::Destroy { .. } => anyhow::bail!("not implemented yet"),
        Command::List => anyhow::bail!("not implemented yet"),
        Command::Completions { .. } => anyhow::bail!("not implemented yet"),
    }
}
