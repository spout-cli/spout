mod date;
mod error;
mod project;
mod registry;
mod services;

use error::SpoutError;
use std::process::exit;

fn main() {
    if let Err(e) = run() {
        eprintln!("spout: {e}");
        exit(e.exit_code());
    }
}

fn run() -> Result<(), SpoutError> {
    Ok(())
}
