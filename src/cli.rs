//! Command-line argument definitions. No logic — only shape.
//!
//! `[READ ONLY]` / `[MUTATES REGISTRY]` annotations in doc comments are
//! intentional: clap renders them in help output, and agents pattern-match
//! on them to reason about which commands are safe to call speculatively.

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

    /// Register a new port (idempotent) [MUTATES REGISTRY]
    Alloc { service: String },

    /// Register a specific port manually [MUTATES REGISTRY]
    Set { service: String, port: u16 },

    /// Remove a registration [MUTATES REGISTRY]
    Rm { service: String },

    /// List all registrations [READ ONLY]
    Ls {
        /// Filter to the current project only
        #[arg(long)]
        project: bool,
    },

    /// Check if a port is free on the OS (exit 0 free, 1 taken) [READ ONLY]
    Check { port: u16 },

    /// Reverse lookup — which project/service owns this port? [READ ONLY]
    Whois {
        port: u16,
        /// Include released ports from the history
        #[arg(long)]
        history: bool,
    },

    /// Generate a shell completion script for the given shell
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
            Commands::Alloc { service } => assert_eq!(service, "postgres"),
            other => panic!("expected Alloc, got {other:?}"),
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
    fn parses_ls_project_flag() {
        let cli = Cli::try_parse_from(["spout", "ls", "--project"]).unwrap();
        match cli.command {
            Commands::Ls { project } => assert!(project),
            other => panic!("expected Ls, got {other:?}"),
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
