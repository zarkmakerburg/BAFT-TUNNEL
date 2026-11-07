use anyhow::{Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

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
}

pub const PHASE1_CAPABILITIES: [StreamCapability; 1] = [StreamCapability::TcpForward];

pub fn authorize_session(
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
    if capabilities.is_empty()
        || capabilities
            .iter()
            .any(|capability| !PHASE1_CAPABILITIES.contains(capability))
    {
        bail!("peer requested an unsupported stream capability");
    }
    let mut unique = capabilities.to_vec();
    unique.sort_by_key(|capability| *capability as u8);
    unique.dedup();
    if unique.len() != capabilities.len() {
        bail!("peer requested a duplicate stream capability");
    }
    Ok(())
}

pub fn authorize_stream(
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
    if !PHASE1_CAPABILITIES.contains(&capability) {
        bail!("stream capability is not authorized");
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
}
