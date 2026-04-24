use super::*;
use tempfile::TempDir;

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
fn format_summary_one_service_uses_singular() {
    let out = format_compose_summary(
        &files_base(PathBuf::from("docker-compose.yml")),
        &[Allocation {
            name: "api",
            port: 20_000,
            protocol: Protocol::Tcp,
            is_new: true,
        }],
    );
    assert!(out.contains("1 service allocated"));
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
                name: "a",
                port: 20_000,
                protocol: Protocol::Tcp,
                is_new: true,
            },
            Allocation {
                name: "b",
                port: 20_001,
                protocol: Protocol::Udp,
                is_new: false,
            },
        ],
    );
    assert!(out.contains("2 services (1 new, 1 existing)"));
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
            name: "api",
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
