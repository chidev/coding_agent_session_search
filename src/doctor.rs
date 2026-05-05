//! Typed cass doctor command boundary.
//!
//! The safety-critical doctor executor is intentionally reached through this
//! module so legacy flag spellings and future subcommands share one command
//! model before any repair code can run.

use std::path::PathBuf;

use crate::{CliError, CliResult, RobotFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorCommandSurface {
    LegacyDoctor,
    Check,
    Repair,
    Cleanup,
    Reconstruct,
    Restore,
    BaselineDiff,
    SupportBundle,
}

const DOCTOR_COMMAND_SURFACES: &[DoctorCommandSurface] = &[
    DoctorCommandSurface::LegacyDoctor,
    DoctorCommandSurface::Check,
    DoctorCommandSurface::Repair,
    DoctorCommandSurface::Cleanup,
    DoctorCommandSurface::Reconstruct,
    DoctorCommandSurface::Restore,
    DoctorCommandSurface::BaselineDiff,
    DoctorCommandSurface::SupportBundle,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorExecutionMode {
    ReadOnlyCheck,
    RepairDryRun,
    FingerprintApply,
    CleanupDryRun,
    CleanupApply,
    SafeAutoFix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorCommandRequest {
    pub surface: DoctorCommandSurface,
    pub mode: DoctorExecutionMode,
    pub data_dir: Option<PathBuf>,
    pub db_path: Option<PathBuf>,
    pub output_format: Option<RobotFormat>,
    pub verbose: bool,
    pub force_rebuild: bool,
    pub allow_repeated_repair: bool,
    pub repair: bool,
    pub cleanup: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub plan_fingerprint: Option<String>,
}

impl DoctorCommandSurface {
    pub(crate) fn stable_name(self) -> &'static str {
        match self {
            Self::LegacyDoctor => "legacy-doctor",
            Self::Check => "check",
            Self::Repair => "repair",
            Self::Cleanup => "cleanup",
            Self::Reconstruct => "reconstruct",
            Self::Restore => "restore",
            Self::BaselineDiff => "baseline-diff",
            Self::SupportBundle => "support-bundle",
        }
    }

    pub(crate) fn mutates_by_default(self) -> bool {
        matches!(
            self,
            Self::Repair | Self::Cleanup | Self::Reconstruct | Self::Restore
        )
    }
}

impl DoctorExecutionMode {
    pub(crate) fn stable_name(self) -> &'static str {
        match self {
            Self::ReadOnlyCheck => "read-only-check",
            Self::RepairDryRun => "repair-dry-run",
            Self::FingerprintApply => "fingerprint-apply",
            Self::CleanupDryRun => "cleanup-dry-run",
            Self::CleanupApply => "cleanup-apply",
            Self::SafeAutoFix => "safe-auto-fix",
        }
    }

    pub(crate) fn permits_mutation(self) -> bool {
        matches!(
            self,
            Self::FingerprintApply | Self::CleanupApply | Self::SafeAutoFix
        )
    }

    pub(crate) fn requires_plan_fingerprint(self) -> bool {
        matches!(self, Self::FingerprintApply | Self::CleanupApply)
    }
}

impl DoctorCommandRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_cli_flags(
        data_dir: Option<PathBuf>,
        db_path: Option<PathBuf>,
        output_format: Option<RobotFormat>,
        check: bool,
        fix: bool,
        repair: bool,
        cleanup: bool,
        dry_run: bool,
        yes: bool,
        plan_fingerprint: Option<String>,
        verbose: bool,
        force_rebuild: bool,
        allow_repeated_repair: bool,
    ) -> CliResult<Self> {
        let surface = if check {
            DoctorCommandSurface::Check
        } else if repair {
            DoctorCommandSurface::Repair
        } else if cleanup {
            DoctorCommandSurface::Cleanup
        } else {
            DoctorCommandSurface::LegacyDoctor
        };
        let mode = if repair && dry_run {
            DoctorExecutionMode::RepairDryRun
        } else if repair && yes && plan_fingerprint.is_some() {
            DoctorExecutionMode::FingerprintApply
        } else if cleanup && yes && plan_fingerprint.is_some() {
            DoctorExecutionMode::CleanupApply
        } else if cleanup {
            DoctorExecutionMode::CleanupDryRun
        } else if fix {
            DoctorExecutionMode::SafeAutoFix
        } else {
            DoctorExecutionMode::ReadOnlyCheck
        };
        let request = Self {
            surface,
            mode,
            data_dir,
            db_path,
            output_format,
            verbose,
            force_rebuild,
            allow_repeated_repair,
            repair,
            cleanup,
            dry_run,
            yes,
            plan_fingerprint,
        };
        request.validate()?;
        Ok(request)
    }

    #[cfg(test)]
    pub(crate) fn from_legacy_flags(
        data_dir: Option<PathBuf>,
        db_path: Option<PathBuf>,
        output_format: Option<RobotFormat>,
        fix: bool,
        verbose: bool,
        force_rebuild: bool,
        allow_repeated_repair: bool,
    ) -> CliResult<Self> {
        Self::from_cli_flags(
            data_dir,
            db_path,
            output_format,
            false,
            fix,
            false,
            false,
            false,
            false,
            None,
            verbose,
            force_rebuild,
            allow_repeated_repair,
        )
    }

    pub(crate) fn validate(&self) -> CliResult<()> {
        debug_assert!(DOCTOR_COMMAND_SURFACES.contains(&self.surface));
        debug_assert!(!self.mode.stable_name().is_empty());
        let explicit_surface_count =
            usize::from(self.surface == DoctorCommandSurface::Check)
                + usize::from(self.repair)
                + usize::from(self.cleanup);
        if explicit_surface_count > 1 {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "cass doctor accepts only one explicit surface at a time".to_string(),
                hint: Some(
                    "Use exactly one of `cass doctor check`, `cass doctor repair`, or `cass doctor cleanup`."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.dry_run && !(self.repair || self.cleanup) {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`--dry-run` is only valid with `cass doctor repair` or `cass doctor cleanup`"
                    .to_string(),
                hint: Some(
                    "Use `cass doctor repair --dry-run --json` for repair plans or `cass doctor cleanup --json` for cleanup plans."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.yes && !(self.repair || self.cleanup) {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`--yes` is only valid with `cass doctor repair` or `cass doctor cleanup`"
                    .to_string(),
                hint: Some(
                    "Use `--yes --plan-fingerprint <fingerprint>` only after inspecting the matching dry-run plan."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.plan_fingerprint.is_some() && !(self.repair || self.cleanup) {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`--plan-fingerprint` is only valid with `cass doctor repair` or `cass doctor cleanup`"
                    .to_string(),
                hint: Some(
                    "First run the matching dry-run command, then apply the exact fingerprint it reports."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if (self.repair || self.cleanup) && self.mode == DoctorExecutionMode::SafeAutoFix {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: format!(
                    "`cass doctor {}` does not accept legacy `--fix`",
                    self.surface.stable_name()
                ),
                hint: Some(
                    "Use the explicit dry-run/apply flow for repair or cleanup instead of legacy `--fix`."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if (self.repair || self.cleanup) && self.dry_run && self.yes {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: format!(
                    "`cass doctor {}` cannot combine `--dry-run` and `--yes`",
                    self.surface.stable_name()
                ),
                hint: Some(
                    "Run the dry-run first, then run a separate apply command with the reported fingerprint."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.repair && !self.dry_run && !self.yes {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`cass doctor repair` requires `--dry-run` or `--yes --plan-fingerprint <fingerprint>`"
                    .to_string(),
                hint: Some(
                    "Start with `cass doctor repair --dry-run --json` so cass can print the exact apply command."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.repair && self.yes && self.plan_fingerprint.is_none() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`cass doctor repair --yes` requires `--plan-fingerprint <fingerprint>`"
                    .to_string(),
                hint: Some(
                    "Copy the plan_fingerprint from `cass doctor repair --dry-run --json`."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.repair && !self.yes && self.plan_fingerprint.is_some() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`--plan-fingerprint` requires `--yes` for `cass doctor repair`"
                    .to_string(),
                hint: Some(
                    "Use `cass doctor repair --yes --plan-fingerprint <fingerprint> --json` after inspecting the dry-run."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.cleanup && self.yes && self.plan_fingerprint.is_none() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message:
                    "`cass doctor cleanup --yes` requires `--plan-fingerprint <fingerprint>`"
                        .to_string(),
                hint: Some(
                    "Copy the cleanup approval fingerprint from `cass doctor cleanup --json`."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.cleanup && !self.yes && self.plan_fingerprint.is_some() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`--plan-fingerprint` requires `--yes` for `cass doctor cleanup`"
                    .to_string(),
                hint: Some(
                    "Use `cass doctor cleanup --yes --plan-fingerprint <fingerprint> --json` after inspecting the dry-run."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.allow_repeated_repair && !self.mode.permits_mutation() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message:
                    "`--allow-repeated-repair` is only valid with a mutating doctor apply"
                        .to_string(),
                hint: Some(
                    "Inspect the previous failure marker before rerunning a mutating doctor command with `--allow-repeated-repair`."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.surface == DoctorCommandSurface::Check && self.mode.permits_mutation() {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`cass doctor check` is always read-only and cannot run with `--fix`"
                    .to_string(),
                hint: Some(
                    "Run `cass doctor check --json` first, then use a separate explicit repair command after inspecting the check result."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        if self.surface == DoctorCommandSurface::Check && self.force_rebuild {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: "`cass doctor check` is read-only and does not accept `--force-rebuild`"
                    .to_string(),
                hint: Some(
                    "Run `cass doctor check --json` first, then use `cass doctor --fix --force-rebuild --json` only after inspecting the check result."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        let read_only_repair_plan = self.surface == DoctorCommandSurface::Repair
            && self.mode == DoctorExecutionMode::RepairDryRun;
        let read_only_cleanup_plan = self.surface == DoctorCommandSurface::Cleanup
            && self.mode == DoctorExecutionMode::CleanupDryRun;
        if self.surface.mutates_by_default()
            && !self.mode.permits_mutation()
            && !read_only_repair_plan
            && !read_only_cleanup_plan
        {
            return Err(CliError {
                code: 2,
                kind: "usage",
                message: format!(
                    "doctor surface `{}` requires an explicit mutating execution mode",
                    self.surface.stable_name()
                ),
                hint: Some(
                    "Use a read-only doctor check first, then apply the exact fingerprint-approved repair command."
                        .to_string(),
                ),
                retryable: false,
            });
        }
        Ok(())
    }
}

pub(crate) fn execute_doctor_command(request: DoctorCommandRequest) -> CliResult<()> {
    request.validate()?;
    crate::run_doctor_impl(
        &request.data_dir,
        request.db_path,
        request.output_format,
        request.mode.permits_mutation(),
        request.verbose,
        request.force_rebuild,
        request.allow_repeated_repair,
        request.surface,
        request.mode,
        request.plan_fingerprint,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_read_only_flags_map_to_typed_check_mode() {
        let request = DoctorCommandRequest::from_legacy_flags(
            Some(PathBuf::from("/tmp/cass-data")),
            None,
            Some(RobotFormat::Json),
            false,
            true,
            false,
            false,
        )
        .expect("legacy read-only doctor flags should map");

        assert_eq!(request.surface, DoctorCommandSurface::LegacyDoctor);
        assert_eq!(request.mode, DoctorExecutionMode::ReadOnlyCheck);
        assert_eq!(request.mode.stable_name(), "read-only-check");
        assert!(!request.mode.permits_mutation());
        assert!(request.verbose);
    }

    #[test]
    fn legacy_fix_flags_map_to_safe_auto_fix_mode() {
        let request = DoctorCommandRequest::from_legacy_flags(
            None,
            Some(PathBuf::from("/tmp/agent_search.db")),
            Some(RobotFormat::Compact),
            true,
            false,
            true,
            true,
        )
        .expect("legacy fix doctor flags should map");

        assert_eq!(request.mode, DoctorExecutionMode::SafeAutoFix);
        assert_eq!(request.mode.stable_name(), "safe-auto-fix");
        assert!(request.mode.permits_mutation());
        assert!(request.force_rebuild);
        assert!(request.allow_repeated_repair);
    }

    #[test]
    fn check_subcommand_maps_to_explicit_read_only_surface() {
        let request = DoctorCommandRequest::from_cli_flags(
            Some(PathBuf::from("/tmp/cass-data")),
            None,
            Some(RobotFormat::Json),
            true,
            false,
            false,
            false,
            false,
            false,
            None,
            false,
            false,
            false,
        )
        .expect("doctor check flags should map");

        assert_eq!(request.surface, DoctorCommandSurface::Check);
        assert_eq!(request.surface.stable_name(), "check");
        assert_eq!(request.mode, DoctorExecutionMode::ReadOnlyCheck);
        assert!(!request.mode.permits_mutation());
    }

    #[test]
    fn allow_repeated_repair_without_fix_fails_closed() {
        let err = DoctorCommandRequest::from_legacy_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
        )
        .expect_err("allow repeated repair without fix must be rejected");

        assert_eq!(err.code, 2);
        assert_eq!(err.kind, "usage");
        assert!(err.message.contains("--allow-repeated-repair"));
    }

    #[test]
    fn check_subcommand_rejects_force_rebuild() {
        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            true,
            false,
            false,
            false,
            false,
            false,
            None,
            false,
            true,
            false,
        )
        .expect_err("doctor check must reject force rebuild flags");

        assert_eq!(err.code, 2);
        assert_eq!(err.kind, "usage");
        assert!(err.message.contains("doctor check"));
    }

    #[test]
    fn check_subcommand_rejects_mutating_execution_mode_inside_typed_boundary() {
        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            true,
            true,
            false,
            false,
            false,
            false,
            None,
            false,
            false,
            false,
        )
        .expect_err("doctor check must reject mutating execution mode");

        assert_eq!(err.code, 2);
        assert_eq!(err.kind, "usage");
        assert!(err.message.contains("read-only"));
    }

    #[test]
    fn mutating_surfaces_require_mutating_mode() {
        let request = DoctorCommandRequest {
            surface: DoctorCommandSurface::Reconstruct,
            mode: DoctorExecutionMode::ReadOnlyCheck,
            data_dir: None,
            db_path: None,
            output_format: Some(RobotFormat::Json),
            verbose: false,
            force_rebuild: false,
            allow_repeated_repair: false,
            repair: false,
            cleanup: false,
            dry_run: false,
            yes: false,
            plan_fingerprint: None,
        };
        let err = request
            .validate()
            .expect_err("mutating doctor surfaces must fail closed without mutating mode");

        assert_eq!(err.code, 2);
        assert!(err.message.contains("reconstruct"));
    }

    #[test]
    fn repair_dry_run_maps_to_non_mutating_plan_mode() {
        let request = DoctorCommandRequest::from_cli_flags(
            Some(PathBuf::from("/tmp/cass-data")),
            None,
            Some(RobotFormat::Json),
            false,
            false,
            true,
            false,
            true,
            false,
            None,
            false,
            false,
            false,
        )
        .expect("doctor repair dry-run should map");

        assert_eq!(request.surface, DoctorCommandSurface::Repair);
        assert_eq!(request.mode, DoctorExecutionMode::RepairDryRun);
        assert_eq!(request.mode.stable_name(), "repair-dry-run");
        assert!(!request.mode.permits_mutation());
        assert!(!request.mode.requires_plan_fingerprint());
    }

    #[test]
    fn repair_apply_requires_yes_and_plan_fingerprint() {
        let request = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            true,
            false,
            false,
            true,
            Some("doctor-repair-apply-plan-v1-abc".to_string()),
            false,
            false,
            false,
        )
        .expect("fingerprint-approved repair should map");

        assert_eq!(request.surface, DoctorCommandSurface::Repair);
        assert_eq!(request.mode, DoctorExecutionMode::FingerprintApply);
        assert_eq!(request.mode.stable_name(), "fingerprint-apply");
        assert!(request.mode.permits_mutation());
        assert!(request.mode.requires_plan_fingerprint());
    }

    #[test]
    fn cleanup_subcommand_maps_to_non_mutating_dry_run_by_default() {
        let request = DoctorCommandRequest::from_cli_flags(
            Some(PathBuf::from("/tmp/cass-data")),
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
            false,
            false,
            None,
            false,
            false,
            false,
        )
        .expect("doctor cleanup should default to read-only cleanup dry-run");

        assert_eq!(request.surface, DoctorCommandSurface::Cleanup);
        assert_eq!(request.mode, DoctorExecutionMode::CleanupDryRun);
        assert_eq!(request.mode.stable_name(), "cleanup-dry-run");
        assert!(!request.mode.permits_mutation());
        assert!(!request.mode.requires_plan_fingerprint());
    }

    #[test]
    fn cleanup_apply_requires_yes_and_plan_fingerprint() {
        let request = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
            false,
            true,
            Some("cleanup-v1-abc".to_string()),
            false,
            false,
            false,
        )
        .expect("fingerprint-approved cleanup should map");

        assert_eq!(request.surface, DoctorCommandSurface::Cleanup);
        assert_eq!(request.mode, DoctorExecutionMode::CleanupApply);
        assert_eq!(request.mode.stable_name(), "cleanup-apply");
        assert!(request.mode.permits_mutation());
        assert!(request.mode.requires_plan_fingerprint());
    }

    #[test]
    fn repair_rejects_missing_mode_or_mismatched_approval_flags() {
        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            true,
            false,
            false,
            false,
            None,
            false,
            false,
            false,
        )
        .expect_err("repair must require dry-run or fingerprint apply");
        assert!(err.message.contains("requires"));

        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            true,
            false,
            true,
            true,
            Some("fp".to_string()),
            false,
            false,
            false,
        )
        .expect_err("dry-run and yes are mutually exclusive");
        assert!(err.message.contains("--dry-run"));

        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            true,
            false,
            false,
            true,
            None,
            false,
            false,
            false,
        )
        .expect_err("yes must require fingerprint");
        assert!(err.message.contains("--plan-fingerprint"));
    }

    #[test]
    fn cleanup_rejects_missing_or_mismatched_approval_flags() {
        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
            true,
            true,
            Some("fp".to_string()),
            false,
            false,
            false,
        )
        .expect_err("cleanup dry-run and yes are mutually exclusive");
        assert!(err.message.contains("--dry-run"));

        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
            false,
            true,
            None,
            false,
            false,
            false,
        )
        .expect_err("cleanup yes must require fingerprint");
        assert!(err.message.contains("--plan-fingerprint"));

        let err = DoctorCommandRequest::from_cli_flags(
            None,
            None,
            Some(RobotFormat::Json),
            false,
            false,
            false,
            true,
            false,
            false,
            Some("fp".to_string()),
            false,
            false,
            false,
        )
        .expect_err("cleanup fingerprint must require yes");
        assert!(err.message.contains("--yes"));
    }

    #[test]
    fn doctor_execution_mode_names_are_stable_for_robot_contracts() {
        let names = [
            DoctorExecutionMode::ReadOnlyCheck.stable_name(),
            DoctorExecutionMode::RepairDryRun.stable_name(),
            DoctorExecutionMode::FingerprintApply.stable_name(),
            DoctorExecutionMode::CleanupDryRun.stable_name(),
            DoctorExecutionMode::CleanupApply.stable_name(),
            DoctorExecutionMode::SafeAutoFix.stable_name(),
        ];

        assert_eq!(
            names,
            [
                "read-only-check",
                "repair-dry-run",
                "fingerprint-apply",
                "cleanup-dry-run",
                "cleanup-apply",
                "safe-auto-fix",
            ]
        );
    }

    #[test]
    fn doctor_surface_names_are_stable_for_robot_contracts() {
        let names = [
            DoctorCommandSurface::LegacyDoctor.stable_name(),
            DoctorCommandSurface::Check.stable_name(),
            DoctorCommandSurface::Repair.stable_name(),
            DoctorCommandSurface::Cleanup.stable_name(),
            DoctorCommandSurface::Reconstruct.stable_name(),
            DoctorCommandSurface::Restore.stable_name(),
            DoctorCommandSurface::BaselineDiff.stable_name(),
            DoctorCommandSurface::SupportBundle.stable_name(),
        ];

        assert_eq!(
            names,
            [
                "legacy-doctor",
                "check",
                "repair",
                "cleanup",
                "reconstruct",
                "restore",
                "baseline-diff",
                "support-bundle",
            ]
        );
    }

    #[test]
    fn legacy_cli_dispatch_routes_through_typed_doctor_module() {
        let lib_source = include_str!("lib.rs");
        assert!(
            lib_source.contains("doctor::DoctorCommandRequest::from_cli_flags"),
            "Commands::Doctor should build the typed doctor request before execution"
        );
        assert!(
            lib_source.contains("doctor::execute_doctor_command(request)?"),
            "Commands::Doctor should execute through the doctor module boundary"
        );
        assert!(
            !lib_source.contains("fn run_doctor("),
            "legacy run_doctor entrypoint should not remain as a bypassable implementation name"
        );
        assert_eq!(
            lib_source.matches("pub(crate) fn run_doctor_impl(").count(),
            1,
            "there should be exactly one internal doctor implementation body"
        );

        let doctor_source = include_str!("doctor.rs");
        let executor_call = ["crate::", "run_doctor_impl("].concat();
        assert_eq!(
            doctor_source.matches(&executor_call).count(),
            1,
            "the doctor module should be the single call site for the internal executor"
        );
    }
}
