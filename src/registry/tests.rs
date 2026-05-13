use super::*;

#[test]
fn registry_set_get_remove_roundtrip() {
    let mut r = Registry::default();
    r.set("myproj", "postgres", 19456, Protocol::default());
    assert_eq!(r.get("myproj", "postgres"), Some(19456));

    let removed = r.remove("myproj", "postgres", "test");
    assert_eq!(removed, Some(19456));
    assert_eq!(r.get("myproj", "postgres"), None);
    assert_eq!(r.history.len(), 1);
    assert_eq!(r.history[0].port, 19456);
    assert_eq!(r.history[0].reason, "test");
}

#[test]
fn remove_carries_allocated_date_into_history() {
    let mut r = Registry::default();
    r.set("myproj", "postgres", 19456, Protocol::default());
    let live_allocated = r
        .projects
        .get("myproj")
        .unwrap()
        .get("postgres")
        .unwrap()
        .allocated
        .clone();
    r.remove("myproj", "postgres", "test");
    assert_eq!(r.history[0].allocated, live_allocated);
}

#[test]
fn remove_empties_project_entry() {
    let mut r = Registry::default();
    r.set("myproj", "postgres", 19456, Protocol::default());
    r.remove("myproj", "postgres", "test");
    assert!(!r.projects.contains_key("myproj"));
}

#[test]
fn is_port_claimed_finds_existing() {
    let mut r = Registry::default();
    r.set("myproj", "postgres", 19456, Protocol::default());
    let owner = r.is_port_claimed(19456, Protocol::Tcp).unwrap();
    assert_eq!(owner, ("myproj".to_owned(), "postgres".to_owned()));
}

#[test]
fn is_port_claimed_returns_none_for_free() {
    let r = Registry::default();
    assert!(r.is_port_claimed(19456, Protocol::Tcp).is_none());
}

#[test]
fn history_for_port_sorted_most_recent_first() {
    let mut r = Registry::default();
    r.history.push(HistoryEntry {
        project: "a".into(),
        service: "s".into(),
        port: 19456,
        allocated: "2025-09-01".into(),
        released: "2026-01-01".into(),
        reason: "x".into(),
        protocol: Protocol::default(),
    });
    r.history.push(HistoryEntry {
        project: "b".into(),
        service: "s".into(),
        port: 19456,
        allocated: "2026-02-01".into(),
        released: "2026-06-01".into(),
        reason: "y".into(),
        protocol: Protocol::default(),
    });
    let entries = r.history_for_port(19456);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].released, "2026-06-01");
    assert_eq!(entries[1].released, "2026-01-01");
}

fn history_entry(
    project: &str,
    service: &str,
    port: u16,
    released: &str,
    reason: &str,
) -> HistoryEntry {
    HistoryEntry {
        project: project.into(),
        service: service.into(),
        port,
        allocated: "2026-01-01".into(),
        released: released.into(),
        reason: reason.into(),
        protocol: Protocol::default(),
    }
}

#[test]
fn history_for_service_returns_empty_when_never_removed() {
    let r = Registry::default();
    assert!(r.history_for_service("p", "postgres").is_empty());
}

#[test]
fn history_for_service_filters_by_project_and_service() {
    let mut r = Registry::default();
    r.history.push(history_entry(
        "p",
        "postgres",
        20_000,
        "2026-04-26",
        "user requested",
    ));
    r.history.push(history_entry(
        "p",
        "redis",
        20_001,
        "2026-04-26",
        "user requested",
    ));
    r.history.push(history_entry(
        "other",
        "postgres",
        20_002,
        "2026-04-26",
        "user requested",
    ));
    let entries = r.history_for_service("p", "postgres");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project, "p");
    assert_eq!(entries[0].service, "postgres");
    assert_eq!(entries[0].port, 20_000);
}

#[test]
fn history_for_service_sorts_most_recent_first() {
    let mut r = Registry::default();
    r.history
        .push(history_entry("p", "api", 20_000, "2026-01-01", "first"));
    r.history
        .push(history_entry("p", "api", 20_001, "2026-04-26", "third"));
    r.history
        .push(history_entry("p", "api", 20_002, "2026-03-01", "second"));
    let entries = r.history_for_service("p", "api");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].released, "2026-04-26");
    assert_eq!(entries[1].released, "2026-03-01");
    assert_eq!(entries[2].released, "2026-01-01");
}

#[test]
fn orphans_for_service_returns_empty_when_no_path_projects() {
    let mut r = Registry::default();
    r.set(
        "github.com/acme/myapp",
        "postgres",
        20_000,
        Protocol::default(),
    );
    assert!(r
        .orphans_for_service(
            "github.com/acme/myapp",
            "postgres",
            Path::new("/home/user/work/myapp"),
        )
        .is_empty());
}

#[test]
fn orphans_for_service_finds_entry_under_cwd_exact_path() {
    let mut r = Registry::default();
    r.set(
        "/home/user/work/myapp",
        "postgres",
        20_000,
        Protocol::default(),
    );
    let orphans = r.orphans_for_service(
        "github.com/acme/myapp",
        "postgres",
        Path::new("/home/user/work/myapp"),
    );
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].0, "/home/user/work/myapp");
    assert_eq!(orphans[0].1.port, 20_000);
}

#[test]
fn orphans_for_service_finds_entry_under_ancestor_path() {
    let mut r = Registry::default();
    r.set("/home/user/work", "postgres", 20_000, Protocol::default());
    let orphans = r.orphans_for_service(
        "github.com/acme/myapp",
        "postgres",
        Path::new("/home/user/work/myapp/subdir"),
    );
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].0, "/home/user/work");
}

#[test]
fn orphans_for_service_ignores_unrelated_paths() {
    let mut r = Registry::default();
    r.set(
        "/elsewhere/project",
        "postgres",
        20_000,
        Protocol::default(),
    );
    assert!(r
        .orphans_for_service(
            "github.com/acme/myapp",
            "postgres",
            Path::new("/home/user/work/myapp"),
        )
        .is_empty());
}

#[test]
fn orphans_for_service_ignores_other_services() {
    let mut r = Registry::default();
    r.set(
        "/home/user/work/myapp",
        "redis",
        20_000,
        Protocol::default(),
    );
    assert!(r
        .orphans_for_service(
            "github.com/acme/myapp",
            "postgres",
            Path::new("/home/user/work/myapp"),
        )
        .is_empty());
}

#[test]
fn orphans_for_service_returns_multiple_ancestor_matches() {
    let mut r = Registry::default();
    r.set("/home/user/work", "postgres", 20_000, Protocol::default());
    r.set(
        "/home/user/work/myapp",
        "postgres",
        20_001,
        Protocol::default(),
    );
    let orphans = r.orphans_for_service(
        "github.com/acme/myapp",
        "postgres",
        Path::new("/home/user/work/myapp"),
    );
    assert_eq!(orphans.len(), 2);
}

#[test]
fn orphans_for_service_excludes_current_project_even_if_path_based() {
    let mut r = Registry::default();
    r.set(
        "/home/user/work/myapp",
        "postgres",
        20_000,
        Protocol::default(),
    );
    let orphans = r.orphans_for_service(
        "/home/user/work/myapp",
        "postgres",
        Path::new("/home/user/work/myapp"),
    );
    assert!(orphans.is_empty());
}
