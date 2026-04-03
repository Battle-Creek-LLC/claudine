use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "claudine", version, about = "Run Claude Code in isolated Docker containers")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new claudine project configuration
    Init {
        /// Name of the project to initialize
        project: String,
    },

    /// Run Claude Code in a container for the given project
    Run {
        /// Name of the project to run
        project: String,

        /// Repository directory to use as working directory
        #[arg(short, long)]
        repo: Option<String>,

        /// Additional arguments passed through to Claude
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Open an interactive shell in a project's container
    Shell {
        /// Name of the project
        project: String,

        /// Repository directory to use as working directory
        #[arg(short, long)]
        repo: Option<String>,
    },

    /// Destroy a project's container and associated resources
    Destroy {
        /// Name of the project to destroy
        project: String,
    },

    /// Manage repositories in a project
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },

    /// Build the claudine Docker image
    Build,

    /// List all claudine projects
    List,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum RepoCommand {
    /// Add a repository to a project
    Add {
        /// Project name
        project: String,
        /// Repository URL
        url: String,
        /// Directory name (defaults to repo name)
        #[arg(short, long)]
        dir: Option<String>,
        /// Branch to clone
        #[arg(short, long)]
        branch: Option<String>,
    },
    /// Remove a repository from a project
    Remove {
        /// Project name
        project: String,
        /// Directory name of the repo to remove
        dir: String,
    },
    /// List repositories in a project
    List {
        /// Project name
        project: String,
    },
}
