const CHECKLIST: &str = include_str!("../docs/planning/DOCTOR_V2_RELEASE_CHECKLIST.md");
const RELEASE_WORKFLOW: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn doctor_v2_release_checklist_records_required_release_gates() {
    for required in [
        "cargo fmt --check",
        "cargo check --all-targets",
        "cargo clippy --all-targets -- -D warnings",
        "UPDATE_GOLDENS=1 rch exec -- env CARGO_TARGET_DIR=/data/tmp/cass_release_check cargo test --test golden_robot_json --test golden_robot_docs",
        "scripts/e2e/doctor_v2.sh run --label quick",
        "scripts/e2e/doctor_v2.sh run --label safe-auto",
        "scripts/e2e/doctor_v2.sh run --label promotion",
        "scripts/e2e/doctor_v2.sh run --label cleanup",
        "Representative Data-Dir Copy Dry Run",
        "No new rusqlite usage",
        r#"r[u]sqlite([[:space:]]*::|[[:space:]]*=|[[:space:]]*;|[[:space:]]+as[[:space:]])"#,
        "No unsafe archive cleanup docs",
    ] {
        assert!(
            CHECKLIST.contains(required),
            "release checklist is missing required gate: {required}"
        );
    }
}

#[test]
fn doctor_v2_release_checklist_pins_archive_first_user_contract() {
    for required in [
        "read-only",
        "safe-auto",
        "plan_fingerprint",
        "Candidate repair must build in isolation",
        "Backups, raw mirrors, DB/WAL/SHM files, receipts, support bundles, configs",
        "bookmarks, and failure markers are precious evidence",
        "Never use bare `cass` in automation",
        "Never recommend manual deletion of cass archive evidence",
        "support bundles include manifest/checksum evidence and redacted",
        "summaries by default",
        "A blocked repair is not necessarily a failure",
    ] {
        assert!(
            CHECKLIST.contains(required),
            "release checklist is missing archive-first contract text: {required}"
        );
    }
}

#[test]
fn doctor_v2_release_checklist_records_current_evidence_index() {
    for required in [
        "/data/tmp/cass_57xo8_verify",
        "/data/tmp/cass-doctor-v2-proof/run-20260506T185419Z-165122",
        "/data/tmp/cass-doctor-v2-proof/run-20260506T185429Z-169162",
        "/data/tmp/cass-doctor-v2-proof/run-20260506T190058Z-345851",
        "cargo test --lib raw_mirror",
        "cargo test --lib doctor_remote_source_sync_report",
        "doctor_e2e_runner_blocks_coverage_decreasing_candidate_promotion",
        "doctor_e2e_runner_reports_no_safe_rebuild_authority_without_mirror",
        "br dep cycles --json",
    ] {
        assert!(
            CHECKLIST.contains(required),
            "release checklist evidence index is missing: {required}"
        );
    }
}

#[test]
fn release_workflow_keeps_scoop_legacy_windows_asset_compatibility() -> Result<(), String> {
    let missing = [
        "coding-agent-search-x86_64-pc-windows-msvc.zip",
        "Copy-Item $zipName $legacyZipName",
        "\"$hash  $legacyZipName\" | Out-File -Encoding ASCII \"$legacyZipName.sha256\"",
    ]
    .into_iter()
    .find(|required| !RELEASE_WORKFLOW.contains(required));

    missing
        .map(|required| {
            format!("release workflow is missing Scoop compatibility asset handling: {required}")
        })
        .map_or(Ok(()), Err)
}

#[test]
fn release_workflow_surfaces_manual_package_manager_dispatch_fallbacks() -> Result<(), String> {
    let missing = [
        "Manual Homebrew tap update required",
        "gh workflow run auto-update.yml --repo Dicklesworthstone/homebrew-tap -f tool=cass -f version=${{ needs.release.outputs.version }}",
        "Manual Scoop bucket update required",
        "gh workflow run auto-update.yml --repo Dicklesworthstone/scoop-bucket -f tool=cass -f version=${{ needs.release.outputs.version }}",
    ]
    .into_iter()
    .find(|required| !RELEASE_WORKFLOW.contains(required));

    missing
        .map(|required| {
            format!("release workflow is missing manual dispatch fallback text: {required}")
        })
        .map_or(Ok(()), Err)
}
