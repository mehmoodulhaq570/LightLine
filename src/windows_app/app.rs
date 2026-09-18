use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SideView {
    Files,
    Search,
    Review,
}

pub(super) enum WorkerMessage {
    Files(PathBuf, Vec<PathBuf>),
    Search(PathBuf, String, Arc<AtomicBool>, Vec<SearchHit>),
    RunLine(PathBuf, String),
    Run(PathBuf, Result<(), String>),
    Changes(PathBuf, Result<Vec<Change>, String>),
    Diff(PathBuf, PathBuf, Result<Vec<DiffRow>, String>),
}

#[derive(Clone, Default)]
pub(super) struct EditorView {
    pub(super) cursor: Pos,
    pub(super) selection_anchor: Option<Pos>,
    pub(super) first_line: usize,
}

fn remap_position(pos: Pos, start: Pos, end: Pos, inserted_end: Pos) -> Pos {
    if pos < start {
        pos
    } else if pos <= end {
        inserted_end
    } else if pos.line == end.line {
        Pos {
            line: inserted_end.line,
            byte: inserted_end.byte + pos.byte - end.byte,
        }
    } else {
        Pos {
            line: pos
                .line
                .saturating_add(inserted_end.line.saturating_sub(end.line))
                .saturating_sub(end.line.saturating_sub(inserted_end.line)),
            byte: pos.byte,
        }
    }
}

fn tab_index_after_close(current: usize, closed: usize, remaining: usize) -> usize {
    if remaining == 0 || current == closed {
        closed.min(remaining.saturating_sub(1))
    } else if current > closed {
        current - 1
    } else {
        current
    }
}

pub(super) struct Tab {
    pub(super) document: Document,
    pub(super) views: [EditorView; 2],
    pub(super) syntax: Option<RustSyntax>,
}

#[derive(Clone)]
pub(super) struct ExplorerEntry {
    pub(super) path: PathBuf,
    pub(super) is_dir: bool,
}

pub(super) struct ExplorerRow {
    pub(super) entry: ExplorerEntry,
    pub(super) depth: usize,
    pub(super) expanded: bool,
}

impl Tab {
    fn new(document: Document) -> Self {
        let syntax = Self::is_rust(&document).then(RustSyntax::new);
        Self {
            document,
            views: [EditorView::default(), EditorView::default()],
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

pub(super) struct App {
    pub(super) tabs: Vec<Tab>,
    pub(super) active: usize,
    pub(super) pane_tabs: [usize; 2],
    pub(super) focused_pane: usize,
    pub(super) split_visible: bool,
    pub(super) split_ratio: i32,
    pub(super) divider_dragging: bool,
    pub(super) tab_first: usize,
    pub(super) font: HFONT,
    pub(super) ui_font: HFONT,
    pub(super) brand_font: HFONT,
    pub(super) brand_icon: HICON,
    pub(super) icons: IconSet,
    pub(super) dpi: u32,
    pub(super) zoom: i32,
    pub(super) line_height: i32,
    pub(super) backbuffer: Option<Surface>,
    pub(super) transition: Option<Transition>,
    pub(super) status: String,
    pub(super) focused: bool,
    pub(super) caret_on: bool,
    pub(super) dragging: bool,
    pub(super) find_mode: bool,
    pub(super) find_query: String,
    pub(super) pending_high_surrogate: Option<u16>,
    pub(super) explorer_visible: bool,
    pub(super) sidebar_width: i32,
    pub(super) sidebar_from: i32,
    pub(super) sidebar_target: i32,
    pub(super) sidebar_started: Option<Instant>,
    pub(super) explorer_first_row: usize,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) workspace_branch: Option<String>,
    pub(super) expanded_dirs: HashSet<PathBuf>,
    pub(super) directory_cache: HashMap<PathBuf, Vec<ExplorerEntry>>,
    pub(super) welcome: bool,
    pub(super) side_view: SideView,
    pub(super) quick_open: bool,
    pub(super) quick_query: String,
    pub(super) quick_selected: usize,
    pub(super) quick_files: Vec<PathBuf>,
    pub(super) quick_loading: bool,
    pub(super) search_input: bool,
    pub(super) project_query: String,
    pub(super) search_results: Vec<SearchHit>,
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
    pub(super) run_visible: bool,
    pub(super) run_output: String,
    pub(super) run_busy: bool,
    pub(super) run_cancel: Option<Arc<AtomicBool>>,
    pub(super) run_pid: Option<Arc<AtomicU32>>,
    pub(super) output_focus: bool,
    pub(super) changes: Vec<Change>,
    pub(super) review_loading: bool,
    pub(super) review_file: Option<PathBuf>,
    pub(super) diff_rows: Vec<DiffRow>,
    pub(super) diff_first: usize,
    pub(super) panel_first: usize,
    pub(super) panel_selected: usize,
    pub(super) panel_focus: bool,
    pub(super) output_scroll: usize,
    pub(super) recent: Vec<PathBuf>,
    pub(super) worker_tx: Sender<WorkerMessage>,
    pub(super) worker_rx: Receiver<WorkerMessage>,
    pub(super) pending_workers: usize,
}

impl App {
    pub(super) fn font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
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

    pub(super) fn ui_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
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

    pub(super) fn brand_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
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

    pub(super) fn new(hwnd: HWND, brand_icon: HICON) -> Self {
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let zoom = 100;
        let (worker_tx, worker_rx) = mpsc::channel();
        Self {
            tabs: vec![Tab::new(Document::new())],
            active: 0,
            pane_tabs: [0, 0],
            focused_pane: 0,
            split_visible: false,
            split_ratio: 50,
            divider_dragging: false,
            tab_first: 0,
            font: Self::font_for_dpi(dpi, zoom),
            ui_font: Self::ui_font_for_dpi(dpi, zoom),
            brand_font: Self::brand_font_for_dpi(dpi, zoom),
            brand_icon,
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
            workspace_branch: None,
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

    pub(super) fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    pub(super) fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
    pub(super) fn doc(&self) -> &Document {
        &self.tab().document
    }
    pub(super) fn doc_mut(&mut self) -> &mut Document {
        &mut self.tab_mut().document
    }
    pub(super) fn view(&self) -> &EditorView {
        &self.tab().views[self.focused_pane]
    }
    pub(super) fn view_mut(&mut self) -> &mut EditorView {
        &mut self.tabs[self.active].views[self.focused_pane]
    }
    pub(super) fn tab_for_pane(&self, pane: usize) -> usize {
        if self.split_visible {
            self.pane_tabs[pane]
        } else {
            self.active
        }
    }
    pub(super) fn view_for_pane(&self, pane: usize) -> &EditorView {
        &self.tabs[self.tab_for_pane(pane)].views[pane]
    }
    pub(super) fn set_active_index(&mut self, index: usize) {
        self.active = index;
        self.pane_tabs[self.focused_pane] = index;
    }
    pub(super) fn focus_pane(&mut self, hwnd: HWND, pane: usize) {
        if !self.split_visible || pane > 1 || pane == self.focused_pane {
            return;
        }
        self.focused_pane = pane;
        self.active = self.pane_tabs[pane];
        self.show_active_tab(hwnd);
    }
    pub(super) fn toggle_split(&mut self, hwnd: HWND) {
        if self.split_visible {
            let selected = self.pane_tabs[self.focused_pane];
            if self.focused_pane == 1 {
                self.tabs[selected].views[0] = self.tabs[selected].views[1].clone();
            }
            self.split_visible = false;
            self.divider_dragging = false;
            self.focused_pane = 0;
            self.pane_tabs = [selected, selected];
            self.active = selected;
            self.status = "Split closed".into();
        } else {
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            if rect.right - self.editor_left() < self.scale(430) {
                self.status = "Widen the window to split the editor".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            self.tabs[self.active].views[1] = self.tabs[self.active].views[0].clone();
            self.pane_tabs = [self.active, self.active];
            self.focused_pane = 0;
            self.split_visible = true;
            self.status = "Editor split into two panes".into();
        }
        self.cancel_transition(hwnd);
        self.show_active_tab(hwnd);
    }
    pub(super) fn pane_divider(&self, hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = (rect.right - self.editor_left()).max(0);
        let minimum = self.scale(150).min(width / 2);
        self.editor_left() + (width * self.split_ratio / 100).clamp(minimum, width - minimum)
    }
    pub(super) fn resize_split(&mut self, hwnd: HWND, x: i32) {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = (rect.right - self.editor_left()).max(1);
        self.split_ratio = (((x - self.editor_left()) * 100) / width).clamp(10, 90);
        self.update_scrollbar(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
    pub(super) fn pane_left(&self, hwnd: HWND, pane: usize) -> i32 {
        if self.split_visible && pane == 1 {
            self.pane_divider(hwnd)
        } else {
            self.editor_left()
        }
    }
    pub(super) fn pane_right(&self, hwnd: HWND, pane: usize) -> i32 {
        if self.split_visible && pane == 0 {
            self.pane_divider(hwnd)
        } else {
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            rect.right
        }
    }
    pub(super) fn editor_top(&self) -> i32 {
        self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT + TOP)
    }

    pub(super) fn editor_left(&self) -> i32 {
        self.scale(RAIL + self.sidebar_width)
    }

    pub(super) fn set_sidebar_visible(&mut self, hwnd: HWND, visible: bool) {
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

    pub(super) fn advance_sidebar(&mut self, hwnd: HWND) {
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

    pub(super) fn code_left(&self, hwnd: HWND) -> i32 {
        self.pane_left(hwnd, self.focused_pane) + self.scale(GUTTER + PAD)
    }

    pub(super) fn show_welcome(&mut self, hwnd: HWND) {
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

    pub(super) fn new_file(&mut self, hwnd: HWND) {
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
            self.set_active_index(self.tabs.len() - 1);
        }
        self.status = "New document".into();
        self.show_active_tab(hwnd);
    }

    pub(super) fn syntax_changed(&mut self, line: usize) {
        if let Some(syntax) = &mut self.tab_mut().syntax {
            syntax.invalidate_from(line);
        }
    }

    pub(super) fn advance_syntax(&mut self, hwnd: HWND) {
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

    pub(super) fn tab_label(&self, index: usize) -> String {
        let doc = &self.tabs[index].document;
        let name = doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        format!("{}{}", name, if doc.is_dirty() { " *" } else { "" })
    }

    pub(super) fn visible_tab_count(&self, hwnd: HWND) -> usize {
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        ((rect.right - self.editor_left() - self.scale(120)) / self.scale(TAB_WIDTH).max(1)).max(1)
            as usize
    }

    pub(super) fn show_active_tab(&mut self, hwnd: HWND) {
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

    pub(super) fn activate_tab(&mut self, hwnd: HWND, index: usize) {
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
            self.set_active_index(index);
            self.show_active_tab(hwnd);
        }
    }

    pub(super) fn same_path(a: &Path, b: &Path) -> bool {
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

    pub(super) fn close_tab(&mut self, hwnd: HWND, index: usize) {
        self.activate_tab(hwnd, index);
        if !self.can_discard(hwnd) {
            return;
        }
        self.start_transition(hwnd);
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(Document::new()));
            self.pane_tabs = [0, 0];
        } else {
            for tab in &mut self.pane_tabs {
                *tab = tab_index_after_close(*tab, index, self.tabs.len());
            }
        }
        self.active = self.pane_tabs[self.focused_pane];
        self.status = "Ready".into();
        self.show_active_tab(hwnd);
    }

    pub(super) fn can_close_window(&mut self, hwnd: HWND) -> bool {
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

    pub(super) fn scale(&self, pixels: i32) -> i32 {
        scaled(pixels, self.dpi, self.zoom)
    }

    pub(super) fn set_dpi(&mut self, dpi: u32) {
        let dpi = dpi.max(96);
        if dpi == self.dpi {
            return;
        }
        self.set_metrics(dpi, self.zoom);
    }

    pub(super) fn set_zoom(&mut self, hwnd: HWND, zoom: i32) {
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

    pub(super) fn set_metrics(&mut self, dpi: u32, zoom: i32) {
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

    pub(super) fn start_transition(&mut self, hwnd: HWND) {
        if self.split_visible {
            return;
        }
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

    pub(super) fn cancel_transition(&mut self, hwnd: HWND) {
        if self.transition.take().is_some() {
            unsafe { KillTimer(hwnd, 3) };
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn visible_lines(&self, hwnd: HWND) -> usize {
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

    pub(super) fn update_scrollbar(&self, hwnd: HWND) {
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

    pub(super) fn keep_cursor_visible(&mut self, hwnd: HWND) {
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

    pub(super) fn update_title(&self, hwnd: HWND) {
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

    pub(super) fn refresh(&mut self, hwnd: HWND) {
        self.update_title(hwnd);
        self.keep_cursor_visible(hwnd);
        self.advance_syntax(hwnd);
    }

    pub(super) fn selection_range(&self) -> Option<(Pos, Pos)> {
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

    pub(super) fn move_cursor(&mut self, pos: Pos, extend: bool) {
        let cursor = self.view().cursor;
        if extend {
            self.view_mut().selection_anchor.get_or_insert(cursor);
        } else {
            self.view_mut().selection_anchor = None;
        }
        let pos = self.doc().clamp(pos);
        self.view_mut().cursor = pos;
    }

    pub(super) fn replace_selection(&mut self, text: &str) {
        let (start, end) = self
            .selection_range()
            .unwrap_or((self.view().cursor, self.view().cursor));
        self.replace_range(start, end, text);
        self.view_mut().selection_anchor = None;
    }

    pub(super) fn replace_range(&mut self, start: Pos, end: Pos, text: &str) {
        let cursor = self.doc_mut().replace(start, end, text);
        self.syntax_changed(start.line);
        self.view_mut().cursor = cursor;
        self.revalidate_other_view(Some((start, end, cursor)));
    }

    pub(super) fn revalidate_other_view(&mut self, edit: Option<(Pos, Pos, Pos)>) {
        if !self.split_visible || self.pane_tabs[1 - self.focused_pane] != self.active {
            return;
        }
        let other = 1 - self.focused_pane;
        let tab = &mut self.tabs[self.active];
        let old = tab.views[other].clone();
        let cursor = edit.map_or(old.cursor, |(start, end, inserted)| {
            remap_position(old.cursor, start, end, inserted)
        });
        let anchor = old.selection_anchor.map(|anchor| {
            edit.map_or(anchor, |(start, end, inserted)| {
                remap_position(anchor, start, end, inserted)
            })
        });
        tab.views[other].cursor = tab.document.clamp(cursor);
        tab.views[other].selection_anchor = anchor.map(|pos| tab.document.clamp(pos));
        tab.views[other].first_line = edit
            .map_or(old.first_line, |(start, end, inserted)| {
                if old.first_line > end.line {
                    old.first_line
                        .saturating_add(inserted.line.saturating_sub(end.line))
                        .saturating_sub(end.line.saturating_sub(inserted.line))
                } else if old.first_line >= start.line {
                    inserted.line
                } else {
                    old.first_line
                }
            })
            .min(tab.document.line_count().saturating_sub(1));
    }

    pub(super) fn copy_selection(&mut self, hwnd: HWND) -> bool {
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

    pub(super) fn find(&mut self, hwnd: HWND, forward: bool) {
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

    pub(super) fn error(&mut self, hwnd: HWND, error: &impl std::fmt::Display) {
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

    pub(super) fn dialog(&self, hwnd: HWND, save: bool) -> Option<PathBuf> {
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

    pub(super) fn save(&mut self, hwnd: HWND, save_as: bool) -> bool {
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

    pub(super) fn can_discard(&mut self, hwnd: HWND) -> bool {
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

    pub(super) fn open(&mut self, hwnd: HWND, path: Option<PathBuf>) {
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
                    self.set_active_index(0);
                } else {
                    self.tabs.push(Tab::new(document));
                    self.set_active_index(self.tabs.len() - 1);
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
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn edits_remap_the_other_panes_cursor_across_lines() {
        let start = Pos { line: 1, byte: 2 };
        let end = Pos { line: 3, byte: 1 };
        let inserted = Pos { line: 2, byte: 4 };
        assert_eq!(
            remap_position(Pos { line: 0, byte: 5 }, start, end, inserted),
            Pos { line: 0, byte: 5 }
        );
        assert_eq!(
            remap_position(Pos { line: 2, byte: 3 }, start, end, inserted),
            inserted
        );
        assert_eq!(
            remap_position(Pos { line: 3, byte: 6 }, start, end, inserted),
            Pos { line: 2, byte: 9 }
        );
        assert_eq!(
            remap_position(Pos { line: 5, byte: 7 }, start, end, inserted),
            Pos { line: 4, byte: 7 }
        );
    }

    #[test]
    fn closing_a_tab_preserves_the_other_panes_reference() {
        assert_eq!(tab_index_after_close(0, 1, 2), 0);
        assert_eq!(tab_index_after_close(2, 1, 2), 1);
        assert_eq!(tab_index_after_close(1, 1, 2), 1);
        assert_eq!(tab_index_after_close(0, 0, 0), 0);
    }
}
