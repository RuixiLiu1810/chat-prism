use std::borrow::Cow;
use std::process::Output;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::timeout;

const AGENT_CANCELLED_MESSAGE: &str = "Agent run cancelled by user.";
const DEFAULT_SHELL_COMMAND_TIMEOUT_SECS: u64 = 30;
const DEFAULT_SHELL_OUTPUT_MAX_BYTES: usize = 32_000;

/// Check if an environment variable should be explicitly passed to child processes.
///
/// NOTE: This is NOT a true whitelist — we do NOT call `env_clear()`, so the
/// child inherits the full parent environment. This helper only identifies vars
/// that we explicitly re-set via `cmd.env()` to guarantee they are present even
/// when other per-key overrides are applied.
pub(crate) fn is_essential_env_var(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    matches!(
        k.as_str(),
        "HOME"
            | "USER"
            | "SHELL"
            | "LANG"
            | "HOMEBREW_PREFIX"
            | "HOMEBREW_CELLAR"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "NO_PROXY"
            | "ALL_PROXY"
    ) || k.starts_with("LC_")
        || matches!(
            k.as_str(),
            "USERPROFILE"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "TEMP"
                | "TMP"
                | "SYSTEMROOT"
                | "SYSTEMDRIVE"
                | "COMPUTERNAME"
                | "USERNAME"
                | "PROGRAMFILES"
                | "PROGRAMFILES(X86)"
                | "COMMONPROGRAMFILES"
                | "PATHEXT"
                | "PSMODULEPATH"
                | "WINDIR"
        )
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

fn strip_nul(s: &str) -> Cow<'_, str> {
    if s.contains('\0') {
        Cow::Owned(s.replace('\0', ""))
    } else {
        Cow::Borrowed(s)
    }
}

#[cfg(target_os = "linux")]
const LINUX_DESKTOP_ENV_VARS: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "DESKTOP_SESSION",
];

#[cfg(target_os = "linux")]
fn sanitize_appimage_env(cmd: &mut tokio::process::Command) {
    cmd.stdin(std::process::Stdio::null());

    if std::env::var("APPIMAGE").is_ok() {
        for key in &[
            "LD_LIBRARY_PATH",
            "PATH",
            "GDK_PIXBUF_MODULE_FILE",
            "PYTHONPATH",
            "PERLLIB",
            "GSETTINGS_SCHEMA_DIR",
        ] {
            let orig_key = format!("{}_ORIG", key);
            match std::env::var(&orig_key) {
                Ok(orig) => {
                    cmd.env(key, orig);
                }
                Err(_) => {
                    cmd.env_remove(key);
                }
            }
        }
        cmd.env_remove("GDK_BACKEND");
        cmd.env_remove("GIO_MODULE_DIR");
        cmd.env_remove("GIO_EXTRA_MODULES");
    }

    for key in LINUX_DESKTOP_ENV_VARS {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
}

#[cfg(target_os = "windows")]
fn resolve_cmd_to_node(program: &str) -> (String, Vec<String>) {
    let lower = program.to_lowercase();
    if !lower.ends_with(".cmd") && !lower.ends_with(".bat") {
        return (program.to_string(), vec![]);
    }
    let cmd_dir = std::path::Path::new(program)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let cli_js = cmd_dir
        .join("node_modules")
        .join("@anthropic-ai")
        .join("claude-code")
        .join("cli.js");
    if cli_js.exists() {
        let node = {
            let local_node = cmd_dir.join("node.exe");
            if local_node.exists() {
                local_node.to_string_lossy().to_string()
            } else {
                "node".to_string()
            }
        };
        return (node, vec![cli_js.to_string_lossy().to_string()]);
    }
    (
        "cmd.exe".to_string(),
        vec!["/C".to_string(), program.to_string()],
    )
}

fn create_command(program: &str, args: Vec<String>, cwd: &str) -> Command {
    let clean_program = strip_nul(program);
    let clean_args: Vec<Cow<str>> = args.iter().map(|a| strip_nul(a)).collect();
    let clean_cwd = strip_nul(cwd);

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let (resolved, prefix) = resolve_cmd_to_node(clean_program.as_ref());
        let mut c = Command::new(&resolved);
        c.creation_flags(CREATE_NO_WINDOW);
        if !prefix.is_empty() {
            c.args(&prefix);
        }
        c.args(clean_args.iter().map(|a| a.as_ref()));
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new(clean_program.as_ref());
        c.args(clean_args.iter().map(|a| a.as_ref()));
        c
    };

    cmd.current_dir(clean_cwd.as_ref());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(target_os = "linux")]
    sanitize_appimage_env(&mut cmd);

    let mut current_path = strip_nul(&std::env::var("PATH").unwrap_or_default()).into_owned();

    #[cfg(not(target_os = "windows"))]
    let sep = ":";
    #[cfg(target_os = "windows")]
    let sep = ";";

    #[cfg(not(target_os = "windows"))]
    {
        let extra_bins = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/opt/homebrew/opt/node/bin",
        ];
        for bin in extra_bins.iter().rev() {
            if !current_path.split(sep).any(|p| p == *bin) {
                current_path = format!("{}{}{}", bin, sep, current_path);
            }
        }

        if let Some(home) = dirs::home_dir() {
            let home_bins = [
                home.join(".local").join("bin"),
                home.join(".cargo").join("bin"),
                home.join(".npm-global").join("bin"),
                home.join(".yarn").join("bin"),
                home.join(".bun").join("bin"),
                home.join("bin"),
            ];
            for bin in home_bins.iter().rev() {
                if bin.exists() {
                    let bin_str = bin.to_string_lossy();
                    if !current_path.split(sep).any(|p| p == bin_str) {
                        current_path = format!("{}{}{}", bin_str, sep, current_path);
                    }
                }
            }
        }
    }

    cmd.env("PATH", current_path);
    cmd
}

fn truncate_command_output(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }

    let mut end = max_bytes.min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }

    let mut text = String::from_utf8_lossy(&bytes[..end]).to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("...[truncated]");
    (text, true)
}

pub(crate) async fn command_available(program: &str, cwd: &str) -> bool {
    let mut cmd = create_command(program, vec!["--version".to_string()], cwd);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    matches!(
        timeout(Duration::from_secs(2), cmd.status()).await,
        // We only care whether the executable can be spawned.
        // Some tools (e.g. pdftotext) return non-zero for `--version`
        // but are still installed and runnable.
        Ok(Ok(_status))
    )
}

pub(crate) async fn wait_for_command_output(
    mut cmd: Command,
    mut cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<Output, String> {
    cmd.kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn command: {}", e))?;
    let output_fut = child.wait_with_output();
    tokio::pin!(output_fut);

    if let Some(cancel_rx) = cancel_rx.as_mut() {
        loop {
            tokio::select! {
                changed = cancel_rx.changed() => {
                    match changed {
                        Ok(_) if *cancel_rx.borrow() => return Err(AGENT_CANCELLED_MESSAGE.to_string()),
                        Ok(_) => continue,
                        Err(_) => return Err(AGENT_CANCELLED_MESSAGE.to_string()),
                    }
                }
                output = &mut output_fut => {
                    return output.map_err(|e| format!("Failed to wait for command: {}", e));
                }
            }
        }
    } else {
        output_fut
            .await
            .map_err(|e| format!("Failed to wait for command: {}", e))
    }
}

#[derive(serde::Serialize)]
pub struct ShellCommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[tauri::command]
pub async fn run_shell_command(command: String, cwd: String) -> Result<ShellCommandResult, String> {
    run_shell_command_with_limits(
        command,
        cwd,
        None,
        DEFAULT_SHELL_COMMAND_TIMEOUT_SECS,
        DEFAULT_SHELL_OUTPUT_MAX_BYTES,
    )
    .await
}

async fn run_shell_command_with_limits(
    command: String,
    cwd: String,
    cancel_rx: Option<watch::Receiver<bool>>,
    timeout_secs: u64,
    output_max_bytes: usize,
) -> Result<ShellCommandResult, String> {
    #[cfg(not(target_os = "windows"))]
    let (shell, args) = ("sh", vec!["-c".to_string(), command]);
    #[cfg(target_os = "windows")]
    let (shell, args) = ("cmd", vec!["/C".to_string(), command]);
    let cmd = create_command(shell, args, &cwd);
    let output = timeout(
        Duration::from_secs(timeout_secs),
        wait_for_command_output(cmd, cancel_rx),
    )
    .await
    .map_err(|_| format!("Shell command timed out after {}s", timeout_secs))??;

    let (stdout, stdout_truncated) = truncate_command_output(&output.stdout, output_max_bytes);
    let (stderr, stderr_truncated) = truncate_command_output(&output.stderr, output_max_bytes);

    Ok(ShellCommandResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

pub(crate) async fn run_shell_command_cancellable(
    command: String,
    cwd: String,
    cancel_rx: Option<watch::Receiver<bool>>,
) -> Result<ShellCommandResult, String> {
    run_shell_command_with_limits(
        command,
        cwd,
        cancel_rx,
        DEFAULT_SHELL_COMMAND_TIMEOUT_SECS,
        DEFAULT_SHELL_OUTPUT_MAX_BYTES,
    )
    .await
}

pub(crate) async fn run_program_with_limits(
    program: &str,
    args: Vec<String>,
    cwd: String,
    cancel_rx: Option<watch::Receiver<bool>>,
    timeout_secs: u64,
    output_max_bytes: usize,
) -> Result<ShellCommandResult, String> {
    let cmd = create_command(program, args, &cwd);
    let output = timeout(
        Duration::from_secs(timeout_secs),
        wait_for_command_output(cmd, cancel_rx),
    )
    .await
    .map_err(|_| format!("{} timed out after {}s", program, timeout_secs))??;

    let (stdout, stdout_truncated) = truncate_command_output(&output.stdout, output_max_bytes);
    let (stderr, stderr_truncated) = truncate_command_output(&output.stderr, output_max_bytes);

    Ok(ShellCommandResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_command_sets_args() {
        let cmd = create_command("/usr/bin/echo", vec!["hello".to_string()], "/tmp");
        let debug_str = format!("{:?}", cmd);
        assert!(debug_str.contains("hello"));
    }

    #[test]
    fn create_command_is_constructible_without_args() {
        let cmd = create_command("/usr/bin/env", vec![], "/tmp");
        let debug_str = format!("{:?}", cmd);
        assert!(debug_str.contains("/usr/bin/env"));
    }

    #[test]
    fn truncate_command_output_marks_and_limits_large_payloads() {
        let long = "中".repeat(20);
        let (truncated, was_truncated) = truncate_command_output(long.as_bytes(), 17);
        assert!(was_truncated);
        assert!(truncated.contains("...[truncated]"));
        assert!(!truncated.contains('\u{fffd}'));
    }
}
