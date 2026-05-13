use super::*;

#[test]
fn service_not_registered_exits_one() {
    assert_eq!(SpoutError::ServiceNotRegistered.exit_code(), 1);
}

#[test]
fn no_free_port_found_exits_two() {
    let err = SpoutError::NoFreePortFound {
        service: "postgres".into(),
        range_start: 5432,
        range_end: 6432,
    };
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn registry_corrupt_exits_three() {
    let err = SpoutError::RegistryCorrupt("bad json".into());
    assert_eq!(err.exit_code(), 3);
}

#[test]
fn registry_version_unknown_exits_four() {
    assert_eq!(SpoutError::RegistryVersionUnknown(2).exit_code(), 4);
}

#[test]
fn port_already_claimed_exits_five() {
    let err = SpoutError::PortAlreadyClaimed {
        port: 5432,
        protocol: Protocol::Tcp,
        project: "other".into(),
    };
    assert_eq!(err.exit_code(), 5);
}

#[test]
fn port_in_use_exits_six() {
    let err = SpoutError::PortInUse {
        port: 5432,
        protocol: Protocol::Tcp,
    };
    assert_eq!(err.exit_code(), 6);
}

#[test]
fn io_exits_seven() {
    let err = SpoutError::Io("broken pipe".into());
    assert_eq!(err.exit_code(), 7);
}

#[test]
fn compose_invalid_exits_eight() {
    let err = SpoutError::ComposeInvalid("bad yaml".into());
    assert_eq!(err.exit_code(), 8);
}

#[test]
fn compose_not_found_exits_eight() {
    assert_eq!(
        SpoutError::ComposeNotFound("no compose file".into()).exit_code(),
        8
    );
}

#[test]
fn usage_exits_nine() {
    let err = SpoutError::Usage("specify a service".into());
    assert_eq!(err.exit_code(), 9);
}

#[test]
fn alloc_orphan_match_exits_ten() {
    let err = SpoutError::AllocOrphanMatch {
        project: "github.com/acme/myapp".into(),
        service: "postgres".into(),
        orphans: vec![OrphanRecord {
            project: "/home/user/work/myapp".into(),
            port: 20_000,
            protocol: Protocol::Tcp,
        }],
    };
    assert_eq!(err.exit_code(), 10);
}

#[test]
fn alloc_orphan_match_single_orphan_inline_format() {
    let err = SpoutError::AllocOrphanMatch {
        project: "github.com/acme/myapp".into(),
        service: "postgres".into(),
        orphans: vec![OrphanRecord {
            project: "/home/user/work/myapp".into(),
            port: 20_000,
            protocol: Protocol::Tcp,
        }],
    };
    let msg = err.to_string();
    assert!(msg.contains("refusing to allocate 'postgres' in project 'github.com/acme/myapp'"));
    assert!(msg.contains(
        "'postgres' already exists under sibling identity: /home/user/work/myapp → 20000/tcp"
    ));
    assert!(msg
        .contains("try `spout reproject --from /home/user/work/myapp --to github.com/acme/myapp`"));
}

#[test]
fn alloc_orphan_match_multiple_orphans_list_format() {
    let err = SpoutError::AllocOrphanMatch {
        project: "github.com/acme/myapp".into(),
        service: "postgres".into(),
        orphans: vec![
            OrphanRecord {
                project: "/home/user/work/myapp".into(),
                port: 20_000,
                protocol: Protocol::Tcp,
            },
            OrphanRecord {
                project: "/home/user/work".into(),
                port: 20_001,
                protocol: Protocol::Udp,
            },
        ],
    };
    let msg = err.to_string();
    assert!(msg.contains("already exists under sibling identities:"));
    assert!(msg.contains("/home/user/work/myapp → 20000/tcp"));
    assert!(msg.contains("/home/user/work → 20001/udp"));
    assert!(msg.contains("--from /home/user/work/myapp"));
}

#[test]
fn reproject_conflict_exits_eleven() {
    let err = SpoutError::ReprojectConflict {
        from: "a".into(),
        to: "b".into(),
        services: vec!["postgres".into()],
    };
    assert_eq!(err.exit_code(), 11);
    let msg = err.to_string();
    assert!(msg.contains("cannot reproject from 'a' to 'b'"));
    assert!(msg.contains("services exist in both: postgres"));
}

#[test]
fn display_messages_are_non_empty() {
    let variants = [
        SpoutError::ServiceNotRegistered,
        SpoutError::NoFreePortFound {
            service: "api".into(),
            range_start: 8080,
            range_end: 9080,
        },
        SpoutError::RegistryCorrupt("expected `{`".into()),
        SpoutError::RegistryVersionUnknown(99),
        SpoutError::PortAlreadyClaimed {
            port: 5432,
            protocol: Protocol::Tcp,
            project: "myapp".into(),
        },
        SpoutError::PortInUse {
            port: 6379,
            protocol: Protocol::Udp,
        },
    ];
    for v in &variants {
        assert!(!v.to_string().is_empty());
    }
}

#[test]
fn port_errors_mention_the_protocol() {
    let claimed = SpoutError::PortAlreadyClaimed {
        port: 5432,
        protocol: Protocol::Udp,
        project: "p".into(),
    };
    assert!(claimed.to_string().contains("5432/udp"), "got {claimed}");

    let in_use = SpoutError::PortInUse {
        port: 53,
        protocol: Protocol::Udp,
    };
    assert!(in_use.to_string().contains("53/udp"), "got {in_use}");
}

fn not_registered(
    recently_removed: Option<RemovedRecord>,
    orphans: Vec<OrphanRecord>,
) -> SpoutError {
    SpoutError::ServiceNotRegisteredInProject {
        project: "github.com/acme/myapp".into(),
        service: "postgres".into(),
        available: vec![],
        recently_removed,
        orphans,
    }
}

#[test]
fn service_not_registered_with_no_orphans_uses_alloc_hint() {
    let msg = not_registered(None, vec![]).to_string();
    assert!(msg.contains("no service 'postgres' in project 'github.com/acme/myapp'"));
    assert!(msg.contains("try `spout alloc postgres`"));
    assert!(!msg.contains("registered under different identity"));
}

#[test]
fn service_not_registered_with_one_orphan_includes_orphan_line() {
    let orphans = vec![OrphanRecord {
        project: "/home/user/work/myapp".into(),
        port: 20_000,
        protocol: Protocol::Tcp,
    }];
    let msg = not_registered(None, orphans).to_string();
    assert!(
        msg.contains(
            "registered under different identity: /home/user/work/myapp/postgres → 20000/tcp"
        ),
        "got: {msg}"
    );
}

#[test]
fn service_not_registered_with_orphan_suggests_reproject_and_drops_other_hints() {
    let orphans = vec![OrphanRecord {
        project: "/home/user/work/myapp".into(),
        port: 20_000,
        protocol: Protocol::Tcp,
    }];
    let msg = not_registered(None, orphans).to_string();
    assert!(msg
        .contains("try `spout reproject --from /home/user/work/myapp --to github.com/acme/myapp`"));
    assert!(!msg.contains("try `spout alloc"));
    assert!(!msg.contains("try `spout env"));
}

#[test]
fn service_not_registered_with_multiple_orphans_lists_all_and_uses_first_in_hint() {
    let orphans = vec![
        OrphanRecord {
            project: "/home/user/work/myapp".into(),
            port: 20_000,
            protocol: Protocol::Tcp,
        },
        OrphanRecord {
            project: "/home/user/work".into(),
            port: 20_001,
            protocol: Protocol::Udp,
        },
    ];
    let msg = not_registered(None, orphans).to_string();
    assert!(msg.contains("/home/user/work/myapp/postgres → 20000/tcp"));
    assert!(msg.contains("/home/user/work/postgres → 20001/udp"));
    assert!(msg.contains("--from /home/user/work/myapp"));
}

#[test]
fn service_not_registered_orphan_hint_wins_over_recently_removed() {
    let orphans = vec![OrphanRecord {
        project: "/home/user/work/myapp".into(),
        port: 20_000,
        protocol: Protocol::Tcp,
    }];
    let recently_removed = Some(RemovedRecord {
        released: "2026-05-13".into(),
        reason: "user requested".into(),
    });
    let msg = not_registered(recently_removed, orphans).to_string();
    assert!(msg.contains("recently removed: postgres"));
    assert!(msg.contains("try `spout reproject"));
    assert!(!msg.contains("alloc postgres"));
}
