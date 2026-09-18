mod app;
mod panels;
mod workspace;
use app::{App, ExplorerEntry, ExplorerRow, SideView, Tab, WorkerMessage};
mod input;
mod language;
use language::LSP_EVENT_MESSAGE;
mod render;
mod window;
pub use window::run;

mod icons;

use icons::{AppIcons, IconSet, material_icon_for};
use lightline::clipboard;
use lightline::document::{Document, Pos};
use lightline::lsp::{
    self, Client as LspClient, Diagnostic as LspDiagnostic, Event as LspEvent,
    Language as LspLanguage,
};
use lightline::syntax::{Color, RustSyntax};
use lightline::workflow::{self, Change, DiffRow, SearchHit};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, CTRL_BREAK_EVENT, CTRL_C_EVENT, GetConsoleWindow,
    SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::SetScrollInfo;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{
    BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const RAIL: i32 = 152;
const SIDEBAR: i32 = 180;
const GUTTER: i32 = 62;
const TOP: i32 = 7;
const STATUS: i32 = 27;
const PAD: i32 = 10;
const TAB_HEIGHT: i32 = 38;
const BREADCRUMB_HEIGHT: i32 = 27;
const TAB_WIDTH: i32 = 180;
const EXPLORER_ROW: i32 = 24;
const EXPLORER_TOP: i32 = 78;
const RAIL_FIRST_ROW: i32 = 48;
const RAIL_ROW: i32 = 32;
const TRANSITION_MS: u128 = 150;
fn scaled(pixels: i32, dpi: u32, zoom: i32) -> i32 {
    ((pixels as i64 * dpi as i64 * zoom as i64 + 4800) / 9600) as i32
}
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
const EDITOR_BG: u32 = rgb(12, 21, 35);
const RAIL_BG: u32 = rgb(12, 20, 34);
const SIDEBAR_BG: u32 = rgb(15, 25, 41);
const TAB_BG: u32 = rgb(15, 25, 42);
const ACTIVE_BG: u32 = rgb(17, 29, 49);
const STATUS_BG: u32 = rgb(18, 31, 51);
const LINE_BG: u32 = rgb(21, 35, 57);
const SELECT_BG: u32 = rgb(48, 55, 112);
const EDGE: u32 = rgb(40, 58, 88);
const TEXT: u32 = rgb(218, 228, 248);
const MUTED: u32 = rgb(140, 164, 199);
const BLUE: u32 = rgb(94, 153, 255);
const VIOLET: u32 = rgb(149, 109, 255);
const TEAL: u32 = rgb(103, 220, 215);
const GREEN: u32 = rgb(111, 220, 163);
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
