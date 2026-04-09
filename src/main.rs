mod cli;
mod config;
mod devcontainer;
mod docker;
mod init;
mod layer;
mod project;
mod repo;
mod resolve;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command, LayerCommand, RepoCommand};

const BRUCE_LEE_QUOTES: &[&str] = &[
    "Be water, my friend.",
    "I fear not the man who has practiced 10,000 kicks once, but I fear the man who has practiced one kick 10,000 times.",
    "Absorb what is useful, discard what is useless and add what is specifically your own.",
    "The key to immortality is first living a life worth remembering.",
    "Knowing is not enough, we must apply. Willing is not enough, we must do.",
    "Mistakes are always forgivable, if one has the courage to admit them.",
    "If you spend too much time thinking about a thing, you'll never get it done.",
    "A wise man can learn more from a foolish question than a fool can learn from a wise answer.",
    "Do not pray for an easy life, pray for the strength to endure a difficult one.",
    "To hell with circumstances; I create opportunities.",
];

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Random Bruce Lee quote on startup
    let quote = BRUCE_LEE_QUOTES[std::process::id() as usize % BRUCE_LEE_QUOTES.len()];
    eprintln!("\n  \"{}\" — Bruce Lee\n", quote);

    match cli.command {
        Command::Build { project: None, all: false, no_cache } => docker::cmd_build(no_cache),
        Command::Build { project: Some(project), no_cache, .. } => {
            let project = resolve::project(&project)?;
            layer::cmd_build_project(&project, no_cache)
        }
        Command::Build { project: None, all: true, no_cache } => layer::cmd_build_all(no_cache),
        Command::Init { project, ssh_key, repos, layers, agent } => {
            if let Some(agent_path) = agent {
                init::cmd_init_agent(&project, &agent_path, ssh_key.as_deref())
            } else {
                init::cmd_init(&project, ssh_key.as_deref(), &repos, &layers)
            }
        }
        Command::Run { project, repo, resume, prompt, args } => {
            let project = resolve::project(&project)?;
            let repo = repo.map(|r| resolve::repo(&project, &r)).transpose()?;
            docker::cmd_run(&project, repo.as_deref(), resume.as_deref(), prompt.as_deref(), &args)
        }
        Command::Shell { project, repo } => {
            let project = resolve::project(&project)?;
            let repo = repo.map(|r| resolve::repo(&project, &r)).transpose()?;
            docker::cmd_shell(&project, repo.as_deref())
        }
        Command::Zed { project, repo } => {
            let project = resolve::project(&project)?;
            let repo = repo.map(|r| resolve::repo(&project, &r)).transpose()?;
            devcontainer::cmd_zed(&project, repo.as_deref())
        }
        Command::Destroy { project } => {
            let project = resolve::project(&project)?;
            docker::cmd_destroy(&project)
        }
        Command::List => docker::cmd_list(),
        Command::Layer { command } => match command {
            LayerCommand::Add { project, layer } => {
                let project = resolve::project(&project)?;
                layer::cmd_layer_add(&project, &layer)
            }
            LayerCommand::Remove { project, layer } => {
                let project = resolve::project(&project)?;
                layer::cmd_layer_remove(&project, &layer)
            }
            LayerCommand::List { project } => {
                let project = resolve::project(&project)?;
                layer::cmd_layer_list(&project)
            }
            LayerCommand::Available => layer::cmd_layer_available(),
            LayerCommand::Validate { layer: Some(name) } => layer::cmd_layer_validate(&name),
            LayerCommand::Validate { layer: None } => layer::cmd_layer_validate_all(),
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
