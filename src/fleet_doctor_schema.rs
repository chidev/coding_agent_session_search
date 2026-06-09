//! Bounded fleet-doctor JSON contract.
//!
//! Bead: coding_agent_session_search-cass-fleet-resilience-20260608-uojcg.6.1
//! ("Define bounded fleet doctor probe schema").
//!
//! This module defines the wire contract for `cass`'s fleet doctor *before* the
//! probe implementation lands, so the producers (`6.2` cheap bounded probes,
//! `8.2` reachability/sync health) and consumers (`6.3` version skew, `6.4`
//! archive coverage, `9.2` root-cause projection, `10.4` per-host incident
//! rollups) all agree on one shape.
//!
//! The defining constraint is **boundedness without loss of host identity**: a
//! fleet sweep contacts many hosts, any of which may be slow, unreachable, or
//! running an old binary. The schema must therefore represent every outcome —
//! success, partial, timeout, old-binary skew, command-not-found, unreachable
//! SSH, macOS path/tool differences, and high archive risk — while *always*
//! preserving who the host is ([`HostDoctorReport::host_alias`] and
//! [`HostDoctorReport::platform`] are non-optional and survive every failure
//! mode). Deep state that could not be probed is `None`/empty and the omission
//! is recorded in [`HostDoctorReport::skipped_sections`], never silently dropped.
//!
//! Every field a diagnostic needs is structured and prose-free; a coarse,
//! optional [`RootCauseFamily`] hint composes with the attribution taxonomy from
//! bead `9.1` so `9.2` can project a likely root cause without changing this
//! contract.

use crate::root_cause_taxonomy::RootCauseFamily;
use serde::{Deserialize, Serialize};

/// Stable schema version for the fleet-doctor wire format. Bump only on a
/// breaking change to the field set or enum string values.
pub const FLEET_DOCTOR_SCHEMA_VERSION: u32 = 1;

/// The overall outcome of probing a single host. This is the rich discriminant;
/// the scalar [`HostDoctorReport::timed_out`] / [`HostDoctorReport::unreachable`]
/// flags mirror the timeout/unreachable cases for consumers that only branch on
/// booleans.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum HostProbeStatus {
    /// Every requested section was probed and the host is healthy.
    Ok,
    /// Some sections were probed; others were skipped or degraded (see
    /// [`HostDoctorReport::skipped_sections`]). Host identity and partial facts
    /// are present.
    Partial,
    /// The probe exceeded its time budget. Identity is present; deep state is
    /// whatever completed before the deadline.
    TimedOut,
    /// The host responded but its `cass` binary is behind the required
    /// contract/api version. [`HostDoctorReport::cass_version`] is populated.
    OldBinarySkew,
    /// The host was reachable but `cass` (or a required tool) was not found on
    /// `PATH`.
    CommandNotFound,
    /// The host could not be contacted at all (SSH/transport failure).
    Unreachable,
    /// The host was fully probed but is unhealthy (e.g. DB not ready, high
    /// archive risk) without a hard failure.
    Degraded,
}

impl HostProbeStatus {
    /// Stable kebab-case wire value (single source of truth; a unit test pins
    /// serde output to this).
    pub const fn as_str(self) -> &'static str {
        match self {
            HostProbeStatus::Ok => "ok",
            HostProbeStatus::Partial => "partial",
            HostProbeStatus::TimedOut => "timed-out",
            HostProbeStatus::OldBinarySkew => "old-binary-skew",
            HostProbeStatus::CommandNotFound => "command-not-found",
            HostProbeStatus::Unreachable => "unreachable",
            HostProbeStatus::Degraded => "degraded",
        }
    }

    /// `true` when the host yielded no deep state (unreachable / command-not-found):
    /// consumers should expect only identity fields to be populated.
    pub const fn is_hard_failure(self) -> bool {
        matches!(
            self,
            HostProbeStatus::Unreachable | HostProbeStatus::CommandNotFound
        )
    }
}

/// Host operating system family. Distinguishes macOS so consumers can account
/// for path and tooling differences (the recurring fleet heterogeneity issue).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum HostOs {
    Linux,
    /// Pinned to `"macos"` (not kebab `"mac-os"`) to match `std::env::consts::OS`
    /// and rustc target conventions.
    #[serde(rename = "macos")]
    MacOs,
    Windows,
    Other,
}

/// Filesystem path convention, so a Linux controller can correctly interpret a
/// macOS/Windows host's paths.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PathStyle {
    Posix,
    Windows,
}

/// Stable host identity and environment shape. Always present, even for an
/// unreachable host (the controller knows *who* it failed to reach).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    /// OS family.
    pub os: HostOs,
    /// CPU architecture (e.g. `"x86_64"`, `"aarch64"`). Free-form because the
    /// set is open; empty string when unknown.
    pub arch: String,
    /// Path convention for interpreting this host's paths.
    pub path_style: PathStyle,
    /// Structured notes about path/tool divergences from the controller's
    /// platform (e.g. `"rsync=bsd"`, `"coreutils=bsd"`, `"data_dir=~/Library"`).
    /// Prose-free key=value-ish tokens, not sentences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_notes: Vec<String>,
}

impl Platform {
    /// A Linux/x86_64 POSIX host with no tool divergences — the common case.
    pub fn linux_x86_64() -> Self {
        Self {
            os: HostOs::Linux,
            arch: "x86_64".to_string(),
            path_style: PathStyle::Posix,
            tool_notes: Vec::new(),
        }
    }
}

/// What the host's `cass` binary can do, used to gate which probes are even
/// meaningful and to surface version/feature skew across the fleet.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityTier {
    /// All features present (semantic, remote sync, robot envelopes, …).
    Full,
    /// Core search/index present; some optional features absent.
    Standard,
    /// Minimal/legacy binary; only basic commands available.
    Minimal,
    /// Could not be determined.
    Unknown,
}

/// Coarse DB / readiness state for the host's CASS store.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ReadinessState {
    Ready,
    Degraded,
    NotReady,
    Unknown,
}

/// Semantic-search asset/state on the host.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticState {
    Enabled,
    Disabled,
    /// Enabled/requested but assets are missing or partial.
    AssetsMissing,
    Unknown,
}

/// Remote source-sync state for the host's mirrors.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteSyncState {
    Synced,
    Stale,
    NeverSynced,
    Failed,
    NotConfigured,
    Unknown,
}

/// Risk that derived/archive state is unrecoverable or diverging — drives the
/// "back this up / re-archive" recommendation. Ordered low→high so
/// [`FleetSummary::highest_archive_risk`] can take a `max`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveRisk {
    Unknown,
    Low,
    Medium,
    High,
}

/// A configured source root on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRoot {
    /// Path as it exists on the host (interpret via [`Platform::path_style`]).
    pub path: String,
    /// Detected agent kind for the root, if known (e.g. `"claude"`, `"codex"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Whether this root is backed by an archive (vs. live-only).
    pub archived: bool,
}

/// Aggregate source counts for the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceCounts {
    /// Number of configured source roots.
    pub roots: u64,
    /// Total sessions discovered across roots.
    pub sessions: u64,
    /// Sessions that are indexed/derived.
    pub indexed_sessions: u64,
}

/// Quarantine state for the host's store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QuarantineState {
    /// Number of quarantined items.
    pub quarantined: u64,
    /// Of those, how many are eligible for automatic recovery.
    pub recoverable: u64,
}

/// The per-host fleet-doctor record. Identity fields ([`Self::host_alias`],
/// [`Self::platform`]) and the bounded scalars ([`Self::status`],
/// [`Self::elapsed_ms`], [`Self::timed_out`], [`Self::unreachable`],
/// [`Self::archive_risk`]) are always present; deep state is optional and absent
/// when not probed, with the omission named in [`Self::skipped_sections`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDoctorReport {
    /// Mirrors [`FLEET_DOCTOR_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Stable host identity. ALWAYS present, including for unreachable hosts.
    pub host_alias: String,
    /// Host platform/identity. ALWAYS present.
    pub platform: Platform,
    /// Rich probe outcome.
    pub status: HostProbeStatus,
    /// Wall-clock the probe took, bounded by the host time budget.
    pub elapsed_ms: u64,
    /// `true` if the probe hit its deadline (mirrors [`HostProbeStatus::TimedOut`]).
    pub timed_out: bool,
    /// `true` if the host could not be contacted (mirrors
    /// [`HostProbeStatus::Unreachable`]).
    pub unreachable: bool,
    /// Highest archive risk observed for this host. `Unknown` when not assessed.
    pub archive_risk: ArchiveRisk,
    /// Sections deliberately not probed or that failed to complete, by stable
    /// name (e.g. `"semantic"`, `"remote_sync"`). Makes partial/timeout results
    /// honest instead of indistinguishable from "all clear".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_sections: Vec<String>,

    /// Running `cass` version string, when the binary answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cass_version: Option<String>,
    /// Binary capability tier, when determined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_tier: Option<CapabilityTier>,
    /// Resolved data dir on the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
    /// Configured source roots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_roots: Vec<SourceRoot>,
    /// Aggregate source counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_counts: Option<SourceCounts>,
    /// DB / readiness state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readiness: Option<ReadinessState>,
    /// Semantic-asset state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<SemanticState>,
    /// Quarantine state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<QuarantineState>,
    /// Remote sync state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_sync: Option<RemoteSyncState>,

    /// Optional coarse root-cause hint, composing with the bead-9.1 taxonomy.
    /// Populated by `9.2`; absent here keeps the two contracts decoupled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likely_root_cause: Option<RootCauseFamily>,
    /// Recommended next action for an operator/agent (single, action-oriented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<String>,
}

impl HostDoctorReport {
    /// Construct an identity-only skeleton with the given status. All deep state
    /// is absent; callers fill in what they probe. Use this so host identity is
    /// established first and can never be lost on a later failure.
    pub fn skeleton(
        host_alias: impl Into<String>,
        platform: Platform,
        status: HostProbeStatus,
        elapsed_ms: u64,
    ) -> Self {
        Self {
            schema_version: FLEET_DOCTOR_SCHEMA_VERSION,
            host_alias: host_alias.into(),
            platform,
            status,
            elapsed_ms,
            timed_out: status == HostProbeStatus::TimedOut,
            unreachable: status == HostProbeStatus::Unreachable,
            archive_risk: ArchiveRisk::Unknown,
            skipped_sections: Vec::new(),
            cass_version: None,
            capability_tier: None,
            data_dir: None,
            source_roots: Vec::new(),
            source_counts: None,
            readiness: None,
            semantic: None,
            quarantine: None,
            remote_sync: None,
            likely_root_cause: None,
            recommended_action: None,
        }
    }

    /// Identity-only record for a host that could not be contacted. Preserves
    /// who the host is and records the recommended remediation.
    pub fn unreachable(
        host_alias: impl Into<String>,
        platform: Platform,
        elapsed_ms: u64,
        recommended_action: impl Into<String>,
    ) -> Self {
        let mut report =
            Self::skeleton(host_alias, platform, HostProbeStatus::Unreachable, elapsed_ms);
        report.likely_root_cause = Some(RootCauseFamily::RemoteTransportAuth);
        report.recommended_action = Some(recommended_action.into());
        report
    }
}

/// Fleet-wide rollup over the per-host reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetSummary {
    /// Total hosts in the sweep.
    pub total_hosts: usize,
    /// Hosts reporting [`HostProbeStatus::Ok`].
    pub ok: usize,
    /// Hosts that were probed but degraded/partial/old/timed-out (soft issues).
    pub degraded: usize,
    /// Hosts that timed out.
    pub timed_out: usize,
    /// Hosts that were unreachable or command-not-found (hard failures).
    pub unreachable: usize,
    /// The worst archive risk seen across all hosts.
    pub highest_archive_risk: ArchiveRisk,
}

/// The top-level fleet-doctor report: every host plus a rollup summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetDoctorReport {
    /// Mirrors [`FLEET_DOCTOR_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Per-host records, identity-preserving.
    pub hosts: Vec<HostDoctorReport>,
    /// Aggregate rollup, derived from `hosts`.
    pub summary: FleetSummary,
}

impl FleetDoctorReport {
    /// Build a report and derive the summary from the hosts. The summary is a
    /// pure function of the host records, so this is the only correct way to
    /// construct one.
    pub fn from_hosts(hosts: Vec<HostDoctorReport>) -> Self {
        let total_hosts = hosts.len();
        let mut ok = 0;
        let mut degraded = 0;
        let mut timed_out = 0;
        let mut unreachable = 0;
        let mut highest_archive_risk = ArchiveRisk::Unknown;

        for host in &hosts {
            match host.status {
                HostProbeStatus::Ok => ok += 1,
                HostProbeStatus::TimedOut => timed_out += 1,
                HostProbeStatus::Unreachable | HostProbeStatus::CommandNotFound => {
                    unreachable += 1;
                }
                HostProbeStatus::Partial
                | HostProbeStatus::OldBinarySkew
                | HostProbeStatus::Degraded => degraded += 1,
            }
            // ArchiveRisk derives Ord low→high (Unknown is the floor).
            if host.archive_risk > highest_archive_risk {
                highest_archive_risk = host.archive_risk;
            }
        }

        Self {
            schema_version: FLEET_DOCTOR_SCHEMA_VERSION,
            hosts,
            summary: FleetSummary {
                total_hosts,
                ok,
                degraded,
                timed_out,
                unreachable,
                highest_archive_risk,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated_ok_host() -> HostDoctorReport {
        let mut h = HostDoctorReport::skeleton(
            "ts1",
            Platform::linux_x86_64(),
            HostProbeStatus::Ok,
            42,
        );
        h.cass_version = Some("0.6.13".to_string());
        h.capability_tier = Some(CapabilityTier::Full);
        h.data_dir = Some("/home/ubuntu/.cass".to_string());
        h.source_roots = vec![SourceRoot {
            path: "/home/ubuntu/.claude".to_string(),
            agent: Some("claude".to_string()),
            archived: true,
        }];
        h.source_counts = Some(SourceCounts {
            roots: 1,
            sessions: 500,
            indexed_sessions: 500,
        });
        h.readiness = Some(ReadinessState::Ready);
        h.semantic = Some(SemanticState::Enabled);
        h.quarantine = Some(QuarantineState {
            quarantined: 0,
            recoverable: 0,
        });
        h.remote_sync = Some(RemoteSyncState::Synced);
        h.archive_risk = ArchiveRisk::Low;
        h
    }

    #[test]
    fn success_scenario_round_trips_with_all_fields() {
        let host = populated_ok_host();
        let value = serde_json::to_value(&host).expect("serialize");
        assert_eq!(value["status"], "ok");
        assert_eq!(value["host_alias"], "ts1");
        assert_eq!(value["platform"]["os"], "linux");
        assert_eq!(value["cass_version"], "0.6.13");
        assert_eq!(value["readiness"], "ready");
        assert_eq!(value["archive_risk"], "low");
        let back: HostDoctorReport = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back, host);
    }

    #[test]
    fn partial_scenario_names_skipped_sections() {
        let mut h = HostDoctorReport::skeleton(
            "css",
            Platform::linux_x86_64(),
            HostProbeStatus::Partial,
            7_900,
        );
        h.readiness = Some(ReadinessState::Ready);
        h.skipped_sections = vec!["semantic".to_string(), "remote_sync".to_string()];
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["status"], "partial");
        assert_eq!(value["skipped_sections"][0], "semantic");
        // Deep, unprobed fields are omitted entirely (not null noise).
        assert!(value.get("semantic").is_none());
        assert_eq!(serde_json::from_value::<HostDoctorReport>(value).unwrap(), h);
    }

    #[test]
    fn timeout_scenario_sets_flag_and_keeps_identity() {
        let h = HostDoctorReport::skeleton(
            "csd",
            Platform::linux_x86_64(),
            HostProbeStatus::TimedOut,
            8_000,
        );
        assert!(h.timed_out, "TimedOut status must set the scalar flag");
        assert!(!h.unreachable);
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["status"], "timed-out");
        assert_eq!(value["timed_out"], true);
        assert_eq!(value["host_alias"], "csd", "identity survives timeout");
        assert!(value.get("readiness").is_none(), "deep state absent on timeout");
    }

    #[test]
    fn old_binary_scenario_carries_version_and_action() {
        let mut h = HostDoctorReport::skeleton(
            "mac-mini-max",
            Platform {
                os: HostOs::MacOs,
                arch: "aarch64".to_string(),
                path_style: PathStyle::Posix,
                tool_notes: vec![],
            },
            HostProbeStatus::OldBinarySkew,
            120,
        );
        h.cass_version = Some("0.5.0".to_string());
        h.capability_tier = Some(CapabilityTier::Standard);
        h.likely_root_cause = Some(RootCauseFamily::OldBinarySkew);
        h.recommended_action = Some("upgrade cass to 0.6.13".to_string());
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["status"], "old-binary-skew");
        assert_eq!(value["cass_version"], "0.5.0");
        assert_eq!(value["likely_root_cause"], "old-binary-skew");
        assert_eq!(serde_json::from_value::<HostDoctorReport>(value).unwrap(), h);
    }

    #[test]
    fn command_not_found_scenario_is_hard_failure() {
        let h = HostDoctorReport::skeleton(
            "ts2",
            Platform::linux_x86_64(),
            HostProbeStatus::CommandNotFound,
            55,
        );
        assert!(h.status.is_hard_failure());
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["status"], "command-not-found");
        assert_eq!(value["host_alias"], "ts2");
    }

    #[test]
    fn unreachable_ssh_scenario_preserves_identity_and_attributes_transport() {
        let h = HostDoctorReport::unreachable(
            "mac-mini-old",
            Platform {
                os: HostOs::MacOs,
                arch: "x86_64".to_string(),
                path_style: PathStyle::Posix,
                tool_notes: vec![],
            },
            5_000,
            "check SSH reachability and host key for mac-mini-old",
        );
        assert!(h.unreachable);
        assert_eq!(h.status, HostProbeStatus::Unreachable);
        assert_eq!(h.likely_root_cause, Some(RootCauseFamily::RemoteTransportAuth));
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["status"], "unreachable");
        assert_eq!(value["unreachable"], true);
        assert_eq!(value["host_alias"], "mac-mini-old", "identity survives unreachable");
        assert!(value["recommended_action"].is_string());
        // No deep state leaked.
        assert!(value.get("readiness").is_none());
    }

    #[test]
    fn macos_path_and_tool_differences_are_representable() {
        let platform = Platform {
            os: HostOs::MacOs,
            arch: "aarch64".to_string(),
            path_style: PathStyle::Posix,
            tool_notes: vec!["rsync=bsd".to_string(), "data_dir=~/Library".to_string()],
        };
        let h = HostDoctorReport::skeleton("mac-mini-max", platform, HostProbeStatus::Ok, 80);
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["platform"]["os"], "macos");
        assert_eq!(value["platform"]["tool_notes"][0], "rsync=bsd");
        assert_eq!(serde_json::from_value::<HostDoctorReport>(value).unwrap(), h);
    }

    #[test]
    fn high_archive_risk_is_representable() {
        let mut h = populated_ok_host();
        h.status = HostProbeStatus::Degraded;
        h.archive_risk = ArchiveRisk::High;
        h.recommended_action = Some("back up derived archive before re-index".to_string());
        let value = serde_json::to_value(&h).unwrap();
        assert_eq!(value["archive_risk"], "high");
        assert_eq!(serde_json::from_value::<HostDoctorReport>(value).unwrap(), h);
    }

    #[test]
    fn host_identity_is_present_for_every_status() {
        for status in [
            HostProbeStatus::Ok,
            HostProbeStatus::Partial,
            HostProbeStatus::TimedOut,
            HostProbeStatus::OldBinarySkew,
            HostProbeStatus::CommandNotFound,
            HostProbeStatus::Unreachable,
            HostProbeStatus::Degraded,
        ] {
            let h = HostDoctorReport::skeleton("host-x", Platform::linux_x86_64(), status, 1);
            let value = serde_json::to_value(&h).unwrap();
            assert_eq!(value["host_alias"], "host-x", "{status:?}: lost host alias");
            assert!(value.get("platform").is_some(), "{status:?}: lost platform");
            // Status flags stay consistent with the discriminant.
            assert_eq!(value["timed_out"], status == HostProbeStatus::TimedOut);
            assert_eq!(value["unreachable"], status == HostProbeStatus::Unreachable);
        }
    }

    #[test]
    fn probe_status_wire_values_match_as_str() {
        for status in [
            HostProbeStatus::Ok,
            HostProbeStatus::Partial,
            HostProbeStatus::TimedOut,
            HostProbeStatus::OldBinarySkew,
            HostProbeStatus::CommandNotFound,
            HostProbeStatus::Unreachable,
            HostProbeStatus::Degraded,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{}\"", status.as_str()));
            let back: HostProbeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn archive_risk_orders_low_to_high() {
        assert!(ArchiveRisk::High > ArchiveRisk::Medium);
        assert!(ArchiveRisk::Medium > ArchiveRisk::Low);
        assert!(ArchiveRisk::Low > ArchiveRisk::Unknown);
    }

    #[test]
    fn fleet_summary_is_derived_and_takes_max_archive_risk() {
        let hosts = vec![
            populated_ok_host(),
            HostDoctorReport::skeleton("csd", Platform::linux_x86_64(), HostProbeStatus::TimedOut, 8000),
            HostDoctorReport::unreachable(
                "mac-mini-old",
                Platform::linux_x86_64(),
                5000,
                "check ssh",
            ),
            {
                let mut h = HostDoctorReport::skeleton(
                    "css",
                    Platform::linux_x86_64(),
                    HostProbeStatus::Degraded,
                    100,
                );
                h.archive_risk = ArchiveRisk::High;
                h
            },
        ];
        let report = FleetDoctorReport::from_hosts(hosts);
        assert_eq!(report.summary.total_hosts, 4);
        assert_eq!(report.summary.ok, 1);
        assert_eq!(report.summary.timed_out, 1);
        assert_eq!(report.summary.unreachable, 1);
        assert_eq!(report.summary.degraded, 1);
        assert_eq!(report.summary.highest_archive_risk, ArchiveRisk::High);

        // Whole report round-trips.
        let value = serde_json::to_value(&report).unwrap();
        let back: FleetDoctorReport = serde_json::from_value(value).unwrap();
        assert_eq!(back, report);
    }

    #[test]
    fn command_not_found_counts_as_unreachable_in_rollup() {
        let hosts = vec![HostDoctorReport::skeleton(
            "ts2",
            Platform::linux_x86_64(),
            HostProbeStatus::CommandNotFound,
            10,
        )];
        let report = FleetDoctorReport::from_hosts(hosts);
        assert_eq!(report.summary.unreachable, 1, "command-not-found is a hard failure");
    }
}
