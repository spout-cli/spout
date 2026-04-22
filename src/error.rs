//! `SpoutError` — every failure mode maps to exactly one exit code.
//!
//! Exit codes are part of the CLI's stable API. See README.md.

use thiserror::Error;

use crate::protocol::Protocol;

#[derive(Debug, Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum SpoutError {
    #[error("service not registered")]
    ServiceNotRegistered,

    #[error("no free port found for {service} in range {range_start}-{range_end}")]
    NoFreePortFound {
        service: String,
        range_start: u16,
        range_end: u16,
    },

    #[error("registry unreadable: {0}")]
    RegistryCorrupt(String),

    #[error("registry version {0} is not supported")]
    RegistryVersionUnknown(u32),

    #[error("port {port}/{protocol} is already registered to project '{project}'")]
    PortAlreadyClaimed {
        port: u16,
        protocol: Protocol,
        project: String,
    },

    #[error("port {port}/{protocol} is already in use by the operating system")]
    PortInUse { port: u16, protocol: Protocol },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("compose file unreadable: {0}")]
    ComposeInvalid(String),

    #[error("no compose file found (looked for docker-compose.yml / .yaml / compose.yml / .yaml); pass -f <PATH> to override")]
    ComposeNotFound,
}

impl SpoutError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ServiceNotRegistered => 1,
            Self::NoFreePortFound { .. } => 2,
            Self::RegistryCorrupt(_) => 3,
            Self::RegistryVersionUnknown(_) => 4,
            Self::PortAlreadyClaimed { .. } => 5,
            Self::PortInUse { .. } => 6,
            Self::Io(_) => 7,
            Self::ComposeInvalid(_) => 8,
            Self::ComposeNotFound => 8,
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(SpoutError::ComposeNotFound.exit_code(), 8);
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
}
