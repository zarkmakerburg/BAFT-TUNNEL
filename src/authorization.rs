use anyhow::{Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_V1: u16 = 1;
pub const PROTOCOL_V2: u16 = 2;

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
}

pub const PHASE1_CAPABILITIES: [StreamCapability; 1] = [StreamCapability::TcpForward];
pub const PROTOCOL_V2_CAPABILITIES: [StreamCapability; 2] = [
    StreamCapability::TcpForward,
    StreamCapability::IntegrityProbe,
];

pub fn capabilities_for_version(version: u16) -> Result<&'static [StreamCapability]> {
    match version {
        PROTOCOL_V1 => Ok(&PHASE1_CAPABILITIES),
        PROTOCOL_V2 => Ok(&PROTOCOL_V2_CAPABILITIES),
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
    let allowed = capabilities_for_version(version)?;
    if capabilities.is_empty()
        || capabilities
            .iter()
            .any(|capability| !allowed.contains(capability))
    {
        bail!("peer requested an unsupported stream capability");
    }
    let mut unique = capabilities.to_vec();
    unique.sort_by_key(|capability| *capability as u8);
    unique.dedup();
    if unique.len() != capabilities.len() {
        bail!("peer requested a duplicate stream capability");
    }
    if !capabilities.contains(&StreamCapability::TcpForward) {
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
}
