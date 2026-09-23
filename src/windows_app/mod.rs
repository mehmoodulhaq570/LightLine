mod app;
mod binary_view;
mod dialog;
mod file_dialog;
mod git;
mod image_view;
mod panels;
mod session;
mod terminal;
mod workspace;
use app::{
    App, EditorView, ExplorerEntry, ExplorerInputState, ExplorerRow, Extension,
    ExtensionInstallKind, ExtensionsTab, GitAction, SideView, Tab, TerminalPane, TerminalTab,
    WorkerMessage, is_c_family_path, is_cpp_path,
};
mod debugger;
use debugger::DEBUG_EVENT_MESSAGE;
mod input;
mod language;
use language::LSP_EVENT_MESSAGE;
use terminal::TERMINAL_EVENT_MESSAGE;
mod render;
use render::WelcomeAction;
mod window;
pub use window::run;

mod color_theme_adapter;
mod icons;
mod theme;
use theme::Theme;

use icons::{AppIcons, GenericIcon, IconSet};
use lightline::clipboard;
use lightline::debug::{
    Command as DebugCommand, DebugClient, Event as DebugEvent, Scope as DebugScope,
    StackFrame as DebugFrame, Variable as DebugVariable,
};
use lightline::document::{Document, Pos};
use lightline::lsp::{
    self, Client as LspClient, CompletionItem as LspCompletionItem, Diagnostic as LspDiagnostic,
    Event as LspEvent, Language as LspLanguage,
};
use lightline::syntax::{Color, Syntax};
use lightline::terminal::{
    Cell, Color as TermColor, Key as TermKey, LaunchRequest, MAX_DRAIN_EVENTS,
    Modifiers as TermModifiers, SessionId, SessionKind, SessionStatus, Snapshot, TerminalService,
    TerminalSize,
};
pub use lightline::workflow::{self, Change, CommitEntry, DiffRow, DiffScope, RepoState, SearchHit};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, CTRL_BREAK_EVENT, CTRL_C_EVENT, GetConsoleWindow,
    SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::{SetScrollInfo, ShowScrollBar};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// Compact workbench proportions. The activity bar intentionally carries only
// icons; the Explorer owns the readable labels and project hierarchy.
const RAIL: i32 = 56;
const SIDEBAR: i32 = 286;
const GUTTER: i32 = 62;
const TOP: i32 = 5;
const STATUS: i32 = 29;
const PAD: i32 = 10;
const TAB_HEIGHT: i32 = 42;
const BREADCRUMB_HEIGHT: i32 = 30;
const TAB_WIDTH: i32 = 180;
const EXPLORER_ROW: i32 = 27;
const EXPLORER_TOP: i32 = 82;
const RAIL_FIRST_ROW: i32 = 58;
const RAIL_ROW: i32 = 52;
// Gutter between the floating side-panel / editor / terminal cards.
const CARD_GAP: i32 = 1;
const CARD_RADIUS: i32 = 4;
const TRANSITION_MS: u128 = 150;
fn scaled(pixels: i32, dpi: u32, zoom: i32) -> i32 {
    ((pixels as i64 * dpi as i64 * zoom as i64 + 4800) / 9600) as i32
}
// Windows' canonicalize() returns the `\\?\` extended-length form; strip it for display.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(text)
}
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Surface {
    fn new(reference: HDC, width: i32, height: i32) -> Option<Self> {
        if width <= 0 || height <= 0 {
            return None;
        }
        unsafe {
            let dc = CreateCompatibleDC(reference);
            if dc.is_null() {
                return None;
            }
            let bitmap = CreateCompatibleBitmap(reference, width, height);
            if bitmap.is_null() {
                DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                previous,
                width,
                height,
            })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

struct Transition {
    previous_frame: Surface,
    started: Instant,
    left: i32,
    top: i32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
