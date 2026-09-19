mod input;
mod launch;
mod model;
#[cfg(windows)]
mod platform;

pub use input::{InputModes, Key, Modifiers, encode_key, encode_paste};
pub use launch::{LaunchRequest, LaunchSpec, encode_powershell_command, powershell_path_expression, prepare_launch, python_command, quote_windows_argument, resolve_powershell};
pub use model::{Cell, Color, Cursor, Row, SCROLLBACK_LINES, Snapshot, TerminalModel};

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_DRAIN_EVENTS: usize = 2;
const INPUT_QUEUE: usize = 32;
const OUTPUT_QUEUE: usize = 64;
const REPLY_QUEUE: usize = 64;
const MAX_ROWS: u16 = 200;
const MAX_COLUMNS: u16 = 400;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionKind {
    Shell,
    ManagedRun,
}

impl SessionKind {
    fn index(self) -> usize {
        match self { Self::Shell => 0, Self::ManagedRun => 1 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub rows: u16,
    pub columns: u16,
}

impl TerminalSize {
    pub fn new(rows: u16, columns: u16) -> Result<Self, ControlError> {
        let size = Self { rows, columns };
        size.validate()?;
        Ok(size)
    }

    fn validate(self) -> Result<(), ControlError> {
        if self.rows == 0 || self.rows > MAX_ROWS || self.columns == 0 || self.columns > MAX_COLUMNS {
            Err(ControlError::InvalidSize)
        } else { Ok(()) }
    }

    pub(super) fn normalized(self) -> Self {
        Self { rows: self.rows.clamp(1, MAX_ROWS), columns: self.columns.clamp(1, MAX_COLUMNS) }
    }

    fn packed(self) -> u32 { (u32::from(self.rows) << 16) | u32::from(self.columns) }
    fn unpack(value: u32) -> Self { Self { rows: (value >> 16) as u16, columns: value as u16 } }
}

impl Default for TerminalSize {
    fn default() -> Self { Self { rows: 24, columns: 80 } }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Starting,
    Running,
    Stopping,
    Exited { code: u32 },
    Stopped,
    Failed(String),
}

impl SessionStatus {
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Exited { .. } | Self::Stopped | Self::Failed(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlError {
    StaleSession,
    Occupied(SessionId),
    Busy,
    Closed,
    InvalidSize,
    InputTooLarge,
    UnsupportedPlatform,
    Spawn(String),
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for ControlError {}

#[derive(Clone, Debug)]
pub struct TerminalEvent {
    pub session_id: SessionId,
    pub generation: u64,
    pub snapshot: Arc<Snapshot>,
}

struct Wake {
    pending: AtomicBool,
    callback: Arc<dyn Fn() + Send + Sync>,
}

impl Wake {
    fn signal(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            // The callback must only schedule work (e.g. PostMessage), never touch UI state.
            // A teardown-racing host callback must not unwind a resource-owning thread.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.callback)()));
        }
    }
}

struct Shared {
    stop: AtomicBool,
    io_stop: AtomicBool,
    reader_abort: AtomicBool,
    output_eof: AtomicBool,
    owner_done: AtomicBool,
    finished: AtomicBool,
    released: AtomicBool,
    desired_size: AtomicU32,
    actual_size: AtomicU32,
    scrollback: AtomicUsize,
    modes: AtomicU8,
    revision: AtomicU64,
    status: Mutex<SessionStatus>,
    failure: Mutex<Option<String>>,
    latest: Mutex<Option<Arc<Snapshot>>>,
    wake: Arc<Wake>,
}

impl Shared {
    fn new(size: TerminalSize, wake: Arc<Wake>) -> Self {
        Self {
            stop: AtomicBool::new(false), io_stop: AtomicBool::new(false),
            reader_abort: AtomicBool::new(false), output_eof: AtomicBool::new(false),
            owner_done: AtomicBool::new(false), finished: AtomicBool::new(false), released: AtomicBool::new(false),
            desired_size: AtomicU32::new(size.packed()), actual_size: AtomicU32::new(size.packed()),
            scrollback: AtomicUsize::new(0), modes: AtomicU8::new(0), revision: AtomicU64::new(1),
            status: Mutex::new(SessionStatus::Starting), failure: Mutex::new(None),
            latest: Mutex::new(None), wake,
        }
    }

    fn set_status(&self, status: SessionStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
        self.revision.fetch_add(1, Ordering::Release);
    }

    fn fail(&self, message: String) {
        let mut failure = self.failure.lock().unwrap_or_else(|e| e.into_inner());
        if failure.is_none() { *failure = Some(message); }
        self.stop.store(true, Ordering::Release);
    }

    fn publish(&self, snapshot: Snapshot) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(snapshot));
        self.wake.signal();
    }
}

struct Session {
    id: SessionId,
    input: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
    seen_generation: u64,
}

pub struct TerminalService {
    sessions: [Option<Session>; 2],
    wake: Arc<Wake>,
    poll_start: usize,
}

impl TerminalService {
    // The callback must be nonblocking and remain safe after the window is destroyed.
    // A no-op callback plus timer polling is also supported.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            sessions: [None, None],
            wake: Arc::new(Wake { pending: AtomicBool::new(false), callback: Arc::new(wake) }),
            poll_start: 0,
        }
    }

    pub fn start(&mut self, kind: SessionKind, request: LaunchRequest, size: TerminalSize) -> Result<SessionId, ControlError> {
        size.validate()?;
        let slot = &mut self.sessions[kind.index()];
        if let Some(session) = slot { return Err(ControlError::Occupied(session.id)); }
        #[cfg(not(windows))]
        { let _ = request; return Err(ControlError::UnsupportedPlatform); }
        #[cfg(windows)]
        {
            let number = NEXT_SESSION.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| ControlError::Spawn("Session ID space exhausted".into()))?;
            let id = SessionId(number);
            let shared = Arc::new(Shared::new(size, self.wake.clone()));
            let worker_shared = shared.clone();
            let (input, input_rx) = mpsc::sync_channel(INPUT_QUEUE);
            std::thread::Builder::new().name(format!("terminal-owner-{number}")).spawn(move || {
                run_session(id, kind, request, size, worker_shared, input_rx);
            }).map_err(|error| ControlError::Spawn(error.to_string()))?;
            *slot = Some(Session { id, input, shared, seen_generation: 0 });
            Ok(id)
        }
    }

    pub fn session_id(&self, kind: SessionKind) -> Option<SessionId> {
        self.sessions[kind.index()].as_ref().map(|session| session.id)
    }

    fn session(&self, id: SessionId) -> Result<&Session, ControlError> {
        self.sessions.iter().flatten().find(|session| session.id == id).ok_or(ControlError::StaleSession)
    }

    pub fn input(&self, id: SessionId, bytes: &[u8]) -> Result<(), ControlError> {
        let session = self.session(id)?;
        if session.shared.stop.load(Ordering::Acquire) || session.shared.owner_done.load(Ordering::Acquire) {
            return Err(ControlError::Closed);
        }
        if bytes.len() > MAX_INPUT_BYTES { return Err(ControlError::InputTooLarge); }
        if bytes.is_empty() { return Ok(()); }
        session.input.try_send(bytes.to_vec()).map_err(|error| match error {
            TrySendError::Full(_) => ControlError::Busy,
            TrySendError::Disconnected(_) => ControlError::Closed,
        })
    }

    pub fn key(&self, id: SessionId, key: Key, modifiers: Modifiers) -> Result<(), ControlError> {
        let modes = InputModes::from_bits(self.session(id)?.shared.modes.load(Ordering::Acquire));
        self.input(id, &encode_key(key, modifiers, modes))
    }

    pub fn paste(&self, id: SessionId, text: &str) -> Result<(), ControlError> {
        if text.len() > MAX_INPUT_BYTES - 12 { return Err(ControlError::InputTooLarge); }
        let modes = InputModes::from_bits(self.session(id)?.shared.modes.load(Ordering::Acquire));
        self.input(id, &encode_paste(text, modes))
    }

    pub fn resize(&self, id: SessionId, size: TerminalSize) -> Result<(), ControlError> {
        size.validate()?;
        let shared = &self.session(id)?.shared;
        if shared.owner_done.load(Ordering::Acquire) { return Err(ControlError::Closed); }
        shared.desired_size.store(size.packed(), Ordering::Release);
        Ok(())
    }

    pub fn scrollback(&self, id: SessionId, offset: usize) -> Result<(), ControlError> {
        let shared = &self.session(id)?.shared;
        if shared.released.load(Ordering::Acquire) { return Err(ControlError::Closed); }
        shared.scrollback.store(offset.min(SCROLLBACK_LINES), Ordering::Release);
        shared.revision.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn stop(&self, id: SessionId) -> Result<(), ControlError> {
        self.session(id)?.shared.stop.store(true, Ordering::Release);
        Ok(())
    }

    // Removes only a fully reaped session. Stop + final event + remove precedes replacement.
    pub fn remove(&mut self, id: SessionId) -> Result<(), ControlError> {
        let index = self.sessions.iter().position(|s| s.as_ref().is_some_and(|s| s.id == id))
            .ok_or(ControlError::StaleSession)?;
        if !self.sessions[index].as_ref().unwrap().shared.finished.load(Ordering::Acquire) {
            return Err(ControlError::Busy);
        }
        self.sessions[index].as_ref().unwrap().shared.released.store(true, Ordering::Release);
        self.sessions[index] = None;
        Ok(())
    }

    // Never waits on the producer. None also means momentary mailbox contention.
    pub fn snapshot(&self, id: SessionId) -> Option<Arc<Snapshot>> {
        self.session(id).ok()?.shared.latest.try_lock().ok()?.clone()
    }

    // At most two latest-value mailboxes, never an unbounded output drain on the UI thread.
    // Intermediate frames/statuses can coalesce; the final status is retained until remove.
    pub fn poll(&mut self, budget: usize) -> Vec<TerminalEvent> {
        self.wake.pending.store(false, Ordering::Release);
        let mut events = Vec::with_capacity(budget.min(MAX_DRAIN_EVENTS));
        let mut pending = false;
        for step in 0..2 {
            let index = (self.poll_start + step) % 2;
            let Some(session) = &mut self.sessions[index] else { continue };
            match session.shared.latest.try_lock() {
                Ok(latest) => {
                    if let Some(snapshot) = latest.as_ref()
                        && snapshot.session_id == session.id
                        && snapshot.generation > session.seen_generation
                    {
                        if events.len() < budget.min(MAX_DRAIN_EVENTS) {
                            session.seen_generation = snapshot.generation;
                            events.push(TerminalEvent { session_id: session.id, generation: snapshot.generation, snapshot: snapshot.clone() });
                        } else { pending = true; }
                    }
                }
                Err(_) => pending = true,
            }
        }
        self.poll_start = (self.poll_start + 1) % 2;
        if pending { self.wake.signal(); }
        events
    }
}

impl Default for TerminalService {
    fn default() -> Self { Self::new(|| {}) }
}

impl Drop for TerminalService {
    fn drop(&mut self) {
        // Detached lifecycle owners retain their resources and perform all blocking cleanup.
        for session in self.sessions.iter().flatten() {
            session.shared.stop.store(true, Ordering::Release);
            session.shared.released.store(true, Ordering::Release);
        }
    }
}

#[cfg(windows)]
fn run_session(id: SessionId, kind: SessionKind, request: LaunchRequest, size: TerminalSize, shared: Arc<Shared>, input: Receiver<Vec<u8>>) {
    let (output_tx, output_rx) = mpsc::sync_channel(OUTPUT_QUEUE);
    let (reply_tx, reply_rx) = mpsc::sync_channel(REPLY_QUEUE);
    let model_shared = shared.clone();
    let model_thread = std::thread::Builder::new().name(format!("terminal-model-{}", id.0)).spawn(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            model_worker(id, kind, size, model_shared.clone(), output_rx, reply_tx);
        })) {
            model_shared.fail("Terminal parser worker panicked".into());
            std::panic::resume_unwind(panic);
        }
    });
    let model_thread = match model_thread {
        Ok(thread) => thread,
        Err(error) => {
            let status = SessionStatus::Failed(format!("Could not start terminal model: {error}"));
            shared.owner_done.store(true, Ordering::Release);
            shared.finished.store(true, Ordering::Release);
            shared.publish(TerminalModel::new(size).snapshot(id, kind, 1, status, 0));
            return;
        }
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let launch = match prepare_launch(&request) {
            Ok(launch) => launch,
            Err(error) => return SessionStatus::Failed(error),
        };
        if shared.stop.load(Ordering::Acquire) { return SessionStatus::Stopped; }
        platform::run(launch, size, shared.clone(), input, output_tx, reply_rx)
    })).unwrap_or_else(|_| SessionStatus::Failed("Terminal lifecycle worker panicked".into()));
    shared.set_status(outcome);
    shared.owner_done.store(true, Ordering::Release);
    if model_thread.join().is_err() {
        shared.finished.store(true, Ordering::Release);
        let status = SessionStatus::Failed("Terminal parser worker panicked".into());
        shared.publish(TerminalModel::new(size).snapshot(id, kind, u64::MAX, status, 0));
    }
}

fn model_worker(id: SessionId, kind: SessionKind, size: TerminalSize, shared: Arc<Shared>, output: Receiver<Vec<u8>>, replies: SyncSender<Vec<u8>>) {
    let mut model = TerminalModel::new(size);
    let mut generation = 0u64;
    let mut revision = 0;
    let mut size = size;
    let mut dirty = true;
    let mut disconnected = false;
    let mut last_publish = Instant::now() - Duration::from_secs(1);
    loop {
        let turn = Instant::now();
        let mut consumed = 0;
        // A finite per-turn budget gives status/scroll/resize work time during output floods.
        while consumed < 64 * 1024 && turn.elapsed() < Duration::from_millis(4) {
            let received = if consumed == 0 && !disconnected {
                output.recv_timeout(Duration::from_millis(12))
            } else {
                output.try_recv().map_err(|error| match error {
                    mpsc::TryRecvError::Empty => mpsc::RecvTimeoutError::Timeout,
                    mpsc::TryRecvError::Disconnected => mpsc::RecvTimeoutError::Disconnected,
                })
            };
            match received {
                Ok(bytes) => {
                    consumed += bytes.len();
                    model.process(&bytes);
                    shared.modes.store(model.modes().bits(), Ordering::Release);
                    match model.take_replies() {
                        Ok(pending) if !shared.io_stop.load(Ordering::Acquire) => {
                            for reply in pending {
                                if replies.try_send(reply).is_err() {
                                    if !shared.io_stop.load(Ordering::Acquire) {
                                        shared.fail("Terminal reply writer is unavailable or saturated".into());
                                    }
                                    break;
                                }
                            }
                        }
                        Err(error) if !shared.io_stop.load(Ordering::Acquire) => shared.fail(error.into()),
                        _ => {}
                    }
                    dirty = true;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => { disconnected = true; break; }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
            }
        }
        let actual_size = TerminalSize::unpack(shared.actual_size.load(Ordering::Acquire));
        if size != actual_size { model.resize(actual_size); size = actual_size; dirty = true; }
        let current_revision = shared.revision.load(Ordering::Acquire);
        if revision != current_revision { revision = current_revision; dirty = true; }
        let done = shared.owner_done.load(Ordering::Acquire) && disconnected;
        let final_frame = done && !shared.finished.load(Ordering::Acquire);
        if final_frame || (dirty && last_publish.elapsed() >= Duration::from_millis(32)) {
            generation = generation.saturating_add(1);
            let mut status = shared.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
            // A final event guarantees that remove is safe, not just that the process exited.
            if !done && status.is_final() { status = SessionStatus::Stopping; }
            let snapshot = model.snapshot(id, kind, generation, status, shared.scrollback.load(Ordering::Acquire));
            if done { shared.finished.store(true, Ordering::Release); }
            shared.publish(snapshot);
            dirty = false;
            last_publish = Instant::now();
        }
        // Retain the bounded model after exit so a stopped session's history can still scroll.
        if done && shared.released.load(Ordering::Acquire) { break; }
        if disconnected { std::thread::sleep(Duration::from_millis(10)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_session(id: u64, wake: Arc<Wake>) -> Session {
        let (input, _) = mpsc::sync_channel(1);
        Session { id: SessionId(id), input, shared: Arc::new(Shared::new(TerminalSize::default(), wake)), seen_generation: 0 }
    }

    #[test]
    fn terminal_stale_ids_and_coalesced_finite_poll() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        let mut service = TerminalService::new(move || { count.fetch_add(1, Ordering::Relaxed); });
        let shell = fake_session(10, service.wake.clone());
        let run = fake_session(11, service.wake.clone());
        let mut model = TerminalModel::new(TerminalSize::default());
        for generation in 1..=100 {
            shell.shared.publish(model.snapshot(shell.id, SessionKind::Shell, generation, SessionStatus::Running, 0));
        }
        run.shared.publish(model.snapshot(run.id, SessionKind::ManagedRun, 1, SessionStatus::Running, 0));
        service.sessions = [Some(shell), Some(run)];
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert_eq!(service.poll(1).len(), 1);
        assert_eq!(service.poll(1).len(), 1);
        assert!(service.poll(20).is_empty());
        assert_eq!(service.stop(SessionId(9)), Err(ControlError::StaleSession));
        assert_eq!(service.remove(SessionId(10)), Err(ControlError::Busy));
        service.sessions[0].as_ref().unwrap().shared.finished.store(true, Ordering::Release);
        service.remove(SessionId(10)).unwrap();
        assert!(service.snapshot(SessionId(10)).is_none());
        assert_eq!(service.key(SessionId(10), Key::Enter, Modifiers::default()), Err(ControlError::StaleSession));
        let replacement = fake_session(12, service.wake.clone());
        replacement.shared.publish(model.snapshot(SessionId(10), SessionKind::Shell, 500, SessionStatus::Running, 0));
        service.sessions[0] = Some(replacement);
        assert!(service.poll(2).is_empty());
        service.sessions[0].as_ref().unwrap().shared.publish(model.snapshot(SessionId(12), SessionKind::Shell, 1, SessionStatus::Running, 0));
        let events = service.poll(2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, SessionId(12));
        assert_eq!(events[0].generation, 1);
    }

    #[test]
    fn terminal_input_queue_is_bounded_and_stop_bypasses_it() {
        let mut service = TerminalService::default();
        let mut session = fake_session(20, service.wake.clone());
        let (tx, _rx) = mpsc::sync_channel(1);
        session.input = tx;
        service.sessions[0] = Some(session);
        service.input(SessionId(20), b"first").unwrap();
        assert_eq!(service.input(SessionId(20), b"second"), Err(ControlError::Busy));
        assert_eq!(service.input(SessionId(20), &vec![0; MAX_INPUT_BYTES + 1]), Err(ControlError::InputTooLarge));
        service.stop(SessionId(20)).unwrap();
        assert_eq!(service.input(SessionId(20), b"third"), Err(ControlError::Closed));
        assert!(TerminalSize::new(0, 80).is_err());
        assert!(TerminalSize::new(24, 401).is_err());
    }
}
