//! Keeps the programs LightLine starts from outliving it. Language servers,
//! the debug adapter, formatters, builds and runs are put in a Windows job
//! that ends them when LightLine exits, however it exits: the job's handle
//! closes with the process, even on a crash or a forced kill.
//!
//! Only what LightLine runs for its own work belongs here: a window it opens
//! for the user (Explorer, a browser) must survive it, so nothing is adopted
//! automatically.
//!
//! A program is adopted just after it starts. Anything it starts in that
//! moment, before adoption, is not covered; what it starts afterwards is.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, ExitStatus, Output};
use std::ptr::null;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

struct Job(HANDLE);
// The handle is owned here and only closed in Drop.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    fn new() -> Option<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return None;
        }
        let job = Job(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        (set != 0).then_some(job)
    }

    fn assign(&self, child: &Child) -> bool {
        unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) != 0 }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

// Never dropped: its handle closes when LightLine's process ends.
fn lifetime_job() -> Option<&'static Job> {
    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    JOB.get_or_init(Job::new).as_ref()
}

/// Ends `child`, and whatever it starts from now on, when LightLine ends.
pub fn adopt(child: &Child) {
    if let Some(job) = lifetime_job() {
        job.assign(child);
    }
}

/// A started program and everything it starts, ended together by `kill` or
/// when this is dropped. `child.kill()` alone ends only the program itself:
/// for an npm `.cmd` launcher that is `cmd.exe`, and the `node` under it
/// keeps running.
pub struct ProcessTree(Option<Job>);

impl ProcessTree {
    /// Adopts `child` (see `adopt`) and gathers it in a tree of its own.
    pub fn new(child: &Child) -> Self {
        adopt(child);
        // A job inside the lifetime job: ending it leaves LightLine's other
        // programs alone.
        Self(Job::new().filter(|job| job.assign(child)))
    }

    pub fn kill(&self) {
        if let Some(job) = &self.0 {
            unsafe { TerminateJobObject(job.0, 1) };
        }
    }
}

/// `command.output()`, with the program adopted while it runs.
pub fn output(command: &mut Command) -> io::Result<Output> {
    use std::process::Stdio;
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    adopt(&child);
    child.wait_with_output()
}

/// `command.status()`, with the program adopted while it runs.
pub fn status(command: &mut Command) -> io::Result<ExitStatus> {
    let mut child = command.spawn()?;
    adopt(&child);
    child.wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::time::{Duration, Instant};

    #[test]
    fn killing_a_tree_ends_what_its_program_started() {
        // cmd starts ping, the way an npm .cmd launcher starts node. Both
        // hold the output pipe, so it only closes once both have ended.
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let tree = ProcessTree::new(&child);
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        // Ping's first lines show it is running.
        while line.trim().is_empty() {
            line.clear();
            output.read_line(&mut line).unwrap();
        }
        let started = Instant::now();
        tree.kill();
        let mut rest = Vec::new();
        output.read_to_end(&mut rest).unwrap();
        child.wait().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "ping outlived its tree"
        );
    }
}
