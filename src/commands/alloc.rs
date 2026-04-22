//! `spout alloc` — single-service port allocation.
//!
//! Compose-file-driven batch allocation will land here as a sibling
//! `compose` submodule in the next commits of Stage 7.

use std::path::Path;

use crate::allocator;
use crate::error::SpoutError;
use crate::project;
use crate::protocol::Protocol;

pub fn alloc(registry_path: &Path, service: &str, protocol: Protocol) -> Result<u16, SpoutError> {
    let project = project::current_project()?;
    allocator::alloc(registry_path, &project, service, protocol)
}
