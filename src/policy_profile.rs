//! Versioned traffic-profile schema (ADR-0031 decision 15, TP-3.1 / #233).
//!
//! Compiled in every build, not only with `policy-pack`: a tunnel config may name a profile in any
//! build, so an unsupported name must be refused with the accepted list whether or not the pack is
//! compiled in. Resolving a profile into the relay's session window and `SchedulerConfig` happens
//! in `policy_relay` (feature only) and only when `BAFT_TUNNEL_POLICY_PACK` enables the pack, so a
//! configured profile alone changes nothing (ADR-0031 decision 19).
//!
//! A flow's class is the profile its tunnel configures, and nothing else: no payload inspection,
//! no inference from port, protocol or volume (ADR-0031 decision 16). Absent means `balanced/1`,
//! the identity profile and the fail-safe.
//!
//! The numeric accessors are read by `policy_relay` (feature build only) and by the tests here, so
//! the module allows dead code in the default build for the same reason `policy` does.
#![allow(dead_code)]

use std::fmt;

use serde::Deserialize;

/// Schema version of this table. A profile id carries its own version (`balanced/1`), so a future
/// schema adds ids rather than changing what an existing id means.
pub(crate) const PROFILE_SCHEMA_VERSION: u32 = 1;

/// The five ratified profiles, with the numbers ADR-0031 decision 15 froze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PolicyProfile {
    /// Identity and fail-safe: exactly the pack-on parameters TP-2.1/TP-2.2 measured.
    #[default]
    Balanced,
    Gaming,
    Trading,
    Streaming,
    Bulk,
}

/// Accepted ids, in the order the error message lists them.
pub(crate) const ACCEPTED_PROFILES: [&str; 5] = [
    "balanced/1",
    "gaming/1",
    "trading/1",
    "streaming/1",
    "bulk/1",
];

impl PolicyProfile {
    /// Versioned id as written in the tunnel config.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Balanced => "balanced/1",
            Self::Gaming => "gaming/1",
            Self::Trading => "trading/1",
            Self::Streaming => "streaming/1",
            Self::Bulk => "bulk/1",
        }
    }

    /// Yamux `max_stream_window_size` for the session. 256 KiB is the carrier floor
    /// (`INITIAL_STREAM_WINDOW`), so Gaming and Trading keep it and differ by quantum and cap.
    pub(crate) const fn window_bytes(self) -> u32 {
        match self {
            Self::Balanced | Self::Gaming | Self::Trading => 256 * 1024,
            Self::Streaming => 512 * 1024,
            Self::Bulk => 2 * 1024 * 1024,
        }
    }

    /// Deficit round-robin quantum: the most one flow is granted per visit.
    pub(crate) const fn quantum_bytes(self) -> usize {
        match self {
            Self::Balanced => 16 * 1024,
            Self::Gaming => 4 * 1024,
            Self::Trading => 2 * 1024,
            Self::Streaming => 32 * 1024,
            Self::Bulk => 64 * 1024,
        }
    }

    /// Hard bound on bytes queued for one flow.
    pub(crate) const fn max_queued_bytes(self) -> usize {
        match self {
            Self::Balanced => 64 * 1024,
            Self::Gaming => 16 * 1024,
            Self::Trading => 8 * 1024,
            Self::Streaming => 128 * 1024,
            Self::Bulk => 256 * 1024,
        }
    }

    /// Exact id match, nothing guessed. `Ok` only for the five ratified ids at schema version 1.
    pub(crate) fn parse(value: &str) -> Result<Self, ProfileError> {
        let profile = match value {
            "balanced/1" => Self::Balanced,
            "gaming/1" => Self::Gaming,
            "trading/1" => Self::Trading,
            "streaming/1" => Self::Streaming,
            "bulk/1" => Self::Bulk,
            _ => return Err(ProfileError::new(value)),
        };
        Ok(profile)
    }
}

impl fmt::Display for PolicyProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// An id the schema does not accept; the message names every accepted value, because the operator
/// has no other way to learn them from the failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileError {
    value: String,
}

impl ProfileError {
    fn new(value: &str) -> Self {
        Self {
            value: value.to_string(),
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported policy profile {:?} (schema version {}); accepted values: {}",
            self.value,
            PROFILE_SCHEMA_VERSION,
            ACCEPTED_PROFILES.join(", ")
        )
    }
}

impl std::error::Error for ProfileError {}

/// Config deserialization rejects an unknown id at parse time, before any tunnel starts, and
/// carries the accepted list in the error. `null` or an absent field is `None`: the caller then
/// resolves `balanced/1` (see `config::PolicySettings::profile`).
impl<'de> Deserialize<'de> for PolicyProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ratified_profile_resolves_to_its_exact_numbers() {
        let table = [
            (
                PolicyProfile::Balanced,
                "balanced/1",
                262_144,
                16_384,
                65_536,
            ),
            (PolicyProfile::Gaming, "gaming/1", 262_144, 4_096, 16_384),
            (PolicyProfile::Trading, "trading/1", 262_144, 2_048, 8_192),
            (
                PolicyProfile::Streaming,
                "streaming/1",
                524_288,
                32_768,
                131_072,
            ),
            (PolicyProfile::Bulk, "bulk/1", 2_097_152, 65_536, 262_144),
        ];
        for (profile, id, window, quantum, cap) in table {
            assert_eq!(profile.id(), id);
            assert_eq!(
                PolicyProfile::parse(id),
                Ok(profile),
                "{id} must round-trip"
            );
            assert_eq!(profile.window_bytes(), window, "{id} window");
            assert_eq!(profile.quantum_bytes(), quantum, "{id} quantum");
            assert_eq!(profile.max_queued_bytes(), cap, "{id} queue cap");
        }
        let ids: Vec<&str> = ACCEPTED_PROFILES.to_vec();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn no_profile_is_balanced() {
        assert_eq!(PolicyProfile::default(), PolicyProfile::Balanced);
        assert_eq!(
            PolicyProfile::default().window_bytes(),
            256 * 1024,
            "the fallback keeps the measured pack-on window"
        );
        assert_eq!(PolicyProfile::default().quantum_bytes(), 16 * 1024);
        assert_eq!(PolicyProfile::default().max_queued_bytes(), 64 * 1024);
    }

    #[test]
    fn unsupported_names_are_rejected_and_the_error_names_the_accepted_values() {
        for value in [
            "",
            "balanced",
            "balanced/2",
            "Balanced/1",
            "gaming/1 ",
            "turbo/1",
            "off",
        ] {
            let error = PolicyProfile::parse(value).expect_err(value);
            let message = error.to_string();
            assert!(
                message.contains(&format!("{value:?}")),
                "the message must echo {value:?}: {message}"
            );
            for accepted in ACCEPTED_PROFILES {
                assert!(
                    message.contains(accepted),
                    "the message must name {accepted}: {message}"
                );
            }
        }
    }

    #[test]
    fn config_deserialization_rejects_an_unsupported_name_at_parse_time() {
        #[derive(Debug, Deserialize)]
        struct Holder {
            profile: Option<PolicyProfile>,
        }
        let absent: Holder = serde_json::from_str("{}").expect("absent profile parses");
        assert_eq!(absent.profile, None);
        let named: Holder =
            serde_json::from_str(r#"{"profile": "trading/1"}"#).expect("a ratified id parses");
        assert_eq!(named.profile, Some(PolicyProfile::Trading));
        let error = serde_json::from_str::<Holder>(r#"{"profile": "turbo/1"}"#)
            .expect_err("unknown ids must fail");
        let message = error.to_string();
        for accepted in ACCEPTED_PROFILES {
            assert!(message.contains(accepted), "{message}");
        }
    }
}
