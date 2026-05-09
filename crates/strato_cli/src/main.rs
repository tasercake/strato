//! Strato command-line entry point.

mod args;
mod config;
mod output;

use args::{Cli, Command};
use clap::Parser;
use strato_core::{AnalysisError, AnalysisOptions, ConfigSource, discovery::DiscoverError};

fn main() {
    let cli = Cli::parse();
    let exit_code = run(cli);
    std::process::exit(exit_code);
}

fn run(cli: Cli) -> i32 {
    match cli.command {
        Command::Check {
            paths,
            config,
            output,
            intervention_strategy,
            severity,
            no_cache,
            clear_cache,
            first_party,
            python_version,
            quiet,
            verbose,
        } => {
            let options = AnalysisOptions {
                config: config.map_or(ConfigSource::Defaults, ConfigSource::Path),
                intervention_strategy: intervention_strategy.map(Into::into),
                severity: severity.map(Into::into),
                output_format: Some(output.into()),
                cache_enabled: no_cache.then_some(false),
                clear_cache,
                first_party_modules: (!first_party.is_empty()).then_some(first_party),
                python_version,
            };
            match analyze_paths(&paths, &options, verbose, quiet) {
                Ok(analysis) => match render_output(output, &analysis.json) {
                    Ok(rendered) => {
                        print!("{rendered}");
                        analysis.exit_code
                    }
                    Err(error) => {
                        eprintln!("failed to render output: {error}");
                        2
                    }
                },
                Err(error) => handle_error(error),
            }
        }
    }
}

fn analyze_paths(
    paths: &[std::path::PathBuf],
    options: &AnalysisOptions,
    verbose: u8,
    quiet: bool,
) -> Result<strato_core::AnalysisOutput, AnalysisError> {
    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();
    let mut exit_code = 0;
    for path in paths {
        if verbose > 0 && !quiet {
            eprintln!("Analyzing {}", path.display());
        }
        let analysis = strato_core::analyze_path_with_options(path, options)?;
        exit_code = exit_code.max(analysis.exit_code);
        diagnostics.extend(
            analysis.json["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
        warnings.extend(
            analysis.json["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
    }
    Ok(strato_core::AnalysisOutput {
        exit_code,
        json: serde_json::json!({
            "version": "1.0",
            "diagnostics": diagnostics,
            "warnings": warnings,
        }),
    })
}

fn render_output(
    output: args::OutputFormat,
    report: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    match output {
        args::OutputFormat::Text => Ok(output::text::render(report)),
        args::OutputFormat::Json => output::json::render(report),
        args::OutputFormat::Sarif => output::sarif::render(report),
    }
}

fn handle_error(error: AnalysisError) -> i32 {
    match error {
        AnalysisError::Discovery(DiscoverError::Config { message }) => {
            eprintln!("{message}");
            2
        }
        AnalysisError::Discovery(DiscoverError::NoAnalyzableSourceFiles) => {
            eprintln!("No analyzable source files");
            3
        }
        other => {
            eprintln!("{other}");
            2
        }
    }
}
