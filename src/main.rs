#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use lightline::clipboard;
    use lightline::document::{Document, Pos};
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
    use std::time::Instant;
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
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const RAIL: i32 = 132;
    const SIDEBAR: i32 = 200;
    const GUTTER: i32 = 62;
    const TOP: i32 = 7;
    const STATUS: i32 = 27;
    const PAD: i32 = 10;
    const TAB_HEIGHT: i32 = 38;
    const BREADCRUMB_HEIGHT: i32 = 27;
    const TAB_WIDTH: i32 = 180;
    const EXPLORER_ROW: i32 = 24;
    const EXPLORER_TOP: i32 = 78;
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
    macro_rules! icon_bytes {
        ($name:literal) => {
            include_bytes!(concat!("../assets/material-icon-theme/", $name, ".ico")) as &[u8]
        };
    }
    const MATERIAL_ICONS: [(&str, &[u8]); 22] = [
        ("file", icon_bytes!("file")),
        ("folder", icon_bytes!("folder")),
        ("folder-open", icon_bytes!("folder-open")),
        ("folder-src", icon_bytes!("folder-src")),
        ("folder-src-open", icon_bytes!("folder-src-open")),
        ("folder-docs", icon_bytes!("folder-docs")),
        ("folder-docs-open", icon_bytes!("folder-docs-open")),
        ("rust", icon_bytes!("rust")),
        ("toml", icon_bytes!("toml")),
        ("markdown", icon_bytes!("markdown")),
        ("json", icon_bytes!("json")),
        ("python", icon_bytes!("python")),
        ("c", icon_bytes!("c")),
        ("cpp", icon_bytes!("cpp")),
        ("html", icon_bytes!("html")),
        ("css", icon_bytes!("css")),
        ("javascript", icon_bytes!("javascript")),
        ("typescript", icon_bytes!("typescript")),
        ("git", icon_bytes!("git")),
        ("lock", icon_bytes!("lock")),
        ("yaml", icon_bytes!("yaml")),
        ("readme", icon_bytes!("readme")),
    ];
    static EDITOR_WINDOW: AtomicIsize = AtomicIsize::new(0);

    struct IconSet {
        handles: Vec<(&'static str, HICON)>,
    }

    impl IconSet {
        fn new(dpi: u32, zoom: i32) -> Self {
            let size = scaled(18, dpi, zoom);
            let handles = MATERIAL_ICONS
                .iter()
                .map(|&(name, data)| (name, Self::load_ico(data, size)))
                .collect();
            Self { handles }
        }

        fn load_ico(data: &[u8], size: i32) -> HICON {
            if data.len() < 6 || data[0..4] != [0, 0, 1, 0] {
                return null_mut();
            }
            let count = u16::from_le_bytes([data[4], data[5]]) as usize;
            let mut best: Option<(i32, usize, usize)> = None;
            for index in 0..count {
                let start = 6 + index * 16;
                if start + 16 > data.len() {
                    break;
                }
                let width = if data[start] == 0 {
                    256
                } else {
                    data[start] as i32
                };
                let length =
                    u32::from_le_bytes(data[start + 8..start + 12].try_into().unwrap()) as usize;
                let offset =
                    u32::from_le_bytes(data[start + 12..start + 16].try_into().unwrap()) as usize;
                if offset
                    .checked_add(length)
                    .is_none_or(|end| end > data.len())
                {
                    continue;
                }
                let score = (width - size).abs();
                if best.is_none_or(|(best_score, _, _)| score < best_score) {
                    best = Some((score, offset, length));
                }
            }
            let Some((_, offset, length)) = best else {
                return null_mut();
            };
            unsafe {
                CreateIconFromResourceEx(
                    data.as_ptr().add(offset),
                    length as u32,
                    1,
                    0x0003_0000,
                    size,
                    size,
                    0,
                )
            }
        }

        fn draw(&self, hdc: HDC, name: &str, x: i32, y: i32, size: i32) -> bool {
            let Some(&(_, icon)) = self.handles.iter().find(|(key, _)| *key == name) else {
                return false;
            };
            if icon.is_null() {
                return false;
            }
            unsafe { DrawIconEx(hdc, x, y, icon, size, size, 0, null_mut(), DI_NORMAL) != 0 }
        }
    }

    impl Drop for IconSet {
        fn drop(&mut self) {
            for (_, icon) in &self.handles {
                if !icon.is_null() {
                    unsafe { DestroyIcon(*icon) };
                }
            }
        }
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

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SideView {
        Files,
        Search,
        Review,
    }

    enum WorkerMessage {
        Files(PathBuf, Vec<PathBuf>),
        Search(PathBuf, String, Arc<AtomicBool>, Vec<SearchHit>),
        RunLine(PathBuf, String),
        Run(PathBuf, Result<(), String>),
        Changes(PathBuf, Result<Vec<Change>, String>),
        Diff(PathBuf, PathBuf, Result<Vec<DiffRow>, String>),
    }

    fn material_icon_for(path: &Path, is_dir: bool, expanded: bool) -> &'static str {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if is_dir {
            return match (file_name.as_str(), expanded) {
                ("src", true) => "folder-src-open",
                ("src", false) => "folder-src",
                ("docs", true) => "folder-docs-open",
                ("docs", false) => "folder-docs",
                (_, true) => "folder-open",
                _ => "folder",
            };
        }
        if file_name == "readme.md" {
            return "readme";
        }
        if file_name == ".gitignore" || file_name == ".gitattributes" {
            return "git";
        }
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => "rust",
            "toml" => "toml",
            "md" | "markdown" => "markdown",
            "json" | "jsonc" => "json",
            "py" | "pyw" => "python",
            "c" | "h" => "c",
            "cc" | "cpp" | "cxx" | "hpp" => "cpp",
            "html" | "htm" => "html",
            "css" => "css",
            "js" | "jsx" | "mjs" => "javascript",
            "ts" | "tsx" => "typescript",
            "lock" => "lock",
            "yaml" | "yml" => "yaml",
            _ => "file",
        }
    }

    #[cfg(test)]
    mod icon_tests {
        use super::*;

        #[test]
        fn bundled_material_icons_load_as_windows_icons() {
            for (name, data) in MATERIAL_ICONS {
                let icon = IconSet::load_ico(data, 24);
                assert!(!icon.is_null(), "failed to load {name}");
                unsafe { DestroyIcon(icon) };
            }
        }
    }

    unsafe extern "system" fn console_control(event: u32) -> i32 {
        if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
            let hwnd = EDITOR_WINDOW.load(Ordering::Relaxed) as HWND;
            if !hwnd.is_null() {
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            }
            return 1;
        }
        0
    }

    fn connect_parent_console(hwnd: HWND) {
        unsafe {
            let attached = AttachConsole(ATTACH_PARENT_PROCESS) != 0;
            let console = GetConsoleWindow();
            if attached || !console.is_null() {
                EDITOR_WINDOW.store(hwnd as isize, Ordering::Relaxed);
                // Launchers can pass down the inheritable "ignore Ctrl+C" setting.
                SetConsoleCtrlHandler(None, 0);
                SetConsoleCtrlHandler(Some(console_control), 1);
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    #[derive(Default)]
    struct EditorView {
        cursor: Pos,
        selection_anchor: Option<Pos>,
        first_line: usize,
    }

    struct Tab {
        document: Document,
        view: EditorView,
        syntax: Option<RustSyntax>,
    }

    #[derive(Clone)]
    struct ExplorerEntry {
        path: PathBuf,
        is_dir: bool,
    }

    struct ExplorerRow {
        entry: ExplorerEntry,
        depth: usize,
        expanded: bool,
    }

    impl Tab {
        fn new(document: Document) -> Self {
            let syntax = Self::is_rust(&document).then(RustSyntax::new);
            Self {
                document,
                view: EditorView::default(),
                syntax,
            }
        }

        fn is_rust(document: &Document) -> bool {
            document
                .path
                .as_deref()
                .and_then(Path::extension)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        }

        fn update_syntax_language(&mut self) {
            if Self::is_rust(&self.document) {
                if self.syntax.is_none() {
                    self.syntax = Some(RustSyntax::new());
                }
            } else {
                self.syntax = None;
            }
        }
    }

    struct App {
        tabs: Vec<Tab>,
        active: usize,
        tab_first: usize,
        font: HFONT,
        ui_font: HFONT,
        brand_font: HFONT,
        icons: IconSet,
        dpi: u32,
        zoom: i32,
        line_height: i32,
        backbuffer: Option<Surface>,
        transition: Option<Transition>,
        status: String,
        focused: bool,
        caret_on: bool,
        dragging: bool,
        find_mode: bool,
        find_query: String,
        pending_high_surrogate: Option<u16>,
        explorer_visible: bool,
        sidebar_width: i32,
        sidebar_from: i32,
        sidebar_target: i32,
        sidebar_started: Option<Instant>,
        explorer_first_row: usize,
        workspace_root: Option<PathBuf>,
        expanded_dirs: HashSet<PathBuf>,
        directory_cache: HashMap<PathBuf, Vec<ExplorerEntry>>,
        welcome: bool,
        side_view: SideView,
        quick_open: bool,
        quick_query: String,
        quick_selected: usize,
        quick_files: Vec<PathBuf>,
        quick_loading: bool,
        search_input: bool,
        project_query: String,
        search_results: Vec<SearchHit>,
        search_cancel: Option<Arc<AtomicBool>>,
        run_visible: bool,
        run_output: String,
        run_busy: bool,
        run_cancel: Option<Arc<AtomicBool>>,
        run_pid: Option<Arc<AtomicU32>>,
        output_focus: bool,
        changes: Vec<Change>,
        review_loading: bool,
        review_file: Option<PathBuf>,
        diff_rows: Vec<DiffRow>,
        diff_first: usize,
        panel_first: usize,
        panel_selected: usize,
        panel_focus: bool,
        output_scroll: usize,
        recent: Vec<PathBuf>,
        worker_tx: Sender<WorkerMessage>,
        worker_rx: Receiver<WorkerMessage>,
        pending_workers: usize,
    }

    impl App {
        fn font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
            let font_name = wide("Consolas");
            unsafe {
                CreateFontW(
                    -scaled(15, dpi, zoom),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                )
            }
        }

        fn ui_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
            let font_name = wide("Segoe UI");
            unsafe {
                CreateFontW(
                    -scaled(14, dpi, zoom),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                )
            }
        }

        fn brand_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
            let font_name = wide("Segoe UI Semibold");
            unsafe {
                CreateFontW(
                    -scaled(17, dpi, zoom),
                    0,
                    0,
                    0,
                    600,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                )
            }
        }

        fn new(hwnd: HWND) -> Self {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            let zoom = 100;
            let (worker_tx, worker_rx) = mpsc::channel();
            Self {
                tabs: vec![Tab::new(Document::new())],
                active: 0,
                tab_first: 0,
                font: Self::font_for_dpi(dpi, zoom),
                ui_font: Self::ui_font_for_dpi(dpi, zoom),
                brand_font: Self::brand_font_for_dpi(dpi, zoom),
                icons: IconSet::new(dpi, zoom),
                dpi,
                zoom,
                line_height: scaled(21, dpi, zoom),
                backbuffer: None,
                transition: None,
                status: "Ready".into(),
                focused: false,
                caret_on: true,
                dragging: false,
                find_mode: false,
                find_query: String::new(),
                pending_high_surrogate: None,
                explorer_visible: true,
                sidebar_width: SIDEBAR,
                sidebar_from: SIDEBAR,
                sidebar_target: SIDEBAR,
                sidebar_started: None,
                explorer_first_row: 0,
                workspace_root: None,
                expanded_dirs: HashSet::new(),
                directory_cache: HashMap::new(),
                welcome: true,
                side_view: SideView::Files,
                quick_open: false,
                quick_query: String::new(),
                quick_selected: 0,
                quick_files: Vec::new(),
                quick_loading: false,
                search_input: false,
                project_query: String::new(),
                search_results: Vec::new(),
                search_cancel: None,
                run_visible: false,
                run_output: String::new(),
                run_busy: false,
                run_cancel: None,
                run_pid: None,
                output_focus: false,
                changes: Vec::new(),
                review_loading: false,
                review_file: None,
                diff_rows: Vec::new(),
                diff_first: 0,
                panel_first: 0,
                panel_selected: 0,
                panel_focus: false,
                output_scroll: 0,
                recent: workflow::recent_workspaces(),
                worker_tx,
                worker_rx,
                pending_workers: 0,
            }
        }

        fn tab(&self) -> &Tab {
            &self.tabs[self.active]
        }
        fn tab_mut(&mut self) -> &mut Tab {
            &mut self.tabs[self.active]
        }
        fn doc(&self) -> &Document {
            &self.tab().document
        }
        fn doc_mut(&mut self) -> &mut Document {
            &mut self.tab_mut().document
        }
        fn view(&self) -> &EditorView {
            &self.tab().view
        }
        fn view_mut(&mut self) -> &mut EditorView {
            &mut self.tab_mut().view
        }
        fn editor_top(&self) -> i32 {
            self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT + TOP)
        }

        fn editor_left(&self) -> i32 {
            self.scale(RAIL + self.sidebar_width)
        }

        fn set_sidebar_visible(&mut self, hwnd: HWND, visible: bool) {
            let target = if visible { SIDEBAR } else { 0 };
            self.explorer_visible = visible;
            if self.sidebar_width == target {
                self.sidebar_target = target;
                self.sidebar_started = None;
                unsafe { KillTimer(hwnd, 5) };
            } else {
                self.sidebar_from = self.sidebar_width;
                self.sidebar_target = target;
                self.sidebar_started = Some(Instant::now());
                unsafe { SetTimer(hwnd, 5, 16, None) };
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn advance_sidebar(&mut self, hwnd: HWND) {
            let Some(started) = self.sidebar_started else {
                return;
            };
            let elapsed = started.elapsed().as_millis().min(160) as f32 / 160.0;
            let ease = 1.0 - (1.0 - elapsed).powi(3);
            self.sidebar_width = self.sidebar_from
                + ((self.sidebar_target - self.sidebar_from) as f32 * ease).round() as i32;
            let visible_tabs = self.visible_tab_count(hwnd);
            if self.active >= self.tab_first + visible_tabs {
                self.tab_first = self.active + 1 - visible_tabs;
            }
            if elapsed >= 1.0 {
                self.sidebar_width = self.sidebar_target;
                self.sidebar_started = None;
                unsafe { KillTimer(hwnd, 5) };
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn code_left(&self) -> i32 {
            self.editor_left() + self.scale(GUTTER + PAD)
        }

        fn load_directory(&mut self, path: &Path) {
            if self.directory_cache.contains_key(path) {
                return;
            }
            let mut entries: Vec<ExplorerEntry> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .take(400)
                .filter_map(|entry| {
                    if matches!(entry.file_name().to_str(), Some(".git" | "target")) {
                        return None;
                    }
                    let is_dir = entry.file_type().ok()?.is_dir();
                    Some(ExplorerEntry {
                        path: entry.path(),
                        is_dir,
                    })
                })
                .collect();
            entries.sort_by(|a, b| {
                let rank = |entry: &ExplorerEntry| {
                    if entry.is_dir && entry.path.file_name().is_some_and(|name| name == "src") {
                        0
                    } else if entry.is_dir {
                        1
                    } else {
                        2
                    }
                };
                rank(a).cmp(&rank(b)).then_with(|| {
                    a.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase()
                        .cmp(
                            &b.path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase(),
                        )
                })
            });
            self.directory_cache.insert(path.to_path_buf(), entries);
        }

        fn set_workspace_from_file(&mut self, path: &Path) {
            if self.workspace_root.is_some() {
                return;
            }
            if let Some(parent) = path.parent() {
                let folder = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                let root = folder
                    .ancestors()
                    .take(8)
                    .find(|dir| dir.join("Cargo.toml").is_file() || dir.join(".git").exists())
                    .unwrap_or(&folder)
                    .to_path_buf();
                self.workspace_root = Some(root.clone());
                self.expanded_dirs.insert(root.clone());
                self.load_directory(&root);
                workflow::remember_workspace(&root);
                self.recent = workflow::recent_workspaces();
            }
        }

        fn set_workspace(&mut self, hwnd: HWND, root: PathBuf) {
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            if !root.is_dir() {
                return;
            }
            self.workspace_root = Some(root.clone());
            self.directory_cache.clear();
            self.expanded_dirs.clear();
            self.expanded_dirs.insert(root.clone());
            self.explorer_first_row = 0;
            self.quick_files.clear();
            self.quick_loading = false;
            self.cancel_search();
            self.search_results.clear();
            self.changes.clear();
            self.review_loading = false;
            self.review_file = None;
            if let Some(cancel) = self.run_cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            self.run_pid = None;
            self.run_busy = false;
            self.run_visible = false;
            self.output_focus = false;
            self.run_output.clear();
            self.welcome = false;
            self.explorer_visible = true;
            self.sidebar_width = SIDEBAR;
            self.sidebar_from = SIDEBAR;
            self.sidebar_target = SIDEBAR;
            self.sidebar_started = None;
            unsafe { KillTimer(hwnd, 5) };
            self.side_view = SideView::Files;
            self.panel_focus = false;
            self.load_directory(&root);
            workflow::remember_workspace(&root);
            self.recent = workflow::recent_workspaces();
            self.status = format!(
                "Workspace: {}",
                root.file_name().unwrap_or_default().to_string_lossy()
            );
            self.show_active_tab(hwnd);
        }

        fn show_welcome(&mut self, hwnd: HWND) {
            self.cancel_search();
            self.welcome = true;
            self.quick_open = false;
            self.search_input = false;
            self.panel_focus = false;
            self.output_focus = false;
            self.find_mode = false;
            self.update_title(hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn folder_dialog(&self, hwnd: HWND) -> Option<PathBuf> {
            let mut display = [0u16; 260];
            let title = wide("Choose a LightLine workspace folder");
            let info = BROWSEINFOW {
                hwndOwner: hwnd,
                pszDisplayName: display.as_mut_ptr(),
                lpszTitle: title.as_ptr(),
                ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
                ..unsafe { zeroed() }
            };
            let id = unsafe { SHBrowseForFolderW(&info) };
            if id.is_null() {
                return None;
            }
            let mut path = [0u16; 260];
            let ok = unsafe { SHGetPathFromIDListW(id, path.as_mut_ptr()) };
            unsafe { CoTaskMemFree(id.cast()) };
            if ok == 0 {
                return None;
            }
            Some(PathBuf::from(String::from_utf16_lossy(
                &path[..path.iter().position(|ch| *ch == 0)?],
            )))
        }

        fn open_folder(&mut self, hwnd: HWND) {
            if let Some(root) = self.folder_dialog(hwnd) {
                self.set_workspace(hwnd, root);
            }
        }

        fn worker_started(&mut self, hwnd: HWND) {
            self.pending_workers += 1;
            unsafe { SetTimer(hwnd, 4, 60, None) };
        }

        fn show_quick_open(&mut self, hwnd: HWND) {
            self.quick_open = true;
            self.panel_focus = false;
            self.output_focus = false;
            self.quick_query.clear();
            self.quick_selected = 0;
            self.quick_loading = false;
            if let Some(root) = self.workspace_root.clone() {
                self.quick_files.clear();
                self.quick_loading = true;
                let tx = self.worker_tx.clone();
                self.worker_started(hwnd);
                std::thread::spawn(move || {
                    let _ = tx.send(WorkerMessage::Files(
                        root.clone(),
                        workflow::workspace_files(&root),
                    ));
                });
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn quick_commands(&self) -> Vec<(&'static str, u8)> {
            let query = self
                .quick_query
                .trim_start_matches('>')
                .trim()
                .to_ascii_lowercase();
            [
                ("Search in files", 0),
                ("Run Rust tests", 1),
                ("Review Git changes", 2),
                ("Open folder", 3),
                ("New file", 4),
            ]
            .into_iter()
            .filter(|(name, _)| name.to_ascii_lowercase().contains(&query))
            .collect()
        }

        fn quick_count(&self) -> usize {
            if self.quick_query.starts_with('>') {
                self.quick_commands().len()
            } else {
                self.quick_matches().len()
            }
        }

        fn activate_quick_item(&mut self, hwnd: HWND, index: usize) {
            if self.quick_query.starts_with('>') {
                let action = self.quick_commands().get(index).map(|(_, action)| *action);
                self.quick_open = false;
                self.backbuffer = None;
                match action {
                    Some(0) => self.open_project_search(hwnd),
                    Some(1) => self.run_project(hwnd),
                    Some(2) => self.show_review(hwnd),
                    Some(3) => self.open_folder(hwnd),
                    Some(4) => self.new_file(hwnd),
                    _ => {}
                }
            } else {
                let path = self.quick_matches().get(index).cloned();
                self.quick_open = false;
                if let Some(path) = path {
                    self.backbuffer = None;
                    self.open(hwnd, Some(path));
                }
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn new_file(&mut self, hwnd: HWND) {
            self.cancel_search();
            let from_welcome = self.welcome;
            self.welcome = false;
            self.search_input = false;
            self.panel_focus = false;
            self.output_focus = false;
            self.review_file = None;
            self.side_view = SideView::Files;
            if !from_welcome
                || self.doc().path.is_some()
                || self.doc().is_dirty()
                || !self.doc().line(0).is_empty()
            {
                if !from_welcome {
                    self.start_transition(hwnd);
                }
                self.tabs.push(Tab::new(Document::new()));
                self.active = self.tabs.len() - 1;
            }
            self.status = "New document".into();
            self.show_active_tab(hwnd);
        }

        fn open_project_search(&mut self, hwnd: HWND) {
            if self.workspace_root.is_none() {
                self.status = "Open a workspace to search files".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            self.welcome = false;
            self.set_sidebar_visible(hwnd, true);
            self.side_view = SideView::Search;
            self.review_file = None;
            self.search_input = true;
            self.panel_focus = true;
            self.output_focus = false;
            self.panel_first = 0;
            self.panel_selected = 0;
            self.update_title(hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn quick_matches(&self) -> Vec<PathBuf> {
            let query = self.quick_query.to_ascii_lowercase();
            self.quick_files
                .iter()
                .filter(|path| {
                    path.strip_prefix(self.workspace_root.as_deref().unwrap_or(Path::new("")))
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(&query)
                })
                .take(8)
                .cloned()
                .collect()
        }

        fn search_project(&mut self, hwnd: HWND) {
            let Some(root) = self.workspace_root.clone() else {
                self.status = "Open a workspace to search files".into();
                return;
            };
            let query = self.project_query.clone();
            if query.is_empty() {
                self.cancel_search();
                self.search_results.clear();
                return;
            }
            self.cancel_search();
            let cancel = Arc::new(AtomicBool::new(false));
            self.search_cancel = Some(cancel.clone());
            self.status = format!("Searching for {query}...");
            self.panel_selected = 0;
            self.panel_first = 0;
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let hits = workflow::search_workspace_with_cancel(&root, &query, &cancel);
                let _ = tx.send(WorkerMessage::Search(root, query, cancel, hits));
            });
        }

        fn cancel_search(&mut self) {
            if let Some(cancel) = self.search_cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
        }

        fn run_project(&mut self, hwnd: HWND) {
            let Some(root) = self.workspace_root.clone() else {
                self.status = "Open a Rust workspace to run tests".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            };
            self.welcome = false;
            self.run_visible = true;
            self.output_focus = true;
            if self.run_busy {
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            self.run_busy = true;
            let cancel = Arc::new(AtomicBool::new(false));
            let pid = Arc::new(AtomicU32::new(0));
            self.run_cancel = Some(cancel.clone());
            self.run_pid = Some(pid.clone());
            self.run_output = "$ cargo test --offline\n\n".into();
            self.output_scroll = 0;
            self.update_title(hwnd);
            self.keep_cursor_visible(hwnd);
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let (line_tx, line_rx) = mpsc::channel();
                let run_root = root.clone();
                let runner = std::thread::spawn(move || {
                    workflow::run_tests_stream(&run_root, line_tx, &cancel, &pid)
                });
                for line in line_rx {
                    let _ = tx.send(WorkerMessage::RunLine(root.clone(), line));
                }
                let result = runner
                    .join()
                    .unwrap_or_else(|_| Err("Test worker stopped unexpectedly".into()));
                let _ = tx.send(WorkerMessage::Run(root, result));
            });
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn stop_run(&mut self, hwnd: HWND) {
            if let Some(cancel) = &self.run_cancel {
                cancel.store(true, Ordering::Relaxed);
                self.status = "Stopping test command...".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }

        fn stop_run_before_close(&mut self) {
            if let Some(cancel) = &self.run_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
            if let Some(pid) = &self.run_pid {
                let pid = pid.load(Ordering::Relaxed);
                if pid != 0 {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/T", "/F", "/PID", &pid.to_string()])
                        .creation_flags(0x0800_0000)
                        .output();
                }
            }
        }

        fn show_review(&mut self, hwnd: HWND) {
            let Some(root) = self.workspace_root.clone() else {
                self.status = "Open a Git workspace to review changes".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            };
            self.welcome = false;
            self.cancel_search();
            self.search_input = false;
            self.output_focus = false;
            self.update_title(hwnd);
            self.set_sidebar_visible(hwnd, true);
            self.side_view = SideView::Review;
            self.panel_focus = true;
            self.panel_selected = 0;
            self.panel_first = 0;
            self.status = "Loading Git changes...".into();
            self.review_loading = true;
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let result = workflow::git_changes(&root);
                let _ = tx.send(WorkerMessage::Changes(root, result));
            });
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn show_diff(&mut self, hwnd: HWND, path: PathBuf) {
            let Some(root) = self.workspace_root.clone() else {
                return;
            };
            self.review_file = Some(path.clone());
            self.diff_rows.clear();
            self.diff_first = 0;
            self.status = format!("Reviewing {}", path.display());
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let result = workflow::git_diff(&root, &path);
                let _ = tx.send(WorkerMessage::Diff(root, path, result));
            });
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn poll_workers(&mut self, hwnd: HWND) {
            let mut received = false;
            while let Ok(message) = self.worker_rx.try_recv() {
                received = true;
                if !matches!(&message, WorkerMessage::RunLine(..)) {
                    self.pending_workers = self.pending_workers.saturating_sub(1);
                }
                match message {
                    WorkerMessage::RunLine(root, line)
                        if self.workspace_root.as_ref() == Some(&root) =>
                    {
                        if self.run_output.len() < 60_000 {
                            self.run_output.push_str(&line);
                            self.run_output.push('\n');
                        }
                    }
                    WorkerMessage::Files(root, files)
                        if self.workspace_root.as_ref() == Some(&root) =>
                    {
                        self.quick_loading = false;
                        self.quick_files = files;
                    }
                    WorkerMessage::Search(root, query, cancel, hits)
                        if self.workspace_root.as_ref() == Some(&root)
                            && self.project_query == query
                            && self
                                .search_cancel
                                .as_ref()
                                .is_some_and(|current| Arc::ptr_eq(current, &cancel)) =>
                    {
                        self.search_cancel = None;
                        self.status = format!("{} results for {}", hits.len(), query);
                        self.search_results = hits;
                    }
                    WorkerMessage::Run(root, result)
                        if self.workspace_root.as_ref() == Some(&root) =>
                    {
                        self.run_busy = false;
                        self.run_cancel = None;
                        self.run_pid = None;
                        match result {
                            Ok(()) => {
                                self.run_output.push_str("\nTests finished successfully.\n");
                                self.status = "Tests passed".into();
                            }
                            Err(error) => {
                                self.run_output.push_str(&format!("\n{error}\n"));
                                self.status = error;
                            }
                        }
                    }
                    WorkerMessage::Changes(root, result)
                        if self.workspace_root.as_ref() == Some(&root) =>
                    {
                        self.review_loading = false;
                        match result {
                            Ok(changes) => {
                                self.status = format!("{} changed files", changes.len());
                                self.changes = changes;
                            }
                            Err(error) => self.status = error,
                        }
                    }
                    WorkerMessage::Diff(root, path, result)
                        if self.workspace_root.as_ref() == Some(&root)
                            && self.review_file.as_ref() == Some(&path) =>
                    {
                        match result {
                            Ok(rows) => self.diff_rows = rows,
                            Err(error) => self.status = error,
                        }
                    }
                    _ => {}
                }
            }
            if self.pending_workers == 0 {
                unsafe { KillTimer(hwnd, 4) };
            }
            if received {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }

        fn reveal_file_in_explorer(&mut self, path: &Path) {
            let Some(root) = self.workspace_root.clone() else {
                return;
            };
            let Some(parent) = path.parent() else {
                return;
            };
            let Ok(relative) = parent.strip_prefix(&root) else {
                return;
            };
            let mut dir = root;
            for part in relative.components().take(8) {
                dir.push(part);
                self.expanded_dirs.insert(dir.clone());
                self.load_directory(&dir);
            }
        }

        fn explorer_rows(&self) -> Vec<ExplorerRow> {
            let mut rows = Vec::new();
            if let Some(root) = &self.workspace_root {
                self.append_explorer_rows(root, 0, &mut rows);
            }
            rows
        }

        fn append_explorer_rows(&self, dir: &Path, depth: usize, rows: &mut Vec<ExplorerRow>) {
            if depth > 8 || rows.len() >= 250 {
                return;
            }
            if let Some(entries) = self.directory_cache.get(dir) {
                for entry in entries {
                    if rows.len() >= 250 {
                        break;
                    }
                    let expanded = entry.is_dir && self.expanded_dirs.contains(&entry.path);
                    rows.push(ExplorerRow {
                        entry: entry.clone(),
                        depth,
                        expanded,
                    });
                    if expanded {
                        self.append_explorer_rows(&entry.path, depth + 1, rows);
                    }
                }
            }
        }

        fn syntax_changed(&mut self, line: usize) {
            if let Some(syntax) = &mut self.tab_mut().syntax {
                syntax.invalidate_from(line);
            }
        }

        fn advance_syntax(&mut self, hwnd: HWND) {
            let target = (self.view().first_line + self.visible_lines(hwnd))
                .min(self.doc().line_count().saturating_sub(1));
            let tab = self.tab_mut();
            let pending = tab
                .syntax
                .as_mut()
                .is_some_and(|syntax| !syntax.advance_to(&tab.document, target, 2048));
            unsafe {
                if pending {
                    SetTimer(hwnd, 2, 16, None);
                } else {
                    KillTimer(hwnd, 2);
                }
            }
        }

        fn tab_label(&self, index: usize) -> String {
            let doc = &self.tabs[index].document;
            let name = doc
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            format!("{}{}", name, if doc.is_dirty() { " *" } else { "" })
        }

        fn visible_tab_count(&self, hwnd: HWND) -> usize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            ((rect.right - self.editor_left()) / self.scale(TAB_WIDTH).max(1)).max(1) as usize
        }

        fn show_active_tab(&mut self, hwnd: HWND) {
            let count = self.visible_tab_count(hwnd);
            if self.active < self.tab_first {
                self.tab_first = self.active;
            } else if self.active >= self.tab_first + count {
                self.tab_first = self.active + 1 - count;
            }
            self.find_mode = false;
            self.dragging = false;
            self.update_title(hwnd);
            self.update_scrollbar(hwnd);
            self.advance_syntax(hwnd);
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
        }

        fn activate_tab(&mut self, hwnd: HWND, index: usize) {
            if index < self.tabs.len() {
                self.output_focus = false;
                if self.side_view == SideView::Search {
                    self.cancel_search();
                }
                if index != self.active {
                    self.start_transition(hwnd);
                }
                self.side_view = SideView::Files;
                self.review_file = None;
                self.search_input = false;
                self.panel_focus = false;
                self.active = index;
                self.show_active_tab(hwnd);
            }
        }

        fn same_path(a: &Path, b: &Path) -> bool {
            let absolute = |path: &Path| {
                std::fs::canonicalize(path)
                    .or_else(|_| {
                        let parent = path.parent().unwrap_or_else(|| Path::new("."));
                        std::fs::canonicalize(parent)
                            .map(|p| p.join(path.file_name().unwrap_or_default()))
                    })
                    .unwrap_or_else(|_| path.to_path_buf())
                    .to_string_lossy()
                    .to_lowercase()
            };
            absolute(a) == absolute(b)
        }

        fn close_tab(&mut self, hwnd: HWND, index: usize) {
            self.activate_tab(hwnd, index);
            if !self.can_discard(hwnd) {
                return;
            }
            self.start_transition(hwnd);
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.tabs.push(Tab::new(Document::new()));
                self.active = 0;
            } else {
                self.active = index.min(self.tabs.len() - 1);
            }
            self.status = "Ready".into();
            self.show_active_tab(hwnd);
        }

        fn can_close_window(&mut self, hwnd: HWND) -> bool {
            for index in 0..self.tabs.len() {
                if self.tabs[index].document.is_dirty() {
                    self.activate_tab(hwnd, index);
                    if !self.can_discard(hwnd) {
                        return false;
                    }
                }
            }
            true
        }

        fn scale(&self, pixels: i32) -> i32 {
            scaled(pixels, self.dpi, self.zoom)
        }

        fn set_dpi(&mut self, dpi: u32) {
            let dpi = dpi.max(96);
            if dpi == self.dpi {
                return;
            }
            self.set_metrics(dpi, self.zoom);
        }

        fn set_zoom(&mut self, hwnd: HWND, zoom: i32) {
            let zoom = zoom.clamp(60, 200);
            if zoom == self.zoom {
                return;
            }
            self.set_metrics(self.dpi, zoom);
            self.transition = None;
            unsafe { KillTimer(hwnd, 3) };
            let count = self.visible_tab_count(hwnd);
            if self.active >= self.tab_first + count {
                self.tab_first = self.active + 1 - count;
            }
            self.status = format!("Zoom: {}%", self.zoom);
            self.keep_cursor_visible(hwnd);
            self.update_scrollbar(hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }

        fn set_metrics(&mut self, dpi: u32, zoom: i32) {
            let font = Self::font_for_dpi(dpi, zoom);
            let ui_font = Self::ui_font_for_dpi(dpi, zoom);
            let brand_font = Self::brand_font_for_dpi(dpi, zoom);
            if font.is_null() || ui_font.is_null() || brand_font.is_null() {
                if !font.is_null() {
                    unsafe {
                        DeleteObject(font);
                    }
                }
                if !ui_font.is_null() {
                    unsafe {
                        DeleteObject(ui_font);
                    }
                }
                if !brand_font.is_null() {
                    unsafe {
                        DeleteObject(brand_font);
                    }
                }
                return;
            }
            unsafe {
                DeleteObject(self.font);
                DeleteObject(self.ui_font);
                DeleteObject(self.brand_font);
            }
            self.font = font;
            self.ui_font = ui_font;
            self.brand_font = brand_font;
            self.icons = IconSet::new(dpi, zoom);
            self.dpi = dpi;
            self.zoom = zoom;
            self.line_height = scaled(21, dpi, zoom);
        }

        fn start_transition(&mut self, hwnd: HWND) {
            let Some(backbuffer) = &self.backbuffer else {
                return;
            };
            let left = self.editor_left();
            let top = self.editor_top();
            let width = backbuffer.width - left;
            let height = backbuffer.height - self.scale(STATUS) - top;
            let dc = unsafe { GetDC(hwnd) };
            let snapshot = Surface::new(dc, width, height);
            unsafe { ReleaseDC(hwnd, dc) };
            let Some(snapshot) = snapshot else { return };
            unsafe {
                BitBlt(
                    snapshot.dc,
                    0,
                    0,
                    snapshot.width,
                    snapshot.height,
                    backbuffer.dc,
                    left,
                    top,
                    SRCCOPY,
                );
                SetTimer(hwnd, 3, 16, None);
            }
            self.transition = Some(Transition {
                previous_frame: snapshot,
                started: Instant::now(),
                left,
                top,
            });
        }

        fn cancel_transition(&mut self, hwnd: HWND) {
            if self.transition.take().is_some() {
                unsafe { KillTimer(hwnd, 3) };
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }

        fn visible_lines(&self, hwnd: HWND) -> usize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            ((rect.bottom
                - rect.top
                - self.editor_top()
                - self.scale(STATUS)
                - if self.run_visible { self.scale(210) } else { 0 })
            .max(1)
                / self.line_height)
                .max(1) as usize
        }

        fn update_scrollbar(&self, hwnd: HWND) {
            let visible = self.visible_lines(hwnd);
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: self
                    .doc()
                    .line_count()
                    .saturating_sub(1)
                    .min(i32::MAX as usize) as i32,
                nPage: visible as u32,
                nPos: self.view().first_line.min(i32::MAX as usize) as i32,
                nTrackPos: 0,
            };
            unsafe {
                SetScrollInfo(hwnd, SB_VERT, &info, 1);
            }
        }

        fn keep_cursor_visible(&mut self, hwnd: HWND) {
            let visible = self.visible_lines(hwnd);
            let line = self.view().cursor.line;
            if line < self.view().first_line {
                self.view_mut().first_line = line;
            }
            if line >= self.view().first_line + visible {
                self.view_mut().first_line = line + 1 - visible;
            }
            self.update_scrollbar(hwnd);
            self.caret_on = true;
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
        }

        fn update_title(&self, hwnd: HWND) {
            if self.welcome {
                unsafe { SetWindowTextW(hwnd, wide("LightLine").as_ptr()) };
                return;
            }
            let file = self
                .doc()
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            let title = format!(
                "{}{} — LightLine",
                file,
                if self.doc().is_dirty() { " *" } else { "" }
            );
            unsafe {
                SetWindowTextW(hwnd, wide(&title).as_ptr());
            }
        }

        fn refresh(&mut self, hwnd: HWND) {
            self.update_title(hwnd);
            self.keep_cursor_visible(hwnd);
            self.advance_syntax(hwnd);
        }

        fn selection_range(&self) -> Option<(Pos, Pos)> {
            let anchor = self.view().selection_anchor?;
            let cursor = self.view().cursor;
            if anchor == cursor {
                None
            } else if anchor < cursor {
                Some((anchor, cursor))
            } else {
                Some((cursor, anchor))
            }
        }

        fn move_cursor(&mut self, pos: Pos, extend: bool) {
            let cursor = self.view().cursor;
            if extend {
                self.view_mut().selection_anchor.get_or_insert(cursor);
            } else {
                self.view_mut().selection_anchor = None;
            }
            let pos = self.doc().clamp(pos);
            self.view_mut().cursor = pos;
        }

        fn replace_selection(&mut self, text: &str) {
            let (start, end) = self
                .selection_range()
                .unwrap_or((self.view().cursor, self.view().cursor));
            self.replace_range(start, end, text);
            self.view_mut().selection_anchor = None;
        }

        fn replace_range(&mut self, start: Pos, end: Pos, text: &str) {
            let cursor = self.doc_mut().replace(start, end, text);
            self.syntax_changed(start.line);
            self.view_mut().cursor = cursor;
        }

        fn copy_selection(&mut self, hwnd: HWND) -> bool {
            let Some((start, end)) = self.selection_range() else {
                return false;
            };
            let text = self.doc().text_range(start, end);
            match clipboard::copy(hwnd, &text) {
                Ok(()) => {
                    self.status = "Copied selection".into();
                    true
                }
                Err(error) => {
                    self.error(hwnd, &error);
                    false
                }
            }
        }

        fn find(&mut self, hwnd: HWND, forward: bool) {
            if self.find_query.is_empty() {
                self.status = "Find: enter a query with Ctrl+F".into();
                self.refresh(hwnd);
                return;
            }
            let match_at = if forward {
                self.doc()
                    .find_forward(self.view().cursor, &self.find_query)
            } else {
                let origin = self
                    .selection_range()
                    .map(|(start, _)| start)
                    .unwrap_or(self.view().cursor);
                self.doc().find_backward(origin, &self.find_query)
            };
            if let Some(start) = match_at {
                let end = Pos {
                    line: start.line,
                    byte: start.byte + self.find_query.len(),
                };
                self.view_mut().selection_anchor = Some(start);
                self.view_mut().cursor = end;
                self.status = format!("Found: {}", self.find_query);
            } else {
                self.status = format!("Not found: {}", self.find_query);
            }
            self.refresh(hwnd);
        }

        fn text_width(&self, hdc: HDC, text: &str) -> i32 {
            let expanded = text.replace('\t', "    ");
            let utf16: Vec<u16> = expanded.encode_utf16().collect();
            let mut size = SIZE::default();
            unsafe {
                GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, &mut size);
            }
            size.cx
        }

        fn caret_rect(&self, hwnd: HWND) -> RECT {
            unsafe {
                let hdc = GetDC(hwnd);
                let old = SelectObject(hdc, self.font);
                let line = self.doc().line(self.view().cursor.line);
                let x = self.code_left() + self.text_width(hdc, &line[..self.view().cursor.byte]);
                SelectObject(hdc, old);
                ReleaseDC(hwnd, hdc);
                let y = self.editor_top()
                    + (self.view().cursor.line as i64 - self.view().first_line as i64) as i32
                        * self.line_height;
                RECT {
                    left: x,
                    top: y,
                    right: x + self.scale(2).max(2),
                    bottom: y + self.line_height,
                }
            }
        }

        fn invalidate_caret(&self, hwnd: HWND) {
            let rect = self.caret_rect(hwnd);
            unsafe {
                InvalidateRect(hwnd, &rect, 0);
            }
        }

        fn fill(hdc: HDC, rect: RECT, color: u32) {
            unsafe {
                let brush = CreateSolidBrush(color);
                FillRect(hdc, &rect, brush);
                DeleteObject(brush);
            }
        }

        fn label(hdc: HDC, text: &str, x: i32, y: i32, color: u32, clip: RECT) {
            unsafe {
                let chars: Vec<u16> = text.encode_utf16().collect();
                SetTextColor(hdc, color);
                ExtTextOutW(
                    hdc,
                    x,
                    y,
                    ETO_CLIPPED,
                    &clip,
                    chars.as_ptr(),
                    chars.len() as u32,
                    null(),
                );
            }
        }

        fn chevron(&self, hdc: HDC, x: i32, y: i32, expanded: bool) {
            unsafe {
                let half = self.scale(4).max(4);
                let pen = CreatePen(PS_SOLID, self.scale(2).max(2), MUTED);
                if pen.is_null() {
                    return;
                }
                let previous = SelectObject(hdc, pen);
                if expanded {
                    MoveToEx(hdc, x - half, y - half / 2, null_mut());
                    LineTo(hdc, x, y + half / 2);
                    LineTo(hdc, x + half, y - half / 2);
                } else {
                    MoveToEx(hdc, x - half / 2, y - half, null_mut());
                    LineTo(hdc, x + half / 2, y);
                    LineTo(hdc, x - half / 2, y + half);
                }
                SelectObject(hdc, previous);
                DeleteObject(pen);
            }
        }

        fn paint_welcome(&self, hdc: HDC, rect: RECT) {
            Self::fill(hdc, rect, EDITOR_BG);
            let clip = rect;
            unsafe { SelectObject(hdc, self.brand_font) };
            Self::label(
                hdc,
                "✦  LightLine",
                self.scale(28),
                self.scale(24),
                TEXT,
                clip,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            let x = (rect.right / 2 - self.scale(250)).max(self.scale(28));
            Self::label(
                hdc,
                "PICK UP WHERE YOU LEFT OFF",
                x,
                self.scale(136),
                VIOLET,
                clip,
            );
            unsafe { SelectObject(hdc, self.brand_font) };
            Self::label(hdc, "Your quiet workbench", x, self.scale(170), TEXT, clip);
            unsafe { SelectObject(hdc, self.ui_font) };
            Self::label(
                hdc,
                "Open a file, choose a workspace, or start writing.",
                x,
                self.scale(207),
                MUTED,
                clip,
            );
            for (index, label) in ["Open file", "Open folder", "New file"].iter().enumerate() {
                let left = x + self.scale(index as i32 * 145);
                Self::fill(
                    hdc,
                    RECT {
                        left,
                        top: self.scale(251),
                        right: left + self.scale(135),
                        bottom: self.scale(289),
                    },
                    if index == 0 { SELECT_BG } else { ACTIVE_BG },
                );
                Self::label(
                    hdc,
                    label,
                    left + self.scale(14),
                    self.scale(260),
                    TEXT,
                    clip,
                );
            }
            Self::label(hdc, "RECENT WORKSPACES", x, self.scale(322), MUTED, clip);
            for (index, path) in self.recent.iter().take(5).enumerate() {
                let top = self.scale(351 + index as i32 * 49);
                Self::fill(
                    hdc,
                    RECT {
                        left: x,
                        top,
                        right: (x + self.scale(430)).min(rect.right - self.scale(20)),
                        bottom: top + self.scale(42),
                    },
                    ACTIVE_BG,
                );
                Self::label(
                    hdc,
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                    x + self.scale(12),
                    top + self.scale(7),
                    TEXT,
                    clip,
                );
                Self::label(
                    hdc,
                    &path.display().to_string(),
                    x + self.scale(115),
                    top + self.scale(7),
                    MUTED,
                    RECT {
                        left: x + self.scale(115),
                        top,
                        right: (x + self.scale(425)).min(rect.right),
                        bottom: top + self.scale(42),
                    },
                );
            }
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: rect.bottom - self.scale(STATUS),
                    right: rect.right,
                    bottom: rect.bottom,
                },
                STATUS_BG,
            );
            Self::label(
                hdc,
                &format!(
                    "{}  •  Ctrl+O file  •  Ctrl+N new  •  Ctrl+Shift+O folder",
                    self.status
                ),
                self.scale(16),
                rect.bottom - self.scale(STATUS) + self.scale(4),
                MUTED,
                clip,
            );
        }

        fn paint_side_panel(&self, hdc: HDC, editor_left: i32, editor_bottom: i32) {
            let left = self.scale(RAIL);
            let clip = RECT {
                left,
                top: 0,
                right: editor_left,
                bottom: editor_bottom,
            };
            let title = if self.side_view == SideView::Search {
                "SEARCH IN FILES"
            } else {
                "CHANGES"
            };
            Self::label(
                hdc,
                title,
                left + self.scale(16),
                self.scale(11),
                MUTED,
                clip,
            );
            Self::fill(
                hdc,
                RECT {
                    left,
                    top: self.scale(39),
                    right: editor_left,
                    bottom: self.scale(40),
                },
                EDGE,
            );
            if self.side_view == SideView::Search {
                Self::fill(
                    hdc,
                    RECT {
                        left: left + self.scale(8),
                        top: self.scale(47),
                        right: editor_left - self.scale(8),
                        bottom: self.scale(78),
                    },
                    ACTIVE_BG,
                );
                let query_label = if self.project_query.is_empty() && !self.search_input {
                    "Type query, press Enter".to_owned()
                } else {
                    format!(
                        "{}{}",
                        self.project_query,
                        if self.search_input && self.focused && self.caret_on {
                            "|"
                        } else {
                            ""
                        }
                    )
                };
                Self::label(
                    hdc,
                    &query_label,
                    left + self.scale(16),
                    self.scale(52),
                    if self.project_query.is_empty() {
                        MUTED
                    } else {
                        TEXT
                    },
                    clip,
                );
                Self::label(
                    hdc,
                    &if self.search_cancel.is_some() {
                        "SEARCHING...".to_owned()
                    } else {
                        format!("{} RESULTS", self.search_results.len())
                    },
                    left + self.scale(16),
                    self.scale(87),
                    MUTED,
                    clip,
                );
                for (index, hit) in self
                    .search_results
                    .iter()
                    .enumerate()
                    .skip(self.panel_first)
                {
                    let top = self.scale(113 + (index - self.panel_first) as i32 * 48);
                    if top >= editor_bottom {
                        break;
                    }
                    if self.panel_focus && index == self.panel_selected {
                        Self::fill(
                            hdc,
                            RECT {
                                left: left + self.scale(7),
                                top,
                                right: editor_left - self.scale(7),
                                bottom: top + self.scale(45),
                            },
                            SELECT_BG,
                        );
                    }
                    Self::label(
                        hdc,
                        &format!(
                            "{}:{}",
                            hit.path.file_name().unwrap_or_default().to_string_lossy(),
                            hit.line + 1
                        ),
                        left + self.scale(14),
                        top,
                        TEXT,
                        clip,
                    );
                    Self::label(
                        hdc,
                        &hit.preview,
                        left + self.scale(14),
                        top + self.scale(19),
                        MUTED,
                        RECT {
                            left: left + self.scale(14),
                            top,
                            right: editor_left - self.scale(8),
                            bottom: top + self.scale(46),
                        },
                    );
                }
            } else {
                Self::label(
                    hdc,
                    &if self.review_loading {
                        "Loading Git changes...".to_owned()
                    } else {
                        format!("{} changed files", self.changes.len())
                    },
                    left + self.scale(16),
                    self.scale(52),
                    MUTED,
                    clip,
                );
                for (index, change) in self.changes.iter().enumerate().skip(self.panel_first) {
                    let top = self.scale(86 + (index - self.panel_first) as i32 * EXPLORER_ROW);
                    if top >= editor_bottom {
                        break;
                    }
                    if self.review_file.as_ref() == Some(&change.path)
                        || self.panel_focus && index == self.panel_selected
                    {
                        Self::fill(
                            hdc,
                            RECT {
                                left: left + self.scale(7),
                                top,
                                right: editor_left - self.scale(7),
                                bottom: top + self.scale(EXPLORER_ROW - 2),
                            },
                            SELECT_BG,
                        );
                    }
                    Self::label(hdc, &change.status, left + self.scale(12), top, GREEN, clip);
                    Self::label(
                        hdc,
                        &change.path.display().to_string(),
                        left + self.scale(37),
                        top,
                        TEXT,
                        RECT {
                            left: left + self.scale(37),
                            top,
                            right: editor_left - self.scale(7),
                            bottom: top + self.scale(EXPLORER_ROW),
                        },
                    );
                }
            }
        }

        fn paint_diff(&self, hdc: HDC, left: i32, right: i32, bottom: i32) {
            let top = self.editor_top();
            let mid = left + (right - left) / 2;
            Self::fill(
                hdc,
                RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
                EDITOR_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left: mid,
                    top,
                    right: mid + 1,
                    bottom,
                },
                EDGE,
            );
            let name = self
                .review_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            Self::label(
                hdc,
                &format!("BEFORE  ·  {name}"),
                left + self.scale(16),
                top + self.scale(9),
                MUTED,
                RECT {
                    left,
                    top,
                    right: mid,
                    bottom: top + self.scale(36),
                },
            );
            Self::label(
                hdc,
                &format!("AFTER  ·  {name}"),
                mid + self.scale(16),
                top + self.scale(9),
                MUTED,
                RECT {
                    left: mid,
                    top,
                    right,
                    bottom: top + self.scale(36),
                },
            );
            unsafe { SelectObject(hdc, self.font) };
            for (index, row) in self.diff_rows.iter().enumerate().skip(self.diff_first) {
                let y = top + self.scale(43) + (index - self.diff_first) as i32 * self.line_height;
                if y >= bottom {
                    break;
                }
                if row.changed && row.before_number.is_some() {
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: y,
                            right: mid,
                            bottom: (y + self.line_height).min(bottom),
                        },
                        rgb(47, 31, 42),
                    );
                }
                if row.changed && row.after_number.is_some() {
                    Self::fill(
                        hdc,
                        RECT {
                            left: mid + 1,
                            top: y,
                            right,
                            bottom: (y + self.line_height).min(bottom),
                        },
                        rgb(24, 55, 50),
                    );
                }
                if let Some(number) = row.before_number {
                    Self::label(
                        hdc,
                        &number.to_string(),
                        left + self.scale(10),
                        y,
                        MUTED,
                        RECT {
                            left,
                            top: y,
                            right: mid,
                            bottom,
                        },
                    );
                }
                if let Some(number) = row.after_number {
                    Self::label(
                        hdc,
                        &number.to_string(),
                        mid + self.scale(10),
                        y,
                        MUTED,
                        RECT {
                            left: mid,
                            top: y,
                            right,
                            bottom,
                        },
                    );
                }
                Self::label(
                    hdc,
                    &row.before,
                    left + self.scale(50),
                    y,
                    TEXT,
                    RECT {
                        left: left + self.scale(50),
                        top: y,
                        right: mid - self.scale(8),
                        bottom,
                    },
                );
                Self::label(
                    hdc,
                    &row.after,
                    mid + self.scale(50),
                    y,
                    TEXT,
                    RECT {
                        left: mid + self.scale(50),
                        top: y,
                        right: right - self.scale(8),
                        bottom,
                    },
                );
            }
            unsafe { SelectObject(hdc, self.ui_font) };
            if self.diff_rows.is_empty() {
                Self::label(
                    hdc,
                    "No unstaged text changes in this file.",
                    left + self.scale(18),
                    top + self.scale(62),
                    MUTED,
                    RECT {
                        left,
                        top,
                        right,
                        bottom,
                    },
                );
            }
        }

        fn paint_search_preview(&self, hdc: HDC, left: i32, right: i32, bottom: i32) {
            if self.side_view != SideView::Search || !self.panel_focus || self.search_input {
                return;
            }
            let Some(hit) = self.search_results.get(self.panel_selected) else {
                return;
            };
            let width = self.scale(620).min(right - left - self.scale(30));
            if width < self.scale(250) {
                return;
            }
            let x = left + self.scale(15);
            let y = (bottom - self.scale(176)).max(self.editor_top() + self.scale(18));
            Self::fill(
                hdc,
                RECT {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + self.scale(152),
                },
                STATUS_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left: x,
                    top: y,
                    right: x + self.scale(3),
                    bottom: y + self.scale(152),
                },
                BLUE,
            );
            Self::label(
                hdc,
                &format!(
                    "PREVIEW  ·  {}:{}",
                    hit.path.file_name().unwrap_or_default().to_string_lossy(),
                    hit.line + 1
                ),
                x + self.scale(13),
                y + self.scale(8),
                TEXT,
                RECT {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + self.scale(30),
                },
            );
            unsafe { SelectObject(hdc, self.font) };
            for (index, (number, line)) in hit.context.iter().enumerate() {
                let top = y + self.scale(34) + index as i32 * self.scale(21);
                let color = if *number == hit.line + 1 {
                    GREEN
                } else {
                    MUTED
                };
                Self::label(
                    hdc,
                    &format!("{number:>4}  {line}"),
                    x + self.scale(12),
                    top,
                    color,
                    RECT {
                        left: x + self.scale(12),
                        top,
                        right: x + width - self.scale(10),
                        bottom: y + self.scale(150),
                    },
                );
            }
            unsafe { SelectObject(hdc, self.ui_font) };
        }

        fn paint_output(&self, hdc: HDC, left: i32, right: i32, bottom: i32) {
            let top = bottom - self.scale(210);
            Self::fill(
                hdc,
                RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
                SIDEBAR_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left,
                    top,
                    right,
                    bottom: top + self.scale(1),
                },
                EDGE,
            );
            Self::label(
                hdc,
                "OUTPUT  ·  cargo test",
                left + self.scale(16),
                top + self.scale(8),
                TEXT,
                RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
            );
            Self::label(
                hdc,
                "×",
                right - self.scale(28),
                top + self.scale(6),
                MUTED,
                RECT {
                    left: right - self.scale(28),
                    top,
                    right,
                    bottom: top + self.scale(33),
                },
            );
            if self.run_busy {
                Self::label(
                    hdc,
                    "Stop",
                    right - self.scale(87),
                    top + self.scale(6),
                    rgb(234, 159, 155),
                    RECT {
                        left: right - self.scale(90),
                        top,
                        right: right - self.scale(36),
                        bottom: top + self.scale(33),
                    },
                );
            }
            let lines: Vec<&str> = self.run_output.lines().collect();
            let visible = 8usize;
            let start = lines.len().saturating_sub(visible + self.output_scroll);
            for (index, line) in lines.iter().skip(start).take(visible).enumerate() {
                Self::label(
                    hdc,
                    line,
                    left + self.scale(18),
                    top + self.scale(39 + index as i32 * 20),
                    if line.contains("FAILED") || line.contains("error") {
                        rgb(234, 159, 155)
                    } else if line.contains("passed") || line.contains("ok") {
                        GREEN
                    } else {
                        TEXT
                    },
                    RECT {
                        left: left + self.scale(18),
                        top: top + self.scale(38),
                        right: right - self.scale(12),
                        bottom,
                    },
                );
            }
        }

        fn paint_quick_open(&self, hdc: HDC, rect: RECT) {
            if !self.quick_open {
                return;
            }
            let width = self.scale(560).min(rect.right - self.scale(30));
            let left = (rect.right - width) / 2;
            let top = self.scale(52);
            let bottom = top + self.scale(70 + 8 * 34);
            Self::fill(
                hdc,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom,
                },
                STATUS_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom: top + self.scale(2),
                },
                VIOLET,
            );
            Self::label(
                hdc,
                &format!(
                    "Quick Open  ›  {}{}",
                    self.quick_query,
                    if self.focused && self.caret_on {
                        "|"
                    } else {
                        ""
                    }
                ),
                left + self.scale(16),
                top + self.scale(11),
                TEXT,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom: top + self.scale(43),
                },
            );
            Self::label(
                hdc,
                "Type to filter  ·  Enter opens  ·  Esc closes",
                left + self.scale(16),
                top + self.scale(39),
                MUTED,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom,
                },
            );
            let items: Vec<String> = if self.quick_query.starts_with('>') {
                self.quick_commands()
                    .iter()
                    .map(|(name, _)| format!(">  {name}"))
                    .collect()
            } else {
                self.quick_matches()
                    .iter()
                    .map(|path| {
                        path.strip_prefix(self.workspace_root.as_deref().unwrap_or(Path::new("")))
                            .unwrap_or(path)
                            .display()
                            .to_string()
                    })
                    .collect()
            };
            for (index, label) in items.iter().enumerate() {
                let y = top + self.scale(68 + index as i32 * 34);
                if index == self.quick_selected {
                    Self::fill(
                        hdc,
                        RECT {
                            left: left + self.scale(8),
                            top: y - self.scale(2),
                            right: left + width - self.scale(8),
                            bottom: y + self.scale(30),
                        },
                        SELECT_BG,
                    );
                }
                Self::label(
                    hdc,
                    label,
                    left + self.scale(18),
                    y + self.scale(2),
                    TEXT,
                    RECT {
                        left: left + self.scale(18),
                        top: y,
                        right: left + width - self.scale(12),
                        bottom: y + self.scale(29),
                    },
                );
            }
            if items.is_empty() {
                Self::label(
                    hdc,
                    if self.quick_loading {
                        "Loading workspace files..."
                    } else if self.workspace_root.is_none() {
                        "Open a workspace first (Ctrl+Shift+O)"
                    } else {
                        "No matching files or commands"
                    },
                    left + self.scale(18),
                    top + self.scale(80),
                    MUTED,
                    RECT {
                        left,
                        top,
                        right: left + width,
                        bottom,
                    },
                );
            }
            if !self.quick_query.starts_with('>') {
                Self::label(
                    hdc,
                    "Type > for commands",
                    left + self.scale(18),
                    bottom - self.scale(28),
                    MUTED,
                    RECT {
                        left,
                        top,
                        right: left + width,
                        bottom,
                    },
                );
            }
        }

        fn paint(&mut self, hwnd: HWND) {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let window_dc = BeginPaint(hwnd, &mut ps);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                if self
                    .backbuffer
                    .as_ref()
                    .is_none_or(|buffer| buffer.width != rect.right || buffer.height != rect.bottom)
                {
                    self.backbuffer = Surface::new(window_dc, rect.right, rect.bottom);
                    self.transition = None;
                    KillTimer(hwnd, 3);
                }
                let hdc = self
                    .backbuffer
                    .as_ref()
                    .map_or(window_dc, |buffer| buffer.dc);
                let old_font = SelectObject(hdc, self.font);
                SelectObject(hdc, self.ui_font);
                SetBkMode(hdc, TRANSPARENT as i32);
                if self.welcome {
                    self.paint_welcome(hdc, rect);
                    self.paint_quick_open(hdc, rect);
                    SelectObject(hdc, old_font);
                    if hdc != window_dc {
                        BitBlt(window_dc, 0, 0, rect.right, rect.bottom, hdc, 0, 0, SRCCOPY);
                    }
                    EndPaint(hwnd, &ps);
                    return;
                }
                let editor_bottom = (rect.bottom - self.scale(STATUS)).max(0);
                let code_bottom =
                    editor_bottom - if self.run_visible { self.scale(210) } else { 0 };
                let editor_left = self.editor_left();
                let code_left = self.code_left();
                let bg = CreateSolidBrush(EDITOR_BG);
                let gutter_bg = CreateSolidBrush(EDITOR_BG);
                let status_bg = CreateSolidBrush(STATUS_BG);
                let selection_bg = CreateSolidBrush(SELECT_BG);
                let selection = self.selection_range();
                FillRect(
                    hdc,
                    &RECT {
                        left: editor_left,
                        top: 0,
                        right: rect.right,
                        bottom: editor_bottom,
                    },
                    bg,
                );
                FillRect(
                    hdc,
                    &RECT {
                        left: editor_left,
                        top: self.scale(TAB_HEIGHT),
                        right: editor_left + self.scale(GUTTER),
                        bottom: editor_bottom,
                    },
                    gutter_bg,
                );
                let tab_bg = CreateSolidBrush(TAB_BG);
                let active_bg = CreateSolidBrush(ACTIVE_BG);
                FillRect(
                    hdc,
                    &RECT {
                        left: editor_left,
                        top: 0,
                        right: rect.right,
                        bottom: self.scale(TAB_HEIGHT),
                    },
                    tab_bg,
                );
                Self::fill(
                    hdc,
                    RECT {
                        left: 0,
                        top: 0,
                        right: self.scale(RAIL),
                        bottom: editor_bottom,
                    },
                    RAIL_BG,
                );
                if self.sidebar_width > 0 {
                    Self::fill(
                        hdc,
                        RECT {
                            left: self.scale(RAIL),
                            top: 0,
                            right: editor_left,
                            bottom: editor_bottom,
                        },
                        SIDEBAR_BG,
                    );
                }
                Self::fill(
                    hdc,
                    RECT {
                        left: editor_left,
                        top: self.scale(TAB_HEIGHT),
                        right: rect.right,
                        bottom: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT),
                    },
                    ACTIVE_BG,
                );
                Self::fill(
                    hdc,
                    RECT {
                        left: editor_left,
                        top: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT - 1),
                        right: rect.right,
                        bottom: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT),
                    },
                    EDGE,
                );
                let path_part = self
                    .doc()
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .map(|part| part.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Editor".into());
                Self::label(
                    hdc,
                    &format!("{}  ›  {}", path_part, self.tab_label(self.active)),
                    editor_left + self.scale(18),
                    self.scale(TAB_HEIGHT + 3),
                    MUTED,
                    RECT {
                        left: editor_left,
                        top: self.scale(TAB_HEIGHT),
                        right: rect.right,
                        bottom: self.editor_top(),
                    },
                );
                let tab_width = self.scale(TAB_WIDTH);
                let tab_height = self.scale(TAB_HEIGHT);
                for slot in 0..self.visible_tab_count(hwnd) {
                    let index = self.tab_first + slot;
                    if index >= self.tabs.len() {
                        break;
                    }
                    let left = editor_left + slot as i32 * tab_width;
                    if left >= rect.right {
                        break;
                    }
                    let bounds = RECT {
                        left,
                        top: 0,
                        right: (left + tab_width).min(rect.right),
                        bottom: tab_height,
                    };
                    if index == self.active {
                        FillRect(hdc, &bounds, active_bg);
                        Self::fill(
                            hdc,
                            RECT {
                                left,
                                top: 0,
                                right: bounds.right,
                                bottom: self.scale(2),
                            },
                            VIOLET,
                        );
                    }
                    let tab_icon = self.tabs[index]
                        .document
                        .path
                        .as_deref()
                        .map(|path| material_icon_for(path, false, false))
                        .unwrap_or("file");
                    self.icons.draw(
                        hdc,
                        tab_icon,
                        left + self.scale(11),
                        self.scale(9),
                        self.scale(18),
                    );
                    let label = self.tab_label(index);
                    let chars: Vec<u16> = label.encode_utf16().collect();
                    SetTextColor(hdc, if index == self.active { TEXT } else { MUTED });
                    let clip = RECT {
                        left: left + self.scale(37),
                        top: 0,
                        right: (left + tab_width - self.scale(30)).min(rect.right),
                        bottom: tab_height,
                    };
                    ExtTextOutW(
                        hdc,
                        clip.left,
                        self.scale(5),
                        ETO_CLIPPED,
                        &clip,
                        chars.as_ptr(),
                        chars.len() as u32,
                        null(),
                    );
                    let close = wide("×");
                    TextOutW(
                        hdc,
                        left + tab_width - self.scale(23),
                        self.scale(5),
                        close.as_ptr(),
                        1,
                    );
                }
                if editor_left
                    + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                    + self.scale(12)
                    < rect.right - self.scale(207)
                {
                    Self::label(
                        hdc,
                        "⌕  Quick Open  Ctrl+P",
                        rect.right - self.scale(207),
                        self.scale(7),
                        MUTED,
                        RECT {
                            left: rect.right - self.scale(207),
                            top: 0,
                            right: rect.right,
                            bottom: self.scale(TAB_HEIGHT),
                        },
                    );
                }
                let rail_clip = RECT {
                    left: 0,
                    top: 0,
                    right: self.scale(RAIL),
                    bottom: editor_bottom,
                };
                SelectObject(hdc, self.brand_font);
                Self::label(hdc, "✦", self.scale(14), self.scale(7), VIOLET, rail_clip);
                Self::label(
                    hdc,
                    "LightLine",
                    self.scale(36),
                    self.scale(7),
                    TEXT,
                    rail_clip,
                );
                SelectObject(hdc, self.ui_font);
                Self::fill(
                    hdc,
                    RECT {
                        left: 0,
                        top: self.scale(38),
                        right: self.scale(RAIL),
                        bottom: self.scale(39),
                    },
                    EDGE,
                );
                if self.explorer_visible && self.side_view == SideView::Files {
                    Self::fill(
                        hdc,
                        RECT {
                            left: self.scale(7),
                            top: self.scale(46),
                            right: self.scale(RAIL - 7),
                            bottom: self.scale(80),
                        },
                        ACTIVE_BG,
                    );
                }
                if self.sidebar_width > 0 {
                    Self::fill(
                        hdc,
                        RECT {
                            left: 0,
                            top: self.scale(if self.side_view == SideView::Files {
                                48
                            } else if self.side_view == SideView::Search {
                                90
                            } else {
                                174
                            }),
                            right: self.scale(3),
                            bottom: self.scale(if self.side_view == SideView::Files {
                                78
                            } else if self.side_view == SideView::Search {
                                120
                            } else {
                                204
                            }),
                        },
                        if self.side_view == SideView::Review {
                            VIOLET
                        } else {
                            BLUE
                        },
                    );
                }
                self.icons.draw(
                    hdc,
                    "folder-open",
                    self.scale(17),
                    self.scale(52),
                    self.scale(18),
                );
                Self::label(
                    hdc,
                    "Explorer",
                    self.scale(42),
                    self.scale(53),
                    if self.explorer_visible && self.side_view == SideView::Files {
                        TEXT
                    } else {
                        MUTED
                    },
                    rail_clip,
                );
                Self::label(hdc, "⌕", self.scale(18), self.scale(95), MUTED, rail_clip);
                Self::label(
                    hdc,
                    "Search",
                    self.scale(42),
                    self.scale(95),
                    if self.explorer_visible && self.side_view == SideView::Search {
                        TEXT
                    } else {
                        MUTED
                    },
                    rail_clip,
                );
                Self::label(hdc, "▶", self.scale(18), self.scale(137), GREEN, rail_clip);
                Self::label(
                    hdc,
                    "Run",
                    self.scale(42),
                    self.scale(137),
                    if self.run_visible { TEXT } else { MUTED },
                    rail_clip,
                );
                Self::label(hdc, "◇", self.scale(18), self.scale(179), VIOLET, rail_clip);
                Self::label(
                    hdc,
                    "Review",
                    self.scale(42),
                    self.scale(179),
                    if self.side_view == SideView::Review {
                        TEXT
                    } else {
                        MUTED
                    },
                    rail_clip,
                );
                if let Some(root) = &self.workspace_root {
                    let name = root.file_name().unwrap_or_default().to_string_lossy();
                    Self::label(
                        hdc,
                        "WORKSPACE",
                        self.scale(16),
                        editor_bottom - self.scale(60),
                        MUTED,
                        rail_clip,
                    );
                    Self::label(
                        hdc,
                        &name,
                        self.scale(16),
                        editor_bottom - self.scale(37),
                        TEXT,
                        rail_clip,
                    );
                }
                let sidebar_state = SaveDC(hdc);
                IntersectClipRect(hdc, self.scale(RAIL), 0, editor_left, editor_bottom);
                if self.sidebar_width > 0 && self.side_view != SideView::Files {
                    self.paint_side_panel(hdc, editor_left, editor_bottom);
                }
                if self.sidebar_width > 0 && self.side_view == SideView::Files {
                    let sidebar_clip = RECT {
                        left: self.scale(RAIL),
                        top: 0,
                        right: editor_left,
                        bottom: editor_bottom,
                    };
                    Self::label(
                        hdc,
                        "FILES",
                        self.scale(RAIL + 16),
                        self.scale(11),
                        MUTED,
                        sidebar_clip,
                    );
                    Self::fill(
                        hdc,
                        RECT {
                            left: self.scale(RAIL),
                            top: self.scale(39),
                            right: editor_left,
                            bottom: self.scale(40),
                        },
                        EDGE,
                    );
                    if let Some(root) = &self.workspace_root {
                        let root_name = root
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| root.display().to_string());
                        self.icons.draw(
                            hdc,
                            "folder-open",
                            self.scale(RAIL + 16),
                            self.scale(48),
                            self.scale(18),
                        );
                        Self::label(
                            hdc,
                            &root_name,
                            self.scale(RAIL + 39),
                            self.scale(49),
                            TEXT,
                            sidebar_clip,
                        );
                        for (row, item) in self
                            .explorer_rows()
                            .iter()
                            .enumerate()
                            .skip(self.explorer_first_row)
                        {
                            let top = self.scale(
                                EXPLORER_TOP
                                    + (row - self.explorer_first_row) as i32 * EXPLORER_ROW,
                            );
                            if top >= editor_bottom {
                                break;
                            }
                            let selected = self
                                .doc()
                                .path
                                .as_deref()
                                .is_some_and(|path| path == item.entry.path);
                            if selected {
                                Self::fill(
                                    hdc,
                                    RECT {
                                        left: self.scale(RAIL + 7),
                                        top,
                                        right: editor_left - self.scale(8),
                                        bottom: top + self.scale(EXPLORER_ROW - 2),
                                    },
                                    SELECT_BG,
                                );
                            }
                            let name = item
                                .entry
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let left = self.scale(RAIL + 16 + item.depth.min(6) as i32 * 13);
                            if item.entry.is_dir {
                                self.chevron(
                                    hdc,
                                    left + self.scale(5),
                                    top + self.scale(EXPLORER_ROW / 2),
                                    item.expanded,
                                );
                            }
                            let icon_name = material_icon_for(
                                &item.entry.path,
                                item.entry.is_dir,
                                item.expanded,
                            );
                            if !self.icons.draw(
                                hdc,
                                icon_name,
                                left + self.scale(12),
                                top + self.scale(2),
                                self.scale(18),
                            ) {
                                Self::fill(
                                    hdc,
                                    RECT {
                                        left: left + self.scale(16),
                                        top: top + self.scale(8),
                                        right: left + self.scale(22),
                                        bottom: top + self.scale(14),
                                    },
                                    MUTED,
                                );
                            }
                            Self::label(
                                hdc,
                                &name,
                                left + self.scale(36),
                                top + self.scale(1),
                                if selected {
                                    TEXT
                                } else if item.entry.is_dir {
                                    MUTED
                                } else {
                                    rgb(185, 205, 230)
                                },
                                RECT {
                                    left: left + self.scale(36),
                                    top,
                                    right: editor_left - self.scale(10),
                                    bottom: top + self.scale(EXPLORER_ROW),
                                },
                            );
                        }
                    } else {
                        Self::label(
                            hdc,
                            "Open a file to browse",
                            self.scale(RAIL + 16),
                            self.scale(49),
                            MUTED,
                            sidebar_clip,
                        );
                        Self::label(
                            hdc,
                            "its folder  (Ctrl+O)",
                            self.scale(RAIL + 16),
                            self.scale(73),
                            MUTED,
                            sidebar_clip,
                        );
                    }
                }
                RestoreDC(hdc, sidebar_state);
                if self.side_view == SideView::Review && self.review_file.is_some() {
                    self.paint_diff(hdc, editor_left, rect.right, code_bottom);
                } else {
                    let visible = self.visible_lines(hwnd) + 1;
                    SelectObject(hdc, self.font);
                    let space_width = self.text_width(hdc, " ").max(1);
                    let guide_brush = CreateSolidBrush(EDGE);
                    for row in 0..visible {
                        let index = self.view().first_line + row;
                        if index >= self.doc().line_count() {
                            break;
                        }
                        let y = self.editor_top() + row as i32 * self.line_height;
                        if y >= code_bottom {
                            break;
                        }
                        if index == self.view().cursor.line {
                            Self::fill(
                                hdc,
                                RECT {
                                    left: editor_left,
                                    top: y,
                                    right: rect.right,
                                    bottom: (y + self.line_height).min(code_bottom),
                                },
                                LINE_BG,
                            );
                        }
                        let number = format!("{}", index + 1);
                        let num: Vec<u16> = number.encode_utf16().collect();
                        SetTextColor(
                            hdc,
                            if index == self.view().cursor.line {
                                TEXT
                            } else {
                                MUTED
                            },
                        );
                        let number_clip = RECT {
                            left: editor_left,
                            top: y,
                            right: editor_left + self.scale(GUTTER),
                            bottom: code_bottom,
                        };
                        ExtTextOutW(
                            hdc,
                            editor_left + self.scale(12),
                            y,
                            ETO_CLIPPED,
                            &number_clip,
                            num.as_ptr(),
                            num.len() as u32,
                            null(),
                        );
                        let source = self.doc().line(index);
                        let indent_columns = source
                            .chars()
                            .take_while(|ch| *ch == ' ' || *ch == '\t')
                            .take(64)
                            .map(|ch| if ch == '\t' { 4 } else { 1 })
                            .sum::<usize>();
                        for level in 1..=(indent_columns / 4).min(8) {
                            let guide_x =
                                code_left + level as i32 * 4 * space_width - self.scale(4);
                            if guide_x < rect.right {
                                FillRect(
                                    hdc,
                                    &RECT {
                                        left: guide_x,
                                        top: y,
                                        right: guide_x + 1,
                                        bottom: (y + self.line_height).min(code_bottom),
                                    },
                                    guide_brush,
                                );
                            }
                        }
                        if let Some((start, end)) = selection
                            && index >= start.line
                            && index <= end.line
                            && !(index == end.line && end.byte == 0)
                        {
                            let from = if index == start.line { start.byte } else { 0 };
                            let to = if index == end.line {
                                end.byte
                            } else {
                                source.len()
                            };
                            let x1 = code_left + self.text_width(hdc, &source[..from]);
                            let x2 = code_left
                                + self.text_width(hdc, &source[..to])
                                + if index < end.line { self.scale(8) } else { 0 };
                            if x2 > x1 && x1 < rect.right {
                                FillRect(
                                    hdc,
                                    &RECT {
                                        left: x1,
                                        top: y,
                                        right: x2.min(rect.right),
                                        bottom: (y + self.line_height).min(code_bottom),
                                    },
                                    selection_bg,
                                );
                            }
                        }
                        let line = source.replace('\t', "    ");
                        let chars: Vec<u16> = line.encode_utf16().collect();
                        SetTextColor(hdc, TEXT);
                        let clip = RECT {
                            left: code_left,
                            top: y,
                            right: rect.right,
                            bottom: code_bottom,
                        };
                        ExtTextOutW(
                            hdc,
                            code_left,
                            y,
                            ETO_CLIPPED,
                            &clip,
                            chars.as_ptr(),
                            chars.len() as u32,
                            null(),
                        );
                        if source.len() <= 16_384
                            && let Some(syntax) = &self.tab().syntax
                        {
                            for span in syntax.spans(self.doc(), index) {
                                let color = match span.color {
                                    Color::Comment => MUTED,
                                    Color::String => GREEN,
                                    Color::Keyword => BLUE,
                                    Color::Type => TEAL,
                                    Color::Number => rgb(248, 180, 130),
                                    Color::Macro => VIOLET,
                                };
                                SetTextColor(hdc, color);
                                let left = code_left + self.text_width(hdc, &source[..span.start]);
                                let text = source[span.start..span.end].replace('\t', "    ");
                                let chars: Vec<u16> = text.encode_utf16().collect();
                                ExtTextOutW(
                                    hdc,
                                    left,
                                    y,
                                    ETO_CLIPPED,
                                    &clip,
                                    chars.as_ptr(),
                                    chars.len() as u32,
                                    null(),
                                );
                            }
                        }
                    }
                    DeleteObject(guide_brush);
                    if self.focused && self.caret_on {
                        let line = self.doc().line(self.view().cursor.line);
                        let x = code_left + self.text_width(hdc, &line[..self.view().cursor.byte]);
                        let y = self.editor_top()
                            + (self.view().cursor.line as i64 - self.view().first_line as i64)
                                as i32
                                * self.line_height;
                        if y >= self.editor_top() && y < code_bottom && x < rect.right {
                            let caret = CreateSolidBrush(BLUE);
                            FillRect(
                                hdc,
                                &RECT {
                                    left: x,
                                    top: y,
                                    right: x + self.scale(2).max(2),
                                    bottom: (y + self.line_height).min(code_bottom),
                                },
                                caret,
                            );
                            DeleteObject(caret);
                        }
                    }
                }
                self.paint_search_preview(hdc, editor_left, rect.right, code_bottom);
                if self.run_visible {
                    self.paint_output(hdc, editor_left, rect.right, editor_bottom);
                }
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: editor_bottom,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                    status_bg,
                );
                SelectObject(hdc, self.ui_font);
                let right_label =
                    if self.side_view == SideView::Review && self.review_file.is_some() {
                        format!("Git review     {} lines", self.diff_rows.len())
                    } else {
                        format!(
                            "Ln {}, Col {}     UTF-8     {}",
                            self.view().cursor.line + 1,
                            self.doc().line(self.view().cursor.line)[..self.view().cursor.byte]
                                .chars()
                                .count()
                                + 1,
                            self.doc()
                                .path
                                .as_deref()
                                .and_then(Path::extension)
                                .map(|ext| ext.to_string_lossy().to_uppercase())
                                .unwrap_or_else(|| "TEXT".into())
                        )
                    };
                let right_width = self.text_width(hdc, &right_label);
                let right_x = (rect.right - right_width - self.scale(16)).max(self.scale(16));
                Self::label(
                    hdc,
                    &self.status,
                    self.scale(14),
                    editor_bottom + self.scale(4),
                    TEXT,
                    RECT {
                        left: self.scale(14),
                        top: editor_bottom,
                        right: (right_x - self.scale(24)).max(self.scale(14)),
                        bottom: rect.bottom,
                    },
                );
                Self::label(
                    hdc,
                    &right_label,
                    right_x,
                    editor_bottom + self.scale(4),
                    MUTED,
                    RECT {
                        left: right_x,
                        top: editor_bottom,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                );
                DeleteObject(bg);
                DeleteObject(gutter_bg);
                DeleteObject(status_bg);
                DeleteObject(selection_bg);
                DeleteObject(tab_bg);
                DeleteObject(active_bg);
                if let Some(transition) = &self.transition {
                    let elapsed = transition.started.elapsed().as_millis();
                    if elapsed < TRANSITION_MS
                        && transition.left == editor_left
                        && transition.top == self.editor_top()
                        && transition.previous_frame.width == rect.right - editor_left
                        && transition.previous_frame.height == editor_bottom - self.editor_top()
                    {
                        let left = editor_left;
                        let top = self.editor_top();
                        let width = (rect.right - left).max(0);
                        let height = (editor_bottom - top).max(0);
                        AlphaBlend(
                            hdc,
                            left,
                            top,
                            width,
                            height,
                            transition.previous_frame.dc,
                            0,
                            0,
                            width,
                            height,
                            BLENDFUNCTION {
                                BlendOp: AC_SRC_OVER as u8,
                                BlendFlags: 0,
                                SourceConstantAlpha: ((TRANSITION_MS - elapsed) * 255
                                    / TRANSITION_MS)
                                    as u8,
                                AlphaFormat: 0,
                            },
                        );
                    } else {
                        self.transition = None;
                        KillTimer(hwnd, 3);
                    }
                }
                SelectObject(hdc, self.ui_font);
                self.paint_quick_open(hdc, rect);
                SelectObject(hdc, old_font);
                if hdc != window_dc {
                    BitBlt(window_dc, 0, 0, rect.right, rect.bottom, hdc, 0, 0, SRCCOPY);
                }
                EndPaint(hwnd, &ps);
            }
        }

        fn error(&mut self, hwnd: HWND, error: &impl std::fmt::Display) {
            self.status = error.to_string();
            unsafe {
                MessageBoxW(
                    hwnd,
                    wide(&self.status).as_ptr(),
                    wide("LightLine").as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
            self.refresh(hwnd);
        }

        fn dialog(&self, hwnd: HWND, save: bool) -> Option<PathBuf> {
            let mut buffer = [0u16; 32768];
            if save && let Some(path) = &self.doc().path {
                let name: Vec<u16> = path.file_name()?.to_string_lossy().encode_utf16().collect();
                buffer[..name.len()].copy_from_slice(&name);
            }
            let filter = wide("Text files\0*.txt;*.rs;*.py;*.c;*.cpp;*.h;*.md\0All files\0*.*\0");
            let mut dialog: OPENFILENAMEW = unsafe { zeroed() };
            dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
            dialog.hwndOwner = hwnd;
            dialog.lpstrFilter = filter.as_ptr();
            dialog.lpstrFile = buffer.as_mut_ptr();
            dialog.nMaxFile = buffer.len() as u32;
            dialog.Flags = OFN_EXPLORER
                | OFN_PATHMUSTEXIST
                | if save {
                    OFN_OVERWRITEPROMPT
                } else {
                    OFN_FILEMUSTEXIST
                };
            let ok = unsafe {
                if save {
                    GetSaveFileNameW(&mut dialog)
                } else {
                    GetOpenFileNameW(&mut dialog)
                }
            };
            if ok == 0 {
                return None;
            }
            Some(PathBuf::from(String::from_utf16_lossy(
                &buffer[..buffer.iter().position(|c| *c == 0)?],
            )))
        }

        fn save(&mut self, hwnd: HWND, save_as: bool) -> bool {
            let path = if save_as || self.doc().path.is_none() {
                match self.dialog(hwnd, true) {
                    Some(path) => path,
                    None => return false,
                }
            } else {
                self.doc().path.clone().unwrap()
            };
            if self.tabs.iter().enumerate().any(|(index, tab)| {
                index != self.active
                    && tab
                        .document
                        .path
                        .as_deref()
                        .is_some_and(|other| Self::same_path(other, &path))
            }) {
                self.error(hwnd, &"That file is already open in another tab");
                return false;
            }
            match self.doc_mut().save(&path) {
                Ok(()) => {
                    self.tab_mut().update_syntax_language();
                    self.set_workspace_from_file(&path);
                    self.reveal_file_in_explorer(&path);
                    if let Some(parent) = path.parent() {
                        self.directory_cache.remove(parent);
                        if self.expanded_dirs.contains(parent) {
                            self.load_directory(parent);
                        }
                    }
                    self.status = format!(
                        "Saved {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.refresh(hwnd);
                    true
                }
                Err(error) => {
                    self.error(hwnd, &error);
                    false
                }
            }
        }

        fn can_discard(&mut self, hwnd: HWND) -> bool {
            if !self.doc().is_dirty() {
                return true;
            }
            let answer = unsafe {
                MessageBoxW(
                    hwnd,
                    wide(&format!("Save changes to {}?", self.tab_label(self.active))).as_ptr(),
                    wide("LightLine").as_ptr(),
                    MB_YESNOCANCEL | MB_ICONQUESTION,
                )
            };
            if answer == IDYES {
                self.save(hwnd, false)
            } else {
                answer == IDNO
            }
        }

        fn open(&mut self, hwnd: HWND, path: Option<PathBuf>) {
            let Some(path) = path.or_else(|| self.dialog(hwnd, false)) else {
                return;
            };
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            if let Some(index) = self.tabs.iter().position(|tab| {
                tab.document
                    .path
                    .as_deref()
                    .is_some_and(|open| Self::same_path(open, &path))
            }) {
                self.activate_tab(hwnd, index);
                self.status = format!(
                    "Already open: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                return;
            }
            match Document::open(path.clone()) {
                Ok(document) => {
                    self.output_focus = false;
                    let from_welcome = self.welcome;
                    self.welcome = false;
                    self.quick_open = false;
                    if from_welcome {
                        self.side_view = SideView::Files;
                        self.review_file = None;
                        self.search_input = false;
                    }
                    if !from_welcome {
                        self.start_transition(hwnd);
                    }
                    if self.tabs.len() == 1
                        && self.doc().path.is_none()
                        && !self.doc().is_dirty()
                        && self.doc().line(0).is_empty()
                    {
                        self.tabs[0] = Tab::new(document);
                        self.active = 0;
                    } else {
                        self.tabs.push(Tab::new(document));
                        self.active = self.tabs.len() - 1;
                    }
                    self.set_workspace_from_file(&path);
                    self.reveal_file_in_explorer(&path);
                    self.status = format!(
                        "Opened {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.show_active_tab(hwnd);
                }
                Err(error) => self.error(hwnd, &error),
            }
        }

        fn key(&mut self, hwnd: HWND, key: u32) -> bool {
            let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
            let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
            if ctrl && key == 0x43 && self.output_focus {
                self.stop_run(hwnd);
                return true;
            }
            if self.welcome && key == VK_ESCAPE as u32 && self.workspace_root.is_some() {
                self.welcome = false;
                self.show_active_tab(hwnd);
                return true;
            }
            if self.quick_open {
                match key {
                    x if x == VK_ESCAPE as u32 => self.quick_open = false,
                    x if x == VK_UP as u32 => {
                        self.quick_selected = self.quick_selected.saturating_sub(1)
                    }
                    x if x == VK_DOWN as u32 => {
                        self.quick_selected =
                            (self.quick_selected + 1).min(self.quick_count().saturating_sub(1))
                    }
                    x if x == VK_BACK as u32 => {
                        self.quick_query.pop();
                        self.quick_selected = 0;
                    }
                    x if x == VK_RETURN as u32 => {
                        self.activate_quick_item(hwnd, self.quick_selected);
                    }
                    _ if !ctrl => return false,
                    _ => return true,
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
            if self.search_input {
                match key {
                    x if x == VK_ESCAPE as u32 => {
                        self.search_input = false;
                        self.cancel_search();
                        self.panel_focus = false;
                        self.set_sidebar_visible(hwnd, false);
                    }
                    x if x == VK_RETURN as u32 => {
                        self.search_input = false;
                        self.search_project(hwnd);
                    }
                    x if x == VK_BACK as u32 => {
                        self.project_query.pop();
                        self.search_results.clear();
                    }
                    _ if !ctrl => return false,
                    _ => {}
                }
                if key == VK_ESCAPE as u32 || key == VK_RETURN as u32 || key == VK_BACK as u32 {
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return true;
                }
            }
            if self.panel_focus && !ctrl {
                let count = if self.side_view == SideView::Search {
                    self.search_results.len()
                } else {
                    self.changes.len()
                };
                match key {
                    x if x == VK_UP as u32 => {
                        self.panel_selected = self.panel_selected.saturating_sub(1)
                    }
                    x if x == VK_DOWN as u32 => {
                        self.panel_selected = (self.panel_selected + 1).min(count.saturating_sub(1))
                    }
                    x if x == VK_RETURN as u32 => {
                        if self.side_view == SideView::Search {
                            if let Some(hit) = self.search_results.get(self.panel_selected).cloned()
                            {
                                self.panel_focus = false;
                                self.open(hwnd, Some(hit.path));
                                self.move_cursor(
                                    Pos {
                                        line: hit.line,
                                        byte: hit.byte,
                                    },
                                    false,
                                );
                                self.keep_cursor_visible(hwnd);
                            }
                        } else if let Some(change) = self.changes.get(self.panel_selected).cloned()
                        {
                            self.panel_focus = false;
                            self.show_diff(hwnd, change.path);
                        }
                        return true;
                    }
                    _ => {}
                }
                if key == VK_UP as u32 || key == VK_DOWN as u32 {
                    let visible = if self.side_view == SideView::Search {
                        10
                    } else {
                        20
                    };
                    if self.panel_selected < self.panel_first {
                        self.panel_first = self.panel_selected;
                    }
                    if self.panel_selected >= self.panel_first + visible {
                        self.panel_first = self.panel_selected + 1 - visible;
                    }
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return true;
                }
            }
            if self.side_view == SideView::Review
                && self.review_file.is_some()
                && !self.panel_focus
                && !ctrl
            {
                match key {
                    x if x == VK_UP as u32 => self.diff_first = self.diff_first.saturating_sub(1),
                    x if x == VK_DOWN as u32 => {
                        self.diff_first =
                            (self.diff_first + 1).min(self.diff_rows.len().saturating_sub(1))
                    }
                    x if x == VK_ESCAPE as u32 => {
                        self.review_file = None;
                        self.panel_focus = true;
                    }
                    _ => return true,
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
            if self.find_mode {
                match key {
                    x if x == VK_ESCAPE as u32 => {
                        self.find_mode = false;
                        self.status = "Ready".into();
                        self.refresh(hwnd);
                        return true;
                    }
                    x if x == VK_BACK as u32 || x == VK_RETURN as u32 => return true,
                    _ => {}
                }
            }
            if ctrl {
                let cursor = self.view().cursor;
                match key {
                    0x48 if shift => {
                        self.show_welcome(hwnd);
                        return true;
                    }
                    0x50 => {
                        self.show_quick_open(hwnd);
                        return true;
                    }
                    0x4f if shift => {
                        self.open_folder(hwnd);
                        return true;
                    }
                    0x46 if shift => {
                        self.open_project_search(hwnd);
                        return true;
                    }
                    0x47 if shift => {
                        self.show_review(hwnd);
                        return true;
                    }
                    0x42 if shift => {
                        self.run_project(hwnd);
                        return true;
                    }
                    x if x == VK_OEM_PLUS as u32 || x == VK_ADD as u32 => {
                        self.set_zoom(hwnd, self.zoom + 20);
                        return true;
                    }
                    x if x == VK_OEM_MINUS as u32 || x == VK_SUBTRACT as u32 => {
                        self.set_zoom(hwnd, self.zoom - 20);
                        return true;
                    }
                    0x30 => {
                        self.set_zoom(hwnd, 100);
                        return true;
                    }
                    x if x == VK_NUMPAD0 as u32 => {
                        self.set_zoom(hwnd, 100);
                        return true;
                    }
                    0x41 => {
                        self.view_mut().selection_anchor = Some(Pos::default());
                        let end = self.doc().end();
                        self.view_mut().cursor = end;
                    }
                    0x43 => {
                        self.copy_selection(hwnd);
                    }
                    0x58 => {
                        if self.copy_selection(hwnd) {
                            self.replace_selection("");
                        }
                    }
                    0x56 => match clipboard::paste(hwnd) {
                        Ok(Some(text)) => self.replace_selection(&text),
                        Ok(None) => {}
                        Err(error) => self.error(hwnd, &error),
                    },
                    0x4e => {
                        self.new_file(hwnd);
                        return true;
                    }
                    0x46 => {
                        self.search_input = false;
                        self.panel_focus = false;
                        self.find_mode = true;
                        self.find_query.clear();
                        self.status = "Find: ".into();
                    }
                    0x42 => {
                        if self.side_view == SideView::Search {
                            self.cancel_search();
                        }
                        self.side_view = SideView::Files;
                        self.panel_focus = false;
                        self.set_sidebar_visible(hwnd, !self.explorer_visible);
                        self.show_active_tab(hwnd);
                        return true;
                    }
                    0x4f => {
                        self.open(hwnd, None);
                        return true;
                    }
                    0x53 => {
                        self.save(hwnd, shift);
                    }
                    0x57 => {
                        self.close_tab(hwnd, self.active);
                        return true;
                    }
                    x if x == VK_TAB as u32 => {
                        let next = if shift {
                            (self.active + self.tabs.len() - 1) % self.tabs.len()
                        } else {
                            (self.active + 1) % self.tabs.len()
                        };
                        self.activate_tab(hwnd, next);
                        return true;
                    }
                    x if x == VK_PRIOR as u32 => {
                        self.activate_tab(hwnd, self.active.saturating_sub(1));
                        return true;
                    }
                    x if x == VK_NEXT as u32 => {
                        self.activate_tab(hwnd, (self.active + 1).min(self.tabs.len() - 1));
                        return true;
                    }
                    0x5a if shift => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().redo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    0x5a => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().undo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    0x59 => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().redo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    x if x == VK_HOME as u32 => self.move_cursor(Pos::default(), shift),
                    x if x == VK_END as u32 => self.move_cursor(self.doc().end(), shift),
                    x if x == VK_LEFT as u32 => {
                        let target = self.doc().previous_word(cursor);
                        self.move_cursor(target, shift);
                    }
                    x if x == VK_RIGHT as u32 => {
                        let target = self.doc().next_word(cursor);
                        self.move_cursor(target, shift);
                    }
                    x if x == VK_BACK as u32 => {
                        if self.selection_range().is_some() {
                            self.replace_selection("");
                        } else {
                            let previous = self.doc().previous_word(cursor);
                            self.replace_range(previous, cursor, "");
                        }
                    }
                    x if x == VK_DELETE as u32 => {
                        if self.selection_range().is_some() {
                            self.replace_selection("");
                        } else {
                            let next = self.doc().next_word(cursor);
                            self.replace_range(cursor, next, "");
                        }
                    }
                    _ => return false,
                }
                self.refresh(hwnd);
                return true;
            }
            let cursor = self.view().cursor;
            match key {
                x if x == VK_F3 as u32 => {
                    self.find_mode = false;
                    self.find(hwnd, !shift);
                    return true;
                }
                x if x == VK_ESCAPE as u32 => {
                    if self.run_visible {
                        self.run_visible = false;
                        self.output_focus = false;
                        self.keep_cursor_visible(hwnd);
                        return true;
                    }
                    if self.side_view != SideView::Files {
                        if self.side_view == SideView::Search {
                            self.cancel_search();
                        }
                        self.side_view = SideView::Files;
                        self.set_sidebar_visible(hwnd, false);
                        self.review_file = None;
                        self.keep_cursor_visible(hwnd);
                        return true;
                    }
                    self.view_mut().selection_anchor = None;
                }
                x if x == VK_LEFT as u32 => {
                    let target = if !shift {
                        self.selection_range().map(|(start, _)| start)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| self.doc().previous(cursor));
                    self.move_cursor(target, shift);
                }
                x if x == VK_RIGHT as u32 => {
                    let target = if !shift {
                        self.selection_range().map(|(_, end)| end)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| self.doc().next(cursor));
                    self.move_cursor(target, shift);
                }
                x if x == VK_UP as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line.saturating_sub(1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_DOWN as u32 => {
                    self.move_cursor(
                        Pos {
                            line: (cursor.line + 1).min(self.doc().line_count() - 1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_PRIOR as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line.saturating_sub(self.visible_lines(hwnd)),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_NEXT as u32 => {
                    self.move_cursor(
                        Pos {
                            line: (cursor.line + self.visible_lines(hwnd))
                                .min(self.doc().line_count() - 1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_HOME as u32 => self.move_cursor(
                    Pos {
                        line: cursor.line,
                        byte: 0,
                    },
                    shift,
                ),
                x if x == VK_END as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line,
                            byte: self.doc().line(cursor.line).len(),
                        },
                        shift,
                    );
                }
                x if x == VK_BACK as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let previous = self.doc().previous(cursor);
                        self.replace_range(previous, cursor, "");
                    }
                }
                x if x == VK_DELETE as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let next = self.doc().next(cursor);
                        self.replace_range(cursor, next, "");
                    }
                }
                _ => return false,
            }
            self.refresh(hwnd);
            true
        }

        fn character(&mut self, hwnd: HWND, unit: u16) {
            if unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
                return;
            }
            if self.output_focus && !self.quick_open && !self.search_input {
                return;
            }
            if self.quick_open || self.search_input {
                if unit >= 32
                    && unit != 127
                    && let Some(ch) = char::from_u32(unit as u32)
                {
                    if self.quick_open {
                        self.quick_query.push(ch);
                        self.quick_selected = 0;
                    } else {
                        self.project_query.push(ch);
                        self.search_results.clear();
                    }
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                return;
            }
            if self.side_view == SideView::Review && self.review_file.is_some() {
                return;
            }
            if self.find_mode && unit == 8 {
                self.find_query.pop();
                self.status = format!("Find: {}", self.find_query);
                self.refresh(hwnd);
                return;
            }
            if self.find_mode && unit == 13 {
                self.find_mode = false;
                self.find(hwnd, true);
                return;
            }
            if (unit < 32 && unit != 9 && unit != 13) || unit == 127 {
                return;
            }
            let ch = if (0xd800..=0xdbff).contains(&unit) {
                self.pending_high_surrogate = Some(unit);
                return;
            } else if (0xdc00..=0xdfff).contains(&unit) {
                let Some(high) = self.pending_high_surrogate.take() else {
                    return;
                };
                char::from_u32(0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00))
            } else {
                self.pending_high_surrogate = None;
                char::from_u32(unit as u32)
            };
            if let Some(ch) = ch {
                if self.find_mode {
                    if !ch.is_control() {
                        self.find_query.push(ch);
                        self.status = format!("Find: {}", self.find_query);
                        self.refresh(hwnd);
                    }
                    return;
                }
                let text = if ch == '\r' {
                    "\n".to_owned()
                } else {
                    ch.to_string()
                };
                self.cancel_transition(hwnd);
                self.replace_selection(&text);
                self.refresh(hwnd);
            }
        }

        fn position_at(&self, hwnd: HWND, x: i32, y: i32) -> Pos {
            let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
            let line = (self.view().first_line + row).min(self.doc().line_count() - 1);
            let target = (x - self.code_left()).max(0);
            unsafe {
                let hdc = GetDC(hwnd);
                let old = SelectObject(hdc, self.font);
                let text = self.doc().line(line);
                let boundaries: Vec<usize> = text
                    .char_indices()
                    .map(|(index, _)| index)
                    .chain(Some(text.len()))
                    .collect();
                let mut low = 0;
                let mut high = boundaries.len();
                while low < high {
                    let mid = (low + high) / 2;
                    if self.text_width(hdc, &text[..boundaries[mid]]) < target {
                        low = mid + 1;
                    } else {
                        high = mid;
                    }
                }
                let right = low.min(boundaries.len() - 1);
                let left = right.saturating_sub(1);
                let left_width = self.text_width(hdc, &text[..boundaries[left]]);
                let right_width = self.text_width(hdc, &text[..boundaries[right]]);
                let byte = if target - left_width <= right_width - target {
                    boundaries[left]
                } else {
                    boundaries[right]
                };
                SelectObject(hdc, old);
                ReleaseDC(hwnd, hdc);
                Pos { line, byte }
            }
        }

        fn mouse_click(&mut self, hwnd: HWND, x: i32, y: i32, extend: bool) {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            if self.welcome {
                let left = (rect.right / 2 - self.scale(250)).max(self.scale(28));
                if y >= self.scale(251) && y < self.scale(289) {
                    let button = (x - left) / self.scale(145).max(1);
                    if x >= left && button == 0 {
                        self.open(hwnd, None);
                    } else if x >= left && button == 1 {
                        self.open_folder(hwnd);
                    } else if x >= left && button == 2 {
                        self.new_file(hwnd);
                    }
                } else if y >= self.scale(351) {
                    let index = ((y - self.scale(351)) / self.scale(49).max(1)) as usize;
                    if x >= left
                        && x < left + self.scale(430)
                        && let Some(path) = self.recent.get(index).cloned()
                    {
                        self.set_workspace(hwnd, path);
                    }
                }
                return;
            }
            if self.quick_open {
                let width = self.scale(560).min(rect.right - self.scale(30));
                let left = (rect.right - width) / 2;
                let top = self.scale(52);
                if x >= left
                    && x < left + width
                    && y >= top + self.scale(68)
                    && y < top + self.scale(70 + 8 * 34)
                {
                    let index = ((y - top - self.scale(68)) / self.scale(34).max(1)) as usize;
                    self.activate_quick_item(hwnd, index);
                } else if x < left
                    || x >= left + width
                    || y < top
                    || y >= top + self.scale(70 + 8 * 34)
                {
                    self.quick_open = false;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                return;
            }
            if y >= rect.bottom - self.scale(STATUS) {
                return;
            }
            let rail = self.scale(RAIL);
            let editor_left = self.editor_left();
            if x < rail {
                if y < self.scale(39) {
                    self.show_welcome(hwnd);
                } else if y >= self.scale(46) && y < self.scale(82) {
                    if self.side_view == SideView::Search {
                        self.cancel_search();
                    }
                    let already_open = self.side_view == SideView::Files && self.explorer_visible;
                    self.side_view = SideView::Files;
                    self.panel_focus = false;
                    self.set_sidebar_visible(hwnd, !already_open);
                    self.show_active_tab(hwnd);
                } else if y >= self.scale(88) && y < self.scale(124) {
                    self.open_project_search(hwnd);
                } else if y >= self.scale(130) && y < self.scale(166) {
                    self.run_project(hwnd);
                } else if y >= self.scale(172) && y < self.scale(208) {
                    self.show_review(hwnd);
                }
                return;
            }
            if self.sidebar_width > 0 && x < editor_left {
                if !self.explorer_visible {
                    return;
                }
                if self.side_view == SideView::Search {
                    if y >= self.scale(47) && y < self.scale(78) {
                        self.search_input = true;
                        return;
                    }
                    if y >= self.scale(113) {
                        let index = self.panel_first
                            + ((y - self.scale(113)) / self.scale(48).max(1)) as usize;
                        if let Some(hit) = self.search_results.get(index).cloned() {
                            self.panel_focus = false;
                            self.open(hwnd, Some(hit.path));
                            self.move_cursor(
                                Pos {
                                    line: hit.line,
                                    byte: hit.byte,
                                },
                                false,
                            );
                            self.keep_cursor_visible(hwnd);
                        }
                    }
                    return;
                }
                if self.side_view == SideView::Review {
                    if y >= self.scale(86) {
                        let index = self.panel_first
                            + ((y - self.scale(86)) / self.scale(EXPLORER_ROW).max(1)) as usize;
                        if let Some(change) = self.changes.get(index).cloned() {
                            self.panel_focus = false;
                            self.show_diff(hwnd, change.path);
                        }
                    }
                    return;
                }
                if y >= self.scale(EXPLORER_TOP) {
                    let row = self.explorer_first_row
                        + ((y - self.scale(EXPLORER_TOP)) / self.scale(EXPLORER_ROW)) as usize;
                    if let Some(item) = self.explorer_rows().get(row) {
                        let path = item.entry.path.clone();
                        if item.entry.is_dir {
                            if self.expanded_dirs.remove(&path) {
                                self.explorer_first_row = self
                                    .explorer_first_row
                                    .min(self.explorer_rows().len().saturating_sub(1));
                                self.refresh(hwnd);
                            } else {
                                self.expanded_dirs.insert(path.clone());
                                self.load_directory(&path);
                                self.refresh(hwnd);
                            }
                        } else {
                            self.open(hwnd, Some(path));
                        }
                    }
                }
                return;
            }
            if self.run_visible && y >= rect.bottom - self.scale(STATUS + 210) {
                self.output_focus = true;
                if y < rect.bottom - self.scale(STATUS + 176) {
                    if x >= rect.right - self.scale(40) {
                        self.run_visible = false;
                        self.output_focus = false;
                        self.keep_cursor_visible(hwnd);
                    } else if x >= rect.right - self.scale(100) && self.run_busy {
                        self.stop_run(hwnd);
                    }
                }
                return;
            }
            if self.side_view == SideView::Review
                && self.review_file.is_some()
                && y >= self.editor_top()
            {
                return;
            }
            if y < self.scale(TAB_HEIGHT) {
                if editor_left
                    + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                    + self.scale(12)
                    < rect.right - self.scale(207)
                    && x >= rect.right - self.scale(207)
                {
                    self.show_quick_open(hwnd);
                    return;
                }
                let slot = ((x - editor_left).max(0) / self.scale(TAB_WIDTH).max(1)) as usize;
                let index = self.tab_first + slot;
                if index < self.tabs.len() {
                    if (x - editor_left) % self.scale(TAB_WIDTH) >= self.scale(TAB_WIDTH - 30) {
                        self.close_tab(hwnd, index);
                    } else {
                        self.activate_tab(hwnd, index);
                    }
                }
                return;
            }
            if y < self.editor_top() {
                return;
            }
            let pos = self.position_at(hwnd, x, y);
            self.panel_focus = false;
            self.output_focus = false;
            self.move_cursor(pos, extend);
            self.dragging = true;
            unsafe {
                SetFocus(hwnd);
                SetCapture(hwnd);
            }
            self.refresh(hwnd);
        }

        fn mouse_drag(&mut self, hwnd: HWND, x: i32, y: i32) {
            if !self.dragging {
                return;
            }
            let pos = self.position_at(hwnd, x, y);
            self.move_cursor(pos, true);
            self.refresh(hwnd);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_DESTROY {
            EDITOR_WINDOW.store(0, Ordering::Relaxed);
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<App> };
        if ptr.is_null() {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        let cell = unsafe { &*ptr };
        let Ok(mut app) = cell.try_borrow_mut() else {
            return if msg == WM_CLOSE {
                0
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            };
        };
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                app.advance_syntax(hwnd);
                app.paint(hwnd);
                0
            }
            WM_DPICHANGED => {
                app.set_dpi((wparam & 0xffff) as u32);
                app.transition = None;
                unsafe { KillTimer(hwnd, 3) };
                let suggested = unsafe { &*(lparam as *const RECT) };
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                app.keep_cursor_visible(hwnd);
                0
            }
            WM_SIZE => {
                app.keep_cursor_visible(hwnd);
                let count = app.visible_tab_count(hwnd);
                if app.active >= app.tab_first + count {
                    app.tab_first = app.active + 1 - count;
                }
                0
            }
            WM_SETFOCUS => {
                app.focused = true;
                app.caret_on = true;
                unsafe {
                    SetTimer(hwnd, 1, 530, None);
                }
                app.invalidate_caret(hwnd);
                0
            }
            WM_KILLFOCUS => {
                app.focused = false;
                unsafe {
                    KillTimer(hwnd, 1);
                }
                app.invalidate_caret(hwnd);
                0
            }
            WM_TIMER if wparam == 1 => {
                if app.focused {
                    app.caret_on = !app.caret_on;
                    app.invalidate_caret(hwnd);
                }
                0
            }
            WM_TIMER if wparam == 2 => {
                app.advance_syntax(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_TIMER if wparam == 3 => {
                unsafe { InvalidateRect(hwnd, null(), 0) };
                0
            }
            WM_TIMER if wparam == 4 => {
                app.poll_workers(hwnd);
                0
            }
            WM_TIMER if wparam == 5 => {
                app.advance_sidebar(hwnd);
                0
            }
            WM_SETCURSOR if (lparam as u32 & 0xffff) == HTCLIENT => {
                unsafe {
                    let mut point = POINT::default();
                    GetCursorPos(&mut point);
                    ScreenToClient(hwnd, &mut point);
                    let cursor = LoadCursorW(
                        null_mut(),
                        if app.welcome
                            || app.quick_open
                            || (app.side_view == SideView::Review && app.review_file.is_some())
                            || point.y < app.editor_top()
                            || point.x < app.editor_left()
                            || {
                                let mut rect = RECT::default();
                                GetClientRect(hwnd, &mut rect);
                                point.y
                                    >= rect.bottom
                                        - app.scale(STATUS + if app.run_visible { 210 } else { 0 })
                            }
                        {
                            IDC_ARROW
                        } else {
                            IDC_IBEAM
                        },
                    );
                    SetCursor(if cursor.is_null() {
                        LoadCursorW(null_mut(), IDC_ARROW)
                    } else {
                        cursor
                    });
                }
                1
            }
            WM_KEYDOWN => {
                if app.key(hwnd, wparam as u32) {
                    0
                } else {
                    drop(app);
                    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
                }
            }
            WM_CHAR => {
                app.character(hwnd, wparam as u16);
                0
            }
            WM_LBUTTONDOWN => {
                app.mouse_click(
                    hwnd,
                    (lparam as u32 & 0xffff) as i16 as i32,
                    ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                    unsafe { GetKeyState(VK_SHIFT as i32) } < 0,
                );
                0
            }
            WM_MOUSEMOVE => {
                if app.dragging && wparam & 1 != 0 {
                    app.mouse_drag(
                        hwnd,
                        (lparam as u32 & 0xffff) as i16 as i32,
                        ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                    );
                }
                0
            }
            WM_LBUTTONUP => {
                app.dragging = false;
                unsafe {
                    ReleaseCapture();
                }
                0
            }
            WM_MOUSEWHEEL => {
                let delta = (wparam >> 16) as i16;
                let mut point = POINT::default();
                unsafe {
                    GetCursorPos(&mut point);
                    ScreenToClient(hwnd, &mut point);
                }
                let mut rect = RECT::default();
                unsafe { GetClientRect(hwnd, &mut rect) };
                if app.run_visible
                    && point.x >= app.editor_left()
                    && point.y >= rect.bottom - app.scale(STATUS + 210)
                {
                    let max = app.run_output.lines().count().saturating_sub(8);
                    app.output_scroll = if delta > 0 {
                        (app.output_scroll + 3).min(max)
                    } else {
                        app.output_scroll.saturating_sub(3)
                    };
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return 0;
                }
                if app.sidebar_width > 0
                    && point.x >= app.scale(RAIL)
                    && point.x < app.editor_left()
                {
                    if !app.explorer_visible {
                        return 0;
                    }
                    if app.side_view != SideView::Files {
                        let count = if app.side_view == SideView::Search {
                            app.search_results.len()
                        } else {
                            app.changes.len()
                        };
                        let rows = if app.side_view == SideView::Search {
                            48
                        } else {
                            EXPLORER_ROW
                        };
                        let visible = ((rect.bottom - app.scale(STATUS + 113))
                            / app.scale(rows).max(1))
                        .max(1) as usize;
                        let max = count.saturating_sub(visible);
                        app.panel_first = if delta > 0 {
                            app.panel_first.saturating_sub(3)
                        } else {
                            (app.panel_first + 3).min(max)
                        };
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                        return 0;
                    }
                    let visible = ((rect.bottom - app.scale(STATUS + EXPLORER_TOP))
                        / app.scale(EXPLORER_ROW).max(1))
                    .max(1) as usize;
                    let max_first = app.explorer_rows().len().saturating_sub(visible);
                    app.explorer_first_row = if delta > 0 {
                        app.explorer_first_row.saturating_sub(3)
                    } else {
                        (app.explorer_first_row + 3).min(max_first)
                    };
                    unsafe {
                        InvalidateRect(hwnd, null(), 0);
                    }
                    return 0;
                }
                if app.side_view == SideView::Review
                    && app.review_file.is_some()
                    && point.x >= app.editor_left()
                {
                    let visible = ((rect.bottom - app.scale(STATUS) - app.editor_top())
                        / app.line_height.max(1))
                    .max(1) as usize;
                    let max = app.diff_rows.len().saturating_sub(visible);
                    app.diff_first = if delta > 0 {
                        app.diff_first.saturating_sub(3)
                    } else {
                        (app.diff_first + 3).min(max)
                    };
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return 0;
                }
                if delta > 0 {
                    app.view_mut().first_line = app.view().first_line.saturating_sub(3);
                } else if delta < 0 {
                    app.view_mut().first_line =
                        (app.view().first_line + 3).min(app.doc().line_count().saturating_sub(1));
                }
                app.update_scrollbar(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_VSCROLL => {
                let code = (wparam & 0xffff) as i32;
                let max = app.doc().line_count().saturating_sub(1);
                app.view_mut().first_line = match code {
                    SB_LINEUP => app.view().first_line.saturating_sub(1),
                    SB_LINEDOWN => (app.view().first_line + 1).min(max),
                    SB_PAGEUP => app
                        .view()
                        .first_line
                        .saturating_sub(app.visible_lines(hwnd)),
                    SB_PAGEDOWN => (app.view().first_line + app.visible_lines(hwnd)).min(max),
                    SB_THUMBPOSITION | SB_THUMBTRACK => {
                        let mut info = SCROLLINFO {
                            cbSize: size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_TRACKPOS,
                            ..unsafe { zeroed() }
                        };
                        unsafe {
                            GetScrollInfo(hwnd, SB_VERT, &mut info);
                        }
                        (info.nTrackPos.max(0) as usize).min(max)
                    }
                    _ => app.view().first_line,
                };
                app.update_scrollbar(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_CLOSE => {
                if app.can_close_window(hwnd) {
                    app.stop_run_before_close();
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                }
                0
            }
            _ => {
                drop(app);
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
    }

    pub fn run() -> io::Result<()> {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let com_initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
            let instance = GetModuleHandleW(null());
            let class = wide("LightLineWindow");
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_IBEAM),
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(io::Error::last_os_error());
            }
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("LightLine").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VSCROLL,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1000,
                700,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return Err(io::Error::last_os_error());
            }
            let dark_titlebar: i32 = 1;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                &dark_titlebar as *const i32 as *const std::ffi::c_void,
                size_of::<i32>() as u32,
            );
            let mut app = Box::new(RefCell::new(App::new(hwnd)));
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut *app as *mut RefCell<App>) as isize,
            );
            connect_parent_console(hwnd);
            app.borrow().update_title(hwnd);
            app.borrow().update_scrollbar(hwnd);
            ShowWindow(hwnd, SW_SHOW);
            SetFocus(hwnd);
            if let Some(path) = std::env::args_os().nth(1) {
                app.borrow_mut().open(hwnd, Some(PathBuf::from(path)));
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            DeleteObject(app.borrow().font);
            DeleteObject(app.borrow().ui_font);
            DeleteObject(app.borrow().brand_font);
            if com_initialized {
                CoUninitialize();
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        eprintln!("LightLine: {error}");
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LightLine currently supports Windows only.");
}
