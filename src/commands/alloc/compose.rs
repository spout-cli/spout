//! Parse a docker-compose file into the minimal shape the allocator
//! needs: one entry per service, with protocol inferred from the
//! first port declaration. No filesystem access, no lock access —
//! just `&str` in, `Vec<ComposeService>` out.

use serde::Deserialize;

use crate::error::SpoutError;
use crate::protocol::Protocol;

#[derive(Debug, PartialEq)]
pub(super) struct ComposeService {
    pub name: String,
    pub protocol: Protocol,
    /// `N - 1` when a service declares N ports (for the "multi-port,
    /// allocating only the first" stderr warning in Commit 4). `0`
    /// otherwise.
    pub extra_ports: usize,
}

pub(super) fn parse(yaml: &str) -> Result<Vec<ComposeService>, SpoutError> {
    let doc: ComposeDoc = serde_yaml_ng::from_str(yaml)
        .map_err(|e| SpoutError::ComposeInvalid(format!("parse failed: {e}")))?;
    Ok(doc
        .services
        .into_iter()
        .filter_map(|(name, def)| service_entry(name, def))
        .collect())
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

fn service_entry(name: String, def: ServiceDef) -> Option<ComposeService> {
    let ports = def.ports?;
    if ports.is_empty() {
        return None;
    }
    let protocol = port_protocol(&ports[0]);
    Some(ComposeService {
        name,
        protocol,
        extra_ports: ports.len() - 1,
    })
}

fn port_protocol(spec: &PortSpec) -> Protocol {
    match spec {
        PortSpec::Short(s) => {
            if s.to_ascii_lowercase().contains("/udp") {
                Protocol::Udp
            } else {
                Protocol::Tcp
            }
        }
        PortSpec::Numeric(_) => Protocol::Tcp,
        PortSpec::Long { protocol, .. } => match protocol.as_deref() {
            Some(p) if p.eq_ignore_ascii_case("udp") => Protocol::Udp,
            _ => Protocol::Tcp,
        },
    }
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
mod tests {
    use super::*;

    fn names(services: &[ComposeService]) -> Vec<&str> {
        services.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn empty_doc_yields_empty_vec() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse("services: {}").unwrap().is_empty());
    }

    #[test]
    fn short_form_numeric_is_tcp() {
        let yaml = r#"
            services:
              postgres:
                ports: ["5432"]
        "#;
        let got = parse(yaml).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "postgres");
        assert_eq!(got[0].protocol, Protocol::Tcp);
        assert_eq!(got[0].extra_ports, 0);
    }

    #[test]
    fn host_container_form_is_tcp() {
        let yaml = r#"
            services:
              postgres:
                ports: ["5432:5432"]
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Tcp);
    }

    #[test]
    fn slash_udp_suffix_is_udp() {
        let yaml = r#"
            services:
              dns:
                ports: ["53:53/udp"]
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Udp);
    }

    #[test]
    fn bind_ip_shorthand_parses_as_tcp() {
        let yaml = r#"
            services:
              pg:
                ports: ["127.0.0.1:5432:5432"]
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Tcp);
    }

    #[test]
    fn long_form_with_udp_protocol() {
        let yaml = r#"
            services:
              api:
                ports:
                  - target: 8080
                    protocol: udp
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Udp);
    }

    #[test]
    fn long_form_default_protocol_is_tcp() {
        let yaml = r#"
            services:
              api:
                ports:
                  - target: 8080
                    published: 8080
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Tcp);
    }

    #[test]
    fn numeric_port_without_quotes_is_tcp() {
        let yaml = r#"
            services:
              api:
                ports:
                  - 8080
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Tcp);
    }

    #[test]
    fn services_without_ports_are_skipped() {
        let yaml = r#"
            services:
              postgres:
                ports: ["5432"]
              web:
                image: nginx
        "#;
        assert_eq!(names(&parse(yaml).unwrap()), vec!["postgres"]);
    }

    #[test]
    fn services_with_empty_ports_list_are_skipped() {
        let yaml = r#"
            services:
              api:
                ports: []
              postgres:
                ports: ["5432"]
        "#;
        assert_eq!(names(&parse(yaml).unwrap()), vec!["postgres"]);
    }

    #[test]
    fn multi_port_service_records_extra_port_count() {
        let yaml = r#"
            services:
              api:
                ports:
                  - "8080"
                  - "9229"
                  - "9230"
        "#;
        let got = parse(yaml).unwrap();
        assert_eq!(got[0].extra_ports, 2);
        assert_eq!(got[0].protocol, Protocol::Tcp);
    }

    #[test]
    fn multi_port_first_wins_for_protocol() {
        // First spec is UDP, second is TCP; first wins.
        let yaml = r#"
            services:
              dns:
                ports: ["53:53/udp", "53:53/tcp"]
        "#;
        assert_eq!(parse(yaml).unwrap()[0].protocol, Protocol::Udp);
    }

    #[test]
    fn malformed_yaml_is_compose_invalid() {
        let err = parse("services:\n  postgres:\n    ports: [[[[").unwrap_err();
        assert!(matches!(err, SpoutError::ComposeInvalid(_)));
    }

    #[test]
    fn services_field_missing_is_ok_returns_empty() {
        // A compose file with no top-level services key yields no
        // candidates. Not an error — the file is valid YAML.
        assert!(parse("version: '3'").unwrap().is_empty());
    }

    #[test]
    fn multiple_services_preserve_names() {
        let yaml = r#"
            services:
              postgres:
                ports: ["5432"]
              redis:
                ports: ["6379"]
              coredns:
                ports: ["53:53/udp"]
        "#;
        let got = parse(yaml).unwrap();
        let mut got_names = names(&got);
        got_names.sort();
        assert_eq!(got_names, vec!["coredns", "postgres", "redis"]);
    }

    fn svc(name: &str, protocol: Protocol, extra: usize) -> ComposeService {
        ComposeService {
            name: name.to_string(),
            protocol,
            extra_ports: extra,
        }
    }

    #[test]
    fn merge_overlay_adds_service_not_in_base() {
        let base = vec![svc("postgres", Protocol::Tcp, 0)];
        let overlay = vec![svc("api", Protocol::Tcp, 0)];
        let merged = merge_services(base, overlay);
        assert_eq!(names(&merged), vec!["api", "postgres"]);
    }

    #[test]
    fn merge_overlay_wins_when_both_declare_service() {
        let base = vec![svc("api", Protocol::Tcp, 0)];
        let overlay = vec![svc("api", Protocol::Udp, 2)];
        let merged = merge_services(base, overlay);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].protocol, Protocol::Udp);
        assert_eq!(merged[0].extra_ports, 2);
    }

    #[test]
    fn merge_preserves_base_only_services() {
        let base = vec![
            svc("postgres", Protocol::Tcp, 0),
            svc("redis", Protocol::Tcp, 0),
        ];
        let merged = merge_services(base, vec![]);
        assert_eq!(names(&merged), vec!["postgres", "redis"]);
    }

    #[test]
    fn merge_both_empty_is_empty() {
        assert!(merge_services(vec![], vec![]).is_empty());
    }

    #[test]
    fn merge_result_is_alphabetical() {
        let base = vec![
            svc("redis", Protocol::Tcp, 0),
            svc("alpha", Protocol::Tcp, 0),
        ];
        let overlay = vec![
            svc("zulu", Protocol::Tcp, 0),
            svc("bravo", Protocol::Tcp, 0),
        ];
        let merged = merge_services(base, overlay);
        assert_eq!(names(&merged), vec!["alpha", "bravo", "redis", "zulu"]);
    }

    #[test]
    fn merge_protocol_follows_winning_file() {
        // Base says TCP for coredns; overlay says UDP. Overlay wins.
        let base = vec![svc("coredns", Protocol::Tcp, 0)];
        let overlay = vec![svc("coredns", Protocol::Udp, 0)];
        let merged = merge_services(base, overlay);
        assert_eq!(merged[0].protocol, Protocol::Udp);
    }
}
