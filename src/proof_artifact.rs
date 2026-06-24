//! Proof artifacts that distinguish real passes from timeouts and stale evidence.
//!
//! Bead: coding_agent_session_search-cass-fleet-resilience-20260608-uojcg.11.4
//! ("Record proof artifacts so pass timeout and stale evidence are
//! distinguishable").
//!
//! The motivating failure: a `cargo test --lib` run once *appeared* successful
//! but had actually timed out at 7200s **before any test ran** — a warm-cache run
//! later passed. A proof artifact must therefore never let "exited 0" or "no
//! failures" masquerade as a real pass: it records the command, binary
//! path/version, data dir/fixture, exit code, elapsed time, timeout status,
//! stdout/stderr artifact paths, and crucially **whether assertions actually
//! ran**, then classifies the run into one explicit [`ProofStatus`].
//!
//! This is pure, deterministic logic over a recorded [`ProofRun`] — unit-testable
//! without invoking anything — so the classification can be trusted as the single
//! source of truth for closeout docs and quality gates.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Stable schema version for the proof-artifact wire format.
pub const PROOF_ARTIFACT_SCHEMA_VERSION: u32 = 1;

/// The classified outcome of a proof run. Ordered so a fleet/gate rollup can take
/// the "worst" status with `max` (Pass is the floor of concern).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProofStatus {
    /// The command ran, assertions executed, and it succeeded.
    Pass,
    /// Assertions ran and at least one failed (a genuine, attributable failure).
    Fail,
    /// A partial proof: the run started and produced *some* evidence but did not
    /// complete (e.g. a bounded surface returned partial results).
    PartialProof,
    /// The run produced/refreshed artifacts but executed NO assertions — evidence
    /// exists but proves nothing about behavior (the "generated-only" trap).
    GeneratedOnly,
    /// The run was skipped (e.g. filtered out, precondition unmet).
    Skipped,
    /// The cited artifact is stale — older than the inputs/binary it claims to
    /// prove, so it must not be trusted as current evidence.
    StaleArtifact,
    /// The run hit its timeout. Critically this OUTRANKS a zero exit code: a
    /// timeout-before-tests-ran is a timeout, never a pass.
    Timeout,
}

impl ProofStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProofStatus::Pass => "pass",
            ProofStatus::Fail => "fail",
            ProofStatus::PartialProof => "partial-proof",
            ProofStatus::GeneratedOnly => "generated-only",
            ProofStatus::Skipped => "skipped",
            ProofStatus::StaleArtifact => "stale-artifact",
            ProofStatus::Timeout => "timeout",
        }
    }

    /// `true` only for [`ProofStatus::Pass`] — the single status a quality gate
    /// may treat as proven-good.
    pub const fn is_trustworthy_pass(self) -> bool {
        matches!(self, ProofStatus::Pass)
    }
}

/// The recorded facts of a single proof run, before classification. Timestamps
/// are epoch-millis; pass these in (don't read the clock here) so classification
/// stays pure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofRun {
    /// The exact command line (for reproduction).
    pub command: String,
    /// Path to the binary under test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    /// Binary version / contract version, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_version: Option<String>,
    /// Data dir or fixture id the run used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir_or_fixture: Option<String>,
    /// Process exit code, if the process completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Wall-clock the run took.
    pub elapsed_ms: u64,
    /// The timeout the run was given (0 = none).
    pub timeout_ms: u64,
    /// Whether the run hit its timeout.
    pub timed_out: bool,
    /// Whether the run was explicitly skipped.
    #[serde(default)]
    pub skipped: bool,
    /// Whether any assertions actually executed. This is the linchpin: exit 0 with
    /// `assertions_ran = false` is NOT a pass.
    pub assertions_ran: bool,
    /// Whether the run produced/refreshed an artifact (logs, golden, manifest).
    #[serde(default)]
    pub produced_artifact: bool,
    /// Whether the run completed all intended work (false => partial).
    #[serde(default = "default_true")]
    pub completed: bool,
    /// Artifact age relative to the newest input/binary it claims to prove, in ms;
    /// `Some(age)` with `age` exceeding the freshness window marks the artifact
    /// stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_age_ms: Option<u64>,
    /// stdout artifact path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_path: Option<String>,
    /// stderr artifact path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_path: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Freshness window: an artifact older than this vs. its inputs is stale.
pub const DEFAULT_STALE_AFTER_MS: u64 = 24 * 60 * 60 * 1000;

/// Classify a [`ProofRun`] into a [`ProofStatus`] using `stale_after_ms` as the
/// freshness window. Precedence is deliberate and safety-first:
/// timeout > skipped > stale > assertions-didn't-run (generated-only) >
/// failed-exit > incomplete (partial) > pass.
pub fn classify(run: &ProofRun, stale_after_ms: u64) -> ProofStatus {
    // A timeout outranks everything — even a zero exit code — so a run that timed
    // out before tests ran can never read as a pass.
    if run.timed_out || (run.timeout_ms > 0 && run.elapsed_ms >= run.timeout_ms) {
        return ProofStatus::Timeout;
    }
    if run.skipped {
        return ProofStatus::Skipped;
    }
    if let Some(age) = run.artifact_age_ms
        && age > stale_after_ms
    {
        return ProofStatus::StaleArtifact;
    }
    // Evidence exists but nothing was actually asserted -> proves nothing.
    if !run.assertions_ran {
        return ProofStatus::GeneratedOnly;
    }
    // Assertions ran; a non-zero exit is a genuine failure.
    if matches!(run.exit_code, Some(code) if code != 0) {
        return ProofStatus::Fail;
    }
    if !run.completed {
        return ProofStatus::PartialProof;
    }
    ProofStatus::Pass
}

/// A fully classified proof artifact: the recorded run plus its trustworthy
/// [`ProofStatus`] and a human summary. Stable snake_case JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofArtifact {
    pub schema_version: u32,
    pub status: ProofStatus,
    pub run: ProofRun,
    /// One-line, action-oriented summary (human convenience; facts live in `run`).
    pub summary: String,
}

impl ProofArtifact {
    /// Build a classified artifact from a run, using the default freshness window.
    pub fn from_run(run: ProofRun) -> Self {
        Self::from_run_with_window(run, DEFAULT_STALE_AFTER_MS)
    }

    /// Build a classified artifact with an explicit freshness window.
    pub fn from_run_with_window(run: ProofRun, stale_after_ms: u64) -> Self {
        let status = classify(&run, stale_after_ms);
        let summary = match status {
            ProofStatus::Pass => format!("pass in {}ms: {}", run.elapsed_ms, run.command),
            ProofStatus::Fail => format!(
                "FAIL (exit {}): {}",
                run.exit_code.unwrap_or(-1),
                run.command
            ),
            ProofStatus::Timeout => format!(
                "TIMEOUT after {}ms (cap {}ms) — assertions_ran={}: {}",
                run.elapsed_ms, run.timeout_ms, run.assertions_ran, run.command
            ),
            ProofStatus::GeneratedOnly => format!(
                "generated-only (no assertions ran) — not a pass: {}",
                run.command
            ),
            ProofStatus::Skipped => format!("skipped: {}", run.command),
            ProofStatus::StaleArtifact => format!(
                "stale artifact (age {}ms): {}",
                run.artifact_age_ms.unwrap_or(0),
                run.command
            ),
            ProofStatus::PartialProof => {
                format!("partial proof (incomplete run): {}", run.command)
            }
        };
        Self {
            schema_version: PROOF_ARTIFACT_SCHEMA_VERSION,
            status,
            run,
            summary,
        }
    }

    /// Whether this artifact may be cited as a current, trustworthy pass.
    pub fn is_trustworthy_pass(&self) -> bool {
        self.status.is_trustworthy_pass()
    }

    /// Serialize this classified artifact to `path` as pretty JSON, creating
    /// parent directories as needed. Emission is the step that turns an
    /// in-memory verdict into a citable artifact on disk (see [`emit_proof_artifact`]).
    pub fn write_json(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(path, bytes)
    }
}

/// One emitted proof-artifact record, as referenced from a [`ProofManifest`]:
/// the caller's `label`, the classified [`ProofStatus`], the artifact file path,
/// and the reproduced command. This is the index entry agents cite — it points
/// at the full [`ProofArtifact`] JSON on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmittedProof {
    pub label: String,
    pub status: ProofStatus,
    pub path: String,
    pub command: String,
}

/// Sanitize an arbitrary label into a filesystem-safe artifact stem.
fn safe_stem(label: &str) -> String {
    let stem: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() {
        "proof".to_string()
    } else {
        stem
    }
}

/// Classify `run`, write `<dir>/<label>.proof.json`, and return its index entry.
/// Uses the default freshness window ([`DEFAULT_STALE_AFTER_MS`]).
pub fn emit_proof_artifact(dir: &Path, label: &str, run: ProofRun) -> io::Result<EmittedProof> {
    emit_proof_artifact_with_window(dir, label, run, DEFAULT_STALE_AFTER_MS)
}

/// [`emit_proof_artifact`] with an explicit freshness window.
pub fn emit_proof_artifact_with_window(
    dir: &Path,
    label: &str,
    run: ProofRun,
    stale_after_ms: u64,
) -> io::Result<EmittedProof> {
    let artifact = ProofArtifact::from_run_with_window(run, stale_after_ms);
    let path = dir.join(format!("{}.proof.json", safe_stem(label)));
    artifact.write_json(&path)?;
    Ok(EmittedProof {
        label: label.to_string(),
        status: artifact.status,
        path: path.to_string_lossy().into_owned(),
        command: artifact.run.command,
    })
}

/// A manifest of the proofs one suite/run emitted. Its log-completeness verdict
/// is what makes a gate "unable to pass by doing nothing": an empty manifest is
/// never a clean pass, and a single timeout / stale / generated-only / fail /
/// partial / skipped entry sinks the verdict — exactly the confusable outcomes
/// the proof taxonomy exists to separate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofManifest {
    pub entries: Vec<EmittedProof>,
}

impl ProofManifest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one emitted proof to the manifest.
    pub fn record(&mut self, emitted: EmittedProof) {
        self.entries.push(emitted);
    }

    /// The worst status across all entries (`Pass` is the floor of concern);
    /// `None` when the manifest is empty.
    #[must_use]
    pub fn worst_status(&self) -> Option<ProofStatus> {
        self.entries.iter().map(|entry| entry.status).max()
    }

    /// `true` only when there is at least one entry and EVERY entry is a
    /// trustworthy pass. Empty manifests return `false` ("cannot pass by doing
    /// nothing"); any non-pass entry returns `false`.
    #[must_use]
    pub fn is_clean_pass(&self) -> bool {
        !self.entries.is_empty()
            && self
                .entries
                .iter()
                .all(|entry| entry.status.is_trustworthy_pass())
    }

    /// Count of entries carrying `status`.
    #[must_use]
    pub fn count_with(&self, status: ProofStatus) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.status == status)
            .count()
    }

    /// Write the manifest as JSONL (one [`EmittedProof`] per line) to `path`,
    /// creating parent directories as needed.
    pub fn write_jsonl(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = String::new();
        for entry in &self.entries {
            let line = serde_json::to_string(entry).map_err(io::Error::other)?;
            buf.push_str(&line);
            buf.push('\n');
        }
        std::fs::write(path, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_run() -> ProofRun {
        ProofRun {
            command: "cargo test --lib".to_string(),
            binary_path: Some("/tmp/cass-tgt/debug/cass".to_string()),
            binary_version: Some("0.6.13".to_string()),
            data_dir_or_fixture: Some("fixture:healthy".to_string()),
            exit_code: Some(0),
            elapsed_ms: 1_200,
            timeout_ms: 60_000,
            timed_out: false,
            skipped: false,
            assertions_ran: true,
            produced_artifact: true,
            completed: true,
            artifact_age_ms: Some(1_000),
            stdout_path: Some("/tmp/proof/out.log".to_string()),
            stderr_path: Some("/tmp/proof/err.log".to_string()),
        }
    }

    #[test]
    fn clean_run_is_pass() {
        let a = ProofArtifact::from_run(base_run());
        assert_eq!(a.status, ProofStatus::Pass);
        assert!(a.is_trustworthy_pass());
    }

    #[test]
    fn timeout_before_tests_ran_is_timeout_not_pass() {
        // The exact motivating failure: exit 0-ish but timed out before assertions.
        let mut run = base_run();
        run.exit_code = Some(0);
        run.assertions_ran = false;
        run.elapsed_ms = 7_200_000;
        run.timeout_ms = 7_200_000;
        run.timed_out = true;
        let a = ProofArtifact::from_run(run);
        assert_eq!(
            a.status,
            ProofStatus::Timeout,
            "timeout must outrank a zero exit"
        );
        assert!(!a.is_trustworthy_pass());
    }

    #[test]
    fn elapsed_exceeding_timeout_is_timeout_even_without_flag() {
        let mut run = base_run();
        run.timed_out = false;
        run.timeout_ms = 1_000;
        run.elapsed_ms = 5_000;
        assert_eq!(classify(&run, DEFAULT_STALE_AFTER_MS), ProofStatus::Timeout);
    }

    #[test]
    fn assertions_not_run_is_generated_only() {
        let mut run = base_run();
        run.assertions_ran = false;
        run.produced_artifact = true;
        let a = ProofArtifact::from_run(run);
        assert_eq!(a.status, ProofStatus::GeneratedOnly);
        assert!(!a.is_trustworthy_pass(), "generated-only is never a pass");
    }

    #[test]
    fn nonzero_exit_with_assertions_is_fail() {
        let mut run = base_run();
        run.exit_code = Some(101);
        assert_eq!(classify(&run, DEFAULT_STALE_AFTER_MS), ProofStatus::Fail);
    }

    #[test]
    fn skipped_run_is_skipped() {
        let mut run = base_run();
        run.skipped = true;
        assert_eq!(classify(&run, DEFAULT_STALE_AFTER_MS), ProofStatus::Skipped);
    }

    #[test]
    fn old_artifact_is_stale() {
        let mut run = base_run();
        run.artifact_age_ms = Some(48 * 60 * 60 * 1000);
        assert_eq!(
            classify(&run, DEFAULT_STALE_AFTER_MS),
            ProofStatus::StaleArtifact
        );
    }

    #[test]
    fn incomplete_run_is_partial_proof() {
        let mut run = base_run();
        run.completed = false;
        assert_eq!(
            classify(&run, DEFAULT_STALE_AFTER_MS),
            ProofStatus::PartialProof
        );
    }

    #[test]
    fn precedence_timeout_outranks_skipped_and_stale() {
        let mut run = base_run();
        run.timed_out = true;
        run.skipped = true;
        run.artifact_age_ms = Some(u64::MAX);
        assert_eq!(classify(&run, DEFAULT_STALE_AFTER_MS), ProofStatus::Timeout);
    }

    #[test]
    fn artifact_serializes_with_stable_fields_and_round_trips() {
        let a = ProofArtifact::from_run(base_run());
        let value = serde_json::to_value(&a).unwrap();
        assert_eq!(value["schema_version"], PROOF_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(value["status"], "pass");
        assert_eq!(value["run"]["command"], "cargo test --lib");
        assert_eq!(value["run"]["assertions_ran"], true);
        assert_eq!(value["run"]["exit_code"], 0);
        assert_eq!(value["run"]["stdout_path"], "/tmp/proof/out.log");
        let back: ProofArtifact = serde_json::from_value(value).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn status_wire_values_are_kebab() {
        for (s, w) in [
            (ProofStatus::Pass, "pass"),
            (ProofStatus::Fail, "fail"),
            (ProofStatus::PartialProof, "partial-proof"),
            (ProofStatus::GeneratedOnly, "generated-only"),
            (ProofStatus::Skipped, "skipped"),
            (ProofStatus::StaleArtifact, "stale-artifact"),
            (ProofStatus::Timeout, "timeout"),
        ] {
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{w}\""));
            assert_eq!(s.as_str(), w);
        }
    }

    // ---- emission + manifest (the "record proof artifacts" half of .11.4) ----

    #[test]
    fn emit_writes_a_classified_artifact_file_that_round_trips() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let emitted = emit_proof_artifact(dir.path(), "clean run", base_run()).expect("emit");
        assert_eq!(emitted.status, ProofStatus::Pass);
        // The label is sanitized into the file stem (space -> '_').
        assert!(
            emitted.path.ends_with("clean_run.proof.json"),
            "{}",
            emitted.path
        );

        let bytes = std::fs::read(&emitted.path).expect("read artifact");
        let back: ProofArtifact = serde_json::from_slice(&bytes).expect("parse artifact");
        assert_eq!(back.status, ProofStatus::Pass);
        assert_eq!(back.run.command, "cargo test --lib");
        assert_eq!(back.schema_version, PROOF_ARTIFACT_SCHEMA_VERSION);
    }

    #[test]
    fn emitted_timeout_before_tests_is_recorded_as_timeout_not_pass() {
        // The motivating trap: exit 0-ish but timed out before assertions ran.
        let mut run = base_run();
        run.exit_code = Some(0);
        run.assertions_ran = false;
        run.timed_out = true;
        run.elapsed_ms = 7_200_000;
        run.timeout_ms = 7_200_000;

        let dir = tempfile::TempDir::new().expect("tempdir");
        let emitted = emit_proof_artifact(dir.path(), "lib-tests", run).expect("emit");
        assert_eq!(emitted.status, ProofStatus::Timeout);

        // The persisted artifact agrees — a reader cannot mistake it for a pass.
        let bytes = std::fs::read(&emitted.path).expect("read");
        let back: ProofArtifact = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(back.status, ProofStatus::Timeout);
        assert!(!back.is_trustworthy_pass());
    }

    #[test]
    fn empty_manifest_cannot_pass_by_doing_nothing() {
        let manifest = ProofManifest::new();
        assert!(
            !manifest.is_clean_pass(),
            "an empty manifest is never a pass"
        );
        assert_eq!(manifest.worst_status(), None);
    }

    #[test]
    fn manifest_of_all_passes_is_a_clean_pass() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mut manifest = ProofManifest::new();
        for label in ["a", "b", "c"] {
            manifest.record(emit_proof_artifact(dir.path(), label, base_run()).expect("emit"));
        }
        assert!(manifest.is_clean_pass());
        assert_eq!(manifest.worst_status(), Some(ProofStatus::Pass));
        assert_eq!(manifest.count_with(ProofStatus::Pass), 3);
    }

    #[test]
    fn a_single_timeout_entry_sinks_the_manifest_verdict() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mut manifest = ProofManifest::new();
        manifest.record(emit_proof_artifact(dir.path(), "ok", base_run()).expect("emit"));
        let mut timed = base_run();
        timed.timed_out = true;
        manifest.record(emit_proof_artifact(dir.path(), "hang", timed).expect("emit"));

        assert!(
            !manifest.is_clean_pass(),
            "one timeout must sink the verdict"
        );
        // `max` over the status ordering surfaces the worst outcome.
        assert_eq!(manifest.worst_status(), Some(ProofStatus::Timeout));
        assert_eq!(manifest.count_with(ProofStatus::Timeout), 1);
        assert_eq!(manifest.count_with(ProofStatus::Pass), 1);
    }

    #[test]
    fn manifest_jsonl_round_trips_every_entry() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mut manifest = ProofManifest::new();
        manifest.record(emit_proof_artifact(dir.path(), "one", base_run()).expect("emit"));
        manifest.record(emit_proof_artifact(dir.path(), "two", base_run()).expect("emit"));
        let manifest_path = dir.path().join("proof-manifest.jsonl");
        manifest
            .write_jsonl(&manifest_path)
            .expect("write manifest");

        let text = std::fs::read_to_string(&manifest_path).expect("read manifest");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let entry: EmittedProof = serde_json::from_str(line).expect("parse entry");
            assert_eq!(entry.status, ProofStatus::Pass);
            assert!(entry.path.ends_with(".proof.json"));
        }
    }

    #[test]
    fn blank_label_falls_back_to_a_stable_stem() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let emitted = emit_proof_artifact(dir.path(), "///", base_run()).expect("emit");
        assert!(emitted.path.ends_with("___.proof.json"), "{}", emitted.path);
        let emitted_empty = emit_proof_artifact(dir.path(), "", base_run()).expect("emit");
        assert!(
            emitted_empty.path.ends_with("proof.proof.json"),
            "{}",
            emitted_empty.path
        );
    }
}
