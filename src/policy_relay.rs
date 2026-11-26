//! Relay wiring for the Adaptive Traffic Policy Pack's in-flight bound (ADR-0048 decision 4).
//!
//! Compiled only with the `policy-pack` feature. With the feature off, or with the runtime switch
//! unset, `tunnel::yamux_config` keeps the product's 4 MiB per-stream window and nothing here is
//! consulted, so the data path is unchanged (ADR-0031 decision 13, ADR-0044 decision 6).
//!
//! The experiments (`src/policy_baseline.rs`, evidence `simulated`) chose the Yamux per-stream
//! window as the in-flight bound. A relay-level byte budget gated on `send_window()` (M1 as
//! specified) cannot be credited below `max_recv_window / 2`: Yamux only emits a window update once
//! the receiver has consumed half of its window, so a 64 KiB budget stalls after 64 KiB and a 2 MiB
//! budget bounds nothing useful. The window is the only place the relay can bound bytes in the
//! engine; 256 KiB cut the loaded ping p50 from about 1.9 s to about 98 ms at a 2.5 MB/s
//! bottleneck (research section 9.2), and the mechanism matrix re-measures it beside M1/M2/M3.
//!
//! Per-class queues and the deficit round-robin rotation (`policy_sched`, TP-2.1) are not connected
//! yet: one window applies to every flow, which is the Balanced fail-safe.

use tokio_yamux::config::INITIAL_STREAM_WINDOW;

/// Runtime switch read by `tunnel::yamux_config`. Unset means the pack is off (today's data path).
pub(crate) const ENV_POLICY: &str = "BAFT_TUNNEL_POLICY_PACK";

/// Per-stream window when the switch is on without a size: Yamux's initial window, about 100 ms of
/// queue in front of a 2.5 MB/s bottleneck.
pub(crate) const DEFAULT_STREAM_WINDOW: u32 = 256 * 1024;

/// The pack's relay configuration. Only the in-flight window bound is implemented; the value comes
/// from configuration, never from payload inspection (ADR-0031 decision 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyRelay {
    /// Yamux `max_stream_window_size` for both session ends.
    pub stream_window: u32,
}

impl PolicyRelay {
    /// Reads [`ENV_POLICY`]. Absent, empty or `off` keeps the pack off.
    pub(crate) fn from_env() -> Option<Self> {
        let value = std::env::var(ENV_POLICY).ok()?;
        Self::parse(&value)
    }

    /// `off` (or an empty value) disables the pack; `window-cap` enables the bound with the default
    /// window and `window-cap:<bytes>` with an explicit one. Sizes below Yamux's initial window
    /// (256 KiB) are refused, because both ends assume that window at stream creation. Anything
    /// else is refused rather than guessed, and the relay stays as it is today.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("off") {
            return None;
        }
        let (name, argument) = match value.split_once(':') {
            Some((name, argument)) => (name, Some(argument)),
            None => (value, None),
        };
        if !name.eq_ignore_ascii_case("window-cap") {
            return None;
        }
        let stream_window = match argument {
            None => DEFAULT_STREAM_WINDOW,
            Some(argument) => {
                let window: u32 = argument.trim().parse().ok()?;
                if window < INITIAL_STREAM_WINDOW {
                    return None;
                }
                window
            }
        };
        Some(Self { stream_window })
    }
}

#[cfg(test)]
mod tests {
    use tokio_yamux::config::INITIAL_STREAM_WINDOW;

    use super::*;

    #[test]
    fn switch_is_off_unless_explicitly_enabled() {
        assert_eq!(PolicyRelay::parse(""), None);
        assert_eq!(PolicyRelay::parse("  "), None);
        assert_eq!(PolicyRelay::parse("off"), None);
        assert_eq!(PolicyRelay::parse("OFF"), None);
        assert_eq!(
            PolicyRelay::from_env(),
            None,
            "the switch is unset in tests"
        );
    }

    #[test]
    fn switch_parses_a_window_and_refuses_anything_else() {
        assert_eq!(
            PolicyRelay::parse("window-cap:1048576"),
            Some(PolicyRelay {
                stream_window: 1024 * 1024
            })
        );
        assert_eq!(
            PolicyRelay::parse("window-cap"),
            Some(PolicyRelay {
                stream_window: DEFAULT_STREAM_WINDOW
            })
        );
        assert_eq!(
            PolicyRelay::parse("Window-Cap: 262144"),
            Some(PolicyRelay {
                stream_window: 256 * 1024
            })
        );
        // Below Yamux's initial window both ends would disagree about the window at stream open.
        assert_eq!(PolicyRelay::parse("window-cap:65536"), None);
        assert_eq!(
            PolicyRelay::parse(&format!("window-cap:{}", INITIAL_STREAM_WINDOW * 2))
                .map(|p| p.stream_window),
            Some(INITIAL_STREAM_WINDOW * 2)
        );
        assert_eq!(PolicyRelay::parse("window-cap:lots"), None);
        assert_eq!(PolicyRelay::parse("window-cap:0"), None);
        // M1's relay-level credit budget was rejected by the experiments; it is not a switch.
        assert_eq!(PolicyRelay::parse("credit-budget:65536"), None);
        assert_eq!(PolicyRelay::parse("on"), None);
    }
}
