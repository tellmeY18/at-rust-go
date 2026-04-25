#![deny(unsafe_code)]
//! CLI binary for at-rust-go (`atrg`).
//!
//! Provides subcommands to scaffold, develop, migrate, inspect, and build
//! AT Protocol API projects.

use clap::{Parser, Subcommand};

mod commands;

/// AT Protocol backend framework CLI.
#[derive(Parser)]
#[command(name = "atrg", about = "AT Protocol backend framework CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new at-rust-go API project
    New {
        /// Project name
        name: String,
        /// Project template
        #[arg(long, default_value = "basic")]
        template: String,
        /// Override target directory
        #[arg(long)]
        path: Option<String>,
        /// Overwrite existing directory
        #[arg(long)]
        force: bool,
    },
    /// Start dev server with file watching
    Dev,
    /// Run pending database migrations
    Migrate,
    /// Print all registered routes
    Routes,
    /// Build for release
    Build,
    /// Generate Rust code from lexicon JSON files
    Generate {
        /// Input directory containing lexicon .json files
        #[arg(long, default_value = "lexicons")]
        input: String,
        /// Output directory for generated Rust code
        #[arg(long, default_value = "src/generated")]
        output: String,
    },
    /// Print version
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New {
            name,
            template,
            path,
            force,
        } => commands::new::run(&name, &template, path.as_deref(), force),
        Commands::Dev => commands::dev::run(),
        Commands::Migrate => tokio::runtime::Runtime::new()?.block_on(commands::migrate::run()),
        Commands::Routes => commands::routes::run(),
        Commands::Build => commands::build::run(),
        Commands::Generate { input, output } => commands::generate::run(&input, &output),
        Commands::Version => {
            println!("atrg {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
