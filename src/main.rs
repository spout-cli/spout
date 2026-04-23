mod allocator;
mod cli;
mod commands;
mod date;
mod error;
mod format;
mod project;
mod project_markers;
mod protocol;
mod registry;
mod services;
mod tui;

use std::process::exit;

use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

use cli::{Cli, Commands};
use error::SpoutError;
use protocol::Protocol;

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
        Commands::Get { service, project } => {
            let port = commands::get(&reg_path, &service, project.as_deref())?;
            println!("{port}");
        }
        Commands::Alloc {
            service,
            udp,
            files,
        } => match (service, udp) {
            (Some(svc), _) => {
                let port = commands::alloc(&reg_path, &svc, proto(udp))?;
                println!("{port}");
            }
            (None, true) => {
                return Err(SpoutError::Usage(
                    "--udp is per-service; pass a service name or declare UDP in the compose port spec".into(),
                ));
            }
            (None, false) => {
                let outcome = commands::alloc_compose(&reg_path, &files)?;
                for w in &outcome.warnings {
                    eprintln!("spout: {w}");
                }
                println!("{}", outcome.summary);
            }
        },
        Commands::Set { service, port, udp } => {
            commands::set(&reg_path, &service, port, proto(udp))?;
        }
        Commands::Rm {
            service,
            project,
            yes,
            dry_run,
        } => {
            let target = build_rm_target(service, project)?;
            let out = commands::rm(&reg_path, target, commands::RmOptions { yes, dry_run })?;
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Commands::Ls { project, no_tui } => {
            if let Some(out) = commands::ls(&reg_path, project, no_tui)? {
                println!("{out}");
            }
        }
        Commands::Env { project } => {
            if let Some(out) = commands::env(&reg_path, project)? {
                println!("{out}");
            }
        }
        Commands::Check { port, udp } => {
            if !commands::check(port, proto(udp)) {
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
        Commands::Prune {
            dry_run,
            yes,
            older_than,
        } => {
            let out = commands::prune(&reg_path, older_than, dry_run, yes)?;
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "spout", &mut std::io::stdout());
        }
    }
    Ok(())
}

fn build_rm_target(
    service: Option<String>,
    project: Option<Option<String>>,
) -> Result<commands::RmTarget, SpoutError> {
    match (service, project) {
        (Some(name), Some(Some(p))) => Ok(commands::RmTarget::Service {
            name,
            project: Some(p),
        }),
        (Some(name), _) => Ok(commands::RmTarget::Service {
            name,
            project: None,
        }),
        (None, Some(Some(p))) => Ok(commands::RmTarget::Project { name: p }),
        (None, Some(None)) => Ok(commands::RmTarget::Project {
            name: crate::project::current_project()?,
        }),
        (None, None) => Err(SpoutError::Usage(
            "spout rm: specify a service or pass --project [NAME]".into(),
        )),
    }
}

fn proto(udp: bool) -> Protocol {
    if udp {
        Protocol::Udp
    } else {
        Protocol::Tcp
    }
}
