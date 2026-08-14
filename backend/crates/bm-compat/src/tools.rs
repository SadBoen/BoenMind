//! `tools` module shim for bm-compat (B1).
//!
//! Verbatim extraction from `legacy/pi_agent_rust/src/tools.rs`: process-group
//! kill/isolation helpers and the SIGPIPE trampoline used by the vendored
//! `extensions_js.rs`. Real upstream semantics preserved (sysinfo-based
//! process-tree kill); no behavioral stubs.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn kill_process_group_tree(pid: Option<u32>) {
    kill_process_tree_with(pid, sysinfo::Signal::Kill, true);
}

fn terminate_process_group_tree(pid: Option<u32>) {
    kill_process_tree_with(pid, sysinfo::Signal::Term, true);
}

fn kill_process_tree_with(pid: Option<u32>, signal: sysinfo::Signal, include_process_group: bool) {
    let Some(pid) = pid else {
        return;
    };

    let root = sysinfo::Pid::from_u32(pid);

    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut children_map: HashMap<sysinfo::Pid, Vec<sysinfo::Pid>> = HashMap::new();
    for (p, proc_) in sys.processes() {
        if let Some(parent) = proc_.parent() {
            children_map.entry(parent).or_default().push(*p);
        }
    }

    let mut to_kill = Vec::new();
    let mut visited = std::collections::HashSet::new();
    collect_process_tree(root, &children_map, &mut to_kill, &mut visited);

    if include_process_group {
        // Some subprocess surfaces isolate the child into its own process group.
        // When they do, killing the group first catches background children even
        // if they have already been reparented away from the original root PID.
        #[cfg(unix)]
        {
            let sig_num = match signal {
                sysinfo::Signal::Kill => "9",
                _ => "15",
            };
            let _ = Command::new("kill")
                .arg(format!("-{sig_num}"))
                .arg("--")
                .arg(format!("-{pid}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    // Kill children first.
    for pid in to_kill.into_iter().rev() {
        if let Some(proc_) = sys.process(pid) {
            match proc_.kill_with(signal) {
                Some(true) => {}
                Some(false) | None => {
                    let _ = proc_.kill();
                }
            }
        }
    }
}

fn collect_process_tree(
    pid: sysinfo::Pid,
    children_map: &HashMap<sysinfo::Pid, Vec<sysinfo::Pid>>,
    out: &mut Vec<sysinfo::Pid>,
    visited: &mut std::collections::HashSet<sysinfo::Pid>,
) {
    if !visited.insert(pid) {
        return;
    }
    out.push(pid);
    if let Some(children) = children_map.get(&pid) {
        for child in children {
            collect_process_tree(*child, children_map, out, visited);
        }
    }
}
// extracted from legacy/pi_agent_rust/src/tools.rs:9720-9798
// (kill_process_group_tree, kill_process_tree_with, collect_process_tree;
//  terminate_process_group_tree kept as part of the contiguous range)
/// Build a child command whose Unix process image starts with SIGPIPE restored
/// to the platform default, without using `Command::pre_exec`.
///
/// Rust binaries ignore SIGPIPE by default, and POSIX inherits that disposition
/// across `exec(2)`. The tiny `/bin/sh` trampoline resets PIPE and then `exec`s
/// the requested program, preserving argv, cwd, stdio, and the process id that
/// later becomes the isolated process-group leader.
pub(crate) const SIGPIPE_TRAMPOLINE_EXEC_FAILURE_PREFIX: &str = "pi-sigpipe-reset: exec failed:";

pub(crate) fn command_with_default_sigpipe(program: impl AsRef<OsStr>) -> std::io::Result<Command> {
    command_with_default_sigpipe_for_cwd(program.as_ref(), None)
}

/// Variant of [`command_with_default_sigpipe`] for commands that will run with
/// `current_dir(cwd)`. This preserves relative `./program` lookup semantics.
pub(crate) fn command_with_default_sigpipe_in_dir(
    program: impl AsRef<OsStr>,
    cwd: &Path,
) -> std::io::Result<Command> {
    command_with_default_sigpipe_for_cwd(program.as_ref(), Some(cwd))
}

#[cfg(unix)]
fn command_with_default_sigpipe_for_cwd(
    program: &OsStr,
    cwd: Option<&Path>,
) -> std::io::Result<Command> {
    let program = resolve_executable_for_shell_trampoline(program, cwd)?;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(
            "trap - PIPE\n\
             exec \"$@\"\n\
             status=$?\n\
             printf 'pi-sigpipe-reset: exec failed: %s\\n' \"$1\" >&2\n\
             exit \"$status\"",
        )
        .arg("pi-sigpipe-reset")
        .arg(program);
    Ok(command)
}

#[cfg(not(unix))]
fn command_with_default_sigpipe_for_cwd(
    program: &OsStr,
    _cwd: Option<&Path>,
) -> std::io::Result<Command> {
    let command = Command::new(program); // ubs:ignore policy-checked non-Unix command runner
    Ok(command)
}

#[cfg(unix)]
fn resolve_executable_for_shell_trampoline(
    program: &OsStr,
    cwd: Option<&Path>,
) -> std::io::Result<OsString> {
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    fn executable_candidate(path: &Path) -> std::io::Result<bool> {
        let metadata = std::fs::metadata(path)?;
        Ok(metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }

    fn absolutize_candidate(path: &Path, cwd: Option<&Path>) -> std::io::Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        let base = std::env::current_dir()?;
        Ok(cwd.map_or_else(|| base.join(path), |cwd| base.join(cwd).join(path)))
    }

    if program.as_bytes().contains(&b'/') {
        let path = Path::new(program);
        let candidate = absolutize_candidate(path, cwd)?;
        if executable_candidate(&candidate)? {
            return Ok(candidate.into_os_string());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("not an executable file: {}", candidate.display()),
        ));
    }

    let mut permission_denied = false;
    let paths = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/bin:/usr/bin"));
    for dir in std::env::split_paths(&paths) {
        let candidate = absolutize_candidate(&dir.join(program), cwd)?;
        match executable_candidate(&candidate) {
            Ok(true) => return Ok(candidate.into_os_string()),
            Ok(false) => permission_denied = true,
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {}
            Err(err) if matches!(err.kind(), std::io::ErrorKind::PermissionDenied) => {
                permission_denied = true;
            }
            Err(_) => {}
        }
    }

    if permission_denied {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("command is not executable: {}", program.to_string_lossy()),
        ))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("command not found: {}", program.to_string_lossy()),
        ))
    }
}

/// Detach a child process from pi's controlling terminal.
pub(crate) fn isolate_command_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }

    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

// extracted from legacy/pi_agent_rust/src/tools.rs:9800-9927
// (SIGPIPE_TRAMPOLINE_EXEC_FAILURE_PREFIX, command_with_default_sigpipe,
//  command_with_default_sigpipe_in_dir, command_with_default_sigpipe_for_cwd,
//  resolve_executable_for_shell_trampoline, isolate_command_process_group)
