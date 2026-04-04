mod cli;
mod config;
mod docker;
mod init;
mod plugin;
mod project;
mod repo;
mod resolve;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command, PluginCommand, RepoCommand};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Build { project: None } => docker::cmd_build(),
        Command::Build { project: Some(project) } => {
            let project = resolve::project(&project)?;
            plugin::cmd_build_project(&project)
        }
        Command::Init { project, ssh_key, repos, plugins } => {
            init::cmd_init(&project, ssh_key.as_deref(), &repos, &plugins)
        }
        Command::Run { project, repo, resume, args } => {
            let project = resolve::project(&project)?;
            let repo = repo.map(|r| resolve::repo(&project, &r)).transpose()?;
            docker::cmd_run(&project, repo.as_deref(), resume.as_deref(), &args)
        }
        Command::Shell { project, repo } => {
            let project = resolve::project(&project)?;
            let repo = repo.map(|r| resolve::repo(&project, &r)).transpose()?;
            docker::cmd_shell(&project, repo.as_deref())
        }
        Command::Destroy { project } => {
            let project = resolve::project(&project)?;
            docker::cmd_destroy(&project)
        }
        Command::List => docker::cmd_list(),
        Command::Plugin { command } => match command {
            PluginCommand::Add { project, plugin } => {
                let project = resolve::project(&project)?;
                plugin::cmd_plugin_add(&project, &plugin)
            }
            PluginCommand::Remove { project, plugin } => {
                let project = resolve::project(&project)?;
                plugin::cmd_plugin_remove(&project, &plugin)
            }
            PluginCommand::List { project } => {
                let project = resolve::project(&project)?;
                plugin::cmd_plugin_list(&project)
            }
            PluginCommand::Available => plugin::cmd_plugin_available(),
        },
        Command::Repo { command } => {
            let resolved = match command {
                RepoCommand::Add { project, url, dir, branch } => {
                    RepoCommand::Add { project: resolve::project(&project)?, url, dir, branch }
                }
                RepoCommand::Remove { project, dir } => {
                    let project = resolve::project(&project)?;
                    let dir = resolve::repo(&project, &dir)?;
                    RepoCommand::Remove { project, dir }
                }
                RepoCommand::List { project } => {
                    RepoCommand::List { project: resolve::project(&project)? }
                }
            };
            repo::cmd_repo(resolved)
        }
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "claudine", &mut std::io::stdout());
            Ok(())
        }
    }
}
