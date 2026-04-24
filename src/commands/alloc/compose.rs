//! Parse a docker-compose file into the minimal shape the allocator
//! needs: one entry per service, carrying every declared `(container
//! port, protocol)` pair. No filesystem access, no lock access — just
//! `&str` in, `(Vec<ComposeService>, Vec<String>)` out where the second
//! element holds per-spec parse warnings for the caller to print.

use serde::Deserialize;

use crate::error::SpoutError;
use crate::protocol::Protocol;

#[derive(Debug, PartialEq)]
pub(super) struct ComposeService {
    pub name: String,
    /// Declaration order as found in the compose file. Non-empty after
    /// `service_entry` — services whose ports all failed to parse are
    /// dropped entirely.
    pub ports: Vec<ComposePort>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(super) struct ComposePort {
    pub container_port: u16,
    pub protocol: Protocol,
}

pub(super) fn parse(yaml: &str) -> Result<(Vec<ComposeService>, Vec<String>), SpoutError> {
    let doc: ComposeDoc = serde_yaml_ng::from_str(yaml)
        .map_err(|e| SpoutError::ComposeInvalid(format!("parse failed: {e}")))?;
    let mut warnings = Vec::new();
    let services = doc
        .services
        .into_iter()
        .filter_map(|(name, def)| service_entry(name, def, &mut warnings))
        .collect();
    Ok((services, warnings))
}

/// Override-wins per service. Output is alphabetical.
pub(super) fn merge_services(
    base: Vec<ComposeService>,
    overlay: Vec<ComposeService>,
) -> Vec<ComposeService> {
    let mut merged: std::collections::BTreeMap<String, ComposeService> =
        base.into_iter().map(|s| (s.name.clone(), s)).collect();
    for svc in overlay {
        merged.insert(svc.name.clone(), svc);
    }
    merged.into_values().collect()
}

fn service_entry(
    name: String,
    def: ServiceDef,
    warnings: &mut Vec<String>,
) -> Option<ComposeService> {
    let specs = def.ports?;
    let mut ports = Vec::with_capacity(specs.len());
    for (idx, spec) in specs.iter().enumerate() {
        match parse_port(spec) {
            Some(p) => ports.push(p),
            None => warnings.push(format!(
                "'{name}' port spec #{} is unparseable; skipping",
                idx + 1
            )),
        }
    }
    if ports.is_empty() {
        return None;
    }
    Some(ComposeService { name, ports })
}

fn parse_port(spec: &PortSpec) -> Option<ComposePort> {
    match spec {
        PortSpec::Short(s) => parse_short(s),
        PortSpec::Numeric(n) => u16::try_from(*n).ok().map(|container_port| ComposePort {
            container_port,
            protocol: Protocol::Tcp,
        }),
        PortSpec::Long {
            target, protocol, ..
        } => target.map(|container_port| ComposePort {
            container_port,
            protocol: match protocol.as_deref() {
                Some(p) if p.eq_ignore_ascii_case("udp") => Protocol::Udp,
                _ => Protocol::Tcp,
            },
        }),
    }
}

/// Short-form grammar: `[[host_ip:]host_port:]container_port[/protocol]`.
/// Ranges (`9000-9005`) and non-numeric tokens return `None` — the caller
/// skips them with a warning.
fn parse_short(s: &str) -> Option<ComposePort> {
    let (port_part, protocol) = match s.rsplit_once('/') {
        Some((left, proto)) if proto.eq_ignore_ascii_case("udp") => (left, Protocol::Udp),
        Some((left, proto)) if proto.eq_ignore_ascii_case("tcp") => (left, Protocol::Tcp),
        Some(_) => return None,
        None => (s, Protocol::Tcp),
    };
    let container_token = port_part.rsplit(':').next()?;
    let container_port = container_token.parse::<u16>().ok()?;
    Some(ComposePort {
        container_port,
        protocol,
    })
}

#[derive(Deserialize)]
struct ComposeDoc {
    #[serde(default)]
    services: std::collections::BTreeMap<String, ServiceDef>,
}

#[derive(Deserialize)]
struct ServiceDef {
    #[serde(default)]
    ports: Option<Vec<PortSpec>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
#[allow(dead_code)] // field payloads consumed only as deserialize anchors
enum PortSpec {
    Numeric(u64),
    Short(String),
    Long {
        target: Option<u16>,
        #[serde(default)]
        protocol: Option<String>,
        #[serde(flatten)]
        extra: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
    },
}

#[cfg(test)]
mod tests;
