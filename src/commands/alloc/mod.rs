//! `spout alloc` — single-service port allocation today; the
//! `compose` submodule parses docker-compose files for batch alloc
//! in Commit 4.

use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::protocol::Protocol;

mod compose;

pub fn alloc(registry_path: &Path, service: &str, protocol: Protocol) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    allocator::alloc(registry_path, &project, service, protocol)
}
