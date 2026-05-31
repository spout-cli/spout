use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn rm_current(path: &Path, service: &str) -> Result<String, SpoutError> {
    let target = RmTarget::Service {
        name: service.into(),
        project: None,
    };
    rm(path, target, RmOptions::default())
}

fn temp_registry() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("spout.json");
    (dir, path)
}

#[test]
fn get_returns_service_not_registered_when_empty() {
    let (_dir, path) = temp_registry();
    let err = get(&path, "postgres", None).unwrap_err();
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn get_failure_in_empty_project_suggests_alloc() {
    let (_dir, path) = temp_registry();
    let err = get(&path, "postgres", None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no services currently registered"),
        "got: {msg}"
    );
    assert!(msg.contains("spout alloc postgres"), "got: {msg}");
}

#[test]
fn get_failure_in_populated_project_lists_available_services() {
    let (_dir, path) = temp_registry();
    alloc(&path, "postgres", Protocol::default()).unwrap();
    alloc(&path, "redis", Protocol::default()).unwrap();
    let err = get(&path, "acme-postgres", None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("acme-postgres"),
        "missing wrong name in: {msg}"
    );
    assert!(msg.contains("postgres"), "missing real name in: {msg}");
    assert!(msg.contains("redis"), "missing redis in: {msg}");
    assert!(msg.contains("spout env"), "missing env hint in: {msg}");
}

#[test]
fn get_failure_includes_recently_removed_when_history_exists() {
    let (_dir, path) = temp_registry();
    alloc(&path, "postgres", Protocol::default()).unwrap();
    alloc(&path, "api", Protocol::default()).unwrap();
    rm_current(&path, "api").unwrap();
    let err = get(&path, "api", None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("recently removed: api"),
        "missing removed line in: {msg}"
    );
    assert!(msg.contains("user requested"), "missing reason in: {msg}");
    assert!(
        msg.contains("available: postgres"),
        "missing available list in: {msg}"
    );
}

#[test]
fn get_failure_picks_most_recent_when_multiple_removals() {
    let (_dir, path) = temp_registry();
    let proj = project::current_project().unwrap();
    registry::with_lock(&path, |r| {
        r.history.push(crate::registry::HistoryEntry {
            project: proj.clone(),
            service: "api".into(),
            port: 20_000,
            allocated: "2025-12-01".into(),
            released: "2026-01-01".into(),
            reason: "old".into(),
            protocol: Protocol::default(),
        });
        r.history.push(crate::registry::HistoryEntry {
            project: proj.clone(),
            service: "api".into(),
            port: 20_001,
            allocated: "2026-04-01".into(),
            released: "2026-04-26".into(),
            reason: "newer".into(),
            protocol: Protocol::default(),
        });
        Ok(())
    })
    .unwrap();
    let err = get(&path, "api", None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("2026-04-26"), "expected newest date in: {msg}");
    assert!(msg.contains("\"newer\""), "expected newer reason in: {msg}");
    assert!(!msg.contains("2026-01-01"), "older date leaked into: {msg}");
    assert!(!msg.contains("\"old\""), "older reason leaked into: {msg}");
}

#[test]
fn get_failure_ignores_history_from_other_projects() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.history.push(crate::registry::HistoryEntry {
            project: "some-other-project".into(),
            service: "api".into(),
            port: 20_000,
            allocated: "2026-04-01".into(),
            released: "2026-04-26".into(),
            reason: "user requested".into(),
            protocol: Protocol::default(),
        });
        Ok(())
    })
    .unwrap();
    let err = get(&path, "api", None).unwrap_err();
    let msg = err.to_string();
    assert!(
        !msg.contains("recently removed"),
        "cross-project history leaked into: {msg}"
    );
}

#[test]
fn get_failure_in_empty_project_with_history_uses_alloc_fresh_hint() {
    let (_dir, path) = temp_registry();
    alloc(&path, "api", Protocol::default()).unwrap();
    rm_current(&path, "api").unwrap();
    let err = get(&path, "api", None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no services currently registered"),
        "missing empty marker in: {msg}"
    );
    assert!(
        msg.contains("recently removed: api"),
        "missing removed line in: {msg}"
    );
    assert!(
        msg.contains("`spout alloc api` to register fresh"),
        "missing fresh hint in: {msg}"
    );
}

#[test]
fn get_with_explicit_project_reads_from_that_project() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.set("p1", "postgres", 20_000, Protocol::default());
        r.set("p2", "postgres", 20_001, Protocol::default());
        Ok(())
    })
    .unwrap();
    assert_eq!(get(&path, "postgres", Some("p1")).unwrap(), 20_000);
    assert_eq!(get(&path, "postgres", Some("p2")).unwrap(), 20_001);
}

#[test]
fn alloc_then_get_returns_same_port() {
    let (_dir, path) = temp_registry();
    let allocated = alloc(&path, "postgres", Protocol::default()).unwrap();
    let fetched = get(&path, "postgres", None).unwrap();
    assert_eq!(allocated, fetched);
}

#[test]
fn set_registers_port_for_current_project() {
    let (_dir, path) = temp_registry();
    set(&path, "web", 25_000, Protocol::default()).unwrap();
    let port = get(&path, "web", None).unwrap();
    assert_eq!(port, 25_000);
}

#[test]
fn set_rejects_privileged_port() {
    let (_dir, path) = temp_registry();
    let err = set(&path, "web", 80, Protocol::default()).unwrap_err();
    assert_eq!(err.exit_code(), 6);
}

#[test]
fn rm_removes_and_appends_to_history() {
    let (_dir, path) = temp_registry();
    alloc(&path, "postgres", Protocol::default()).unwrap();
    rm_current(&path, "postgres").unwrap();
    assert!(matches!(
        get(&path, "postgres", None).unwrap_err(),
        SpoutError::ServiceNotRegisteredInProject { .. }
    ));
    let reg = registry::read(&path).unwrap();
    assert_eq!(reg.history.len(), 1);
    assert_eq!(reg.history[0].reason, "user requested");
}

#[test]
fn rm_unregistered_service_errors() {
    let (_dir, path) = temp_registry();
    let err = rm_current(&path, "nothing").unwrap_err();
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn ls_empty_registry_is_descriptive() {
    let (_dir, path) = temp_registry();
    let out = ls(&path, None, true).unwrap();
    assert!(out.contains("no registrations"));
}

#[test]
fn ls_shows_project_and_services_after_alloc() {
    let (_dir, path) = temp_registry();
    alloc(&path, "postgres", Protocol::default()).unwrap();
    alloc(&path, "redis", Protocol::default()).unwrap();
    let out = ls(&path, None, true).unwrap();
    assert!(out.contains("postgres"));
    assert!(out.contains("redis"));
}

#[test]
fn ls_filters_to_named_project() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.set("alpha", "postgres", 20_000, Protocol::default());
        r.set("beta", "redis", 20_001, Protocol::default());
        Ok(())
    })
    .unwrap();
    let out = ls(&path, Some(Some("alpha".to_owned())), true).unwrap();
    assert!(out.contains("alpha"));
    assert!(out.contains("postgres"));
    assert!(!out.contains("redis"));
}

#[test]
fn env_unknown_project_returns_none() {
    let (_dir, path) = temp_registry();
    assert!(env(&path, Some(Some("never-existed".to_owned())))
        .unwrap()
        .is_none());
}

#[test]
fn env_named_project_emits_sorted_key_value_lines() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.set("myproj", "redis", 20_001, Protocol::default());
        r.set("myproj", "postgres", 20_000, Protocol::default());
        r.set("myproj", "mailpit-smtp", 20_002, Protocol::default());
        Ok(())
    })
    .unwrap();
    let out = env(&path, Some(Some("myproj".to_owned())))
        .unwrap()
        .unwrap();
    assert_eq!(
        out,
        "MAILPIT_SMTP_PORT=20002\nPOSTGRES_PORT=20000\nREDIS_PORT=20001"
    );
}

#[test]
fn env_current_project_after_alloc_contains_the_service() {
    let (_dir, path) = temp_registry();
    let port = alloc(&path, "postgres", Protocol::default()).unwrap();
    let out = env(&path, None).unwrap().unwrap();
    assert!(out.contains(&format!("POSTGRES_PORT={port}")));
}

#[test]
fn env_named_project_with_no_services_returns_none() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.set("proj", "svc", 20_000, Protocol::default());
        r.remove("proj", "svc", "test").unwrap();
        Ok(())
    })
    .unwrap();
    assert!(env(&path, Some(Some("proj".to_owned()))).unwrap().is_none());
}

#[test]
fn check_returns_false_when_port_is_bound() {
    use std::net::TcpListener;
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(!check(port, Protocol::default()));
    // `l` drops at end of scope; no race window.
}

#[test]
fn whois_returns_active_registration() {
    let (_dir, path) = temp_registry();
    let port = alloc(&path, "postgres", Protocol::default()).unwrap();
    let result = whois(&path, port, false).unwrap().unwrap();
    assert!(result.contains("postgres"));
    assert!(result.contains("active"));
}

#[test]
fn whois_returns_none_when_unknown() {
    let (_dir, path) = temp_registry();
    assert!(whois(&path, 30_000, false).unwrap().is_none());
}

#[test]
fn whois_lists_both_protocols_tcp_first() {
    let (_dir, path) = temp_registry();
    registry::with_lock(&path, |r| {
        r.set("p", "tcp-svc", 20_000, Protocol::Tcp);
        r.set("p", "udp-svc", 20_000, Protocol::Udp);
        Ok(())
    })
    .unwrap();
    let out = whois(&path, 20_000, false).unwrap().unwrap();
    let tcp_pos = out.find("20000/tcp").expect("tcp row missing");
    let udp_pos = out.find("20000/udp").expect("udp row missing");
    assert!(tcp_pos < udp_pos, "expected tcp before udp, got:\n{out}");
}

#[test]
fn whois_history_finds_released_ports() {
    let (_dir, path) = temp_registry();
    let port = alloc(&path, "postgres", Protocol::default()).unwrap();
    rm_current(&path, "postgres").unwrap();
    assert!(whois(&path, port, false).unwrap().is_none()); // not in live
    let hit = whois(&path, port, true).unwrap().unwrap(); // in history
    assert!(hit.contains("postgres"));
    assert!(hit.contains("user requested"));
}
