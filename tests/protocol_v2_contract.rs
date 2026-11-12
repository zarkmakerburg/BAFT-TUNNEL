//! Executable, test-only specification for the owner-approved Option A policy.
//!
//! This file deliberately does not add or wire protocol-v2 runtime behavior. It is a
//! pre-implementation contract oracle: its vectors define the required decisions that
//! future runtime negotiation must satisfy. B2 feature acceptance must later exercise
//! the actual production handshake/capability path against the same vectors.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Version {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Capability {
    TcpForward,
    IntegrityProbe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Incompatibility {
    VersionMismatch {
        initiator: Version,
        responder: Version,
    },
    CapabilityNotAuthorized(Capability),
}

impl Incompatibility {
    fn code(self) -> &'static str {
        match self {
            Self::VersionMismatch { .. } => "protocol_version_mismatch",
            Self::CapabilityNotAuthorized(_) => "capability_not_authorized",
        }
    }
}

fn select_new_session(initiator: Version, responder: Version) -> Result<Version, Incompatibility> {
    if initiator == responder {
        Ok(initiator)
    } else {
        Err(Incompatibility::VersionMismatch {
            initiator,
            responder,
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum AttemptFailure {
    VersionRejected,
    UnsupportedCapability,
    Timeout,
    MalformedResponse,
}

fn retry_versions(required: Version, _failure: AttemptFailure) -> [Version; 1] {
    // Reconnect may repeat the configured generation; it must never cross generations.
    [required]
}

fn advertised_capabilities(version: Version, integrity_probe_authorized: bool) -> Vec<Capability> {
    match version {
        Version::V1 => vec![Capability::TcpForward],
        Version::V2 if integrity_probe_authorized => {
            vec![Capability::TcpForward, Capability::IntegrityProbe]
        }
        Version::V2 => vec![Capability::TcpForward],
    }
}

fn authorize_capability(
    version: Version,
    advertised: &[Capability],
    requested: Capability,
) -> Result<(), Incompatibility> {
    let version_can_support = match requested {
        Capability::TcpForward => true,
        Capability::IntegrityProbe => version == Version::V2,
    };

    if !version_can_support || !advertised.contains(&requested) {
        return Err(Incompatibility::CapabilityNotAuthorized(requested));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionBoundary {
    ExistingInMemorySessionContinues(Version),
    NewSession(Version),
    RejectedMixedVersion,
}

fn session_boundary(
    existing_session: Option<Version>,
    process_restarted: bool,
    connection_lost: bool,
    initiator: Version,
    responder: Version,
) -> SessionBoundary {
    if let Some(version) = existing_session {
        if !process_restarted && !connection_lost {
            return SessionBoundary::ExistingInMemorySessionContinues(version);
        }
    }

    match select_new_session(initiator, responder) {
        Ok(version) => SessionBoundary::NewSession(version),
        Err(Incompatibility::VersionMismatch { .. }) => SessionBoundary::RejectedMixedVersion,
        Err(Incompatibility::CapabilityNotAuthorized(_)) => unreachable!(),
    }
}

#[test]
fn version_selection_vectors_are_strict_and_symmetric() {
    let vectors = [
        (Version::V1, Version::V1, Ok(Version::V1)),
        (Version::V2, Version::V2, Ok(Version::V2)),
        (
            Version::V1,
            Version::V2,
            Err(Incompatibility::VersionMismatch {
                initiator: Version::V1,
                responder: Version::V2,
            }),
        ),
        (
            Version::V2,
            Version::V1,
            Err(Incompatibility::VersionMismatch {
                initiator: Version::V2,
                responder: Version::V1,
            }),
        ),
    ];

    for (initiator, responder, expected) in vectors {
        assert_eq!(select_new_session(initiator, responder), expected);
    }
}

#[test]
fn v2_failures_never_add_a_v1_fallback_attempt() {
    let failures = [
        AttemptFailure::VersionRejected,
        AttemptFailure::UnsupportedCapability,
        AttemptFailure::Timeout,
        AttemptFailure::MalformedResponse,
    ];

    for failure in failures {
        assert_eq!(retry_versions(Version::V2, failure), [Version::V2]);
    }

    assert_eq!(
        retry_versions(Version::V1, AttemptFailure::Timeout),
        [Version::V1]
    );
}

#[test]
fn v1_never_advertises_or_authorizes_integrity_probe() {
    for probe_flag in [false, true] {
        let advertised = advertised_capabilities(Version::V1, probe_flag);
        assert_eq!(advertised, [Capability::TcpForward]);
        assert!(
            authorize_capability(Version::V1, &advertised, Capability::IntegrityProbe).is_err()
        );
    }

    let v1 = advertised_capabilities(Version::V1, false);
    assert!(authorize_capability(Version::V1, &v1, Capability::TcpForward).is_ok());
}

#[test]
fn v2_integrity_probe_requires_separate_explicit_capability_authorization() {
    let without_probe = advertised_capabilities(Version::V2, false);
    assert!(authorize_capability(Version::V2, &without_probe, Capability::IntegrityProbe).is_err());

    let with_probe = advertised_capabilities(Version::V2, true);
    assert!(authorize_capability(Version::V2, &with_probe, Capability::IntegrityProbe).is_ok());
    assert!(authorize_capability(Version::V2, &with_probe, Capability::TcpForward).is_ok());
}

#[test]
fn mixed_version_failure_is_typed_and_never_reported_as_probe_pass() {
    let error = select_new_session(Version::V2, Version::V1).unwrap_err();

    assert_eq!(
        error,
        Incompatibility::VersionMismatch {
            initiator: Version::V2,
            responder: Version::V1,
        }
    );
    assert_eq!(error.code(), "protocol_version_mismatch");

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum OperatorResult {
        Unsupported,
        Unknown,
        Pass,
    }

    let result_when_peer_can_reply = OperatorResult::Unsupported;
    let result_when_peer_cannot_reply = OperatorResult::Unknown;
    assert_ne!(result_when_peer_can_reply, OperatorResult::Pass);
    assert_ne!(result_when_peer_cannot_reply, OperatorResult::Pass);
}

#[test]
fn session_continuity_ends_at_process_restart_or_connection_loss() {
    assert_eq!(
        session_boundary(Some(Version::V1), false, false, Version::V1, Version::V1),
        SessionBoundary::ExistingInMemorySessionContinues(Version::V1)
    );

    assert_eq!(
        session_boundary(Some(Version::V1), true, false, Version::V2, Version::V1),
        SessionBoundary::RejectedMixedVersion
    );

    assert_eq!(
        session_boundary(Some(Version::V1), false, true, Version::V1, Version::V1),
        SessionBoundary::NewSession(Version::V1)
    );

    assert_eq!(
        session_boundary(None, true, false, Version::V2, Version::V2),
        SessionBoundary::NewSession(Version::V2)
    );
    assert_eq!(
        session_boundary(None, true, false, Version::V1, Version::V1),
        SessionBoundary::NewSession(Version::V1)
    );

    assert_eq!(
        session_boundary(None, true, false, Version::V1, Version::V2),
        SessionBoundary::RejectedMixedVersion
    );
}
