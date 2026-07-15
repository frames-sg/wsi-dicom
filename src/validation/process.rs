use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::staged_dicom3tools_command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
    pub(crate) success: bool,
    pub(crate) timed_out: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

pub(crate) trait ValidationCommandRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf>;
    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<CommandOutcome, io::Error>;
}

pub(super) struct SystemCommandRunner;

impl ValidationCommandRunner for SystemCommandRunner {
    fn find_command(&self, name: &str) -> Option<PathBuf> {
        let command = Path::new(name);
        if command.is_absolute() {
            return command.is_file().then(|| command.to_path_buf());
        }
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|path| path.join(name))
                    .find(|path| path.is_file())
            })
            .or_else(|| staged_dicom3tools_command(name))
    }

    fn run(
        &self,
        program: &Path,
        args: &[OsString],
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<CommandOutcome, io::Error> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child stdout pipe was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("child stderr pipe was not captured"))?;
        let stdout_reader = read_child_pipe(stdout, max_output_bytes);
        let stderr_reader = read_child_pipe(stderr, max_output_bytes);
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let (stdout, stderr, stdout_truncated, stderr_truncated) =
                        collect_child_pipes(stdout_reader, stderr_reader)?;
                    return Ok(CommandOutcome {
                        success: status.success(),
                        timed_out: false,
                        stdout,
                        stderr,
                        stdout_truncated,
                        stderr_truncated,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let mut cleanup_errors = Vec::new();
                    if let Err(cleanup_err) = terminate_validation_process_tree(&mut child) {
                        cleanup_errors.push(format!("terminate process tree: {cleanup_err}"));
                    }
                    if let Err(cleanup_err) = child.wait() {
                        cleanup_errors.push(format!("wait for child: {cleanup_err}"));
                    }
                    if let Err(cleanup_err) = collect_child_pipes(stdout_reader, stderr_reader) {
                        cleanup_errors.push(format!("collect child output: {cleanup_err}"));
                    }
                    if cleanup_errors.is_empty() {
                        return Err(err);
                    }
                    return Err(io::Error::new(
                        err.kind(),
                        format!(
                            "{err}; cleanup after child wait failure also failed: {}",
                            cleanup_errors.join("; ")
                        ),
                    ));
                }
            }
            if started.elapsed() >= timeout {
                terminate_validation_process_tree(&mut child)?;
                child.wait()?;
                let (stdout, stderr, stdout_truncated, stderr_truncated) =
                    collect_child_pipes(stdout_reader, stderr_reader)?;
                return Ok(CommandOutcome {
                    success: false,
                    timed_out: true,
                    stdout,
                    stderr,
                    stdout_truncated,
                    stderr_truncated,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(unix)]
fn terminate_validation_process_tree(child: &mut std::process::Child) -> io::Result<()> {
    let process_group = rustix::process::Pid::from_child(child);
    match rustix::process::kill_process_group(process_group, rustix::process::Signal::KILL) {
        Ok(()) => Ok(()),
        Err(_) => child.kill(),
    }
}

#[cfg(windows)]
fn terminate_validation_process_tree(child: &mut std::process::Child) -> io::Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) | Err(_) => child.kill(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_child_pipe(
    mut pipe: impl Read + Send + 'static,
    max_output_bytes: usize,
) -> JoinHandle<io::Result<CapturedOutput>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut limited = pipe.by_ref().take(
            u64::try_from(max_output_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        );
        limited.read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max_output_bytes;
        if truncated {
            bytes.truncate(max_output_bytes);
            io::copy(&mut pipe, &mut io::sink())?;
        }
        Ok(CapturedOutput { bytes, truncated })
    })
}

fn collect_child_pipes(
    stdout_reader: JoinHandle<io::Result<CapturedOutput>>,
    stderr_reader: JoinHandle<io::Result<CapturedOutput>>,
) -> io::Result<(String, String, bool, bool)> {
    let stdout = collect_child_pipe(stdout_reader)?;
    let stderr = collect_child_pipe(stderr_reader)?;
    Ok((
        String::from_utf8_lossy(&stdout.bytes).into_owned(),
        String::from_utf8_lossy(&stderr.bytes).into_owned(),
        stdout.truncated,
        stderr.truncated,
    ))
}

fn collect_child_pipe(
    reader: JoinHandle<io::Result<CapturedOutput>>,
) -> io::Result<CapturedOutput> {
    reader
        .join()
        .map_err(|_| io::Error::other("child output reader thread panicked"))?
}
