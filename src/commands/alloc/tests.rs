use super::*;
use crate::registry;
use compose::ComposePort;
use tempfile::TempDir;

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

fn temp_registry() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("spout.json");
    (dir, path)
}

fn write_compose(dir: &Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

fn basic_compose() -> &'static str {
    r#"
services:
  postgres:
    ports: ["5432"]
  redis:
    ports: ["6379"]
  dns:
    ports: ["53:53/udp"]
"#
}

fn files_base(p: PathBuf) -> ComposeFiles {
    ComposeFiles {
        base: p,
        overlay: None,
    }
}

#[test]
fn discover_finds_docker_compose_yml() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "docker-compose.yml", basic_compose());
    let got = discover_compose(dir.path(), None).unwrap();
    assert!(got.base.ends_with("docker-compose.yml"));
    assert!(got.overlay.is_none());
}

#[test]
fn discover_falls_through_to_compose_yaml() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "compose.yaml", basic_compose());
    let got = discover_compose(dir.path(), None).unwrap();
    assert!(got.base.ends_with("compose.yaml"));
}

#[test]
fn discover_prefers_docker_compose_yml_over_compose_yaml() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "docker-compose.yml", basic_compose());
    write_compose(dir.path(), "compose.yaml", basic_compose());
    let got = discover_compose(dir.path(), None).unwrap();
    assert!(got.base.ends_with("docker-compose.yml"));
}

#[test]
fn discover_honours_explicit_path() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("prod.yml");
    std::fs::write(&p, basic_compose()).unwrap();
    let got = discover_compose(dir.path(), Some(&p)).unwrap();
    assert_eq!(got.base, p);
    assert!(got.overlay.is_none());
}

#[test]
fn discover_missing_file_is_compose_not_found() {
    let dir = TempDir::new().unwrap();
    let err = discover_compose(dir.path(), None).unwrap_err();
    assert!(matches!(err, SpoutError::ComposeNotFound(_)));
    assert_eq!(err.exit_code(), 8);
}

#[test]
fn discover_missing_explicit_path_is_compose_not_found() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("does-not-exist.yml");
    let err = discover_compose(dir.path(), Some(&missing)).unwrap_err();
    assert!(matches!(err, SpoutError::ComposeNotFound(_)));
}

#[test]
fn discover_returns_base_and_override_when_both_exist() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "docker-compose.yml", basic_compose());
    write_compose(dir.path(), "docker-compose.override.yml", basic_compose());
    let got = discover_compose(dir.path(), None).unwrap();
    assert!(got.base.ends_with("docker-compose.yml"));
    assert!(got
        .overlay
        .as_ref()
        .is_some_and(|o| o.ends_with("docker-compose.override.yml")));
}

#[test]
fn discover_pairs_base_with_mismatched_override_extension() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "docker-compose.yml", basic_compose());
    write_compose(dir.path(), "compose.override.yaml", basic_compose());
    let got = discover_compose(dir.path(), None).unwrap();
    assert!(got.base.ends_with("docker-compose.yml"));
    assert!(got
        .overlay
        .as_ref()
        .is_some_and(|o| o.ends_with("compose.override.yaml")));
}

#[test]
fn discover_override_without_base_is_friendly_error() {
    let dir = TempDir::new().unwrap();
    write_compose(dir.path(), "docker-compose.override.yml", basic_compose());
    let err = discover_compose(dir.path(), None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("override"), "got {msg:?}");
    assert!(msg.contains("no base"), "got {msg:?}");
    assert_eq!(err.exit_code(), 8);
}

#[test]
fn discover_explicit_path_ignores_override_in_cwd() {
    let dir = TempDir::new().unwrap();
    let explicit = dir.path().join("prod.yml");
    std::fs::write(&explicit, basic_compose()).unwrap();
    write_compose(dir.path(), "docker-compose.override.yml", basic_compose());
    let got = discover_compose(dir.path(), Some(&explicit)).unwrap();
    assert_eq!(got.base, explicit);
    assert!(got.overlay.is_none());
}

#[test]
fn format_summary_one_port_uses_singular() {
    let out = format_compose_summary(
        &files_base(PathBuf::from("docker-compose.yml")),
        &[Allocation {
            name: "api".to_string(),
            port: 20_000,
            protocol: Protocol::Tcp,
            is_new: true,
        }],
    );
    assert!(out.contains("1 port allocated"));
    assert!(out.contains("api"));
    assert!(out.contains("20000"));
    assert!(out.contains("tcp"));
}

#[test]
fn format_summary_mixed_new_and_existing() {
    let out = format_compose_summary(
        &files_base(PathBuf::from("docker-compose.yml")),
        &[
            Allocation {
                name: "a".to_string(),
                port: 20_000,
                protocol: Protocol::Tcp,
                is_new: true,
            },
            Allocation {
                name: "b".to_string(),
                port: 20_001,
                protocol: Protocol::Udp,
                is_new: false,
            },
        ],
    );
    assert!(out.contains("2 ports (1 new, 1 existing)"));
    assert!(out.contains("udp"));
}

#[test]
fn format_summary_cites_both_files_when_override_present() {
    let files = ComposeFiles {
        base: PathBuf::from("docker-compose.yml"),
        overlay: Some(PathBuf::from("docker-compose.override.yml")),
    };
    let out = format_compose_summary(
        &files,
        &[Allocation {
            name: "api".to_string(),
            port: 20_000,
            protocol: Protocol::Tcp,
            is_new: true,
        }],
    );
    assert!(out.contains("docker-compose.yml"));
    assert!(out.contains("docker-compose.override.yml"));
    assert!(out.contains(" + "));
}

#[test]
fn load_and_merge_overlay_adds_services_missing_from_base() {
    let dir = TempDir::new().unwrap();
    let base = r#"
services:
  postgres:
    image: postgres:15
  api:
    image: api
"#;
    let overlay = r#"
services:
  postgres:
    ports: ["5432:5432"]
  api:
    ports: ["8080:8080"]
"#;
    write_compose(dir.path(), "docker-compose.yml", base);
    write_compose(dir.path(), "docker-compose.override.yml", overlay);
    let files = discover_compose(dir.path(), None).unwrap();
    let (services, _warnings) = load_and_merge(&files).unwrap();
    let mut names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["api", "postgres"]);
}

#[test]
fn load_and_merge_overlay_wins_on_port_conflict() {
    let dir = TempDir::new().unwrap();
    let base = r#"
services:
  coredns:
    ports: ["53:53"]
"#;
    let overlay = r#"
services:
  coredns:
    ports: ["53:53/udp"]
"#;
    write_compose(dir.path(), "docker-compose.yml", base);
    write_compose(dir.path(), "docker-compose.override.yml", overlay);
    let files = discover_compose(dir.path(), None).unwrap();
    let (services, _warnings) = load_and_merge(&files).unwrap();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].ports.len(), 1);
    assert_eq!(services[0].ports[0].protocol, Protocol::Udp);
}

#[test]
fn build_allocations_registers_every_port_of_multi_port_service() {
    let (_dir, path) = temp_registry();
    let services = vec![svc(
        "mailpit",
        vec![port(8025, Protocol::Tcp), port(1025, Protocol::Tcp)],
    )];
    let mut warnings = Vec::new();
    let allocs = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    assert_eq!(allocs.len(), 2);
    assert_eq!(allocs[0].name, "mailpit");
    assert_eq!(allocs[1].name, "mailpit-1025");
    assert_ne!(allocs[0].port, allocs[1].port);
    let reg = registry::read(&path).unwrap();
    assert_eq!(reg.get("proj", "mailpit"), Some(allocs[0].port));
    assert_eq!(reg.get("proj", "mailpit-1025"), Some(allocs[1].port));
    assert!(warnings.is_empty());
}

#[test]
fn build_allocations_multi_port_is_idempotent() {
    let (_dir, path) = temp_registry();
    let services = vec![svc(
        "mailpit",
        vec![port(8025, Protocol::Tcp), port(1025, Protocol::Tcp)],
    )];
    let mut warnings = Vec::new();
    let first = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    let second = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    assert_eq!(first[0].port, second[0].port);
    assert_eq!(first[1].port, second[1].port);
    assert!(first.iter().all(|a| a.is_new));
    assert!(second.iter().all(|a| !a.is_new));
}

#[test]
fn build_allocations_multi_port_mixed_tcp_udp() {
    let (_dir, path) = temp_registry();
    let services = vec![svc(
        "dns",
        vec![port(53, Protocol::Tcp), port(53, Protocol::Udp)],
    )];
    let mut warnings = Vec::new();
    let allocs = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    assert_eq!(allocs.len(), 2);
    assert_eq!(allocs[0].name, "dns");
    assert_eq!(allocs[0].protocol, Protocol::Tcp);
    assert_eq!(allocs[1].name, "dns-53");
    assert_eq!(allocs[1].protocol, Protocol::Udp);
}

#[test]
fn build_allocations_single_port_service_keeps_bare_name() {
    let (_dir, path) = temp_registry();
    let services = vec![svc("postgres", vec![port(5432, Protocol::Tcp)])];
    let mut warnings = Vec::new();
    let allocs = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    assert_eq!(allocs.len(), 1);
    assert_eq!(allocs[0].name, "postgres");
}

#[test]
fn build_allocations_duplicate_container_port_skipped_with_warning() {
    // Pathological input — same container port declared twice after the
    // first. The duplicate would collide with itself on the suffix
    // naming, so we skip it rather than invent a hidden discriminator.
    let (_dir, path) = temp_registry();
    let services = vec![svc(
        "svc",
        vec![
            port(80, Protocol::Tcp),
            port(80, Protocol::Udp),
            port(80, Protocol::Udp),
        ],
    )];
    let mut warnings = Vec::new();
    let allocs = build_allocations(&path, "proj", &services, &mut warnings).unwrap();
    assert_eq!(allocs.len(), 2);
    assert_eq!(allocs[0].name, "svc");
    assert_eq!(allocs[1].name, "svc-80");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("'svc'"));
    assert!(warnings[0].contains("80"));
}
