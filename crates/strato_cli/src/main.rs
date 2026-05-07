//! Strato command-line entry point.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about = "Detect blocking calls in Python async contexts")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyze Python files under a path.
    Check {
        /// File or directory to analyze.
        path: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Sarif,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { path, output } => {
            eprintln!(
                "strato check is scaffolded but not implemented yet: path={}, output={output:?}",
                path.display()
            );
        }
    }
}
