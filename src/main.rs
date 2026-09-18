#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use lightline::clipboard;
    use lightline::document::{Document, Pos};
    use lightline::syntax::{Color, RustSyntax};
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    use windows_sys::Win32::Graphics::Gdi::*;
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
        fn new(dpi: u32) -> Self {
            let size = (18 * dpi as i32 + 48) / 96;
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
        line_height: i32,
        status: String,
        focused: bool,
        caret_on: bool,
        dragging: bool,
        find_mode: bool,
        find_query: String,
        pending_high_surrogate: Option<u16>,
        explorer_visible: bool,
        explorer_first_row: usize,
        workspace_root: Option<PathBuf>,
        expanded_dirs: HashSet<PathBuf>,
        directory_cache: HashMap<PathBuf, Vec<ExplorerEntry>>,
    }

    impl App {
        fn font_for_dpi(dpi: u32) -> HFONT {
            let font_name = wide("Consolas");
            unsafe {
                CreateFontW(
                    -((17 * dpi as i32 + 48) / 96),
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

        fn ui_font_for_dpi(dpi: u32) -> HFONT {
            let font_name = wide("Segoe UI");
            unsafe {
                CreateFontW(
                    -((13 * dpi as i32 + 48) / 96),
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

        fn brand_font_for_dpi(dpi: u32) -> HFONT {
            let font_name = wide("Segoe UI Semibold");
            unsafe {
                CreateFontW(
                    -((17 * dpi as i32 + 48) / 96),
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
            Self {
                tabs: vec![Tab::new(Document::new())],
                active: 0,
                tab_first: 0,
                font: Self::font_for_dpi(dpi),
                ui_font: Self::ui_font_for_dpi(dpi),
                brand_font: Self::brand_font_for_dpi(dpi),
                icons: IconSet::new(dpi),
                dpi,
                line_height: (23 * dpi as i32 + 48) / 96,
                status: "Ready".into(),
                focused: false,
                caret_on: true,
                dragging: false,
                find_mode: false,
                find_query: String::new(),
                pending_high_surrogate: None,
                explorer_visible: true,
                explorer_first_row: 0,
                workspace_root: None,
                expanded_dirs: HashSet::new(),
                directory_cache: HashMap::new(),
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
            self.scale(RAIL + if self.explorer_visible { SIDEBAR } else { 0 })
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
            (pixels * self.dpi as i32 + 48) / 96
        }

        fn set_dpi(&mut self, dpi: u32) {
            let dpi = dpi.max(96);
            if dpi == self.dpi {
                return;
            }
            let font = Self::font_for_dpi(dpi);
            let ui_font = Self::ui_font_for_dpi(dpi);
            let brand_font = Self::brand_font_for_dpi(dpi);
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
            self.icons = IconSet::new(dpi);
            self.dpi = dpi;
            self.line_height = (23 * dpi as i32 + 48) / 96;
        }

        fn visible_lines(&self, hwnd: HWND) -> usize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            ((rect.bottom - rect.top - self.editor_top() - self.scale(STATUS)).max(1)
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

        fn paint(&self, hwnd: HWND) {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let old_font = SelectObject(hdc, self.font);
                SelectObject(hdc, self.ui_font);
                SetBkMode(hdc, TRANSPARENT as i32);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                let editor_bottom = (rect.bottom - self.scale(STATUS)).max(0);
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
                if self.explorer_visible {
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
                if self.explorer_visible {
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
                Self::fill(
                    hdc,
                    RECT {
                        left: 0,
                        top: self.scale(48),
                        right: self.scale(3),
                        bottom: self.scale(78),
                    },
                    BLUE,
                );
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
                    if self.explorer_visible { TEXT } else { MUTED },
                    rail_clip,
                );
                Self::label(hdc, "⌕", self.scale(18), self.scale(95), MUTED, rail_clip);
                Self::label(
                    hdc,
                    "Search",
                    self.scale(42),
                    self.scale(95),
                    MUTED,
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
                if self.explorer_visible {
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
                                Self::label(
                                    hdc,
                                    if item.expanded { "⌄" } else { "›" },
                                    left,
                                    top + self.scale(1),
                                    MUTED,
                                    RECT {
                                        left,
                                        top,
                                        right: editor_left - self.scale(9),
                                        bottom: top + self.scale(EXPLORER_ROW),
                                    },
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
                    if y >= editor_bottom {
                        break;
                    }
                    if index == self.view().cursor.line {
                        Self::fill(
                            hdc,
                            RECT {
                                left: editor_left,
                                top: y,
                                right: rect.right,
                                bottom: (y + self.line_height).min(editor_bottom),
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
                        bottom: editor_bottom,
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
                        let guide_x = code_left + level as i32 * 4 * space_width - self.scale(4);
                        if guide_x < rect.right {
                            FillRect(
                                hdc,
                                &RECT {
                                    left: guide_x,
                                    top: y,
                                    right: guide_x + 1,
                                    bottom: (y + self.line_height).min(editor_bottom),
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
                                    bottom: (y + self.line_height).min(editor_bottom),
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
                        bottom: editor_bottom,
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
                        + (self.view().cursor.line as i64 - self.view().first_line as i64) as i32
                            * self.line_height;
                    if y >= self.editor_top() && y < editor_bottom && x < rect.right {
                        let caret = CreateSolidBrush(BLUE);
                        FillRect(
                            hdc,
                            &RECT {
                                left: x,
                                top: y,
                                right: x + self.scale(2).max(2),
                                bottom: (y + self.line_height).min(editor_bottom),
                            },
                            caret,
                        );
                        DeleteObject(caret);
                    }
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
                let right_label = format!(
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
                );
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
                SelectObject(hdc, old_font);
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
                        self.tabs.push(Tab::new(Document::new()));
                        self.active = self.tabs.len() - 1;
                        self.status = "New document".into();
                        self.show_active_tab(hwnd);
                        return true;
                    }
                    0x46 => {
                        self.find_mode = true;
                        self.find_query.clear();
                        self.status = "Find: ".into();
                    }
                    0x42 => {
                        self.explorer_visible = !self.explorer_visible;
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
            if y >= rect.bottom - self.scale(STATUS) {
                return;
            }
            let rail = self.scale(RAIL);
            let editor_left = self.editor_left();
            if x < rail {
                if y >= self.scale(46) && y < self.scale(82) {
                    self.explorer_visible = !self.explorer_visible;
                    self.show_active_tab(hwnd);
                } else if y >= self.scale(88) && y < self.scale(124) {
                    self.find_mode = true;
                    self.find_query.clear();
                    self.status = "Find: ".into();
                    self.refresh(hwnd);
                }
                return;
            }
            if self.explorer_visible && x < editor_left {
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
            if y < self.scale(TAB_HEIGHT) {
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
            WM_PAINT => {
                app.advance_syntax(hwnd);
                app.paint(hwnd);
                0
            }
            WM_DPICHANGED => {
                app.set_dpi((wparam & 0xffff) as u32);
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
            WM_SETCURSOR if (lparam as u32 & 0xffff) == HTCLIENT => {
                unsafe {
                    let mut point = POINT::default();
                    GetCursorPos(&mut point);
                    ScreenToClient(hwnd, &mut point);
                    let cursor = LoadCursorW(
                        null_mut(),
                        if point.y < app.editor_top() || point.x < app.editor_left() || {
                            let mut rect = RECT::default();
                            GetClientRect(hwnd, &mut rect);
                            point.y >= rect.bottom - app.scale(STATUS)
                        } {
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
                if app.explorer_visible && point.x >= app.scale(RAIL) && point.x < app.editor_left()
                {
                    let mut rect = RECT::default();
                    unsafe {
                        GetClientRect(hwnd, &mut rect);
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
