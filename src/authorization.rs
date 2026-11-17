use anyhow::{Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_V1: u16 = 1;
pub const PROTOCOL_V2: u16 = 2;
/// Protocol v3 (ADR-0040): v2 plus the `ResumeStreams` session capability. Strict like v2: v1,
/// v2 and v3 peers reject each other. Direct only in its first release. Not selectable in
/// configuration or setup yet (no resume implementation exists).
pub const PROTOCOL_V3: u16 = 3;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum ConnectionDirection {
    Direct,
    Reverse,
}

impl ConnectionDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Reverse => "reverse",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum StreamCapability {
    TcpForward,
    IntegrityProbe,
    /// Session capability of protocol v3: streams may be detached and resumed (ADR-0040).
    ResumeStreams,
}

/// Whether integrity-probe streams exist in this protocol version (v2 and later).
pub const fn supports_integrity_probe(version: u16) -> bool {
    version >= PROTOCOL_V2
}

pub const PHASE1_CAPABILITIES: [StreamCapability; 1] = [StreamCapability::TcpForward];
pub const PROTOCOL_V2_CAPABILITIES: [StreamCapability; 2] = [
    StreamCapability::TcpForward,
    StreamCapability::IntegrityProbe,
];
pub const PROTOCOL_V3_CAPABILITIES: [StreamCapability; 3] = [
    StreamCapability::TcpForward,
    StreamCapability::IntegrityProbe,
    StreamCapability::ResumeStreams,
];

pub fn capabilities_for_version(version: u16) -> Result<&'static [StreamCapability]> {
    match version {
        PROTOCOL_V1 => Ok(&PHASE1_CAPABILITIES),
        PROTOCOL_V2 => Ok(&PROTOCOL_V2_CAPABILITIES),
        PROTOCOL_V3 => Ok(&PROTOCOL_V3_CAPABILITIES),
        _ => bail!("unsupported protocol version"),
    }
}

#[allow(dead_code)]
pub fn authorize_session(
    authenticated_peer: &str,
    expected_peer: &str,
    requested_tunnel: &str,
    expected_tunnel: &str,
    requested_direction: ConnectionDirection,
    expected_direction: ConnectionDirection,
    capabilities: &[StreamCapability],
) -> Result<()> {
    authorize_session_versioned(
        PROTOCOL_V1,
        authenticated_peer,
        expected_peer,
        requested_tunnel,
        expected_tunnel,
        requested_direction,
        expected_direction,
        capabilities,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn authorize_session_versioned(
    version: u16,
    authenticated_peer: &str,
    expected_peer: &str,
    requested_tunnel: &str,
    expected_tunnel: &str,
    requested_direction: ConnectionDirection,
    expected_direction: ConnectionDirection,
    capabilities: &[StreamCapability],
) -> Result<()> {
    if authenticated_peer != expected_peer {
        bail!("authenticated peer is not authorized for this tunnel");
    }
    if requested_tunnel != expected_tunnel {
        bail!("peer is not authorized for the requested tunnel");
    }
    if requested_direction != expected_direction {
        bail!("peer is not authorized for the requested connection direction");
    }
    if version == PROTOCOL_V3 && requested_direction != ConnectionDirection::Direct {
        bail!("protocol v3 (stream resume) is Direct only");
    }
    let allowed = capabilities_for_version(version)?;
    if capabilities.is_empty()
        || capabilities
            .iter()
            .any(|capability| !allowed.contains(capability))
    {
        bail!("peer requested an unsupported stream capability");
    }
    // The capability enum is intentionally small and closed. Track each variant directly
    // instead of cloning/sorting the peer-provided slice on every authenticated session.
    let mut has_tcp_forward = false;
    let mut has_integrity_probe = false;
    let mut has_resume = false;
    for capability in capabilities {
        let seen = match capability {
            StreamCapability::TcpForward => &mut has_tcp_forward,
            StreamCapability::IntegrityProbe => &mut has_integrity_probe,
            StreamCapability::ResumeStreams => &mut has_resume,
        };
        if *seen {
            bail!("peer requested a duplicate stream capability");
        }
        *seen = true;
    }
    if !has_tcp_forward {
        bail!("peer omitted the required TCP forwarding capability");
    }
    Ok(())
}

#[allow(dead_code)]
pub fn authorize_stream(
    requested_tunnel: &str,
    expected_tunnel: &str,
    requested_direction: ConnectionDirection,
    expected_direction: ConnectionDirection,
    capability: StreamCapability,
) -> Result<()> {
    authorize_stream_versioned(
        PROTOCOL_V1,
        requested_tunnel,
        expected_tunnel,
        requested_direction,
        expected_direction,
        capability,
    )
}

pub fn authorize_stream_versioned(
    version: u16,
    requested_tunnel: &str,
    expected_tunnel: &str,
    requested_direction: ConnectionDirection,
    expected_direction: ConnectionDirection,
    capability: StreamCapability,
) -> Result<()> {
    if requested_tunnel != expected_tunnel {
        bail!("stream is not authorized for this tunnel");
    }
    if requested_direction != expected_direction {
        bail!("stream is not authorized for this connection direction");
    }
    if capability == StreamCapability::ResumeStreams {
        bail!("ResumeStreams is a session capability, not a stream type");
    }
    if !capabilities_for_version(version)?.contains(&capability) {
        bail!("stream capability is not authorized for this protocol version");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_authorization_binds_peer_tunnel_direction_and_capabilities() {
        let caps = [StreamCapability::TcpForward];
        assert!(
            authorize_session(
                "iran",
                "iran",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &caps
            )
            .is_ok()
        );
        assert!(
            authorize_session(
                "attacker",
                "iran",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &caps
            )
            .is_err()
        );
        assert!(
            authorize_session(
                "iran",
                "iran",
                "other",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &caps
            )
            .is_err()
        );
        assert!(
            authorize_session(
                "iran",
                "iran",
                "lab",
                "lab",
                ConnectionDirection::Reverse,
                ConnectionDirection::Direct,
                &caps
            )
            .is_err()
        );
        assert!(
            authorize_session(
                "iran",
                "iran",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &[]
            )
            .is_err()
        );
        assert!(
            authorize_session(
                "iran",
                "iran",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &[StreamCapability::TcpForward, StreamCapability::TcpForward]
            )
            .is_err()
        );
    }

    #[test]
    fn stream_authorization_binds_tunnel_direction_and_capability() {
        assert!(
            authorize_stream(
                "lab",
                "lab",
                ConnectionDirection::Reverse,
                ConnectionDirection::Reverse,
                StreamCapability::TcpForward
            )
            .is_ok()
        );
        assert!(
            authorize_stream(
                "other",
                "lab",
                ConnectionDirection::Reverse,
                ConnectionDirection::Reverse,
                StreamCapability::TcpForward
            )
            .is_err()
        );
        assert!(
            authorize_stream(
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Reverse,
                StreamCapability::TcpForward
            )
            .is_err()
        );
    }

    #[test]
    fn protocol_v1_excludes_probe_and_v2_requires_explicit_stream_authorization() {
        assert_eq!(
            capabilities_for_version(PROTOCOL_V1).unwrap(),
            &[StreamCapability::TcpForward]
        );
        assert_eq!(
            capabilities_for_version(PROTOCOL_V2).unwrap(),
            &[
                StreamCapability::TcpForward,
                StreamCapability::IntegrityProbe
            ]
        );
        assert!(
            authorize_stream(
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                StreamCapability::IntegrityProbe
            )
            .is_err()
        );
        assert!(
            authorize_stream_versioned(
                PROTOCOL_V2,
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                StreamCapability::IntegrityProbe
            )
            .is_ok()
        );
        assert!(
            authorize_session_versioned(
                PROTOCOL_V2,
                "peer",
                "peer",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &[
                    StreamCapability::TcpForward,
                    StreamCapability::IntegrityProbe
                ]
            )
            .is_ok()
        );
        assert!(
            authorize_session_versioned(
                PROTOCOL_V1,
                "peer",
                "peer",
                "lab",
                "lab",
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                &[
                    StreamCapability::TcpForward,
                    StreamCapability::IntegrityProbe
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn protocol_v3_adds_resume_streams_is_direct_only_and_not_a_stream_type() {
        use ConnectionDirection::{Direct, Reverse};
        let v3 = capabilities_for_version(PROTOCOL_V3).unwrap();
        assert_eq!(v3, &PROTOCOL_V3_CAPABILITIES);
        assert!(v3.contains(&StreamCapability::ResumeStreams));
        // v1 and v2 never allow the resume capability.
        for version in [PROTOCOL_V1, PROTOCOL_V2] {
            assert!(
                !capabilities_for_version(version)
                    .unwrap()
                    .contains(&StreamCapability::ResumeStreams)
            );
            assert!(
                authorize_session_versioned(
                    version,
                    "iran",
                    "iran",
                    "lab",
                    "lab",
                    Direct,
                    Direct,
                    &PROTOCOL_V3_CAPABILITIES,
                )
                .is_err()
            );
        }
        assert!(
            authorize_session_versioned(
                PROTOCOL_V3,
                "iran",
                "iran",
                "lab",
                "lab",
                Direct,
                Direct,
                &PROTOCOL_V3_CAPABILITIES,
            )
            .is_ok()
        );
        // Direct only in the first release.
        assert!(
            authorize_session_versioned(
                PROTOCOL_V3,
                "iran",
                "iran",
                "lab",
                "lab",
                Reverse,
                Reverse,
                &PROTOCOL_V3_CAPABILITIES,
            )
            .is_err()
        );
        // The resume capability is a session capability, never a stream type.
        assert!(
            authorize_stream_versioned(
                PROTOCOL_V3,
                "lab",
                "lab",
                Direct,
                Direct,
                StreamCapability::ResumeStreams
            )
            .is_err()
        );
        assert!(supports_integrity_probe(PROTOCOL_V3) && !supports_integrity_probe(PROTOCOL_V1));
    }
}
