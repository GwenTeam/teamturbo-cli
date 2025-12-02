use clap::{Parser, Subcommand};
use anyhow::Result;

mod auth;
mod api;
mod commands;
mod config;
mod utils;

#[derive(Parser)]
#[command(name = "teamturbo")]
#[command(about = "TeamTurbo CLI for Docuram", long_about = None)]
#[command(version)]
struct Cli {
    /// Enable verbose output (detailed logs and HTTP requests)
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to TeamTurbo
    Login {
        /// Server domain (subdomain or full URL, e.g., 'example' or 'https://example.com')
        #[arg(short, long)]
        domain: Option<String>,
        /// Force browser authorization mode
        #[arg(long)]
        browser: bool,
        /// Force manual token input mode
        #[arg(long)]
        manual: bool,
    },
    /// Logout from TeamTurbo
    Logout,
    /// Show current login status
    Whoami,
    /// Initialize docuram project
    Init {
        /// Config URL to download from
        #[arg(long)]
        config_url: Option<String>,
        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
        /// Skip downloading documents
        #[arg(long)]
        no_download: bool,
    },
    /// Pull document updates from server
    Pull {
        /// Specific documents to pull (by slug)
        documents: Vec<String>,
        /// Force overwrite local changes
        #[arg(short, long)]
        force: bool,
    },
    /// Push new documents to server
    Push {
        /// Specific documents to push (by path)
        documents: Vec<String>,
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Sync documents (pull then push)
    Sync {
        /// Force overwrite conflicts
        #[arg(short, long)]
        force: bool,
    },
    /// Show diff between local and remote
    Diff {
        /// Specific document to diff (by slug)
        document: Option<String>,
    },
    /// List all documents with version information
    List,
    /// Import documents from a git repository or local directory
    Import {
        /// Paths to import (files or directories). If provided, converts in-place.
        paths: Vec<String>,
        /// Source (git URL or local path) - use with --to for remote import
        #[arg(long)]
        from: Option<String>,
        /// Target category path - use with --from for remote import
        #[arg(long)]
        to: Option<String>,
    },
    /// Delete documents or directories
    Delete {
        /// Paths to delete (files or directories in docs/)
        paths: Vec<String>,
        /// Force deletion without confirmation
        #[arg(short, long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize verbose mode
    utils::logger::init(cli.verbose);

    match cli.command {
        Commands::Login { domain, browser, manual } => {
            commands::login::execute(domain, browser, manual).await?;
        }
        Commands::Logout => {
            commands::logout::execute().await?;
        }
        Commands::Whoami => {
            commands::whoami::execute().await?;
        }
        Commands::Init { config_url, force, no_download } => {
            commands::init::execute(config_url, force, no_download).await?;
        }
        Commands::Pull { documents, force } => {
            commands::pull::execute(documents, force).await?;
        }
        Commands::Push { documents, message } => {
            commands::push::execute(documents, message).await?;
        }
        Commands::Sync { force } => {
            commands::sync::execute(force).await?;
        }
        Commands::Diff { document } => {
            commands::diff::execute(document).await?;
        }
        Commands::List => {
            commands::list::execute().await?;
        }
        Commands::Import { paths, from, to } => {
            commands::import::execute(paths, from, to).await?;
        }
        Commands::Delete { paths, force } => {
            commands::delete::execute(paths, force, cli.verbose).await?;
        }
    }

    Ok(())
}
