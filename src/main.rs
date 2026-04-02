mod cli;
mod config;
mod docker;
mod init;
mod project;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Build => docker::cmd_build(),
        Command::Init { project } => init::cmd_init(&project),
        Command::Run { project, args } => docker::cmd_run(&project, &args),
        Command::Shell { project } => docker::cmd_shell(&project),
        Command::Destroy { project } => docker::cmd_destroy(&project),
        Command::List => docker::cmd_list(),
        Command::Completions { .. } => anyhow::bail!("not implemented yet"),
    }
}
