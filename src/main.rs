mod allocator;
mod cli;
mod commands;
mod date;
mod error;
mod project;
mod registry;
mod services;
mod tui;

use std::process::exit;

use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};
use error::SpoutError;

fn main() {
    let parsed = Cli::parse();
    init_logging(parsed.verbose);

    if let Err(e) = run(parsed) {
        eprintln!("spout: {e}");
        exit(e.exit_code());
    }
}

fn init_logging(verbose: bool) {
    let default_level = if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::WARN
    };
    let filter = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy();
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init();
}

fn run(cli: Cli) -> Result<(), SpoutError> {
    let reg_path = registry::registry_path()?;
    match cli.command {
        Commands::Get { service } => {
            let port = commands::get(&reg_path, &service)?;
            println!("{port}");
        }
        Commands::Alloc { service } => {
            let port = commands::alloc(&reg_path, &service)?;
            println!("{port}");
        }
        Commands::Set { service, port } => {
            commands::set(&reg_path, &service, port)?;
        }
        Commands::Rm { service } => {
            commands::rm(&reg_path, &service)?;
        }
        Commands::Ls { project, no_tui } => {
            if let Some(out) = commands::ls(&reg_path, project, no_tui)? {
                println!("{out}");
            }
        }
        Commands::Check { port } => {
            if !commands::check(port) {
                exit(1);
            }
        }
        Commands::Whois { port, history } => match commands::whois(&reg_path, port, history)? {
            Some(result) => println!("{result}"),
            None => {
                if history {
                    eprintln!("spout: {port} has no live or historical registration");
                } else {
                    eprintln!(
                        "spout: {port} is not currently registered. Try 'spout whois {port} --history'"
                    );
                }
                exit(1);
            }
        },
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "spout", &mut std::io::stdout());
        }
    }
    Ok(())
}
