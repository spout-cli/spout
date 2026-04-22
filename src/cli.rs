//! Command-line argument definitions. No logic — only shape.
//!
//! `[READ ONLY]` / `[MUTATES REGISTRY]` annotations in doc comments are
//! intentional: clap renders them in help output, and agents pattern-match
//! on them to reason about which commands are safe to call speculatively.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser, Debug)]
#[command(name = "spout", about = "Local development port registry", version)]
pub struct Cli {
    /// Verbose logging to stderr. RUST_LOG takes precedence.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Read a registered port [READ ONLY]
    Get { service: String },

    /// Register a new port (idempotent) [MUTATES REGISTRY].
    ///
    /// With a service name, registers that single service. With no
    /// service name, reads docker-compose.yml (or a sibling) and
    /// registers one port per declared service.
    Alloc {
        service: Option<String>,
        /// Allocate a UDP port instead of TCP (single-service mode only)
        #[arg(long)]
        udp: bool,
        /// Compose file path. Default: auto-detect in the current
        /// directory. Ignored when <service> is given.
        #[arg(short = 'f', long = "file", value_name = "PATH")]
        file: Option<PathBuf>,
    },

    /// Register a specific port manually [MUTATES REGISTRY]
    Set {
        service: String,
        port: u16,
        /// Register the UDP port instead of TCP
        #[arg(long)]
        udp: bool,
    },

    /// Remove a registration [MUTATES REGISTRY]
    Rm { service: String },

    /// List all registrations [READ ONLY]
    Ls {
        /// Filter to a project. With no value, uses the current project.
        #[arg(long, value_name = "NAME", num_args = 0..=1)]
        project: Option<Option<String>>,

        /// Force plain-text output even when stdout is a TTY
        #[arg(long)]
        no_tui: bool,
    },

    /// Print KEY=VALUE port assignments for a project [READ ONLY]
    Env {
        /// Project to print. Defaults to the current project.
        #[arg(long, value_name = "NAME", num_args = 0..=1)]
        project: Option<Option<String>>,
    },

    /// Check if a port is free on the OS (exit 0 free, 1 taken) [READ ONLY]
    Check {
        port: u16,
        /// Check UDP instead of TCP
        #[arg(long)]
        udp: bool,
    },

    /// Reverse lookup — which project/service owns this port? [READ ONLY]
    Whois {
        port: u16,
        /// Include released ports from the history
        #[arg(long)]
        history: bool,
    },

    /// Surface stale registrations and optionally remove them [MUTATES REGISTRY unless --dry-run]
    Prune {
        /// Surface candidates only; make no changes
        #[arg(long)]
        dry_run: bool,
        /// Bulk-remove every candidate without per-entry prompts
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
        /// Age cutoff in days; entries older than this are candidates
        #[arg(long, value_name = "DAYS", default_value_t = 90)]
        older_than: u64,
    },

    /// Generate a shell completion script for the given shell [READ ONLY]
    Completions { shell: Shell },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_alloc() {
        let cli = Cli::try_parse_from(["spout", "alloc", "postgres"]).unwrap();
        match cli.command {
            Commands::Alloc { service, udp, file } => {
                assert_eq!(service.as_deref(), Some("postgres"));
                assert!(!udp);
                assert!(file.is_none());
            }
            other => panic!("expected Alloc, got {other:?}"),
        }
    }

    #[test]
    fn parses_alloc_with_udp_flag() {
        let cli = Cli::try_parse_from(["spout", "alloc", "dns", "--udp"]).unwrap();
        match cli.command {
            Commands::Alloc { service, udp, file } => {
                assert_eq!(service.as_deref(), Some("dns"));
                assert!(udp);
                assert!(file.is_none());
            }
            other => panic!("expected Alloc, got {other:?}"),
        }
    }

    #[test]
    fn parses_alloc_with_no_service_and_no_file() {
        let cli = Cli::try_parse_from(["spout", "alloc"]).unwrap();
        match cli.command {
            Commands::Alloc { service, udp, file } => {
                assert!(service.is_none());
                assert!(!udp);
                assert!(file.is_none());
            }
            other => panic!("expected Alloc, got {other:?}"),
        }
    }

    #[test]
    fn parses_alloc_with_file_flag() {
        let cli = Cli::try_parse_from(["spout", "alloc", "-f", "compose.prod.yml"]).unwrap();
        match cli.command {
            Commands::Alloc { service, file, .. } => {
                assert!(service.is_none());
                assert_eq!(
                    file.as_deref(),
                    Some(std::path::Path::new("compose.prod.yml"))
                );
            }
            other => panic!("expected Alloc, got {other:?}"),
        }
    }

    #[test]
    fn parses_set_with_udp_flag() {
        let cli = Cli::try_parse_from(["spout", "set", "dns", "5353", "--udp"]).unwrap();
        match cli.command {
            Commands::Set { service, port, udp } => {
                assert_eq!(service, "dns");
                assert_eq!(port, 5353);
                assert!(udp);
            }
            other => panic!("expected Set, got {other:?}"),
        }
    }

    #[test]
    fn parses_check_with_udp_flag() {
        let cli = Cli::try_parse_from(["spout", "check", "5353", "--udp"]).unwrap();
        match cli.command {
            Commands::Check { port, udp } => {
                assert_eq!(port, 5353);
                assert!(udp);
            }
            other => panic!("expected Check, got {other:?}"),
        }
    }

    #[test]
    fn parses_whois_with_history() {
        let cli = Cli::try_parse_from(["spout", "whois", "19123", "--history"]).unwrap();
        match cli.command {
            Commands::Whois { port, history } => {
                assert_eq!(port, 19123);
                assert!(history);
            }
            other => panic!("expected Whois, got {other:?}"),
        }
    }

    #[test]
    fn parses_ls_bare() {
        let cli = Cli::try_parse_from(["spout", "ls"]).unwrap();
        match cli.command {
            Commands::Ls { project, no_tui } => {
                assert_eq!(project, None);
                assert!(!no_tui);
            }
            other => panic!("expected Ls, got {other:?}"),
        }
    }

    #[test]
    fn parses_ls_project_flag_without_value_means_current() {
        let cli = Cli::try_parse_from(["spout", "ls", "--project"]).unwrap();
        match cli.command {
            Commands::Ls { project, .. } => assert_eq!(project, Some(None)),
            other => panic!("expected Ls, got {other:?}"),
        }
    }

    #[test]
    fn parses_ls_project_flag_with_name() {
        let cli = Cli::try_parse_from(["spout", "ls", "--project", "foo"]).unwrap();
        match cli.command {
            Commands::Ls { project, .. } => assert_eq!(project, Some(Some("foo".to_owned()))),
            other => panic!("expected Ls, got {other:?}"),
        }
    }

    #[test]
    fn parses_ls_no_tui_flag() {
        let cli = Cli::try_parse_from(["spout", "ls", "--no-tui"]).unwrap();
        match cli.command {
            Commands::Ls { project, no_tui } => {
                assert_eq!(project, None);
                assert!(no_tui);
            }
            other => panic!("expected Ls, got {other:?}"),
        }
    }

    #[test]
    fn parses_env_bare_means_current_project() {
        let cli = Cli::try_parse_from(["spout", "env"]).unwrap();
        match cli.command {
            Commands::Env { project } => assert_eq!(project, None),
            other => panic!("expected Env, got {other:?}"),
        }
    }

    #[test]
    fn parses_env_with_project_name() {
        let cli = Cli::try_parse_from(["spout", "env", "--project", "foo"]).unwrap();
        match cli.command {
            Commands::Env { project } => assert_eq!(project, Some(Some("foo".to_owned()))),
            other => panic!("expected Env, got {other:?}"),
        }
    }

    #[test]
    fn parses_completions_for_bash() {
        let cli = Cli::try_parse_from(["spout", "completions", "bash"]).unwrap();
        match cli.command {
            Commands::Completions { shell } => assert_eq!(shell, Shell::Bash),
            other => panic!("expected Completions, got {other:?}"),
        }
    }

    #[test]
    fn verbose_is_global() {
        // -v before the subcommand
        let cli = Cli::try_parse_from(["spout", "-v", "get", "postgres"]).unwrap();
        assert!(cli.verbose);
        // -v after the subcommand
        let cli = Cli::try_parse_from(["spout", "get", "postgres", "-v"]).unwrap();
        assert!(cli.verbose);
    }
}
