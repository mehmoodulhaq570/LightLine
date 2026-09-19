use super::launch::{os_wide, reject_nul};
use super::{LaunchSpec, SessionStatus, Shared, TerminalSize, quote_windows_argument};
use std::ffi::{OsStr, c_void};
use std::mem::{size_of, size_of_val};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_OPERATION_ABORTED, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{COORD, HPCON};
use windows_sys::Win32::System::IO::CancelSynchronousIo;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentThreadId, GetExitCodeProcess, INFINITE,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenThread,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW,
    THREAD_TERMINATE, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};

struct Handle(HANDLE);
// These owned kernel handles can move between threads; no borrowed handles enter this type.
unsafe impl Send for Handle {}

impl Handle {
    fn new(raw: HANDLE, operation: &str) -> Result<Self, String> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            Err(last_error(operation))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn last_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

fn pipe() -> Result<(Handle, Handle), String> {
    let (mut read, mut write) = (null_mut(), null_mut());
    // Null security attributes create noninheritable endpoints. ConPTY duplicates what it needs.
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    Ok((Handle(read), Handle(write)))
}

type CreateConpty = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
type ResizeConpty = unsafe extern "system" fn(HPCON, COORD) -> i32;
type CloseConpty = unsafe extern "system" fn(HPCON);

struct PseudoConsole {
    raw: HPCON,
    resize: ResizeConpty,
    close: CloseConpty,
}

impl PseudoConsole {
    fn new(size: TerminalSize, input: &Handle, output: &Handle) -> Result<Self, String> {
        // Dynamic binding gives unsupported Windows versions a recoverable error instead
        // of a loader failure. kernel32 stays loaded for the lifetime of this process.
        unsafe {
            let module = GetModuleHandleW(wide(OsStr::new("kernel32.dll"))?.as_ptr());
            if module.is_null() {
                return Err(last_error("GetModuleHandleW"));
            }
            let create = GetProcAddress(module, c"CreatePseudoConsole".as_ptr().cast())
                .ok_or("ConPTY requires Windows 10 version 1809 or newer")?;
            let resize = GetProcAddress(module, c"ResizePseudoConsole".as_ptr().cast())
                .ok_or("ResizePseudoConsole is unavailable")?;
            let close = GetProcAddress(module, c"ClosePseudoConsole".as_ptr().cast())
                .ok_or("ClosePseudoConsole is unavailable")?;
            // Signatures match the fetched windows-sys 0.61.2 declarations exactly.
            let create: CreateConpty = std::mem::transmute(create);
            let resize: ResizeConpty = std::mem::transmute(resize);
            let close: CloseConpty = std::mem::transmute(close);
            let mut raw: HPCON = 0;
            let result = create(coord(size), input.0, output.0, 0, &mut raw);
            if result < 0 {
                return Err(format!(
                    "CreatePseudoConsole failed: HRESULT {result:#010x}"
                ));
            }
            Ok(Self { raw, resize, close })
        }
    }

    fn resize(&self, size: TerminalSize) -> Result<(), String> {
        let result = unsafe { (self.resize)(self.raw, coord(size)) };
        if result < 0 {
            Err(format!(
                "ResizePseudoConsole failed: HRESULT {result:#010x}"
            ))
        } else {
            Ok(())
        }
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.raw);
        }
    }
}

fn coord(size: TerminalSize) -> COORD {
    COORD {
        X: size.columns as i16,
        Y: size.rows as i16,
    }
}

struct Attributes {
    storage: Vec<usize>,
}

impl Attributes {
    fn new(console: HPCON) -> Result<Self, String> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(last_error("Size process attributes"));
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let pointer = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &mut bytes) } == 0 {
            return Err(last_error("InitializeProcThreadAttributeList"));
        }
        let mut attributes = Self { storage };
        // lpValue is the HPCON VALUE cast to a pointer, not &console.
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const c_void,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(last_error("UpdateProcThreadAttribute(PSEUDOCONSOLE)"));
        }
        Ok(attributes)
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}

struct Process {
    process: Handle,
    thread: Handle,
}

impl Drop for Process {
    fn drop(&mut self) {
        // Also covers CreateProcess success followed by job-assignment or resume failure.
        unsafe {
            if WaitForSingleObject(self.process.0, 0) != WAIT_OBJECT_0 {
                TerminateProcess(self.process.0, 1);
            }
            WaitForSingleObject(self.process.0, INFINITE);
        }
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>, String> {
    reject_nul(value)?;
    let mut value = os_wide(value);
    value.push(0);
    Ok(value)
}

fn environment(spec: &LaunchSpec) -> Result<Vec<u16>, String> {
    let mut entries: Vec<_> = std::env::vars_os().collect();
    for (key, value) in &spec.environment {
        reject_nul(key)?;
        if key.is_empty() || key.to_string_lossy().contains('=') {
            return Err("Invalid child environment key".into());
        }
        entries.retain(|(existing, _)| {
            !existing
                .to_string_lossy()
                .eq_ignore_ascii_case(&key.to_string_lossy())
        });
        if let Some(value) = value {
            entries.push((key.clone(), value.clone()));
        }
    }
    entries.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut result = Vec::new();
    for (key, value) in entries {
        reject_nul(&key)?;
        reject_nul(&value)?;
        result.extend(os_wide(&key));
        result.push(u16::from(b'='));
        result.extend(os_wide(&value));
        result.push(0);
    }
    if result.is_empty() {
        result.push(0);
    }
    result.push(0);
    Ok(result)
}

fn create_job() -> Result<Handle, String> {
    let job = Handle::new(
        unsafe { CreateJobObjectW(null(), null()) },
        "CreateJobObjectW",
    )?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(last_error("SetInformationJobObject"));
    }
    Ok(job)
}

fn spawn(spec: &LaunchSpec, console: &PseudoConsole, job: &Handle) -> Result<Process, String> {
    if !spec.executable.is_absolute() || !spec.cwd.is_absolute() {
        return Err("Terminal launch needs an absolute executable and working directory".into());
    }
    let executable = wide(spec.executable.as_os_str())?;
    let directory = wide(spec.cwd.as_os_str())?;
    let mut command = quote_windows_argument(spec.executable.as_os_str())?;
    for argument in &spec.arguments {
        command.push(u16::from(b' '));
        command.extend(quote_windows_argument(argument)?);
    }
    command.push(0);
    if command.len() > 32_767 {
        return Err("Terminal command line exceeds the Windows limit".into());
    }
    let environment = environment(spec)?;
    let mut attributes = Attributes::new(console.raw)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.pointer();
    let mut information = PROCESS_INFORMATION::default();
    let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT;
    if unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            flags,
            environment.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut information,
        )
    } == 0
    {
        return Err(last_error("CreateProcessW"));
    }
    let process = Process {
        process: Handle(information.hProcess),
        thread: Handle(information.hThread),
    };
    if unsafe { AssignProcessToJobObject(job.0, process.process.0) } == 0 {
        return Err(last_error(
            "AssignProcessToJobObject (process was not resumed)",
        ));
    }
    if unsafe { ResumeThread(process.thread.0) } == u32::MAX {
        return Err(last_error("ResumeThread"));
    }
    Ok(process)
}

struct IoWorker {
    thread: Option<JoinHandle<()>>,
    native_thread: Handle,
}

impl IoWorker {
    fn spawn(
        name: &str,
        shared: Arc<Shared>,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                // Keep a real handle, not a thread ID that Windows could later recycle.
                let handle = Handle::new(
                    unsafe { OpenThread(THREAD_TERMINATE, 0, GetCurrentThreadId()) },
                    "OpenThread for I/O cancellation",
                );
                let ready = handle.is_ok();
                if ready_tx.send(handle).is_err() || !ready {
                    return;
                }
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_err() {
                    shared.fail("Terminal I/O worker panicked".into());
                }
            })
            .map_err(|error| format!("Start terminal I/O worker: {error}"))?;
        match ready_rx.recv() {
            Ok(Ok(native_thread)) => Ok(Self {
                thread: Some(thread),
                native_thread,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(format!("Terminal I/O startup: {error}"))
            }
        }
    }

    fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn cancel(&self) {
        unsafe {
            CancelSynchronousIo(self.native_thread.0);
        }
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Runtime {
    shared: Arc<Shared>,
    console: Option<PseudoConsole>,
    job: Option<Handle>,
    process: Option<Process>,
    reader: Option<IoWorker>,
    writer: Option<IoWorker>,
}

impl Runtime {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            console: None,
            job: None,
            process: None,
            reader: None,
            writer: None,
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shared.set_status(SessionStatus::Stopping);
        self.shared.io_stop.store(true, Ordering::Release);
        // Kill the entire tree before closing the PTY. No descendant can keep pipes alive.
        if let Some(job) = self.job.take() {
            unsafe {
                TerminateJobObject(job.0, 1);
            }
            drop(job); // KILL_ON_JOB_CLOSE also covers termination failure.
        }
        drop(self.process.take());
        if let Some(writer) = &mut self.writer {
            while !writer.finished() {
                // Repeat to cover cancellation racing the next synchronous WriteFile.
                writer.cancel();
                thread::sleep(Duration::from_millis(5));
            }
            writer.join();
        }
        // On Windows before 11 24H2 ClosePseudoConsole can block while flushing output.
        // Both reader and model keep draining on independent threads throughout this call.
        drop(self.console.take());
        if let Some(reader) = &mut self.reader {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !reader.finished() {
                if Instant::now() >= deadline {
                    // Newer Windows closes asynchronously; a bounded grace period preserves
                    // tail output, then cancellation handles a nonterminating pipe safely.
                    self.shared.reader_abort.store(true, Ordering::Release);
                    reader.cancel();
                }
                thread::sleep(Duration::from_millis(5));
            }
            reader.join();
        }
    }
}

fn read_output(pipe: Handle, shared: Arc<Shared>, output: SyncSender<Vec<u8>>) {
    let mut buffer = [0u8; 4096];
    loop {
        if shared.reader_abort.load(Ordering::Acquire) {
            break;
        }
        let mut count = 0;
        let ok = unsafe {
            ReadFile(
                pipe.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut count,
                null_mut(),
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_OPERATION_ABORTED as i32)
                && !shared.io_stop.load(Ordering::Acquire)
            {
                shared.fail(format!("Terminal read: {error}"));
            }
            break;
        }
        if count == 0 {
            break;
        }
        let mut bytes = buffer[..count as usize].to_vec();
        loop {
            match output.try_send(bytes) {
                Ok(()) => break,
                Err(TrySendError::Full(pending)) => {
                    if shared.reader_abort.load(Ordering::Acquire) {
                        return;
                    }
                    bytes = pending;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Disconnected(_)) => {
                    shared.fail("Terminal parser disconnected".into());
                    return;
                }
            }
        }
    }
    shared.output_eof.store(true, Ordering::Release);
}

fn write_bytes(pipe: &Handle, shared: &Shared, bytes: &[u8]) -> bool {
    for chunk in bytes.chunks(4096) {
        let mut remaining = chunk;
        while !remaining.is_empty() {
            if shared.io_stop.load(Ordering::Acquire) {
                return false;
            }
            let mut count = 0;
            let ok = unsafe {
                WriteFile(
                    pipe.0,
                    remaining.as_ptr(),
                    remaining.len() as u32,
                    &mut count,
                    null_mut(),
                )
            };
            if ok == 0 || count == 0 {
                if !shared.io_stop.load(Ordering::Acquire) {
                    shared.fail(last_error("Terminal write"));
                }
                return false;
            }
            remaining = &remaining[count as usize..];
        }
    }
    true
}

fn write_input(
    pipe: Handle,
    shared: Arc<Shared>,
    input: Receiver<Vec<u8>>,
    replies: Receiver<Vec<u8>>,
) {
    while !shared.io_stop.load(Ordering::Acquire) {
        // Replies have their own bounded priority queue, so bulk user paste cannot starve DSR.
        for _ in 0..64 {
            match replies.try_recv() {
                Ok(reply) if !write_bytes(&pipe, &shared, &reply) => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        match input.recv_timeout(Duration::from_millis(8)) {
            Ok(bytes) if !write_bytes(&pipe, &shared, &bytes) => return,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

pub(super) fn run(
    spec: LaunchSpec,
    size: TerminalSize,
    shared: Arc<Shared>,
    input: Receiver<Vec<u8>>,
    output: SyncSender<Vec<u8>>,
    replies: Receiver<Vec<u8>>,
) -> SessionStatus {
    let result = run_inner(spec, size, shared.clone(), input, output, replies);
    // run_inner's Runtime has finished all teardown before this status is returned.
    let failure = shared
        .failure
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(error) = failure {
        return SessionStatus::Failed(error);
    }
    match result {
        Ok(Some(code)) => SessionStatus::Exited { code },
        Ok(None) => SessionStatus::Stopped,
        Err(error) => SessionStatus::Failed(error),
    }
}

fn run_inner(
    spec: LaunchSpec,
    size: TerminalSize,
    shared: Arc<Shared>,
    input: Receiver<Vec<u8>>,
    output: SyncSender<Vec<u8>>,
    replies: Receiver<Vec<u8>>,
) -> Result<Option<u32>, String> {
    let mut runtime = Runtime::new(shared.clone());
    let (pty_input, host_input) = pipe()?;
    let (host_output, pty_output) = pipe()?;
    runtime.console = Some(PseudoConsole::new(size, &pty_input, &pty_output)?);
    // ConPTY owns duplicates; keeping redundant pipe ends alive would suppress EOF.
    drop(pty_input);
    drop(pty_output);
    let reader_shared = shared.clone();
    runtime.reader = Some(IoWorker::spawn(
        "terminal-reader",
        shared.clone(),
        move || read_output(host_output, reader_shared, output),
    )?);
    let writer_shared = shared.clone();
    runtime.writer = Some(IoWorker::spawn(
        "terminal-writer",
        shared.clone(),
        move || write_input(host_input, writer_shared, input, replies),
    )?);
    runtime.job = Some(create_job()?);
    runtime.process = Some(spawn(
        &spec,
        runtime.console.as_ref().unwrap(),
        runtime.job.as_ref().unwrap(),
    )?);
    shared.set_status(SessionStatus::Running);
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let process = &runtime.process.as_ref().unwrap().process;
        match unsafe { WaitForSingleObject(process.0, 0) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
                    return Err(last_error("GetExitCodeProcess"));
                }
                return Ok(Some(code));
            }
            WAIT_TIMEOUT => {}
            _ => return Err(last_error("WaitForSingleObject")),
        }
        if shared.output_eof.load(Ordering::Acquire) {
            return Err("Terminal output closed while the shell was running".into());
        }
        let desired = shared.desired_size.load(Ordering::Acquire);
        if desired != shared.actual_size.load(Ordering::Acquire) {
            runtime
                .console
                .as_ref()
                .unwrap()
                .resize(TerminalSize::unpack(desired))?;
            shared.actual_size.store(desired, Ordering::Release);
            shared.revision.fetch_add(1, Ordering::Release);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{LaunchRequest, SessionKind, TerminalService};
    use std::path::PathBuf;

    fn await_snapshot(
        service: &mut TerminalService,
        id: crate::terminal::SessionId,
        predicate: impl Fn(&crate::terminal::Snapshot) -> bool,
    ) -> Arc<crate::terminal::Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(snapshot) = service.snapshot(id)
                && predicate(&snapshot)
            {
                return snapshot;
            }
            for event in service.poll(2) {
                if event.session_id == id && predicate(&event.snapshot) {
                    return event.snapshot;
                }
            }
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for terminal session {id:?}"
            );
            thread::sleep(Duration::from_millis(15));
        }
    }

    #[test]
    #[ignore = "live Windows ConPTY/PowerShell lifecycle; run explicitly with --ignored"]
    fn terminal_live_shell_resize_input_exit_and_stale_id() {
        let mut service = TerminalService::default();
        let request = LaunchRequest::shell(std::env::current_dir().unwrap()).without_profile();
        let id = service
            .start(
                SessionKind::Shell,
                request,
                TerminalSize::new(12, 80).unwrap(),
            )
            .unwrap();
        await_snapshot(&mut service, id, |s| s.status == SessionStatus::Running);
        service
            .resize(id, TerminalSize::new(15, 90).unwrap())
            .unwrap();
        service
            .input(id, b"[Console]::WriteLine('CONPTY_' + 'READY')\r")
            .unwrap();
        let snap = await_snapshot(&mut service, id, |s| {
            s.text().contains("CONPTY_READY") && s.size.columns == 90
        });
        assert_eq!(snap.size.rows, 15);
        service.input(id, b"exit 7\r").unwrap();
        let snap = await_snapshot(&mut service, id, |s| s.status.is_final());
        assert_eq!(snap.status, SessionStatus::Exited { code: 7 });
        service.remove(id).unwrap();
        assert!(service.input(id, b"stale").is_err());
    }

    #[test]
    #[ignore = "live Windows ConPTY shutdown under output flood; run explicitly with --ignored"]
    fn terminal_live_independent_shells_and_flood_stop() {
        let mut service = TerminalService::default();
        let request = LaunchRequest::shell(std::env::current_dir().unwrap()).without_profile();
        let shell = service
            .start(SessionKind::Shell, request.clone(), TerminalSize::default())
            .unwrap();
        let run = service
            .start(SessionKind::ManagedRun, request, TerminalSize::default())
            .unwrap();
        await_snapshot(&mut service, shell, |s| s.status == SessionStatus::Running);
        // Check the mailbox too: polling one session may already have consumed the other's event.
        if !service
            .snapshot(run)
            .is_some_and(|s| s.status == SessionStatus::Running)
        {
            await_snapshot(&mut service, run, |s| s.status == SessionStatus::Running);
        }
        service
            .input(
                run,
                b"while ($true) { [Console]::WriteLine(('x' * 200)) }\r",
            )
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        service.stop(run).unwrap();
        let snap = await_snapshot(&mut service, run, |s| s.status.is_final());
        assert_eq!(snap.status, SessionStatus::Stopped);
        assert_eq!(
            service.snapshot(shell).unwrap().status,
            SessionStatus::Running
        );
        service.remove(run).unwrap();
        service.stop(shell).unwrap();
        await_snapshot(&mut service, shell, |s| s.status.is_final());
        service.remove(shell).unwrap();
    }

    #[test]
    #[ignore = "live Windows descendant kill-on-close; run explicitly with --ignored"]
    fn terminal_live_stop_kills_descendant() {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        let mut service = TerminalService::default();
        let request = LaunchRequest::shell(std::env::current_dir().unwrap()).without_profile();
        let id = service
            .start(SessionKind::Shell, request, TerminalSize::default())
            .unwrap();
        await_snapshot(&mut service, id, |s| s.status == SessionStatus::Running);
        service.input(id, b"$child = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList '-NoProfile','-Command','Start-Sleep 300' -NoNewWindow -PassThru; [Console]::WriteLine(('CHILD_' + $child.Id))\r").unwrap();
        let child_pid = |snapshot: &crate::terminal::Snapshot| {
            snapshot.text().lines().find_map(|line| {
                line.trim()
                    .strip_prefix("CHILD_")
                    .and_then(|id| id.parse::<u32>().ok())
            })
        };
        let snapshot = await_snapshot(&mut service, id, |s| child_pid(s).is_some());
        let child = Handle::new(
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, child_pid(&snapshot).unwrap()) },
            "Open child for lifecycle test",
        )
        .unwrap();
        service.stop(id).unwrap();
        await_snapshot(&mut service, id, |s| s.status.is_final());
        assert_eq!(
            unsafe { WaitForSingleObject(child.0, 2_000) },
            WAIT_OBJECT_0
        );
        service.remove(id).unwrap();
    }

    #[test]
    #[ignore = "live managed Python; set LIGHTLINE_TEST_PYTHON to python.exe and run explicitly"]
    fn terminal_live_python_input_safe_paths_and_retained_shell() {
        let interpreter = PathBuf::from(
            std::env::var_os("LIGHTLINE_TEST_PYTHON")
                .expect("Set LIGHTLINE_TEST_PYTHON to the selected python.exe"),
        );
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("lightline '‘’“” {nonce}"));
        std::fs::create_dir_all(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let script = root.join("script '‘’“”.py");
        std::fs::write(
            &script,
            "print('PY_READY', flush=True)\nvalue = input()\nprint('GOT_' + value, flush=True)\n",
        )
        .unwrap();
        let mut service = TerminalService::default();
        let request = LaunchRequest::python(interpreter, script, root).without_profile();
        let id = service
            .start(SessionKind::ManagedRun, request, TerminalSize::default())
            .unwrap();
        await_snapshot(&mut service, id, |s| s.text().contains("PY_READY"));
        service.input(id, "hé界\r".as_bytes()).unwrap();
        await_snapshot(&mut service, id, |s| s.text().contains("GOT_hé界"));
        service
            .input(id, b"Write-Output ('SHELL_' + 'ALIVE')\r")
            .unwrap();
        await_snapshot(&mut service, id, |s| {
            s.text().contains("SHELL_ALIVE") && s.status == SessionStatus::Running
        });
        service.stop(id).unwrap();
        await_snapshot(&mut service, id, |s| s.status.is_final());
        service.remove(id).unwrap();
    }

    #[test]
    #[ignore = "live Windows CreateProcess failure cleanup; run explicitly with --ignored"]
    fn terminal_live_failed_spawn_reaps_pipe_workers() {
        let size = TerminalSize::default();
        let wake = Arc::new(crate::terminal::Wake {
            pending: std::sync::atomic::AtomicBool::new(false),
            callback: Arc::new(|| {}),
        });
        let shared = Arc::new(Shared::new(size, wake));
        let (_input, input_rx) = mpsc::sync_channel(1);
        let (_replies, reply_rx) = mpsc::sync_channel(1);
        let (output_tx, output_rx) = mpsc::sync_channel(1);
        let drain = thread::spawn(move || while output_rx.recv().is_ok() {});
        let cwd = std::env::current_dir().unwrap();
        let spec = LaunchSpec {
            executable: cwd.join("lightline-intentionally-missing-shell.exe"),
            arguments: vec![],
            cwd,
            environment: vec![],
        };
        let result = run(spec, size, shared, input_rx, output_tx, reply_rx);
        assert!(matches!(result, SessionStatus::Failed(_)));
        drain.join().unwrap();
    }

    #[test]
    #[ignore = "live Windows failed launch cleanup; run explicitly with --ignored"]
    fn terminal_live_missing_cwd_reports_failure() {
        let mut service = TerminalService::default();
        let missing = PathBuf::from("Z:\\lightline-nonexistent-terminal-cwd");
        let id = service
            .start(
                SessionKind::Shell,
                LaunchRequest::shell(missing),
                TerminalSize::default(),
            )
            .unwrap();
        let snap = await_snapshot(&mut service, id, |s| s.status.is_final());
        assert!(matches!(snap.status, SessionStatus::Failed(_)));
        service.remove(id).unwrap();
    }
}
