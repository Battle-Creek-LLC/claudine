use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "claudine", version, about = "Run Claude Code in isolated Docker containers", infer_subcommands = true)]
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

        /// SSH key path (skips interactive prompt)
        #[arg(long)]
        ssh_key: Option<String>,

        /// Repository URLs (repeatable, skips interactive prompt)
        #[arg(long = "repo")]
        repos: Vec<String>,

        /// Plugins to install (repeatable)
        #[arg(long = "plugin")]
        plugins: Vec<String>,
    },

    /// Run Claude Code in a container for the given project
    #[command(alias = "r")]
    Run {
        /// Name of the project to run
        project: String,

        /// Repository directory to use as working directory
        repo: Option<String>,

        /// Resume a previous conversation by name or session ID
        #[arg(long, short = 'R')]
        resume: Option<String>,

        /// Additional arguments passed through to Claude (after --)
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Open an interactive shell in a project's container
    Shell {
        /// Name of the project
        project: String,

        /// Repository directory to use as working directory
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

    /// Manage plugins for a project
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },

    /// Build the claudine Docker image (or a project's plugin image)
    Build {
        /// Project name (rebuilds project plugin image; omit for base image)
        project: Option<String>,
    },

    /// List all claudine projects
    List,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum PluginCommand {
    /// Add a plugin to a project
    Add {
        /// Project name
        project: String,
        /// Plugin name
        plugin: String,
    },
    /// Remove a plugin from a project
    Remove {
        /// Project name
        project: String,
        /// Plugin name
        plugin: String,
    },
    /// List plugins installed in a project
    List {
        /// Project name
        project: String,
    },
    /// Show all available plugins
    Available,
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
