use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tracing::info;

pub fn pid_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("telerust.pid")
}

pub fn log_path() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/state")
        })
        .join("telerust/telerust.log")
}

pub fn daemonize() -> Result<()> {
    let pid_file = pid_path();
    let log_file = log_path();

    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
    }

    let stdout = std::fs::File::create(&log_file)
        .with_context(|| format!("Failed to create log file: {}", log_file.display()))?;
    let stderr = stdout.try_clone()?;

    let daemon = daemonize::Daemonize::new()
        .pid_file(&pid_file)
        .working_directory("/tmp")
        .stdout(stdout)
        .stderr(stderr);

    daemon.start().context("Failed to daemonize")?;
    info!("Daemonized. PID file: {}", pid_file.display());
    Ok(())
}

pub fn read_pid() -> Result<i32> {
    let path = pid_path();
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read PID file: {}", path.display()))?;
    let pid: i32 = content
        .trim()
        .parse()
        .with_context(|| format!("Invalid PID in file: {}", content.trim()))?;
    Ok(pid)
}

pub fn is_running(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

pub fn stop(timeout_secs: u64) -> Result<()> {
    let pid = read_pid().context("Cannot stop: no PID file found")?;

    if !is_running(pid) {
        let _ = std::fs::remove_file(pid_path());
        bail!("Process {pid} is not running (stale PID file cleaned up)");
    }

    info!("Sending SIGTERM to process {pid}");
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        bail!("Failed to send SIGTERM to process {pid}");
    }

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    while start.elapsed() < timeout {
        if !is_running(pid) {
            let _ = std::fs::remove_file(pid_path());
            info!("Process {pid} stopped successfully");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    bail!("Process {pid} did not stop within {timeout_secs}s. Consider kill -9 {pid}.")
}
