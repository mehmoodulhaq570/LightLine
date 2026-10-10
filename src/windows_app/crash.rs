//! What happens when LightLine hits a bug (a panic). Every panic, on any
//! thread, is written to crash.log. One while handling a window message --
//! where it would otherwise end the process on the spot, since a panic can't
//! leave a Windows callback -- first saves unsaved work for recovery, then
//! says so and closes: the app's state may be half-updated, so it doesn't
//! carry on. The next start offers the work back (restore_session).

use super::*;
use std::io::Write;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};

// Set once a message handler has panicked: every later message goes to
// Windows' default handling, so nothing else runs on the damaged state while
// the closing notice is up.
static CRASHED: AtomicBool = AtomicBool::new(false);

/// Debug builds only: posting this makes the window's handler panic, to
/// check this path in the running app.
#[cfg(debug_assertions)]
pub(super) const CRASH_TEST_MESSAGE: u32 = WM_APP + 99;

fn log_path() -> Option<PathBuf> {
    lightline::settings::Settings::settings_dir().map(|dir| dir.join("crash.log"))
}

pub(super) fn install_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_log(info);
        default(info);
    }));
}

fn write_log(info: &PanicHookInfo) {
    let Some(path) = log_path() else {
        return;
    };
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|text| text.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let place = info
        .location()
        .map(|place| format!("{}:{}", place.file(), place.line()))
        .unwrap_or_default();
    let thread = std::thread::current();
    let report = format!(
        "==== {} UTC, LightLine {}\nthread '{}' panicked at {place}: {message}\n{}\n",
        utc_now(),
        option_env!("LIGHTLINE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
        thread.name().unwrap_or("unnamed"),
        std::backtrace::Backtrace::force_capture(),
    );
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(report.as_bytes());
    }
}

// "YYYY-MM-DD hh:mm:ss" for now, in UTC.
fn utc_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    format_utc(seconds)
}

fn format_utc(seconds: u64) -> String {
    let (days, time) = (seconds / 86_400, seconds % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        time / 3_600,
        time / 60 % 60,
        time % 60
    )
}

/// Runs a window message's handler; if it panics, saves unsaved work for
/// recovery, tells the user, and closes LightLine.
pub(super) fn guard(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    handle: impl FnOnce() -> LRESULT,
) -> LRESULT {
    if CRASHED.load(Ordering::Relaxed) {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    match catch_unwind(AssertUnwindSafe(handle)) {
        Ok(result) => result,
        Err(_) => {
            CRASHED.store(true, Ordering::Relaxed);
            close_after_crash(hwnd)
        }
    }
}

fn close_after_crash(hwnd: HWND) -> ! {
    // Unwinding dropped the handler's borrow of the app, unless an outer
    // handler (one running a dialog) still holds it.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App> };
    let saved = !ptr.is_null()
        && catch_unwind(AssertUnwindSafe(|| {
            unsafe { &*ptr }
                .try_borrow()
                .ok()
                .is_some_and(|app| app.save_recovery_now().is_ok())
        }))
        .unwrap_or(false);
    let log = log_path().map_or_else(|| "crash.log".into(), |path| path.display().to_string());
    let message = format!(
        "LightLine ran into a problem and has to close.\n\n{}\n\nDetails were written to {log}",
        if saved {
            "Your unsaved work was kept: LightLine offers it back when it starts again."
        } else {
            "Unsaved work from the last few seconds may be lost; anything older is \
             offered back when LightLine starts again."
        }
    );
    unsafe {
        MessageBoxW(
            hwnd,
            wide(&message).as_ptr(),
            wide("LightLine").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_times_are_readable_utc() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(format_utc(1_791_590_400 + 3_723), "2026-10-10 01:02:03");
    }
}
