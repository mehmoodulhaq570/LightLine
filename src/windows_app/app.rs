use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SideView {
    Files,
    Search,
    Review,
    Debug,
    Extensions,
}

// Output holds run/build results (cargo test, Run Python) in a dedicated
// ManagedRun session; Terminal is the persistent interactive user shell.
// Never conflate the two: run output must not be typed into the user's shell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalTab {
    Output,
    Terminal,
}

// One interactive shell instance in the bottom Terminal area. Each pane owns
// its own TerminalService, which hosts a single Shell session, so many
// terminals coexist (VS Code style) without reworking the core service's
// fixed Shell + ManagedRun mailbox that the Output session still uses.
pub(super) struct TerminalPane {
    pub(super) title: String,
    pub(super) service: TerminalService,
    pub(super) id: SessionId,
    pub(super) snapshot: Option<Arc<Snapshot>>,
    pub(super) applied_size: Option<TerminalSize>,
    pub(super) shell_kind: ShellKind,
}

// What a generic Zed-registry install turned out to be, resolved on the
// worker thread so poll_workers only has to apply the result.
pub(super) enum ExtensionInstallKind {
    IconTheme,
    ColorTheme(Theme),
}

pub(super) enum WorkerMessage {
    Files(PathBuf, Vec<PathBuf>),
    Search(PathBuf, String, Arc<AtomicBool>, Vec<SearchHit>),
    // Generation guards the status request: only the newest answer may paint,
    // because refreshes fire on activation, save and every completed action.
    Repo(u64, Result<RepoState, String>),
    Diff(PathBuf, PathBuf, Result<Vec<DiffRow>, String>),
    GutterDiff(PathBuf, PathBuf, Option<String>, Result<Vec<DiffRow>, String>),
    // A live gutter recompute: (request generation, file, marks).
    GutterComputed(u64, PathBuf, GutterDiff),
    GitWrite(GitAction, Result<(), String>),
    ExtensionInstalled(String, bool),
    // The full Zed registry id/version list, for the Extensions panel search.
    ZedRegistryList(Result<Vec<(String, String)>, String>),
    // id, and the outcome. An extension that's neither an icon theme nor a
    // color theme (needs WASM execution LightLine doesn't have) is
    // uninstalled again before this fires and reported as Err(reason)
    // instead of a silent partial success.
    ZedExtensionInstalled(String, Result<ExtensionInstallKind, String>),
    DebugBuild(Result<PathBuf, String>),
    CDiagnostics(PathBuf, Vec<LspDiagnostic>),
    // path, the formatter's display name, the document's change_serial() at
    // request time (so a stale result from a buffer the user kept editing is
    // discarded instead of clobbering newer text), and the outcome.
    Formatted(PathBuf, &'static str, u64, Result<String, String>),
}

// A pending source-control write. Each one runs on the shared worker channel
// and re-reads the repository when it finishes, so the panel never drifts
// ahead of what Git actually did.
#[derive(Clone, Debug)]
pub(super) enum GitAction {
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    Discard {
        paths: Vec<PathBuf>,
        untracked: Vec<PathBuf>,
    },
    Commit,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ExtensionsTab {
    Marketplace,
    Installed,
}

// Owned strings (not &'static str) because entries can now come from the
// live Zed registry at runtime, not just the two extensions LightLine ships
// knowledge of.
#[derive(Clone, Debug)]
pub(super) struct Extension {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) publisher: String,
    pub(super) version: String,
    pub(super) description: String,
    pub(super) downloads: String,
    #[allow(dead_code)]
    pub(super) rating: String,
    pub(super) installed: bool,
    pub(super) installing: bool,
}

pub(super) struct Tab {
    pub(super) document: Document,
    pub(super) views: [EditorView; 2],
    pub(super) syntax: Option<Syntax>,
    pub(super) diagnostics: Vec<LspDiagnostic>,
    pub(super) lsp_version: i32,
    pub(super) lsp_serial: u64,
    pub(super) lsp_opened: bool,
    pub(super) lsp_language: Option<LspLanguage>,
    // Some for a read-only raster image preview; `document` is then an empty,
    // unsaved placeholder that must never actually be written to disk.
    pub(super) image: Option<image_view::ImageAsset>,
    // True for a read-only hex-dump preview of a non-UTF-8, non-image file;
    // `document` holds the generated dump text as its only "content", which
    // must never be written back over the real file.
    pub(super) binary_preview: bool,
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

// Shared by Tab::is_c_family/is_cpp and the on-save syntax check
// (panels::check_c_syntax_on_save), which only has a bare path, not a Document.
pub(super) fn is_c_family_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "c" | "cc" | "cpp" | "cxx"))
}

// LightLine's internal extension id for Material Icon Theme predates this
// session's registry work and is used throughout the UI (has_extension
// checks, keybinding hints, ...); the real Zed registry entry is named
// "material-icon-theme". Every other id (found via registry search) is
// already the real registry id, so this is the identity function for them.
fn zed_registry_id(internal_id: &str) -> &str {
    if internal_id == "material-icons" {
        "material-icon-theme"
    } else {
        internal_id
    }
}

pub(super) fn is_cpp_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "cc" | "cpp" | "cxx"))
}

impl Tab {
    pub(super) fn new(document: Document) -> Self {
        let syntax = if Self::is_rust(&document) {
            Some(Syntax::new_rust())
        } else if Self::is_python(&document) {
            Some(Syntax::new_python())
        } else {
            None
        };
        Self {
            document,
            views: [EditorView::default(), EditorView::default()],
            syntax,
            diagnostics: Vec::new(),
            lsp_version: 1,
            lsp_serial: 0,
            lsp_opened: false,
            lsp_language: None,
            image: None,
            binary_preview: false,
        }
    }

    fn new_image(path: PathBuf, image: image_view::ImageAsset) -> Self {
        let mut document = Document::new();
        document.path = Some(path);
        let mut tab = Self::new(document);
        tab.image = Some(image);
        tab
    }

    // A read-only hex dump of a file that is neither valid UTF-8 text nor a
    // decodable image. `document` holds the generated dump text, which must
    // never be edited or written back over the real bytes on disk.
    fn new_binary_preview(path: PathBuf, bytes: &[u8]) -> Self {
        let mut document = Document::new();
        document.path = Some(path);
        document.seed(&binary_view::hex_dump(bytes));
        Self {
            document,
            views: [EditorView::default(), EditorView::default()],
            syntax: None,
            diagnostics: Vec::new(),
            lsp_version: 1,
            lsp_serial: 0,
            lsp_opened: false,
            lsp_language: None,
            image: None,
            binary_preview: true,
        }
    }

    // True for a generated preview (image or hex dump) whose content is not
    // real document text and must be treated as read-only everywhere else.
    pub(super) fn read_only(&self) -> bool {
        self.image.is_some() || self.binary_preview
    }

    pub(super) fn is_rust(document: &Document) -> bool {
        document
            .path
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    }

    pub(super) fn is_python(document: &Document) -> bool {
        document
            .path
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    }

    // True for a standalone C or C++ source file that can be compiled and run
    // on its own (not a header, which has no entry point to run).
    pub(super) fn is_c_family(document: &Document) -> bool {
        document.path.as_deref().is_some_and(is_c_family_path)
    }

    // True for any file the Run action knows how to execute on its own.
    pub(super) fn is_runnable(document: &Document) -> bool {
        Self::is_python(document) || Self::is_c_family(document)
    }

    pub(super) fn is_cpp(document: &Document) -> bool {
        document.path.as_deref().is_some_and(is_cpp_path)
    }

    pub(super) fn lsp_language(document: &Document) -> Option<LspLanguage> {
        if Self::is_rust(document) {
            Some(LspLanguage::Rust)
        } else if Self::is_python(document) {
            Some(LspLanguage::Python)
        } else {
            None
        }
    }

    fn update_syntax_language(&mut self) {
        if Self::is_rust(&self.document) {
            if !matches!(self.syntax, Some(Syntax::Rust(_))) {
                self.syntax = Some(Syntax::new_rust());
            }
        } else if Self::is_python(&self.document) {
            if !matches!(self.syntax, Some(Syntax::Python(_))) {
                self.syntax = Some(Syntax::new_python());
            }
        } else {
            self.syntax = None;
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ExplorerInputState {
    pub is_folder: bool,
    pub is_rename: bool,
    pub target_dir: PathBuf,
    pub old_path: Option<PathBuf>,
    pub buffer: String,
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
    pub(super) title_font: HFONT,
    pub(super) hero_font: HFONT,
    pub(super) brand_icon: HICON,
    pub(super) hero_icon: HICON,
    pub(super) icons: IconSet,
    pub(super) theme: Theme,
    // The registry id of the color-theme extension currently applied to
    // `theme`, if any -- lets uninstalling *that* extension revert to
    // LightLine's default, without disturbing the theme when an unrelated
    // extension is installed/removed.
    pub(super) active_color_theme: Option<String>,
    pub(super) dpi: u32,
    pub(super) zoom: i32,
    pub(super) line_height: i32,
    pub(super) backbuffer: Option<Surface>,
    // Tracks the native scrollbar's last ShowScrollBar state so
    // update_scrollbar only calls it on an actual change. Calling
    // ShowScrollBar every keystroke (it used to run unconditionally on every
    // cursor move) forces Windows to recompute the non-client frame each
    // time, which is what caused the reported flicker while typing.
    pub(super) scrollbar_visible: Option<bool>,
    pub(super) transition: Option<Transition>,
    pub(super) status: String,
    pub(super) focused: bool,
    pub(super) caret_on: bool,
    pub(super) dragging: bool,
    pub(super) find_mode: bool,
    pub(super) find_query: String,
    pub(super) replace_mode: bool,
    pub(super) replace_query: String,
    pub(super) replace_field: usize,
    pub(super) pending_high_surrogate: Option<u16>,
    pub(super) explorer_visible: bool,
    pub(super) sidebar_width: i32,
    pub(super) sidebar_user_width: i32,
    pub(super) sidebar_from: i32,
    pub(super) sidebar_target: i32,
    pub(super) sidebar_started: Option<Instant>,
    pub(super) sidebar_dragging: bool,
    pub(super) terminal_height: i32,
    pub(super) terminal_resizing: bool,
    // Cell coordinates (column, row), not pixels. Selecting text is allowed
    // on both tabs (copying a build error from Output is legitimate) even
    // though only Terminal accepts typed input.
    pub(super) terminal_selecting: bool,
    pub(super) terminal_select_anchor: Option<(u16, u16)>,
    pub(super) terminal_select_end: Option<(u16, u16)>,
    pub(super) explorer_first_row: usize,
    pub(super) explorer_input: Option<ExplorerInputState>,
    pub(super) selected_explorer_path: Option<PathBuf>,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) workspace_branch: Option<String>,
    pub(super) expanded_dirs: HashSet<PathBuf>,
    pub(super) directory_cache: HashMap<PathBuf, Vec<ExplorerEntry>>,
    pub(super) welcome: bool,
    pub(super) ai_assistant_visible: bool,
    pub(super) side_view: SideView,
    pub(super) quick_open: bool,
    pub(super) quick_query: String,
    pub(super) quick_selected: usize,
    // First Quick Open row on screen; moves to keep the selection visible.
    pub(super) quick_first: usize,
    pub(super) quick_files: Vec<PathBuf>,
    pub(super) quick_loading: bool,
    pub(super) search_input: bool,
    pub(super) project_query: String,
    pub(super) search_results: Vec<SearchHit>,
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
    pub(super) terminal: TerminalService,
    pub(super) terminal_tab: TerminalTab,
    // Interactive shell sessions shown as tabs in the Terminal area; the
    // active index points into `terminals`. `terminal_counter` only feeds
    // the "1", "2", ... tab labels so a closed tab's number is not reused.
    pub(super) terminals: Vec<TerminalPane>,
    pub(super) terminal_active: usize,
    pub(super) terminal_counter: usize,
    pub(super) run_session: Option<SessionId>,
    pub(super) run_snapshot: Option<Arc<Snapshot>>,
    pub(super) run_applied_size: Option<TerminalSize>,
    pub(super) terminal_visible: bool,
    pub(super) terminal_focus: bool,
    pub(super) cell_width: i32,
    pub(super) changes: Vec<Change>,
    pub(super) review_loading: bool,
    pub(super) review_file: Option<PathBuf>,
    // True while the open diff shows the staged side of a file.
    pub(super) review_staged: bool,
    // Repository top level, which sits above workspace_root when a subfolder
    // was opened; every Git path is relative to it.
    pub(super) git_root: Option<PathBuf>,
    pub(super) git_busy: bool,
    // Absolute path whose gutter diff is currently in flight.
    pub(super) gutter_request: Option<PathBuf>,
    // Absolute path whose gutter diff is already cached.
    pub(super) gutter_done: Option<PathBuf>,
    pub(super) git_generation: u64,
    pub(super) git_ahead: usize,
    pub(super) git_behind: usize,
    pub(super) git_conflicted: bool,
    pub(super) git_staged_collapsed: bool,
    pub(super) git_changes_collapsed: bool,
    pub(super) git_history_collapsed: bool,
    pub(super) commit_message: String,
    pub(super) commit_focus: bool,
    // Set when Commit staged everything first, so the commit follows the stage
    // instead of racing it.
    pub(super) commit_after_stage: bool,
    pub(super) history: Vec<CommitEntry>,
    pub(super) diff_rows: Vec<DiffRow>,
    pub(super) diff_first: usize,
    pub(super) panel_first: usize,
    pub(super) panel_selected: usize,
    pub(super) panel_focus: bool,
    pub(super) recent: Vec<PathBuf>,
    pub(super) worker_tx: Sender<WorkerMessage>,
    pub(super) worker_rx: Receiver<WorkerMessage>,
    pub(super) pending_workers: usize,
    pub(super) lsp: HashMap<LspLanguage, LspClient>,
    pub(super) lsp_event_tx: Sender<LspEvent>,
    pub(super) lsp_events: Receiver<LspEvent>,
    pub(super) lsp_failed_at: HashMap<LspLanguage, Instant>,
    pub(super) python_interpreter: Option<PathBuf>,
    pub(super) hover_mouse: Option<(i32, i32)>,
    pub(super) hover_target: Option<HoverTarget>,
    pub(super) hover_card: Option<HoverCard>,
    pub(super) hover_request_id: u64,
    pub(super) definition_target: Option<NavTarget>,
    pub(super) references_target: Option<NavTarget>,
    pub(super) format_target: Option<NavTarget>,
    pub(super) request_id: u64,
    pub(super) completion_request: Option<CompletionRequest>,
    pub(super) completion: Option<CompletionPopup>,
    // Suppresses session snapshots while restore_session replays the last
    // run's tabs, so opening many files does not rewrite the file each time.
    pub(super) restoring: bool,
    pub(super) watcher: Option<lightline::watcher::FileWatcher>,
    pub(super) settings: lightline::settings::Settings,
    pub(super) git_diff_cache: HashMap<PathBuf, GutterDiff>,
    // HEAD's text per file, shared with gutter diff workers without copying.
    pub(super) git_head_cache: HashMap<PathBuf, Arc<str>>,
    // Bumped per gutter diff request; only the latest request's result is kept.
    pub(super) gutter_generation: u64,
    // The status text last painted and when it first appeared (see fresh_status).
    pub(super) status_seen: RefCell<(String, Instant)>,
    // The main window, for timers started from code paths without an HWND
    // parameter (edits funnel through replace_range, which has none).
    pub(super) hwnd: HWND,
    // Files that are new relative to HEAD, whose every line is marked added.
    pub(super) git_untracked: HashSet<PathBuf>,
    // The repository's .git/index and .git/HEAD, watched so a commit, stage
    // or checkout run in a terminal refreshes the Git panel and gutter.
    pub(super) git_watch_files: Vec<PathBuf>,
    pub(super) extensions: Vec<Extension>,
    pub(super) extensions_tab: ExtensionsTab,
    pub(super) extensions_query: String,
    pub(super) extensions_search_active: bool,
    pub(super) zed_registry_loaded: bool,
    pub(super) zed_registry_loading: bool,
    pub(super) debug: Option<DebugClient>,
    pub(super) debug_event_tx: Sender<DebugEvent>,
    pub(super) debug_events: Receiver<DebugEvent>,
    pub(super) debug_state: DebugState,
    pub(super) debug_pending_root: Option<PathBuf>,
    pub(super) debug_pending_breakpoints: Vec<(PathBuf, Vec<u32>)>,
}

// Everything the Debug side panel paints, kept separate from the live
// `DebugClient` so it survives the moment a session ends (the last stop
// stays visible, like VS Code, until the next run clears it).
#[derive(Default)]
pub(super) struct DebugState {
    pub(super) status: String,
    pub(super) running: bool,
    pub(super) thread_id: i64,
    pub(super) frames: Vec<DebugFrame>,
    pub(super) scopes: Vec<DebugScope>,
    // Which struct/collection variables the user has expanded, and the
    // children fetched for each (keyed by DAP `variablesReference`); a
    // reference present in `expanded` but absent from `children` is still
    // waiting on its `Command::Variables` round trip.
    pub(super) expanded: HashSet<i64>,
    pub(super) children: HashMap<i64, Vec<DebugVariable>>,
}

// Records which pane/request an in-flight definition or format call belongs to
// so poll_lsp can validate the response against the current document state.
pub(super) struct NavTarget {
    pub(super) language: LspLanguage,
    pub(super) id: u64,
    pub(super) uri: String,
    pub(super) version: i32,
    pub(super) pane: usize,
}

pub(super) struct HoverTarget {
    pub(super) language: LspLanguage,
    pub(super) id: u64,
    pub(super) uri: String,
    pub(super) version: i32,
    pub(super) pane: usize,
    pub(super) x: i32,
    pub(super) y: i32,
}

pub(super) struct HoverCard {
    pub(super) text: String,
    pub(super) x: i32,
    pub(super) y: i32,
}

// Tracks an in-flight textDocument/completion request. The replace range and
// caret anchor are captured at trigger time so the popup can be positioned and
// applied even though the reply arrives asynchronously through poll_lsp.
pub(super) struct CompletionRequest {
    pub(super) language: LspLanguage,
    pub(super) id: u64,
    pub(super) uri: String,
    pub(super) version: i32,
    pub(super) pane: usize,
    pub(super) replace_start: Pos,
    pub(super) replace_end: Pos,
    pub(super) x: i32,
    pub(super) y: i32,
}

// A resolved completion list shown as a popup near the caret. Selecting an
// item replaces [replace_start, replace_end) with the item's insert text.
pub(super) struct CompletionPopup {
    pub(super) items: Vec<LspCompletionItem>,
    pub(super) selected: usize,
    pub(super) replace_start: Pos,
    pub(super) replace_end: Pos,
    pub(super) x: i32,
    pub(super) y: i32,
}

// Checks whether a font family is actually installed, rather than just
// asking Windows to substitute the closest match silently.
unsafe extern "system" fn note_family_found(
    _logfont: *const LOGFONTW,
    _metrics: *const TEXTMETRICW,
    _font_type: u32,
    found: LPARAM,
) -> i32 {
    unsafe {
        *(found as *mut bool) = true;
    }
    0
}

fn family_available(name: &str) -> bool {
    unsafe {
        let hdc = GetDC(null_mut());
        if hdc.is_null() {
            return false;
        }
        let mut logfont: LOGFONTW = zeroed();
        logfont.lfCharSet = DEFAULT_CHARSET;
        let wide_name = wide(name);
        let len = wide_name.len().min(logfont.lfFaceName.len());
        logfont.lfFaceName[..len].copy_from_slice(&wide_name[..len]);
        let mut found = false;
        EnumFontFamiliesExW(
            hdc,
            &logfont,
            Some(note_family_found),
            &mut found as *mut bool as LPARAM,
            0,
        );
        ReleaseDC(null_mut(), hdc);
        found
    }
}

// Gutter mark lines after an edit that replaced lines `start..=old_end` and
// changed the line count by `delta`: marks above stay, marks below move by
// `delta`, and marks on lines the edit removed are dropped.
fn shifted_mark_lines(lines: &HashSet<usize>, start: usize, old_end: usize, delta: isize) -> HashSet<usize> {
    let new_end = old_end as isize + delta;
    lines
        .iter()
        .filter_map(|&line| {
            if line <= start {
                Some(line)
            } else if line > old_end {
                usize::try_from(line as isize + delta).ok()
            } else {
                (line as isize <= new_end).then_some(line)
            }
        })
        .collect()
}

impl App {
    // Cascadia Mono ships with Windows Terminal and recent Windows builds but
    // isn't guaranteed present (e.g. Windows 10 without Terminal installed),
    // so this is resolved once and cached rather than assumed.
    pub(super) fn code_font_family() -> &'static str {
        static FAMILY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
        FAMILY.get_or_init(|| {
            if family_available("Cascadia Code") {
                "Cascadia Code"
            } else if family_available("Cascadia Mono") {
                "Cascadia Mono"
            } else {
                "Consolas"
            }
        })
    }

    pub(super) fn font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
        let font_name = wide(Self::code_font_family());
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
                FIXED_PITCH as u32 | (FF_MODERN as u32),
                font_name.as_ptr(),
            )
        }
    }

    // Real ascent+descent+leading for the resolved font, instead of a fixed
    // guess — keeps the editor and terminal's cell grid consistent with
    // whichever font actually got selected (Cascadia Mono or the Consolas
    // fallback have different metrics).
    fn measured_line_height(font: HFONT, dpi: u32, zoom: i32) -> i32 {
        unsafe {
            let hdc = GetDC(null_mut());
            if hdc.is_null() {
                return scaled(21, dpi, zoom);
            }
            let old = SelectObject(hdc, font);
            let mut metrics: TEXTMETRICW = zeroed();
            let ok = GetTextMetricsW(hdc, &mut metrics);
            SelectObject(hdc, old);
            ReleaseDC(null_mut(), hdc);
            if ok == 0 {
                return scaled(21, dpi, zoom);
            }
            (metrics.tmHeight + metrics.tmExternalLeading).max(scaled(12, dpi, zoom))
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

    // Display sizes used by the welcome screen's hero block. They are far
    // larger than brand_font, which is sized for tab strips and headers.
    pub(super) fn title_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
        Self::display_font(23, dpi, zoom)
    }

    pub(super) fn hero_font_for_dpi(dpi: u32, zoom: i32) -> HFONT {
        Self::display_font(38, dpi, zoom)
    }

    fn display_font(points: i32, dpi: u32, zoom: i32) -> HFONT {
        let font_name = wide("Segoe UI Semibold");
        unsafe {
            CreateFontW(
                -scaled(points, dpi, zoom),
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

    pub(super) fn new(hwnd: HWND, brand_icon: HICON, hero_icon: HICON) -> Self {
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let zoom = 100;
        let font = Self::font_for_dpi(dpi, zoom);
        let line_height = Self::measured_line_height(font, dpi, zoom);
        let (worker_tx, worker_rx) = mpsc::channel();
        let (lsp_tx, lsp_rx) = mpsc::channel();
        let (debug_tx, debug_rx) = mpsc::channel();
        let (settings, settings_error) = match lightline::settings::Settings::try_load() {
            Ok(settings) => (settings, None),
            Err(error) => (lightline::settings::Settings::default(), Some(error)),
        };
        let theme = Theme::default_dark().with_overrides(&settings.colors);
        Self {
            tabs: vec![Tab::new(Document::new())],
            active: 0,
            pane_tabs: [0, 0],
            focused_pane: 0,
            split_visible: false,
            split_ratio: 50,
            divider_dragging: false,
            tab_first: 0,
            font,
            ui_font: Self::ui_font_for_dpi(dpi, zoom),
            brand_font: Self::brand_font_for_dpi(dpi, zoom),
            title_font: Self::title_font_for_dpi(dpi, zoom),
            hero_font: Self::hero_font_for_dpi(dpi, zoom),
            brand_icon,
            hero_icon,
            icons: IconSet::new(dpi, zoom),
            theme,
            active_color_theme: None,
            dpi,
            zoom,
            line_height,
            backbuffer: None,
            scrollbar_visible: None,
            transition: None,
            status: settings_error.unwrap_or_else(|| "Ready".into()),
            focused: false,
            caret_on: true,
            dragging: false,
            find_mode: false,
            find_query: String::new(),
            replace_mode: false,
            replace_query: String::new(),
            replace_field: 0,
            pending_high_surrogate: None,
            explorer_visible: true,
            sidebar_width: SIDEBAR,
            sidebar_user_width: SIDEBAR,
            sidebar_from: SIDEBAR,
            sidebar_target: SIDEBAR,
            sidebar_started: None,
            sidebar_dragging: false,
            terminal_height: 210,
            terminal_resizing: false,
            terminal_selecting: false,
            terminal_select_anchor: None,
            terminal_select_end: None,
            explorer_first_row: 0,
            explorer_input: None,
            selected_explorer_path: None,
            workspace_root: None,
            workspace_branch: None,
            expanded_dirs: HashSet::new(),
            directory_cache: HashMap::new(),
            welcome: true,
            ai_assistant_visible: false,
            side_view: SideView::Files,
            quick_open: false,
            quick_query: String::new(),
            quick_selected: 0,
            quick_first: 0,
            quick_files: Vec::new(),
            quick_loading: false,
            search_input: false,
            project_query: String::new(),
            search_results: Vec::new(),
            search_cancel: None,
            terminal: TerminalService::new({
                let hwnd_value = hwnd as isize;
                move || unsafe {
                    PostMessageW(hwnd_value as HWND, TERMINAL_EVENT_MESSAGE, 0, 0);
                }
            }),
            terminal_tab: TerminalTab::Terminal,
            terminals: Vec::new(),
            terminal_active: 0,
            terminal_counter: 0,
            run_session: None,
            run_snapshot: None,
            run_applied_size: None,
            terminal_visible: false,
            terminal_focus: false,
            cell_width: 0,
            changes: Vec::new(),
            review_loading: false,
            review_file: None,
            review_staged: false,
            git_root: None,
            git_busy: false,
            gutter_request: None,
            gutter_done: None,
            git_generation: 0,
            git_ahead: 0,
            git_behind: 0,
            git_conflicted: false,
            git_staged_collapsed: false,
            git_changes_collapsed: false,
            git_history_collapsed: false,
            commit_message: String::new(),
            commit_focus: false,
            commit_after_stage: false,
            history: Vec::new(),
            diff_rows: Vec::new(),
            diff_first: 0,
            panel_first: 0,
            panel_selected: 0,
            panel_focus: false,
            recent: workflow::recent_workspaces(),
            worker_tx,
            worker_rx,
            pending_workers: 0,
            lsp: HashMap::new(),
            lsp_event_tx: lsp_tx,
            lsp_events: lsp_rx,
            lsp_failed_at: HashMap::new(),
            python_interpreter: None,
            hover_mouse: None,
            hover_target: None,
            hover_card: None,
            hover_request_id: 1000,
            definition_target: None,
            references_target: None,
            format_target: None,
            request_id: 5000,
            completion_request: None,
            completion: None,
            restoring: false,
            watcher: Some(lightline::watcher::FileWatcher::start()),
            settings,
            git_diff_cache: HashMap::new(),
            git_head_cache: HashMap::new(),
            gutter_generation: 0,
            status_seen: RefCell::new((String::new(), Instant::now())),
            hwnd,
            git_untracked: HashSet::new(),
            git_watch_files: Vec::new(),
            extensions: vec![
                Extension {
                    id: "prettier".into(),
                    name: "Prettier - Code formatter".into(),
                    publisher: "esbenp".into(),
                    version: "v3.4.2".into(),
                    description: "Code formatter using prettier for JS, TS, HTML, CSS".into(),
                    downloads: "42.8M".into(),
                    rating: "★ 4.8".into(),
                    installed: false,
                    installing: false,
                },
                Extension {
                    id: "material-icons".into(),
                    name: "Material Icon Theme".into(),
                    publisher: "Zed Industries (zed-extensions/material-icon-theme)".into(),
                    version: "1.3.1".into(),
                    description: "Material Design file & folder icons, consumed from the real Zed extension registry".into(),
                    downloads: "24.1M".into(),
                    rating: "★ 4.9".into(),
                    installed: lightline::extensions::installer::is_installed("material-icon-theme"),
                    installing: false,
                },
            ],
            extensions_tab: ExtensionsTab::Marketplace,
            extensions_query: String::new(),
            zed_registry_loaded: false,
            zed_registry_loading: false,
            debug: None,
            debug_event_tx: debug_tx,
            debug_events: debug_rx,
            debug_state: DebugState {
                status: "Not running".into(),
                ..Default::default()
            },
            debug_pending_root: None,
            debug_pending_breakpoints: Vec::new(),
            extensions_search_active: false,
        }
    }

    // Replaces the active color theme at runtime and repaints. Not called
    // from anywhere yet (no theme-switching UI, no color-theme parser) --
    // this is the extension point a future Zed color-theme adapter uses,
    // the same way icon themes already replace `self.icons`' loaded theme.
    #[allow(dead_code)]
    pub(super) fn set_theme(&mut self, hwnd: HWND, theme: Theme) {
        self.theme = theme;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn has_extension(&self, id: &str) -> bool {
        self.extensions
            .iter()
            .any(|ext| ext.id == id && ext.installed)
    }

    pub(super) fn filtered_extensions(&self) -> Vec<&Extension> {
        let q = self.extensions_query.trim().to_lowercase();
        self.extensions
            .iter()
            .filter(|ext| {
                if self.extensions_tab == ExtensionsTab::Installed {
                    return ext.installed;
                }
                if q.is_empty() {
                    // Marketplace with no search yet: only the two curated
                    // entries LightLine ships knowledge of, not every id in
                    // the Zed registry the moment it's loaded -- the search
                    // box is what reveals the rest.
                    return ext.id == "prettier" || ext.id == "material-icons";
                }
                ext.id.to_lowercase().contains(&q)
                    || ext.name.to_lowercase().contains(&q)
                    || ext.description.to_lowercase().contains(&q)
                    || ext.publisher.to_lowercase().contains(&q)
            })
            .collect()
    }

    // Kicks off a one-time fetch of every id/version in the Zed registry, so
    // the search box has something beyond the two curated entries to match
    // against. Idempotent: does nothing once loaded or while already loading.
    pub(super) fn ensure_zed_registry_loaded(&mut self, hwnd: HWND) {
        if self.zed_registry_loaded || self.zed_registry_loading {
            return;
        }
        self.zed_registry_loading = true;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = lightline::extensions::zed_registry::list_ids();
            let _ = tx.send(WorkerMessage::ZedRegistryList(result));
        });
    }

    pub(super) fn toggle_extension(&mut self, hwnd: HWND, id: &str) {
        let Some(idx) = self.extensions.iter().position(|e| e.id == id) else {
            return;
        };
        if self.extensions[idx].installing {
            return;
        }
        if id != "prettier" {
            // Every non-Prettier entry is a real Zed extension -- either the
            // one LightLine installs itself (material-icons) or one found
            // via registry search -- installed/uninstalled the same way.
            let registry_id = zed_registry_id(id).to_string();
            if self.extensions[idx].installed {
                // A real uninstall (deletes the local files), not just a
                // preference flip -- IconSet already handles "no theme
                // loaded" gracefully, so this is a safe, meaningful action.
                let _ = lightline::extensions::installer::uninstall(&registry_id);
                self.extensions[idx].installed = false;
                let name = self.extensions[idx].name.clone();
                if registry_id == "material-icon-theme" {
                    self.icons = IconSet::new(self.dpi, self.zoom);
                }
                if self.active_color_theme.as_deref() == Some(registry_id.as_str()) {
                    self.theme = Theme::default_dark().with_overrides(&self.settings.colors);
                    self.active_color_theme = None;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                self.status = format!("{name} removed");
                self.refresh(hwnd);
                return;
            }
            self.extensions[idx].installing = true;
            let name = self.extensions[idx].name.clone();
            self.status = format!("Resolving {name} from the Zed registry...");
            let tx = self.worker_tx.clone();
            let worker_id = id.to_string();
            let color_overrides = self.settings.colors.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let outcome = lightline::extensions::zed_registry::resolve(&registry_id).and_then(
                    |resolved| {
                        let dir = lightline::extensions::installer::install(
                            &registry_id,
                            &resolved.git_url,
                            Some(&format!("v{}", resolved.version)),
                        )?;
                        if lightline::extensions::zed_manifest::is_icon_theme(&dir) {
                            return Ok(ExtensionInstallKind::IconTheme);
                        }
                        if lightline::extensions::zed_manifest::is_color_theme(&dir) {
                            let Some(zed_theme) =
                                lightline::color_theme::ZedColorTheme::load(&dir)
                            else {
                                let _ = lightline::extensions::installer::uninstall(&registry_id);
                                return Err(
                                    "its theme file could not be parsed".to_string()
                                );
                            };
                            let base = Theme::default_dark().with_overrides(&color_overrides);
                            let theme = Theme::from_zed_color_theme(&base, &zed_theme);
                            return Ok(ExtensionInstallKind::ColorTheme(theme));
                        }
                        // Not something LightLine can actually use yet
                        // (needs WASM execution) -- don't leave a dead,
                        // silently-half-installed entry behind.
                        let _ = lightline::extensions::installer::uninstall(&registry_id);
                        Err(
                            "isn't supported yet (LightLine can only use icon-theme and \
                             color-theme extensions right now)"
                                .to_string(),
                        )
                    },
                );
                let _ = tx.send(WorkerMessage::ZedExtensionInstalled(worker_id, outcome));
            });
            self.refresh(hwnd);
            return;
        }
        // Prettier: unrelated to the Zed registry, detected via PATH probing.
        {
            if self.extensions[idx].installed {
                // Turning this off is just a preference flip: LightLine never
                // owned an install to undo, so there's nothing to uninstall.
                self.extensions[idx].installed = false;
                self.status = "Prettier formatting disabled".into();
                self.refresh(hwnd);
                return;
            }
            // This used to run `npm install -g prettier`, mutating the
            // user's global npm state from a toggle in the editor, and would
            // still mark the extension "installed" via an npx fallback even
            // when that global install failed — an install state that wasn't
            // true and that LightLine didn't actually own. Detect instead:
            // format_with_prettier already tries `prettier` directly and
            // falls back to `npx --yes prettier`, so this only needs to
            // confirm one of those paths is actually usable before turning
            // the feature on.
            self.extensions[idx].installing = true;
            self.status = "Checking for Prettier...".into();
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                #[cfg(windows)]
                use std::os::windows::process::CommandExt;
                let probe = |program: &str, args: &[&str]| {
                    let mut cmd = std::process::Command::new(program);
                    cmd.args(args)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    #[cfg(windows)]
                    cmd.creation_flags(0x08000000);
                    cmd.status().map(|status| status.success()).unwrap_or(false)
                };
                let found = probe("prettier", &["--version"])
                    || probe("npx", &["--yes", "prettier", "--version"]);
                let _ = tx.send(WorkerMessage::ExtensionInstalled("prettier".into(), found));
            });
            self.refresh(hwnd);
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
    pub(super) fn ai_width(&self, rect: RECT) -> i32 {
        if self.ai_assistant_visible {
            self.scale(360).min((rect.right - self.editor_left()) / 2).max(self.scale(260))
        } else {
            0
        }
    }

    pub(super) fn editor_right(&self, hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let gap = self.chrome_gap();
        let total_right = rect.right - gap;
        if self.ai_assistant_visible {
            let ai_w = self.ai_width(rect);
            (total_right - ai_w - gap).max(self.editor_left() + self.scale(160))
        } else {
            total_right
        }
    }


    pub(super) fn pane_divider(&self, hwnd: HWND) -> i32 {
        let right = self.editor_right(hwnd);
        let width = (right - self.editor_left()).max(0);
        let minimum = self.scale(150).min(width / 2);
        self.editor_left() + (width * self.split_ratio / 100).clamp(minimum, width - minimum)
    }
    pub(super) fn resize_split(&mut self, hwnd: HWND, x: i32) {
        let right = self.editor_right(hwnd);
        let width = (right - self.editor_left()).max(1);
        self.split_ratio = (((x - self.editor_left()) * 100) / width).clamp(10, 90);
        self.update_scrollbar(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn resize_sidebar(&mut self, hwnd: HWND, x: i32) {
        let width = self.unscale((x - self.scale(RAIL)).max(0)).clamp(140, 480);
        self.sidebar_width = width;
        self.sidebar_user_width = width;
        self.sidebar_from = width;
        self.sidebar_target = width;
        self.sidebar_started = None;
        unsafe {
            KillTimer(hwnd, 5);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    pub(super) fn resize_terminal_panel(&mut self, hwnd: HWND, y: i32) {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let max_height = self.unscale((rect.bottom - self.editor_top()).max(0));
        let height = self
            .unscale((rect.bottom - self.scale(STATUS) - y).max(0))
            .clamp(80, max_height.saturating_sub(80).max(80));
        self.terminal_height = height;
        self.resize_terminal_to_fit(hwnd);
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
            self.editor_right(hwnd)
        }
    }

    // The side panel, editor and terminal are drawn as cards floating on the
    // window background. These four are the single source of truth for that
    // geometry: painting and hit testing both derive from them, so a card can
    // never be drawn somewhere clicks do not follow.
    pub(super) fn chrome_gap(&self) -> i32 {
        self.scale(CARD_GAP)
    }

    pub(super) fn chrome_top(&self) -> i32 {
        self.scale(WORKBENCH_HEADER)
    }

    pub(super) fn tab_strip_bottom(&self) -> i32 {
        self.chrome_top() + self.scale(TAB_HEIGHT)
    }

    // The command center is shared by painting and hit testing. Keeping it in
    // the tab chrome makes Quick Open discoverable without spending another
    // permanent row of vertical editor space.
    pub(super) fn command_center_rect(&self, hwnd: HWND) -> RECT {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let usable_left = self.scale(176);
        let usable_right = client.right - self.scale(46 * 3 + 16);
        let available = (usable_right - usable_left).max(0);
        let width = self
            .scale(520)
            .min((available - self.scale(32)).max(self.scale(260)));
        let left = usable_left + (available - width) / 2;
        RECT {
            left,
            top: self.scale(8),
            right: left + width,
            bottom: self.scale(WORKBENCH_HEADER - 8),
        }
    }

    // Right edge of the side panel card, which is one gap left of the editor.
    pub(super) fn sidebar_right(&self) -> i32 {
        self.scale(RAIL + self.sidebar_width)
    }

    pub(super) fn editor_top(&self) -> i32 {
        self.chrome_top() + self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT + TOP)
    }

    pub(super) fn editor_left(&self) -> i32 {
        self.sidebar_right() + self.chrome_gap()
    }

    // The controls at the right end of a pane's breadcrumb row, measured here
    // rather than in the painter so the glyph and the pixel that reacts to a
    // click are derived from the same numbers. A control sized inside the paint
    // code is a control that can end up drawn where clicks never land.
    pub(super) fn pane_actions(&self, pane_right: i32) -> (RECT, RECT) {
        let top = self.tab_strip_bottom();
        let bottom = self.editor_top();
        let width = self.scale(24);
        let more = RECT {
            left: pane_right - width - self.scale(6),
            top,
            right: pane_right - self.scale(6),
            bottom,
        };
        let split = RECT {
            left: more.left - width,
            top,
            right: more.left,
            bottom,
        };
        (split, more)
    }

    pub(super) fn set_sidebar_visible(&mut self, hwnd: HWND, visible: bool) {
        let target = if visible { self.sidebar_user_width } else { 0 };
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

    pub(super) fn toggle_side_view(&mut self, hwnd: HWND, view: SideView) {
        if self.side_view == SideView::Search && view != SideView::Search {
            self.cancel_search();
        }
        // The commit box only exists in the source control view.
        self.commit_focus = false;
        self.terminal_focus = false;
        if view != SideView::Extensions {
            self.extensions_search_active = false;
        }
        if self.side_view == view && self.explorer_visible {
            self.set_sidebar_visible(hwnd, false);
        } else {
            self.side_view = view;
            self.panel_focus = false;
            self.set_sidebar_visible(hwnd, true);
        }
        self.show_active_tab(hwnd);
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
        self.terminal_focus = false;
        self.find_mode = false;
        self.update_title(hwnd);
        self.update_scrollbar(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn new_file(&mut self, hwnd: HWND) {
        self.cancel_search();
        let from_welcome = self.welcome;
        self.welcome = false;
        self.search_input = false;
        self.panel_focus = false;
        self.terminal_focus = false;
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
        self.clear_hover(hwnd);
        let count = self.visible_tab_count(hwnd);
        if self.active < self.tab_first {
            self.tab_first = self.active;
        } else if self.active >= self.tab_first + count {
            self.tab_first = self.active + 1 - count;
        }
        self.find_mode = false;
        self.replace_mode = false;
        self.dragging = false;
        self.update_title(hwnd);
        self.update_scrollbar(hwnd);
        self.advance_syntax(hwnd);
        self.ensure_lsp(hwnd);
        self.refresh_active_git_diff(hwnd);
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
    }

    pub(super) fn activate_tab(&mut self, hwnd: HWND, index: usize) {
        if index < self.tabs.len() {
            self.terminal_focus = false;
            if self.side_view == SideView::Search {
                self.cancel_search();
            }
            if index != self.active {
                self.start_transition(hwnd);
            }
            self.side_view = SideView::Files;
            self.review_file = None;
            self.search_input = false;
            self.extensions_search_active = false;
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
        if let Some(path) = self.tabs.get(index).and_then(|t| t.document.path.clone())
            && let Some(watcher) = &self.watcher
        {
            watcher.unwatch_file(path);
        }
        self.close_lsp_tab(index);
        self.start_transition(hwnd);
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            if self.workspace_root.is_none() {
                self.welcome = true;
            }
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
        self.save_session();
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

    // Approximate inverse of `scale`, for turning a live drag position back
    // into the logical pixels layout fields are stored in. Only used for
    // interactive resize feedback, where sub-pixel drift is not visible.
    pub(super) fn unscale(&self, pixels: i32) -> i32 {
        let denom = (self.dpi as i64 * self.zoom as i64).max(1);
        ((pixels as i64 * 9600) / denom) as i32
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
        let title_font = Self::title_font_for_dpi(dpi, zoom);
        let hero_font = Self::hero_font_for_dpi(dpi, zoom);
        // All or nothing: a partial swap would leave the app drawing with a
        // mix of old and new metrics, so drop everything and keep the old set.
        let created = [font, ui_font, brand_font, title_font, hero_font];
        if created.iter().any(|handle| handle.is_null()) {
            for handle in created {
                if !handle.is_null() {
                    unsafe {
                        DeleteObject(handle);
                    }
                }
            }
            return;
        }
        unsafe {
            DeleteObject(self.font);
            DeleteObject(self.ui_font);
            DeleteObject(self.brand_font);
            DeleteObject(self.title_font);
            DeleteObject(self.hero_font);
        }
        self.line_height = Self::measured_line_height(font, dpi, zoom);
        self.font = font;
        self.ui_font = ui_font;
        self.brand_font = brand_font;
        self.title_font = title_font;
        self.hero_font = hero_font;
        self.icons = IconSet::new(dpi, zoom);
        self.dpi = dpi;
        self.zoom = zoom;
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
            - if self.terminal_visible {
                self.scale(self.terminal_height)
            } else {
                0
            })
        .max(1)
            / self.line_height)
            .max(1) as usize
    }

    pub(super) fn update_scrollbar(&mut self, hwnd: HWND) {
        // The vertical scrollbar is a permanent window-style feature (WS_VSCROLL),
        // so it stays visible with whatever thumb was last set for the editor
        // unless explicitly hidden here — it must not bleed into the welcome pane.
        let show = !self.welcome;
        if self.scrollbar_visible != Some(show) {
            unsafe { ShowScrollBar(hwnd, SB_VERT, show as i32) };
            self.scrollbar_visible = Some(show);
        }
        if !show {
            return;
        }
        let visible = self.visible_lines(hwnd);
        // The scrollbar counts screen rows, so folded lines don't inflate
        // the range or push the thumb down.
        let doc = self.doc();
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: doc.visible_line_count().saturating_sub(1).min(i32::MAX as usize) as i32,
            nPage: visible as u32,
            nPos: doc.visual_index(self.view().first_line).min(i32::MAX as usize) as i32,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(hwnd, SB_VERT, &info, 1);
        }
    }

    pub(super) fn keep_cursor_visible(&mut self, hwnd: HWND) {
        // A minimized window has no rows; scrolling the caret into that view
        // would leave the editor scrolled to the caret line after restoring.
        if unsafe { IsIconic(hwnd) } != 0 {
            return;
        }
        let visible = self.visible_lines(hwnd);
        let line = self.view().cursor.line;
        // The caret must never sit inside collapsed text, whatever moved it
        // there (search, go to definition, undo): open the folds around it.
        let anchor = self.view().selection_anchor;
        let doc = self.doc_mut();
        doc.unfold_to_reveal(line);
        if let Some(anchor) = anchor {
            doc.unfold_to_reveal(anchor.line);
        }
        let first = self.doc().visible_line_for(self.view().first_line);
        self.view_mut().first_line = first;
        if line < first {
            self.view_mut().first_line = line;
        } else if self.doc().visual_row_of(first, line, visible - 1).is_none() {
            // Scroll so the caret is on the last row: `visible - 1` visible
            // lines above it, skipping folded ones.
            self.view_mut().first_line = self.doc().step_visible_lines(line, 1 - visible as isize);
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
            "{}{} — LightLine IDE",
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
        // The single choke point every edit path (typing, backspace, delete,
        // paste, cut) goes through. A generated preview tab (image or hex dump)
        // holds a placeholder document that must never be written to disk as if
        // it were real content, so refuse to touch it here rather than trusting
        // every caller to check first.
        if self.tab().read_only() {
            return;
        }
        let lines_before = self.doc().line_count();
        let cursor = self.doc_mut().replace(start, end, text);
        let delta = self.doc().line_count() as isize - lines_before as isize;
        self.shift_gutter_marks(start.line, end.line, delta);
        self.schedule_gutter_diff();
        self.syntax_changed(start.line);
        self.view_mut().cursor = cursor;
        self.revalidate_other_view(Some((start, end, cursor)));
        self.sync_lsp_edit();
        self.hover_target = None;
        self.hover_card = None;
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

    pub(super) fn replace_next(&mut self, hwnd: HWND) {
        if self.find_query.is_empty() {
            self.status = "Find & Replace: enter search term".into();
            self.refresh(hwnd);
            return;
        }
        let replacement = self.replace_query.clone();
        if let Some((start, end)) = self.selection_range() {
            let sel_text = self.doc().text_range(start, end);
            if sel_text == self.find_query {
                self.replace_selection(&replacement);
            }
        }
        self.find(hwnd, true);
    }

    pub(super) fn replace_all(&mut self, hwnd: HWND) {
        if self.find_query.is_empty() {
            self.status = "Find & Replace: enter search term".into();
            self.refresh(hwnd);
            return;
        }
        let replacement = self.replace_query.clone();
        let mut count = 0;
        let mut pos = Pos::default();
        while let Some(start) = self.doc().find_forward(pos, &self.find_query) {
            let end = Pos {
                line: start.line,
                byte: start.byte + self.find_query.len(),
            };
            self.replace_range(start, end, &replacement);
            count += 1;
            pos = Pos {
                line: start.line,
                byte: start.byte + replacement.len(),
            };
            if pos.line >= self.doc().line_count() {
                break;
            }
        }
        self.find_mode = false;
        self.replace_mode = false;
        self.status = format!("Replaced {} occurrence(s) of '{}'", count, self.find_query);
        self.refresh(hwnd);
    }

    pub(super) fn update_find_replace_status(&mut self) {
        let f_marker = if self.replace_field == 0 { "> " } else { "  " };
        let r_marker = if self.replace_field == 1 { "> " } else { "  " };
        self.status = format!(
            "{}Find: {} | {}Replace: {}  [Tab: switch, Enter: Replace Next, Alt+Enter: Replace All, Esc: Exit]",
            f_marker, self.find_query, r_marker, self.replace_query
        );
    }

    pub(super) fn poll_watcher(&mut self, hwnd: HWND) {
        let Some(watcher) = &self.watcher else { return; };
        let events = watcher.poll();
        if events.is_empty() { return; }
        let mut needs_refresh = false;
        let mut git_changed = false;
        for event in events {
            match event {
                lightline::watcher::WatchEvent::FileChanged(path)
                    if self.git_watch_files.contains(&path) =>
                {
                    git_changed = true;
                }
                lightline::watcher::WatchEvent::FileChanged(path) => {
                    for tab in &mut self.tabs {
                        if tab
                            .document
                            .path
                            .as_deref()
                            .is_some_and(|p| Self::same_path(p, &path))
                        {
                            if !tab.document.is_dirty() {
                                if let Ok(doc) = Document::open(path.clone()) {
                                    tab.document = doc;
                                    if let Some(syntax) = &mut tab.syntax {
                                        syntax.invalidate_from(0);
                                    }
                                    self.status = format!(
                                        "Reloaded: {}",
                                        path.file_name().unwrap_or_default().to_string_lossy()
                                    );
                                    needs_refresh = true;
                                }
                            } else {
                                self.status = format!(
                                    "External change in {} (unsaved edits kept)",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                );
                                needs_refresh = true;
                            }
                        }
                    }
                }
                lightline::watcher::WatchEvent::DirectoryChanged(dir) => {
                    self.directory_cache.remove(&dir);
                    if self.workspace_root.as_ref() == Some(&dir) || self.expanded_dirs.contains(&dir) {
                        self.load_directory(&dir);
                    }
                    needs_refresh = true;
                }
            }
        }
        if needs_refresh {
            // A reload (e.g. after Discard) replaces the buffer without going
            // through replace_range, so the gutter has to be recomputed here.
            self.schedule_gutter_diff();
            self.backbuffer = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        // Something outside LightLine's own Git actions (which refresh when
        // they finish) staged, committed or checked out -- e.g. in the
        // built-in terminal, where the window never loses focus.
        if git_changed && !self.git_busy {
            self.refresh_git(hwnd);
        }
    }

    /// Gutter change marks for the visible file, read with `git diff HEAD`.
    /// This runs on the worker channel: doing it inline spawned two Git
    /// processes per repaint, and repaints follow every keystroke.
    pub(super) fn refresh_active_git_diff(&mut self, hwnd: HWND) {
        let Some(path) = self.doc().path.clone() else {
            return;
        };
        // Only files inside a Git repository get change marks; a plain folder
        // (or a file outside the repo) has nothing to compare against.
        let Some(root) = self.git_root.clone() else {
            return;
        };
        let Some(relative) = repo_relative(&path, &root) else {
            return;
        };
        // Gutter marks compare the file on disk with HEAD, so unsaved edits do
        // not change them: one read per file until it is saved or the
        // repository moves. Drop `gutter_done` to ask again.
        if self.gutter_done.as_deref() == Some(path.as_path()) {
            return;
        }
        if self.gutter_request.as_ref() == Some(&path) {
            return;
        }
        self.gutter_request = Some(path.clone());
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let head_text = workflow::git_head_text(&root, &relative).ok();
            let result = workflow::git_diff(&root, &relative, DiffScope::Head);
            let _ = tx.send(WorkerMessage::GutterDiff(path, relative, head_text, result));
        });
    }

    pub(super) fn gutter_diff_finished(
        &mut self,
        path: &Path,
        head_text: Option<String>,
        result: Result<Vec<DiffRow>, String>,
    ) {
        if self.gutter_request.as_deref() != Some(path) {
            return;
        }
        self.gutter_request = None;
        self.gutter_done = Some(path.to_path_buf());
        if let Some(text) = head_text {
            self.git_untracked.remove(path);
            self.git_head_cache.insert(path.to_path_buf(), text.into());
        } else {
            self.git_head_cache.remove(path);
            // Not in HEAD. `git diff` lists a new (untracked or newly staged)
            // file as all-added rows; anything else is an error, which shows
            // no marks rather than a misleading all-green gutter.
            if matches!(&result, Ok(rows) if rows.iter().all(|row| row.before_number.is_none())) {
                self.git_untracked.insert(path.to_path_buf());
            } else {
                self.git_untracked.remove(path);
            }
        }
        self.start_gutter_diff();
    }

    /// Asks for the active buffer's gutter marks to be recomputed once typing
    /// pauses. Called after every edit: restarting the timer coalesces a burst
    /// of keystrokes into a single diff.
    /// The status message while it's fresh: shown for a few seconds after it
    /// changes, so a stale "Saved x" doesn't sit in the status bar forever.
    /// Paint calls this, so the change is tracked through a RefCell.
    pub(super) fn fresh_status(&self) -> Option<&str> {
        const VISIBLE: std::time::Duration = std::time::Duration::from_secs(5);
        let mut seen = self.status_seen.borrow_mut();
        if seen.0 != self.status {
            *seen = (self.status.clone(), Instant::now());
            if !self.status.is_empty() {
                // Repaint once it expires so the message goes away on time.
                unsafe { SetTimer(self.hwnd, STATUS_TIMER, VISIBLE.as_millis() as u32 + 50, None) };
            }
        }
        (!self.status.is_empty() && seen.1.elapsed() < VISIBLE).then_some(self.status.as_str())
    }

    pub(super) fn schedule_gutter_diff(&mut self) {
        unsafe { SetTimer(self.hwnd, GUTTER_DIFF_TIMER, 120, None) };
    }

    /// Moves the active buffer's gutter marks with an edit that replaced
    /// lines `start..=old_end` and changed the line count by `delta`, so
    /// they stay on their lines until the debounced recompute lands.
    pub(super) fn shift_gutter_marks(&mut self, start: usize, old_end: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        let Some(path) = self.doc().path.clone() else {
            return;
        };
        let Some(diff) = self.git_diff_cache.get_mut(&path) else {
            return;
        };
        for lines in [&mut diff.added, &mut diff.modified, &mut diff.deleted] {
            *lines = shifted_mark_lines(lines, start, old_end, delta);
        }
    }

    /// Recomputes the active buffer's gutter marks against its cached HEAD
    /// text. The diff runs on a worker thread against a snapshot of the
    /// buffer; `gutter_generation` discards a result that a newer request has
    /// already superseded.
    pub(super) fn start_gutter_diff(&mut self) {
        const LIVE_GUTTER_LINE_LIMIT: usize = 50_000;
        unsafe { KillTimer(self.hwnd, GUTTER_DIFF_TIMER) };
        let Some(path) = self.doc().path.clone() else {
            return;
        };
        self.gutter_generation += 1;
        if self.doc().line_count() > LIVE_GUTTER_LINE_LIMIT {
            self.git_diff_cache.remove(&path);
        } else if let Some(head_text) = self.git_head_cache.get(&path).cloned() {
            let lines = self.doc().lines().to_vec();
            let generation = self.gutter_generation;
            let tx = self.worker_tx.clone();
            self.worker_started(self.hwnd);
            std::thread::spawn(move || {
                let diff = workflow::compute_gutter_diff(&head_text, &lines);
                let _ = tx.send(WorkerMessage::GutterComputed(generation, path, diff));
            });
            return;
        } else if self.git_untracked.contains(&path) {
            let added = (0..self.doc().line_count()).collect();
            self.git_diff_cache.insert(path, GutterDiff { added, ..GutterDiff::default() });
        } else {
            self.git_diff_cache.remove(&path);
        }
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    pub(super) fn gutter_diff_computed(&mut self, generation: u64, path: PathBuf, diff: GutterDiff) {
        if generation == self.gutter_generation {
            self.git_diff_cache.insert(path, diff);
        }
    }

    pub(super) fn error(&mut self, hwnd: HWND, error: &impl std::fmt::Display) {
        self.status = error.to_string();
        dialog::show_dialog(
            hwnd,
            "LightLine",
            &self.status,
            dialog::DialogIcon::Error,
            &[dialog::BTN_OK],
        );
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
        if self.tab().image.is_some() {
            self.status = "This is an image preview; there is nothing to save".into();
            return false;
        }
        if self.tab().binary_preview {
            self.status = "This is a read-only hex preview; there is nothing to save".into();
            return false;
        }
        let old_path = self.doc().path.clone();
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
        self.apply_format_on_save(&path);
        match self.doc_mut().save(&path) {
            Ok(()) => {
                self.lsp_after_save(hwnd, old_path.as_deref());
                self.tab_mut().update_syntax_language();
                self.check_c_syntax_on_save(hwnd, &path);
                self.set_workspace_from_file(&path);
                self.reveal_file_in_explorer(&path);
                if let Some(parent) = path.parent() {
                    self.directory_cache.remove(parent);
                    if self.expanded_dirs.contains(parent) || self.workspace_root.as_deref() == Some(parent) {
                        self.load_directory(parent);
                    }
                }
                self.status = format!(
                    "Saved {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                if lightline::settings::Settings::settings_path()
                    .is_some_and(|settings| Self::same_path(&settings, &path))
                    && self.reload_settings()
                {
                    self.status = "Settings saved and applied".into();
                }
                self.gutter_done = None;
                self.refresh_active_git_diff(hwnd);
                // A save is the moment a change appears or disappears, so the
                // panel and the branch chip ask Git again rather than guessing.
                self.refresh_git(hwnd);
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
        let message = format!("Save changes to {}?", self.tab_label(self.active));
        let answer = dialog::show_dialog(
            hwnd,
            "LightLine",
            &message,
            dialog::DialogIcon::Question,
            &dialog::yes_no_cancel(),
        );
        if answer == dialog::DLG_YES {
            self.save(hwnd, false)
        } else {
            answer == dialog::DLG_NO
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
        let new_tab = if image_view::is_image_path(&path) {
            image_view::load_image(&path)
                .map(|image| Tab::new_image(path.clone(), image))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Could not decode this image")
                })
        } else {
            match Document::open(path.clone()) {
                Ok(document) => Ok(Tab::new(document)),
                // Not valid UTF-8 text and not a decodable image: fall back to
                // a read-only hex dump instead of refusing to open the file.
                Err(error) if error.kind() == io::ErrorKind::InvalidData => std::fs::read(&path)
                    .map(|bytes| Tab::new_binary_preview(path.clone(), &bytes)),
                Err(error) => Err(error),
            }
        };
        match new_tab {
            Ok(tab) => {
                let preview = tab.binary_preview;
                self.terminal_focus = false;
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
                    self.tabs[0] = tab;
                    self.set_active_index(0);
                } else {
                    self.tabs.push(tab);
                    self.set_active_index(self.tabs.len() - 1);
                }
                self.set_workspace_from_file(&path);
                self.reveal_file_in_explorer(&path);
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.status = if preview {
                    format!("Opened {name} (read-only hex preview)")
                } else {
                    format!("Opened {name}")
                };
                self.show_active_tab(hwnd);
                if let Some(watcher) = &self.watcher {
                    watcher.watch_file(path.clone());
                }
                self.gutter_done = None;
                self.refresh_active_git_diff(hwnd);
                self.save_session();
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

    #[test]
    fn binary_preview_tab_is_a_read_only_unsaved_hex_dump() {
        let bytes = [0x00, 0x01, 0xff, b'H', b'i'];
        let tab = Tab::new_binary_preview(std::path::PathBuf::from("blob.bin"), &bytes);
        assert!(tab.binary_preview);
        assert!(tab.read_only());
        // Seeding must not mark the document dirty, so closing never prompts
        // to save the generated hex text back over the real file.
        assert!(!tab.document.is_dirty());
        assert!(tab.document.line(0).starts_with("00000000  "));
        assert!(tab.document.line(0).contains("00 01 ff 48 69"));
        assert!(tab.document.line(0).ends_with("|...Hi|"));
    }

    #[test]
    fn find_and_replace_all_replaces_all_occurrences() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "hello world hello rust hello");
        let query = "hello";
        let replacement = "hi";
        let mut count = 0;
        let mut pos = Pos::default();
        while let Some(start) = doc.find_forward(pos, query) {
            let end = Pos {
                line: start.line,
                byte: start.byte + query.len(),
            };
            doc.replace(start, end, replacement);
            count += 1;
            pos = Pos {
                line: start.line,
                byte: start.byte + replacement.len(),
            };
        }
        assert_eq!(count, 3);
        assert_eq!(doc.line(0), "hi world hi rust hi");
    }

    #[test]
    fn extensions_initial_list_and_toggle() {
        let mut extensions = [
            Extension {
                id: "prettier".into(),
                name: "Prettier - Code formatter".into(),
                publisher: "esbenp".into(),
                version: "v3.4.2".into(),
                description: "Code formatter using prettier for JS, TS, HTML, CSS".into(),
                downloads: "42.8M".into(),
                rating: "★ 4.8".into(),
                installed: false,
                installing: false,
            },
            Extension {
                id: "material-icons".into(),
                name: "Material Icon Theme".into(),
                publisher: "Philipp Kief".into(),
                version: "v5.1.0".into(),
                description: "Material Design file & folder icons for LightLine".into(),
                downloads: "24.1M".into(),
                rating: "★ 4.9".into(),
                installed: true,
                installing: false,
            },
        ];
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].name, "Prettier - Code formatter");
        assert!(!extensions[0].installed);
        assert_eq!(extensions[1].name, "Material Icon Theme");
        assert!(extensions[1].installed);

        // Toggle install
        extensions[0].installed = !extensions[0].installed;
        assert!(extensions[0].installed);
        extensions[0].installed = !extensions[0].installed;
        assert!(!extensions[0].installed);
    }

    #[test]
    fn explorer_input_state_initialization() {
        let state = ExplorerInputState {
            is_folder: false,
            is_rename: false,
            target_dir: PathBuf::from("C:\\test\\workspace"),
            old_path: None,
            buffer: "hello.rs".into(),
        };
        assert!(!state.is_folder);
        assert!(!state.is_rename);
        assert_eq!(state.buffer, "hello.rs");
        assert_eq!(state.target_dir, PathBuf::from("C:\\test\\workspace"));
    }

    #[test]
    fn explorer_file_create_rename_delete() {
        let temp_dir = std::env::temp_dir().join(format!("lightline_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create file
        let file_path = temp_dir.join("test_file.txt");
        std::fs::File::create(&file_path).unwrap();
        assert!(file_path.exists());

        // Rename file
        let new_file_path = temp_dir.join("renamed_file.txt");
        std::fs::rename(&file_path, &new_file_path).unwrap();
        assert!(!file_path.exists());
        assert!(new_file_path.exists());

        // Delete file
        std::fs::remove_file(&new_file_path).unwrap();
        assert!(!new_file_path.exists());

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn gutter_marks_move_with_inserted_and_deleted_lines() {
        let marks: HashSet<usize> = [1, 5, 9].into_iter().collect();
        let sorted = |set: HashSet<usize>| {
            let mut lines: Vec<usize> = set.into_iter().collect();
            lines.sort_unstable();
            lines
        };
        // Enter on line 3: one line inserted after it; marks below move down.
        assert_eq!(sorted(shifted_mark_lines(&marks, 3, 3, 1)), vec![1, 6, 10]);
        // Lines 4..=6 joined into line 4 (two lines removed): the mark on
        // removed line 5 goes, the one below moves up.
        assert_eq!(sorted(shifted_mark_lines(&marks, 4, 6, -2)), vec![1, 7]);
        // Undo of that: two lines re-inserted after line 4.
        assert_eq!(sorted(shifted_mark_lines(&[1, 7].into_iter().collect(), 4, 4, 2)), vec![1, 9]);
    }

    #[test]
    fn repo_relative_matches_canonical_paths_against_git_roots() {
        // An opened file (canonicalize() form) against Git's own root form.
        assert_eq!(
            repo_relative(Path::new(r"\\?\C:\Work\repo\src\main.rs"), Path::new("C:/Work/repo")),
            Some(PathBuf::from(r"src\main.rs"))
        );
        // Case differences, as Windows paths are case-insensitive.
        assert_eq!(
            repo_relative(Path::new(r"\\?\C:\work\Repo\a.rs"), Path::new("C:/Work/repo")),
            Some(PathBuf::from("a.rs"))
        );
        // A file outside the repository has no repo-relative path.
        assert_eq!(repo_relative(Path::new(r"\\?\C:\Other\a.rs"), Path::new("C:/Work/repo")), None);
        assert_eq!(repo_relative(Path::new(r"C:\Work\repository\a.rs"), Path::new("C:/Work/repo")), None);
    }
}
