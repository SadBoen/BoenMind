//! `extensions` module shim for bm-compat (B1).
//!
//! Minimal verbatim extraction from `legacy/pi_agent_rust/src/extensions.rs`
//! and `legacy/pi_agent_rust/src/extensions/exec_mediation.rs`, providing
//! exactly the symbols referenced by the vendored `extensions_js.rs` /
//! `error.rs`. Each code block is a byte-identical line-range extract (see the
//! per-block "extracted from" headers); the only additions are the imports
//! below and the `mod exec_mediation` wrapper. Serde shapes are kept verbatim
//! — they are the serialization contract for frontend permission tiers.
//!
//! Security-critical symbols (exec mediation, secret broker, path
//! canonicalization) are full verbatim copies of the upstream logic — no
//! behavioral stubs.

use serde::{Deserialize, Serialize};
use serde_json::Value;
// Mirror of upstream extensions.rs:42 `use sha2::Digest as _;` — required by
// the verbatim hash functions below (`hasher.update(...)`).
use sha2::Digest as _;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Classification of dangerous command patterns for exec mediation.
///
/// Each variant represents a class of commands that pose a security risk when
/// executed by an extension. The classifier is deterministic: given the same
/// command string, the same classification is always returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DangerousCommandClass {
    /// Recursive deletion targeting root or broad paths (`rm -rf /`).
    RecursiveDelete,
    /// Device-level writes (`dd`, `mkfs`, `fdisk`).
    DeviceWrite,
    /// Fork bomb or process exhaustion patterns.
    ForkBomb,
    /// Pipe to shell execution (`curl | sh`, `wget | bash`).
    PipeToShell,
    /// System shutdown or reboot commands.
    SystemShutdown,
    /// Broad permission changes (`chmod 777`, `chmod -R 777`).
    PermissionEscalation,
    /// Killing critical system processes (`kill -9 1`, `pkill init`).
    ProcessTermination,
    /// Modifying /etc/passwd, /etc/shadow, or sudoers.
    CredentialFileModification,
    /// Disk wipe or overwrite patterns (`shred`, `wipefs`).
    DiskWipe,
    /// Network exfiltration via raw sockets or reverse shells.
    ReverseShell,
}

/// Risk tier for exec command classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecRiskTier {
    /// Low risk — normal commands.
    Low,
    /// Medium risk — commands that could be misused.
    Medium,
    /// High risk — commands with significant destructive potential.
    High,
    /// Critical risk — commands that could cause irreversible damage.
    Critical,
}

/// Policy configuration for exec mediation (SEC-4.3).
///
/// Controls which commands are allowed/denied based on pattern matching
/// and dangerous command classification. Evaluated after capability-level
/// policy and quota checks but before the actual command is spawned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExecMediationPolicy {
    /// When true, exec mediation is active and commands are classified.
    pub enabled: bool,
    /// Minimum risk tier that triggers a deny (default: Critical).
    /// Commands at or above this tier are blocked.
    pub deny_threshold: ExecRiskTier,
    /// Explicit command prefixes to deny (case-insensitive prefix match).
    /// These are checked before the built-in classifier.
    #[serde(default)]
    pub deny_patterns: Vec<String>,
    /// Explicit command prefixes to allow even if classified as dangerous.
    /// Use sparingly — allows overriding the classifier for specific tools.
    #[serde(default)]
    pub allow_patterns: Vec<String>,
    /// When true, commands classified as dangerous are logged even if allowed.
    pub audit_all_classified: bool,
}

/// Result of exec mediation evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecMediationResult {
    /// Command is allowed to proceed.
    Allow,
    /// Command is allowed but was classified as potentially dangerous.
    AllowWithAudit {
        class: DangerousCommandClass,
        reason: String,
    },
    /// Command is denied.
    Deny {
        class: Option<DangerousCommandClass>,
        reason: String,
    },
}

/// Patterns used to identify environment variables likely to contain secrets.
///
/// The broker uses suffix and prefix matching to catch common naming
/// conventions for API keys, tokens, passwords, and credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SecretBrokerPolicy {
    /// When true, the secret broker is active.
    pub enabled: bool,
    /// Env var name suffixes that indicate a secret (case-insensitive).
    pub secret_suffixes: Vec<String>,
    /// Env var name prefixes that indicate a secret (case-insensitive).
    pub secret_prefixes: Vec<String>,
    /// Exact env var names that are always treated as secrets (case-insensitive).
    pub secret_exact: Vec<String>,
    /// Env vars on this list are never redacted, even if they match a pattern.
    pub disclosure_allowlist: Vec<String>,
    /// The replacement string used in place of secret values.
    pub redaction_placeholder: String,
}
// ---------------------------------------------------------------------------
// extracted from legacy/pi_agent_rust/src/extensions/exec_mediation.rs
// (whole file, inlined as `mod exec_mediation` so the upstream
// `use super::{...}` and `pub(super)` code needs no edits)
// ---------------------------------------------------------------------------
mod exec_mediation {
//! Dangerous-command mediation and secret-broker policy.

use super::{DangerousCommandClass, ExecMediationPolicy, ExecRiskTier, SecretBrokerPolicy};

// ---------------------------------------------------------------------------
// Exec mediation and secret broker (SEC-4.3 / bd-zh0hj)
// ---------------------------------------------------------------------------

impl DangerousCommandClass {
    /// Human-readable label for incident logging.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RecursiveDelete => "recursive_delete",
            Self::DeviceWrite => "device_write",
            Self::ForkBomb => "fork_bomb",
            Self::PipeToShell => "pipe_to_shell",
            Self::SystemShutdown => "system_shutdown",
            Self::PermissionEscalation => "permission_escalation",
            Self::ProcessTermination => "process_termination",
            Self::CredentialFileModification => "credential_file_modification",
            Self::DiskWipe => "disk_wipe",
            Self::ReverseShell => "reverse_shell",
        }
    }

    /// Risk tier for this command class (used for policy decisions).
    #[must_use]
    pub const fn risk_tier(self) -> ExecRiskTier {
        match self {
            Self::RecursiveDelete
            | Self::DeviceWrite
            | Self::ForkBomb
            | Self::DiskWipe
            | Self::ReverseShell => ExecRiskTier::Critical,
            Self::PipeToShell
            | Self::SystemShutdown
            | Self::PermissionEscalation
            | Self::ProcessTermination
            | Self::CredentialFileModification => ExecRiskTier::High,
        }
    }
}

impl Default for ExecMediationPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            deny_threshold: ExecRiskTier::Critical,
            deny_patterns: Vec::new(),
            allow_patterns: Vec::new(),
            audit_all_classified: true,
        }
    }
}

impl ExecMediationPolicy {
    /// Strict preset: blocks High and above.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            enabled: true,
            deny_threshold: ExecRiskTier::High,
            deny_patterns: Vec::new(),
            allow_patterns: Vec::new(),
            audit_all_classified: true,
        }
    }

    /// Permissive preset: only blocks Critical.
    #[must_use]
    pub const fn permissive() -> Self {
        Self {
            enabled: true,
            deny_threshold: ExecRiskTier::Critical,
            deny_patterns: Vec::new(),
            allow_patterns: Vec::new(),
            audit_all_classified: false,
        }
    }

    /// Disabled preset: no exec mediation.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            deny_threshold: ExecRiskTier::Critical,
            deny_patterns: Vec::new(),
            allow_patterns: Vec::new(),
            audit_all_classified: false,
        }
    }
}

pub(super) fn normalize_command_for_classification(command: &str) -> String {
    let mut normalized = String::with_capacity(command.len());
    let mut previous_was_space = false;
    let mut remaining = command;

    while !remaining.is_empty() {
        // Normalize common shell-obfuscated spacing forms that still evaluate
        // to whitespace at runtime.
        if let Some(rest) = remaining.strip_prefix("${ifs}") {
            if !previous_was_space {
                normalized.push(' ');
                previous_was_space = true;
            }
            remaining = rest;
            continue;
        }
        if let Some(rest) = remaining.strip_prefix("$ifs") {
            if !previous_was_space {
                normalized.push(' ');
                previous_was_space = true;
            }
            remaining = rest;
            continue;
        }

        let mut chars = remaining.chars();
        let Some(mut ch) = chars.next() else {
            break;
        };

        // Strip quotes to prevent obfuscation like `r"m" -rf /`
        if ch == '\'' || ch == '"' {
            remaining = chars.as_str();
            continue;
        }

        if ch == '\\' {
            let mut peek_chars = chars.clone();
            if let Some(next) = peek_chars.next() {
                if next == '\n' || next == '\r' {
                    remaining = peek_chars.as_str();
                    continue;
                }

                chars.next(); // consume the escaped character

                if next.is_ascii_whitespace() {
                    if !previous_was_space {
                        normalized.push(' ');
                        previous_was_space = true;
                    }
                    remaining = chars.as_str();
                    continue;
                }

                // Strip escaped quotes as well
                if next == '\'' || next == '"' {
                    remaining = chars.as_str();
                    continue;
                }

                ch = next;
            }
        }

        if ch.is_ascii_whitespace() {
            if !previous_was_space {
                normalized.push(' ');
                previous_was_space = true;
            }
        } else {
            normalized.push(ch);
            previous_was_space = false;
        }
        remaining = chars.as_str();
    }

    normalized
}

pub(super) fn classify_recursive_delete(lower: &str) -> bool {
    // rm -rf / or rm -rf /* or rm -rf ~
    if !lower.contains("rm") {
        return false;
    }
    // Detect recursive+force in any combination: -rf, -fr, --recursive,
    // or separate flags like -r -f / -f -r.
    let has_rf = lower.contains("-rf")
        || lower.contains("-fr")
        || lower.contains("--recursive")
        || (lower.contains("-r") && lower.contains("-f"));
    if !has_rf {
        return false;
    }
    // Target root, home, or wildcard
    let dangerous_targets = [" /", " /*", " /.", " ~/", " ~/*", " --no-preserve-root"];
    dangerous_targets.iter().any(|t| lower.contains(t))
}

pub(super) fn classify_device_write(lower: &str) -> bool {
    // dd writing to devices, mkfs, fdisk
    let dd_to_dev = lower.contains("dd ") && lower.contains("of=/dev/");
    let mkfs = lower.starts_with("mkfs") || lower.contains(" mkfs") || lower.contains(";mkfs");
    let fdisk = lower.starts_with("fdisk") || lower.contains(" fdisk") || lower.contains(";fdisk");
    dd_to_dev || mkfs || fdisk
}

pub(super) fn classify_fork_bomb(lower: &str) -> bool {
    // Classic bash fork bomb: :(){ :|:& };:
    // Also: while true; do ... & done
    lower.contains(":(){ :|:&")
        || lower.contains(":(){ :|: &")
        || (lower.contains("while true") && lower.contains("& done"))
        || (lower.contains("fork") && lower.contains("while") && lower.contains('&'))
}

pub(super) fn classify_disk_wipe(lower: &str) -> bool {
    let shred = lower.starts_with("shred") || lower.contains(" shred ") || lower.contains(";shred");
    let wipefs =
        lower.starts_with("wipefs") || lower.contains(" wipefs") || lower.contains(";wipefs");
    let dd_zero = lower.contains("dd ") && lower.contains("if=/dev/zero");
    let dd_urandom = lower.contains("dd ") && lower.contains("if=/dev/urandom");
    shred || wipefs || dd_zero || dd_urandom
}

pub(super) fn classify_reverse_shell(lower: &str) -> bool {
    // Common reverse shell patterns
    let bash_rev = lower.contains("/dev/tcp/") && lower.contains("bash");
    let nc_rev = (lower.contains("nc ") || lower.contains("ncat ") || lower.contains("netcat "))
        && lower.contains("-e ");
    let python_rev = lower.contains("socket") && lower.contains("connect") && lower.contains("sh");
    bash_rev || nc_rev || python_rev
}

pub(super) fn classify_pipe_to_shell(lower: &str) -> bool {
    // curl/wget piped to sh/bash. Cover bare `sh`/`bash` plus the absolute
    // paths the shell may live at on the target system: `/bin/{sh,bash}`
    // (Linux base layout), `/usr/bin/{sh,bash}` (/usr-merge distros — Arch,
    // Fedora, modern Debian/Ubuntu — where /bin is a symlink to /usr/bin),
    // and `/usr/local/bin/{sh,bash}` (FreeBSD `pkg install bash`, custom
    // installs). Both spaced and unspaced pipe forms.
    const PIPE_SHELL_PATTERNS: &[&str] = &[
        "| sh",
        "| bash",
        "|sh",
        "|bash",
        "| /bin/sh",
        "| /bin/bash",
        "|/bin/sh",
        "|/bin/bash",
        "| /usr/bin/sh",
        "| /usr/bin/bash",
        "|/usr/bin/sh",
        "|/usr/bin/bash",
        "| /usr/local/bin/sh",
        "| /usr/local/bin/bash",
        "|/usr/local/bin/sh",
        "|/usr/local/bin/bash",
    ];
    let has_download = lower.contains("curl ") || lower.contains("wget ");
    let has_pipe_to_shell = PIPE_SHELL_PATTERNS.iter().any(|p| lower.contains(p));
    let download_exec_patterns = [
        "eval \"$(curl ",
        "eval \"$(wget ",
        "eval '$(curl ",
        "eval '$(wget ",
        "eval $(curl ",
        "eval $(wget ",
        "source <(curl ",
        "source <(wget ",
        "bash -c \"$(curl ",
        "bash -c \"$(wget ",
        "bash -c '$(curl ",
        "bash -c '$(wget ",
        "sh -c \"$(curl ",
        "sh -c \"$(wget ",
        "sh -c '$(curl ",
        "sh -c '$(wget ",
    ];
    (has_download && has_pipe_to_shell)
        || download_exec_patterns
            .iter()
            .any(|pattern| lower.contains(pattern))
}

pub(super) fn classify_system_shutdown(lower: &str) -> bool {
    lower.starts_with("shutdown")
        || lower.contains(" shutdown")
        || lower.contains(";shutdown")
        || lower.starts_with("reboot")
        || lower.contains(" reboot")
        || lower.contains(";reboot")
        || lower.starts_with("halt")
        || lower.contains(" halt")
        || lower.contains(";halt")
        || lower.starts_with("poweroff")
        || lower.contains(" poweroff")
        || lower.contains(";poweroff")
        || lower.starts_with("init 0")
        || lower.contains(" init 0")
        || lower.starts_with("init 6")
        || lower.contains(" init 6")
}

pub(super) fn classify_permission_escalation(lower: &str) -> bool {
    // chmod 777, chmod -R 777, chown root
    let chmod_broad = lower.contains("chmod")
        && (lower.contains("777") || lower.contains("a+rwx") || lower.contains("o+w"));
    let chmod_suid = lower.contains("chmod") && (lower.contains("+s") || lower.contains("4755"));
    chmod_broad || chmod_suid
}

pub(super) fn classify_process_termination(lower: &str) -> bool {
    // kill -9 1, pkill init, killall
    let kill_pid1 = lower.contains("kill") && (lower.contains(" 1 ") || lower.ends_with(" 1"));
    let kill_9 = lower.contains("kill -9") || lower.contains("kill -kill");
    let pkill_critical = lower.contains("pkill")
        && (lower.contains("init") || lower.contains("systemd") || lower.contains("sshd"));
    let killall = lower.starts_with("killall") || lower.contains(" killall");
    // Only flag kill of PID 1 or critical processes
    (kill_pid1 && kill_9) || pkill_critical || killall
}

pub(super) fn classify_credential_file_modification(lower: &str) -> bool {
    let cred_files = [
        "/etc/passwd",
        "/etc/shadow",
        "/etc/sudoers",
        "/etc/ssh/sshd_config",
    ];
    let write_cmds = ["tee ", "cat >", "echo >", "sed -i", "cp ", "mv "];
    cred_files
        .iter()
        .any(|f| lower.contains(f) && write_cmds.iter().any(|w| lower.contains(w)))
}

impl ExecRiskTier {
    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

// ---------------------------------------------------------------------------
// Secret broker (SEC-4.3 / bd-zh0hj)
// ---------------------------------------------------------------------------

impl Default for SecretBrokerPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            secret_suffixes: vec![
                "_KEY".to_string(),
                "_SECRET".to_string(),
                "_TOKEN".to_string(),
                "_PASSWORD".to_string(),
                "_PASSWD".to_string(),
                "_CREDENTIAL".to_string(),
                "_CREDENTIALS".to_string(),
                "_AUTH".to_string(),
                "_API_KEY".to_string(),
                "_PRIVATE_KEY".to_string(),
            ],
            secret_prefixes: vec![
                "SECRET_".to_string(),
                "AUTH_".to_string(),
                "CREDENTIAL_".to_string(),
            ],
            secret_exact: vec![
                "ANTHROPIC_API_KEY".to_string(),
                "OPENAI_API_KEY".to_string(),
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "AWS_SESSION_TOKEN".to_string(),
                "GITHUB_TOKEN".to_string(),
                "GOOGLE_API_KEY".to_string(),
                "AZURE_CLIENT_SECRET".to_string(),
                "DATABASE_URL".to_string(),
                "REDIS_URL".to_string(),
                "PRIVATE_KEY".to_string(),
                "NPM_TOKEN".to_string(),
                "DOCKER_PASSWORD".to_string(),
                "SLACK_TOKEN".to_string(),
                "STRIPE_SECRET_KEY".to_string(),
                "TWILIO_AUTH_TOKEN".to_string(),
                "SENDGRID_API_KEY".to_string(),
            ],
            disclosure_allowlist: Vec::new(),
            redaction_placeholder: "[REDACTED]".to_string(),
        }
    }
}

impl SecretBrokerPolicy {
    /// Returns `true` if the given env var name matches a known secret pattern.
    #[must_use]
    pub fn is_secret(&self, name: &str) -> bool {
        if !self.enabled {
            return false;
        }

        let upper = name.to_ascii_uppercase();

        // Check disclosure allowlist first (overrides everything).
        if self
            .disclosure_allowlist
            .iter()
            .any(|a| a.eq_ignore_ascii_case(name))
        {
            return false;
        }

        // Exact match.
        if self
            .secret_exact
            .iter()
            .any(|e| e.eq_ignore_ascii_case(name))
        {
            return true;
        }

        // Suffix match.
        if self
            .secret_suffixes
            .iter()
            .any(|s| upper.ends_with(&s.to_ascii_uppercase()))
        {
            return true;
        }

        // Prefix match.
        self.secret_prefixes
            .iter()
            .any(|p| upper.starts_with(&p.to_ascii_uppercase()))
    }

    /// Redact a value if the env var name is a secret.
    ///
    /// Returns the original value if not a secret, or the redaction
    /// placeholder if it is.
    #[must_use]
    pub fn maybe_redact<'a>(&'a self, name: &str, value: &'a str) -> &'a str {
        if self.is_secret(name) {
            &self.redaction_placeholder
        } else {
            value
        }
    }
}
}

use exec_mediation::{
    classify_credential_file_modification, classify_device_write, classify_disk_wipe,
    classify_fork_bomb, classify_permission_escalation, classify_pipe_to_shell,
    classify_process_termination, classify_recursive_delete, classify_reverse_shell,
    classify_system_shutdown, normalize_command_for_classification,
};
// extracted from legacy/pi_agent_rust/src/extensions.rs:901-906
// (upstream import of the inlined exec_mediation functions)
///
/// Returns all matching classifications. A command may match multiple
/// classes (e.g., a reverse shell that also pipes to shell).
/// The classifier is deterministic and case-insensitive.
#[must_use]
pub fn classify_dangerous_command(cmd: &str, args: &[String]) -> Vec<DangerousCommandClass> {
    let mut classes = Vec::new();
    let full_cmd = if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {}", args.join(" "))
    };
    let lower = normalize_command_for_classification(&full_cmd.to_ascii_lowercase());

    // --- Critical tier ---

    // Recursive delete targeting root or broad paths.
    if classify_recursive_delete(&lower) {
        classes.push(DangerousCommandClass::RecursiveDelete);
    }

    // Device-level writes.
    if classify_device_write(&lower) {
        classes.push(DangerousCommandClass::DeviceWrite);
    }

    // Fork bomb patterns.
    if classify_fork_bomb(&lower) {
        classes.push(DangerousCommandClass::ForkBomb);
    }

    // Disk wipe.
    if classify_disk_wipe(&lower) {
        classes.push(DangerousCommandClass::DiskWipe);
    }

    // Reverse shell.
    if classify_reverse_shell(&lower) {
        classes.push(DangerousCommandClass::ReverseShell);
    }

    // --- High tier ---

    // Pipe to shell.
    if classify_pipe_to_shell(&lower) {
        classes.push(DangerousCommandClass::PipeToShell);
    }

    // System shutdown.
    if classify_system_shutdown(&lower) {
        classes.push(DangerousCommandClass::SystemShutdown);
    }

    // Permission escalation.
    if classify_permission_escalation(&lower) {
        classes.push(DangerousCommandClass::PermissionEscalation);
    }

    // Process termination of critical processes.
    if classify_process_termination(&lower) {
        classes.push(DangerousCommandClass::ProcessTermination);
    }

    // Credential file modification.
    if classify_credential_file_modification(&lower) {
        classes.push(DangerousCommandClass::CredentialFileModification);
    }

    classes
}

/// Evaluate exec mediation policy for a command.
///
/// Called after capability-level policy allows exec, but before spawning.
/// Returns [`ExecMediationResult`] indicating whether the command should
/// proceed, be audited, or be denied.
#[must_use]
pub fn evaluate_exec_mediation(
    policy: &ExecMediationPolicy,
    cmd: &str,
    args: &[String],
) -> ExecMediationResult {
    if !policy.enabled {
        return ExecMediationResult::Allow;
    }

    let full_cmd = if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {}", args.join(" "))
    };
    let lower = full_cmd.to_ascii_lowercase();

    // 1. Check explicit allow patterns (highest precedence override).
    for pattern in &policy.allow_patterns {
        if lower.starts_with(&pattern.to_ascii_lowercase()) {
            return ExecMediationResult::Allow;
        }
    }

    // 2. Check explicit deny patterns.
    for pattern in &policy.deny_patterns {
        if lower.starts_with(&pattern.to_ascii_lowercase()) {
            return ExecMediationResult::Deny {
                class: None,
                reason: format!("Command matches deny pattern: {pattern}"),
            };
        }
    }

    // 3. Classify via built-in rules.
    let classes = classify_dangerous_command(cmd, args);
    if classes.is_empty() {
        return ExecMediationResult::Allow;
    }

    // Find the highest-risk classification.
    let worst = classes
        .iter()
        .max_by_key(|c| c.risk_tier())
        .copied()
        .expect("classes is non-empty");

    if worst.risk_tier() >= policy.deny_threshold {
        ExecMediationResult::Deny {
            class: Some(worst),
            reason: format!(
                "Command classified as {} ({})",
                worst.label(),
                worst.risk_tier().label()
            ),
        }
    } else if policy.audit_all_classified {
        ExecMediationResult::AllowWithAudit {
            class: worst,
            reason: format!(
                "Command classified as {} ({}) — below deny threshold",
                worst.label(),
                worst.risk_tier().label()
            ),
        }
    } else {
        ExecMediationResult::Allow
    }
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:1364-1472
// (safe_canonicalize, normalize_dot_segments, strip_unc_prefix)
/// Canonicalize a path, stripping the `\\?\` verbatim prefix on Windows.
///
/// `std::fs::canonicalize` on Windows returns extended-length paths (`\\?\C:\...`)
/// which break QuickJS module resolution and JS string interpolation. This helper
/// strips that prefix so paths remain compatible with downstream consumers.
///
/// If `canonicalize` fails (e.g. path does not exist), this falls back to logical
/// normalization (`normalize_dot_segments`) of the absolute path to prevent
/// directory traversal exploits in security checks.
pub fn safe_canonicalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map_or_else(
        |_| {
            // Fallback for non-existent paths:
            // 1. Resolve to an absolute logical path.
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            };

            // 2. Try to anchor on the longest existing ancestor to respect symlinks.
            //    If we are in `/link/new_file` and `/link` -> `/target`, we want
            //    to resolve to `/target/new_file` to match the root resolution.
            for ancestor in absolute.ancestors().skip(1) {
                if let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor)
                    && let Ok(suffix) = absolute.strip_prefix(ancestor)
                {
                    let combined = canonical_ancestor.join(suffix);
                    // Normalize handles any `..` in the suffix.
                    return strip_unc_prefix(normalize_dot_segments(&combined));
                }
            }

            // 3. Last resort: purely logical normalization.
            strip_unc_prefix(normalize_dot_segments(&absolute))
        },
        strip_unc_prefix,
    )
}

fn normalize_dot_segments(path: &Path) -> PathBuf {
    use std::ffi::{OsStr, OsString};
    use std::path::Component;

    let mut out = PathBuf::new();
    let mut normals: Vec<OsString> = Vec::new();
    let mut has_prefix = false;
    let mut has_root = false;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                out.push(prefix.as_os_str());
                has_prefix = true;
            }
            Component::RootDir => {
                out.push(component.as_os_str());
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => match normals.last() {
                Some(last) if last.as_os_str() != OsStr::new("..") => {
                    normals.pop();
                }
                _ => {
                    if !has_root && !has_prefix {
                        normals.push(OsString::from(".."));
                    }
                }
            },
            Component::Normal(part) => normals.push(part.to_os_string()),
        }
    }

    for part in normals {
        out.push(part);
    }

    out
}

/// Strip the `\\?\` or `//?/` verbatim prefix from a path on Windows. No-op on Unix.
#[allow(clippy::missing_const_for_fn)]
pub fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            if let Some(unc) = stripped.strip_prefix("UNC") {
                if unc.starts_with('\\') {
                    return PathBuf::from(format!(r"\{}", unc));
                }
            }
            return PathBuf::from(stripped);
        }
        // fd normalises separators to `/`, producing `//?/` instead of `\\?\`.
        if let Some(stripped) = s.strip_prefix("//?/") {
            if let Some(unc) = stripped.strip_prefix("UNC") {
                if unc.starts_with('/') {
                    return PathBuf::from(format!("/{}", unc));
                }
            }
            return PathBuf::from(stripped);
        }
    }
    path
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:1539-1625
// (hash_canonical_json, hash_canonical_json_depth, hash_json_escaped_str,
//  sha256_to_hex, hostcall_params_hash)
/// Feed canonical JSON with sorted object keys directly into a SHA-256 hasher,
/// bypassing the intermediate `String` buffer entirely.
pub(crate) fn hash_canonical_json(value: &Value, hasher: &mut sha2::Sha256) {
    hash_canonical_json_depth(value, hasher, 0);
}

fn hash_canonical_json_depth(value: &Value, hasher: &mut sha2::Sha256, depth: usize) {
    if depth > 128 {
        hasher.update(b"too_deep");
        return;
    }

    match value {
        Value::Null => hasher.update(b"null"),
        Value::Bool(b) => hasher.update(if *b { &b"true"[..] } else { &b"false"[..] }),
        Value::Number(n) => {
            // Numbers are short — write to a small stack buffer.
            let mut buf = String::with_capacity(24);
            let _ = write!(buf, "{n}");
            hasher.update(buf.as_bytes());
        }
        Value::String(s) => {
            hash_json_escaped_str(s, hasher);
        }
        Value::Array(items) => {
            hasher.update(b"[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    hasher.update(b",");
                }
                hash_canonical_json_depth(item, hasher, depth + 1);
            }
            hasher.update(b"]");
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            hasher.update(b"{");
            let mut first = true;
            for key in keys {
                if let Some(v) = map.get(key) {
                    if !first {
                        hasher.update(b",");
                    }
                    first = false;
                    hash_json_escaped_str(key, hasher);
                    hasher.update(b":");
                    hash_canonical_json_depth(v, hasher, depth + 1);
                }
            }
            hasher.update(b"}");
        }
    }
}

/// Feed a JSON-escaped string (with quotes) directly into a SHA-256 hasher.
pub(crate) fn hash_json_escaped_str(s: &str, hasher: &mut sha2::Sha256) {
    use sha2::Digest as _;
    if s.bytes().all(|b| b >= 0x20 && b != b'"' && b != b'\\') {
        hasher.update(b"\"");
        hasher.update(s.as_bytes());
        hasher.update(b"\"");
    } else {
        let escaped = serde_json::to_string(s).expect("string serialization");
        hasher.update(escaped.as_bytes());
    }
}

/// Convert a SHA-256 digest to a lowercase hex string using a lookup table.
pub(crate) fn sha256_to_hex(digest: &[u8]) -> String {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for &b in digest {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    out
}

pub(crate) fn hostcall_params_hash(method: &str, params: &Value) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hash_hostcall_envelope(method, br#","params":"#, &mut hasher, |h| {
        hash_canonical_json(params, h);
    });
    sha256_to_hex(hasher.finalize().as_slice())
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:1685-1699
// (hash_hostcall_envelope)
/// Hash the canonical `{"method": ..., "<payload_key>": ...}` envelope using
/// the exact byte layout expected by historical hostcall hash artifacts.
fn hash_hostcall_envelope(
    method: &str,
    payload_key_prefix: &[u8],
    hasher: &mut sha2::Sha256,
    payload_writer: impl FnOnce(&mut sha2::Sha256),
) {
    use sha2::Digest as _;
    hasher.update(br#"{"method":"#);
    hash_json_escaped_str(method, hasher);
    hasher.update(payload_key_prefix);
    payload_writer(hasher);
    hasher.update(b"}");
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:1941-1992
// (PolicyProfile + to_policy)
// ---------------------------------------------------------------------------
// Policy profile presets
// ---------------------------------------------------------------------------

/// Named policy profiles providing curated defaults.
///
/// Profiles are convenience constructors for [`ExtensionPolicy`] — once
/// constructed the policy is fully mutable and can be further customised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyProfile {
    /// Safe defaults: only non-dangerous capabilities allowed, dangerous
    /// denied. Mode = Strict.
    Safe,
    /// Standard defaults (current production behaviour): non-dangerous
    /// allowed, dangerous prompt. Mode = Prompt.
    Standard,
    /// Everything allowed, nothing denied. Mode = Permissive.
    Permissive,
}

impl PolicyProfile {
    /// Expand this profile into a concrete [`ExtensionPolicy`].
    pub fn to_policy(self) -> ExtensionPolicy {
        match self {
            Self::Safe => ExtensionPolicy {
                mode: ExtensionPolicyMode::Strict,
                max_memory_mb: 256,
                default_caps: vec![
                    "read".to_string(),
                    "write".to_string(),
                    "http".to_string(),
                    "events".to_string(),
                    "session".to_string(),
                ],
                deny_caps: vec!["exec".to_string(), "env".to_string()],
                per_extension: HashMap::new(),
                exec_mediation: ExecMediationPolicy::strict(),
                secret_broker: SecretBrokerPolicy::default(),
            },
            Self::Standard => ExtensionPolicy::default(),
            Self::Permissive => ExtensionPolicy {
                mode: ExtensionPolicyMode::Permissive,
                max_memory_mb: 256,
                default_caps: Vec::new(),
                deny_caps: Vec::new(),
                per_extension: HashMap::new(),
                exec_mediation: ExecMediationPolicy::permissive(),
                secret_broker: SecretBrokerPolicy::default(),
            },
        }
    }
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:1994-2021
// (ExtensionOverride)

// ---------------------------------------------------------------------------
// Per-extension overrides
// ---------------------------------------------------------------------------

/// Per-extension policy override.
///
/// When present for an extension ID, these fields take precedence over the
/// global policy fields at the corresponding layer in the precedence chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionOverride {
    /// Mode override for this extension. `None` inherits the global mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ExtensionPolicyMode>,
    /// Additional capabilities to allow for this extension (merged with
    /// global `default_caps`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// Additional capabilities to deny for this extension (merged with
    /// global `deny_caps`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    /// Per-extension resource quota overrides (SEC-4.1).
    /// `None` inherits the global quota defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<ExtensionQuotaConfig>,
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:2022-2033
// (ExtensionPolicyMode)

// ---------------------------------------------------------------------------
// Core policy types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionPolicyMode {
    Strict,
    Prompt,
    Permissive,
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:2043-2081
// (ExtensionPolicy + Default impl)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionPolicy {
    pub mode: ExtensionPolicyMode,
    pub max_memory_mb: u32,
    pub default_caps: Vec<String>,
    pub deny_caps: Vec<String>,
    /// Per-extension overrides keyed by extension ID.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub per_extension: HashMap<String, ExtensionOverride>,
    /// Exec mediation policy (SEC-4.3). Controls command-level allow/deny
    /// after capability-level exec is granted.
    #[serde(default)]
    pub exec_mediation: ExecMediationPolicy,
    /// Secret broker policy (SEC-4.3). Controls redaction of secret env vars
    /// and prevents raw disclosure when policy forbids it.
    #[serde(default)]
    pub secret_broker: SecretBrokerPolicy,
}

impl Default for ExtensionPolicy {
    fn default() -> Self {
        Self {
            mode: ExtensionPolicyMode::Prompt,
            max_memory_mb: 256,
            default_caps: vec![
                "read".to_string(),
                "write".to_string(),
                "http".to_string(),
                "events".to_string(),
                "session".to_string(),
            ],
            deny_caps: vec!["exec".to_string(), "env".to_string()],
            per_extension: HashMap::new(),
            exec_mediation: ExecMediationPolicy::default(),
            secret_broker: SecretBrokerPolicy::default(),
        }
    }
}
// extracted from legacy/pi_agent_rust/src/extensions.rs:2384-2447
// (ExtensionQuotaConfig + Default + for_mode)
// Per-extension resource quota engine (SEC-4.1 / bd-b1d7o)
// ---------------------------------------------------------------------------

/// Configurable per-extension resource quotas. When a quota is `None`, the
/// corresponding limit is not enforced. All values are per-extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionQuotaConfig {
    /// Maximum hostcalls permitted per 1-second sliding window.
    pub max_hostcalls_per_second: Option<u32>,
    /// Maximum hostcalls permitted per 60-second sliding window.
    pub max_hostcalls_per_minute: Option<u32>,
    /// Maximum total hostcalls before the extension is throttled.
    pub max_hostcalls_total: Option<u64>,
    /// Maximum concurrent subprocesses spawned via exec hostcalls.
    pub max_subprocesses: Option<u32>,
    /// Maximum cumulative bytes written via fs/write hostcalls.
    pub max_write_bytes: Option<u64>,
    /// Maximum cumulative HTTP requests issued via http hostcalls.
    pub max_http_requests: Option<u64>,
}

impl Default for ExtensionQuotaConfig {
    fn default() -> Self {
        Self::for_mode(ExtensionPolicyMode::Prompt)
    }
}

impl ExtensionQuotaConfig {
    /// Create quota defaults appropriate for a given policy mode.
    ///
    /// - **Strict**: restrictive burst/rate limits and low subprocess fan-out.
    /// - **Prompt**: moderate defaults (original baseline).
    /// - **Permissive**: relaxed limits for trusted extensions.
    #[must_use]
    pub const fn for_mode(mode: ExtensionPolicyMode) -> Self {
        match mode {
            ExtensionPolicyMode::Strict => Self {
                max_hostcalls_per_second: Some(20),
                max_hostcalls_per_minute: Some(500),
                max_hostcalls_total: Some(5_000),
                max_subprocesses: Some(4),
                max_write_bytes: Some(50 * 1024 * 1024), // 50 MB
                max_http_requests: Some(200),
            },
            ExtensionPolicyMode::Prompt => Self {
                max_hostcalls_per_second: Some(100),
                max_hostcalls_per_minute: Some(2_000),
                max_hostcalls_total: None,
                max_subprocesses: Some(8),
                max_write_bytes: None,
                max_http_requests: None,
            },
            ExtensionPolicyMode::Permissive => Self {
                max_hostcalls_per_second: Some(500),
                max_hostcalls_per_minute: Some(10_000),
                max_hostcalls_total: None,
                max_subprocesses: Some(32),
                max_write_bytes: None,
                max_http_requests: None,
            },
        }
    }
}
