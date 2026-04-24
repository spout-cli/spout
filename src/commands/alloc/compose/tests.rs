use super::*;

fn names(services: &[ComposeService]) -> Vec<&str> {
    services.iter().map(|s| s.name.as_str()).collect()
}

fn port(container_port: u16, protocol: Protocol) -> ComposePort {
    ComposePort {
        container_port,
        protocol,
    }
}

fn svc(name: &str, ports: Vec<ComposePort>) -> ComposeService {
    ComposeService {
        name: name.to_string(),
        ports,
    }
}

fn parse_ok(yaml: &str) -> Vec<ComposeService> {
    parse(yaml).unwrap().0
}

#[test]
fn empty_doc_yields_empty_vec() {
    assert!(parse_ok("").is_empty());
    assert!(parse_ok("services: {}").is_empty());
}

#[test]
fn short_form_numeric_is_tcp() {
    let yaml = r#"
        services:
          postgres:
            ports: ["5432"]
    "#;
    let got = parse_ok(yaml);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "postgres");
    assert_eq!(got[0].ports, vec![port(5432, Protocol::Tcp)]);
}

#[test]
fn host_container_form_is_tcp() {
    let yaml = r#"
        services:
          postgres:
            ports: ["5432:5432"]
    "#;
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(5432, Protocol::Tcp)]);
}

#[test]
fn slash_udp_suffix_is_udp() {
    let yaml = r#"
        services:
          dns:
            ports: ["53:53/udp"]
    "#;
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(53, Protocol::Udp)]);
}

#[test]
fn bind_ip_shorthand_parses_as_tcp() {
    let yaml = r#"
        services:
          pg:
            ports: ["127.0.0.1:5432:5432"]
    "#;
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(5432, Protocol::Tcp)]);
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
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(8080, Protocol::Udp)]);
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
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(8080, Protocol::Tcp)]);
}

#[test]
fn long_form_missing_target_is_dropped() {
    let yaml = r#"
        services:
          api:
            ports:
              - protocol: tcp
    "#;
    let (services, warnings) = parse(yaml).unwrap();
    assert!(services.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("unparseable"));
}

#[test]
fn numeric_port_without_quotes_is_tcp() {
    let yaml = r#"
        services:
          api:
            ports:
              - 8080
    "#;
    assert_eq!(parse_ok(yaml)[0].ports, vec![port(8080, Protocol::Tcp)]);
}

#[test]
fn port_range_shorthand_is_unparseable() {
    let yaml = r#"
        services:
          api:
            ports: ["9000-9005:9000-9005"]
    "#;
    let (services, warnings) = parse(yaml).unwrap();
    assert!(services.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("'api'"));
    assert!(warnings[0].contains("#1"));
}

#[test]
fn non_numeric_container_port_is_unparseable() {
    let yaml = r#"
        services:
          api:
            ports: ["abc"]
    "#;
    let (services, warnings) = parse(yaml).unwrap();
    assert!(services.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn unknown_protocol_suffix_is_unparseable() {
    let yaml = r#"
        services:
          api:
            ports: ["80:80/sctp"]
    "#;
    let (services, warnings) = parse(yaml).unwrap();
    assert!(services.is_empty());
    assert_eq!(warnings.len(), 1);
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
    assert_eq!(names(&parse_ok(yaml)), vec!["postgres"]);
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
    assert_eq!(names(&parse_ok(yaml)), vec!["postgres"]);
}

#[test]
fn multi_port_service_records_all_ports_in_order() {
    let yaml = r#"
        services:
          api:
            ports:
              - "8080"
              - "9229"
              - "9230"
    "#;
    let got = parse_ok(yaml);
    assert_eq!(
        got[0].ports,
        vec![
            port(8080, Protocol::Tcp),
            port(9229, Protocol::Tcp),
            port(9230, Protocol::Tcp),
        ]
    );
}

#[test]
fn multi_port_preserves_mixed_protocols() {
    let yaml = r#"
        services:
          dns:
            ports: ["53:53/udp", "53:53/tcp"]
    "#;
    let got = parse_ok(yaml);
    assert_eq!(
        got[0].ports,
        vec![port(53, Protocol::Udp), port(53, Protocol::Tcp)]
    );
}

#[test]
fn unparseable_spec_keeps_valid_siblings() {
    let yaml = r#"
        services:
          api:
            ports:
              - "8080"
              - "9000-9005:9000-9005"
              - "9090"
    "#;
    let (services, warnings) = parse(yaml).unwrap();
    assert_eq!(
        services[0].ports,
        vec![port(8080, Protocol::Tcp), port(9090, Protocol::Tcp)]
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("#2"));
}

#[test]
fn malformed_yaml_is_compose_invalid() {
    let err = parse("services:\n  postgres:\n    ports: [[[[").unwrap_err();
    assert!(matches!(err, SpoutError::ComposeInvalid(_)));
}

#[test]
fn services_field_missing_is_ok_returns_empty() {
    assert!(parse_ok("version: '3'").is_empty());
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
    let got = parse_ok(yaml);
    let mut got_names = names(&got);
    got_names.sort();
    assert_eq!(got_names, vec!["coredns", "postgres", "redis"]);
}

#[test]
fn merge_overlay_adds_service_not_in_base() {
    let base = vec![svc("postgres", vec![port(5432, Protocol::Tcp)])];
    let overlay = vec![svc("api", vec![port(8080, Protocol::Tcp)])];
    let merged = merge_services(base, overlay);
    assert_eq!(names(&merged), vec!["api", "postgres"]);
}

#[test]
fn merge_overlay_wins_when_both_declare_service() {
    let base = vec![svc("api", vec![port(8080, Protocol::Tcp)])];
    let overlay = vec![svc(
        "api",
        vec![port(8080, Protocol::Udp), port(9000, Protocol::Tcp)],
    )];
    let merged = merge_services(base, overlay);
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].ports,
        vec![port(8080, Protocol::Udp), port(9000, Protocol::Tcp)]
    );
}

#[test]
fn merge_preserves_base_only_services() {
    let base = vec![
        svc("postgres", vec![port(5432, Protocol::Tcp)]),
        svc("redis", vec![port(6379, Protocol::Tcp)]),
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
        svc("redis", vec![port(6379, Protocol::Tcp)]),
        svc("alpha", vec![port(1, Protocol::Tcp)]),
    ];
    let overlay = vec![
        svc("zulu", vec![port(2, Protocol::Tcp)]),
        svc("bravo", vec![port(3, Protocol::Tcp)]),
    ];
    let merged = merge_services(base, overlay);
    assert_eq!(names(&merged), vec!["alpha", "bravo", "redis", "zulu"]);
}

#[test]
fn merge_protocol_follows_winning_file() {
    let base = vec![svc("coredns", vec![port(53, Protocol::Tcp)])];
    let overlay = vec![svc("coredns", vec![port(53, Protocol::Udp)])];
    let merged = merge_services(base, overlay);
    assert_eq!(merged[0].ports, vec![port(53, Protocol::Udp)]);
}
