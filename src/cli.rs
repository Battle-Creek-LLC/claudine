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
        #[arg(long = "repo", conflicts_with = "agent")]
        repos: Vec<String>,

        /// Layers to install (repeatable)
        #[arg(long = "layer", conflicts_with = "agent")]
        layers: Vec<String>,

        /// Analyze a local folder with Claude to discover repos and layers
        #[arg(long)]
        agent: Option<String>,
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

        /// Run a prompt non-interactively (--output-format stream-json --verbose)
        #[arg(short, long)]
        prompt: Option<String>,

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

    /// Open the project in Zed via dev containers
    Zed {
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

    /// Manage layers for a project
    Layer {
        #[command(subcommand)]
        command: LayerCommand,
    },

    /// Build the claudine Docker image (or a project's layer image)
    Build {
        /// Project name (rebuilds project layer image; omit for base image)
        #[arg(conflicts_with = "all")]
        project: Option<String>,

        /// Rebuild all project images that have layers
        #[arg(long)]
        all: bool,

        /// Build without using Docker layer cache
        #[arg(long)]
        no_cache: bool,
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
pub enum LayerCommand {
    /// Add a layer to a project
    Add {
        /// Project name
        project: String,
        /// Layer name
        layer: String,
    },
    /// Remove a layer from a project
    Remove {
        /// Project name
        project: String,
        /// Layer name
        layer: String,
    },
    /// List layers installed in a project
    List {
        /// Project name
        project: String,
    },
    /// Show all available layers
    Available,
    /// Validate a layer by building and running checks
    Validate {
        /// Layer name (omit to validate all)
        layer: Option<String>,
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
