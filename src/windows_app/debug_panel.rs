// The Run & Debug panel's layout, rows, clicks and hover
// (design/run-debug-design.html). Painting is in render/debug.rs; both use
// `debug_panel_layout` and `debug_rows`, so what's drawn is what's clicked.

use super::*;

// In logical pixels from the top of the panel's 40 px header.
const BAR_TOP: i32 = 48;
const BAR_HEIGHT: i32 = 28;
// The ▶ at the launch bar's left; the configuration picker takes the rest.
const GO_WIDTH: i32 = 30;
// The session toolbar's buttons.
const TOOL_WIDTH: i32 = 28;
const TOOL_HEIGHT: i32 = 22;
const STATE_HEIGHT: i32 = 22;
pub(super) const DEBUG_ROW: i32 = EXPLORER_ROW;
pub(super) const DEBUG_INDENT: i32 = 20;
// A child variable sits this much further right than its parent.
pub(super) const DEBUG_NEST: i32 = 12;

/// The panel's buttons outside the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DebugButton {
    /// ··· in the header.
    More,
    /// The launch bar's ▶ and its configuration picker.
    Go,
    Pick,
    /// The session toolbar: 0 continue or pause, 1 step over, 2 step into,
    /// 3 step out, 4 restart, 5 stop.
    Tool(usize),
    /// The status line's link (Show Details, Install).
    Link,
}

/// What the mouse is over in the panel, for hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DebugHot {
    Button(DebugButton),
    Row(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DebugSection {
    Variables,
    CallStack,
    Breakpoints,
}

impl DebugSection {
    fn index(self) -> usize {
        match self {
            Self::Variables => 0,
            Self::CallStack => 1,
            Self::Breakpoints => 2,
        }
    }
}

/// One 22 px line of the panel's list.
pub(super) enum DebugRow {
    Header {
        section: DebugSection,
        title: &'static str,
        count: usize,
    },
    /// A muted line of explanation.
    Note(String),
    /// A scope such as Locals.
    Scope(String),
    Variable {
        depth: usize,
        /// DAP variablesReference; 0 for a value without children.
        reference: i64,
        expanded: bool,
        name: String,
        value: String,
    },
    /// Children still on their way from the debugger.
    Loading(usize),
    /// A call stack frame, by its index in `debug_state.frames`.
    Frame(usize),
    /// A breakpoint: its tab and zero-based line.
    Breakpoint { tab: usize, line: usize },
}

/// What the status line says.
pub(super) struct DebugStatus {
    pub tone: DebugTone,
    pub title: String,
    pub detail: String,
    pub link: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DebugTone {
    Idle,
    Busy,
    Running,
    Paused,
    Failed,
}

pub(super) struct DebugLayout {
    pub more: RECT,
    /// The launch bar (or, during a session, the toolbar's frame).
    pub bar: RECT,
    pub go: RECT,
    pub pick: RECT,
    pub tools: [RECT; 6],
    pub state: RECT,
    pub divider: i32,
    pub list_top: i32,
}

pub(super) enum DebugHit {
    Button(DebugButton),
    Header(DebugSection),
    CollapseVariables,
    RemoveBreakpoints,
    Variable(i64),
    Frame(usize),
    Breakpoint { tab: usize, line: usize },
    RemoveBreakpoint { tab: usize, line: usize },
    Nothing,
}

fn contains(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

impl App {
    /// A session is on, or its build is: the toolbar replaces the launch bar.
    pub(super) fn debug_session_shown(&self) -> bool {
        self.debug.is_some() || self.debug_pending_root.is_some()
    }

    pub(super) fn debug_panel_layout(&self, left: i32, right: i32) -> DebugLayout {
        let s = |value: i32| self.scale(value);
        let bar = RECT {
            left: left + s(12),
            top: s(BAR_TOP),
            right: right - s(12),
            bottom: s(BAR_TOP + BAR_HEIGHT),
        };
        let go = RECT {
            right: bar.left + s(GO_WIDTH),
            ..bar
        };
        let pick = RECT {
            left: go.right,
            ..bar
        };
        let middle = (bar.top + bar.bottom) / 2;
        let mut x = bar.left + s(3);
        let tools = std::array::from_fn(|index| {
            if index == 4 {
                // A gap with a line in it between stepping and the session.
                x += s(7);
            }
            let rect = RECT {
                left: x,
                top: middle - s(TOOL_HEIGHT) / 2,
                right: x + s(TOOL_WIDTH),
                bottom: middle - s(TOOL_HEIGHT) / 2 + s(TOOL_HEIGHT),
            };
            x = rect.right + s(2);
            rect
        });
        let state_top = bar.bottom + s(6);
        let state = RECT {
            left: bar.left,
            top: state_top,
            right: bar.right,
            bottom: state_top + s(STATE_HEIGHT),
        };
        DebugLayout {
            more: RECT {
                left: right - s(36),
                top: s(SIDE_SHIFT),
                right: right - s(14),
                bottom: s(40),
            },
            bar,
            go,
            pick,
            tools,
            state,
            divider: state.bottom + s(4),
            list_top: state.bottom + s(7),
        }
    }

    /// Where the status line's link is, if it has one.
    pub(super) fn debug_link_rect(&self, layout: &DebugLayout, hdc: Option<HDC>) -> Option<RECT> {
        let link = self.debug_status().link?;
        let width = match hdc {
            Some(hdc) => self.text_width(hdc, link),
            None => unsafe {
                let hdc = GetDC(null_mut());
                let old = SelectObject(hdc, self.ui_font);
                let width = self.text_width(hdc, link);
                SelectObject(hdc, old);
                ReleaseDC(null_mut(), hdc);
                width
            },
        };
        Some(RECT {
            left: layout.state.right - width,
            ..layout.state
        })
    }

    /// Which session toolbar buttons can be pressed now.
    pub(super) fn debug_tool_enabled(&self, index: usize) -> bool {
        match index {
            0 | 4 => self.debug.is_some(),
            1..=3 => self.debug_paused(),
            _ => self.debug_session_shown(),
        }
    }

    pub(super) fn debug_status(&self) -> DebugStatus {
        let state = &self.debug_state;
        let status = |tone, title: &str, detail: &str, link| DebugStatus {
            tone,
            title: title.to_owned(),
            detail: detail.to_owned(),
            link,
        };
        let adapter = state.config.map_or("LLDB", DebugConfig::adapter_name);
        if self.debug.is_some() && state.running {
            return status(DebugTone::Running, "Running", adapter, None);
        }
        if self.debug_paused() {
            let reason = state.status.strip_prefix("Paused: ").unwrap_or("");
            let title = match reason {
                "breakpoint" | "exception" | "function breakpoint" => {
                    format!("Paused on {reason}")
                }
                _ => "Paused".to_owned(),
            };
            let place = state
                .frames
                .first()
                .and_then(|frame| {
                    let name = frame.path.as_ref()?.file_name()?.to_string_lossy();
                    Some(format!("{name}:{}", frame.line))
                })
                .unwrap_or_default();
            return status(DebugTone::Paused, &title, &place, None);
        }
        if self.debug_session_shown() {
            let detail = if self.debug.is_none() {
                "cargo build"
            } else {
                adapter
            };
            return status(DebugTone::Busy, &state.status, detail, None);
        }
        if let Some(error) = state.status.strip_prefix("Build failed: ") {
            let first = error
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            return status(
                DebugTone::Failed,
                "Build failed",
                first.trim(),
                Some("Show Details"),
            );
        }
        if state.status.contains("debugpy is not installed") {
            return status(
                DebugTone::Failed,
                "debugpy is not installed",
                "",
                Some("Install"),
            );
        }
        let idle = matches!(
            state.status.as_str(),
            "" | "Not running" | "Program exited" | "Restarting..."
        );
        if !idle {
            return status(DebugTone::Failed, &state.status, "", Some("Show Details"));
        }
        match self.effective_debug_config() {
            None => status(
                DebugTone::Idle,
                "Nothing to debug",
                "open a Rust or Python file",
                None,
            ),
            Some(_) if state.status == "Program exited" => status(
                DebugTone::Idle,
                "Program exited",
                "press F5 to debug again",
                None,
            ),
            Some(DebugConfig::RustWorkspace) => status(
                DebugTone::Idle,
                "Ready",
                "builds and debugs the Cargo workspace",
                None,
            ),
            Some(DebugConfig::PythonFile) => status(
                DebugTone::Idle,
                "Ready",
                "debugs the active Python file",
                None,
            ),
        }
    }

    /// Every breakpoint in the open tabs, by tab then line.
    pub(super) fn debug_breakpoint_list(&self) -> Vec<(usize, usize)> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.document.path.is_some())
            .flat_map(|(index, tab)| {
                tab.document
                    .breakpoints()
                    .iter()
                    .map(move |&line| (index, line))
            })
            .collect()
    }

    /// The list model, built fresh so paint and hit-testing agree.
    pub(super) fn debug_rows(&self) -> Vec<DebugRow> {
        let state = &self.debug_state;
        let expanded = |section: DebugSection| self.debug_sections_expanded[section.index()];
        let mut rows = Vec::new();

        let count: usize = state.scopes.iter().map(|scope| scope.variables.len()).sum();
        rows.push(DebugRow::Header {
            section: DebugSection::Variables,
            title: "VARIABLES",
            count,
        });
        if expanded(DebugSection::Variables) {
            if state.scopes.is_empty() {
                rows.push(DebugRow::Note(
                    if self.debug.is_some() && state.running {
                        "Pause to see variables."
                    } else {
                        "Variables appear when execution pauses."
                    }
                    .into(),
                ));
            }
            for scope in &state.scopes {
                rows.push(DebugRow::Scope(scope.name.clone()));
                for variable in &scope.variables {
                    self.push_debug_variable(&mut rows, variable, 0);
                }
            }
        }

        rows.push(DebugRow::Header {
            section: DebugSection::CallStack,
            title: "CALL STACK",
            count: state.frames.len(),
        });
        if expanded(DebugSection::CallStack) {
            if state.frames.is_empty() {
                rows.push(DebugRow::Note(
                    if self.debug.is_some() && state.running {
                        "Running. Pause to see the call stack."
                    } else {
                        "The call stack appears while debugging."
                    }
                    .into(),
                ));
            }
            rows.extend((0..state.frames.len()).map(DebugRow::Frame));
        }

        let breakpoints = self.debug_breakpoint_list();
        rows.push(DebugRow::Header {
            section: DebugSection::Breakpoints,
            title: "BREAKPOINTS",
            count: breakpoints.len(),
        });
        if expanded(DebugSection::Breakpoints) {
            if breakpoints.is_empty() {
                rows.push(DebugRow::Note(
                    "Click the gutter or press F9 to add one.".into(),
                ));
            }
            rows.extend(
                breakpoints
                    .into_iter()
                    .map(|(tab, line)| DebugRow::Breakpoint { tab, line }),
            );
        }

        if !self.debug_session_shown() && self.effective_debug_config().is_none() {
            rows.push(DebugRow::Note(String::new()));
            rows.push(DebugRow::Note(
                "LightLine debugs Rust with LLDB and Python with debugpy.".into(),
            ));
            rows.push(DebugRow::Note(
                "Open a .rs file or a Cargo folder, or a .py file, then press F5.".into(),
            ));
        }
        rows
    }

    fn push_debug_variable(
        &self,
        rows: &mut Vec<DebugRow>,
        variable: &DebugVariable,
        depth: usize,
    ) {
        let reference = variable.variables_reference;
        let expanded = reference != 0 && self.debug_state.expanded.contains(&reference);
        rows.push(DebugRow::Variable {
            depth,
            reference,
            expanded,
            name: variable.name.clone(),
            value: variable.value.clone(),
        });
        if !expanded {
            return;
        }
        match self.debug_state.children.get(&reference) {
            Some(children) => {
                for child in children {
                    self.push_debug_variable(rows, child, depth + 1);
                }
            }
            None => rows.push(DebugRow::Loading(depth + 1)),
        }
    }

    /// Each row on screen with its rectangle, in the panel's coordinates.
    pub(super) fn debug_rows_on_screen(
        &self,
        count: usize,
        left: i32,
        right: i32,
        list_top: i32,
    ) -> Vec<(usize, RECT)> {
        let height = self.scale(DEBUG_ROW);
        // A list that shrank under the scroll position still shows its end.
        (self.debug_first.min(count.saturating_sub(1))..count)
            .enumerate()
            .map(|(shown, index)| {
                let top = list_top + height * shown as i32;
                (
                    index,
                    RECT {
                        left,
                        top,
                        right,
                        bottom: top + height,
                    },
                )
            })
            .collect()
    }

    /// The square button at the right end of a row, `slot` buttons in.
    pub(super) fn debug_row_button(&self, rect: RECT, slot: i32) -> RECT {
        let size = self.scale(22);
        let right = rect.right - self.scale(8) - size * slot;
        RECT {
            left: right - size,
            top: rect.top,
            right,
            bottom: rect.top + size,
        }
    }

    /// Where a breakpoint row's line number ends and its ✕ sits.
    pub(super) fn debug_breakpoint_remove(&self, rect: RECT, hdc: HDC, line: usize) -> RECT {
        let number = self.text_width(hdc, &(line + 1).to_string());
        let right = rect.right - self.scale(8) - number - self.scale(6);
        RECT {
            left: right - self.scale(22),
            top: rect.top,
            right,
            bottom: rect.top + self.scale(22),
        }
    }

    pub(super) fn debug_hit(&self, x: i32, y: i32, left: i32, right: i32) -> DebugHit {
        let layout = self.debug_panel_layout(left, right);
        if let Some(button) = self.debug_button_at(&layout, x, y) {
            return DebugHit::Button(button);
        }
        if y < layout.list_top {
            return DebugHit::Nothing;
        }
        let rows = self.debug_rows();
        let Some((index, rect)) = self
            .debug_rows_on_screen(rows.len(), left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
        else {
            return DebugHit::Nothing;
        };
        match &rows[index] {
            DebugRow::Header { section, count, .. } => {
                let button = self.debug_row_button(rect, 0);
                match section {
                    DebugSection::Variables if *count > 0 && contains(&button, x, y) => {
                        DebugHit::CollapseVariables
                    }
                    DebugSection::Breakpoints if *count > 0 && contains(&button, x, y) => {
                        DebugHit::RemoveBreakpoints
                    }
                    _ => DebugHit::Header(*section),
                }
            }
            DebugRow::Variable { reference, .. } if *reference != 0 => {
                DebugHit::Variable(*reference)
            }
            DebugRow::Frame(frame) => DebugHit::Frame(*frame),
            DebugRow::Breakpoint { tab, line } => {
                let remove = unsafe {
                    let hdc = GetDC(null_mut());
                    let old = SelectObject(hdc, self.ui_font);
                    let rect = self.debug_breakpoint_remove(rect, hdc, *line);
                    SelectObject(hdc, old);
                    ReleaseDC(null_mut(), hdc);
                    rect
                };
                if contains(&remove, x, y) {
                    DebugHit::RemoveBreakpoint {
                        tab: *tab,
                        line: *line,
                    }
                } else {
                    DebugHit::Breakpoint {
                        tab: *tab,
                        line: *line,
                    }
                }
            }
            _ => DebugHit::Nothing,
        }
    }

    fn debug_button_at(&self, layout: &DebugLayout, x: i32, y: i32) -> Option<DebugButton> {
        if contains(&layout.more, x, y) {
            return Some(DebugButton::More);
        }
        if self.debug_session_shown() {
            if let Some(index) = layout.tools.iter().position(|rect| contains(rect, x, y)) {
                return Some(DebugButton::Tool(index));
            }
        } else if contains(&layout.go, x, y) {
            return Some(DebugButton::Go);
        } else if contains(&layout.pick, x, y) {
            return Some(DebugButton::Pick);
        }
        self.debug_link_rect(layout, None)
            .filter(|rect| contains(rect, x, y))
            .map(|_| DebugButton::Link)
    }

    /// What's under the mouse at (x, y) in the panel's coordinates.
    pub(super) fn debug_hot_at(&self, x: i32, y: i32) -> Option<DebugHot> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.debug_panel_layout(left, right);
        if let Some(button) = self.debug_button_at(&layout, x, y) {
            return Some(DebugHot::Button(button));
        }
        if y < layout.list_top {
            return None;
        }
        let rows = self.debug_rows();
        self.debug_rows_on_screen(rows.len(), left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
            .filter(|(index, _)| {
                matches!(
                    rows[*index],
                    DebugRow::Header { .. }
                        | DebugRow::Variable { .. }
                        | DebugRow::Frame(_)
                        | DebugRow::Breakpoint { .. }
                )
            })
            .map(|(index, _)| DebugHot::Row(index))
    }

    /// Where `hot` is, in the panel's coordinates.
    pub(super) fn debug_hot_rect(&self, hot: DebugHot) -> Option<RECT> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.debug_panel_layout(left, right);
        match hot {
            DebugHot::Button(DebugButton::More) => Some(layout.more),
            DebugHot::Button(DebugButton::Go) => Some(layout.go),
            DebugHot::Button(DebugButton::Pick) => Some(layout.pick),
            DebugHot::Button(DebugButton::Tool(index)) => layout.tools.get(index).copied(),
            DebugHot::Button(DebugButton::Link) => Some(layout.state),
            DebugHot::Row(index) => self
                .debug_rows_on_screen(index + 1, left, right, layout.list_top)
                .into_iter()
                .find(|(row, _)| *row == index)
                .map(|(_, rect)| rect),
        }
    }

    pub(super) fn debug_hot_row(&self) -> Option<usize> {
        match self.hot {
            Some(Hot::Debug(DebugHot::Row(index))) => Some(index),
            _ => None,
        }
    }

    /// The list scrolled or changed under a still mouse.
    pub(super) fn forget_debug_hot_row(&mut self, hwnd: HWND) {
        if self.debug_hot_row().is_some() {
            self.clear_hot(hwnd);
        }
    }

    /// How many rows fit below the status line.
    pub(super) fn debug_visible_rows(&self, hwnd: HWND) -> usize {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let layout = self.debug_panel_layout(self.scale(RAIL), self.sidebar_right());
        let available =
            (client.bottom - self.scale(STATUS) - self.side_origin() - layout.list_top).max(0);
        (available / self.scale(DEBUG_ROW).max(1)).max(1) as usize
    }

    /// A click in the panel (panel coordinates).
    pub(super) fn debug_panel_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        match self.debug_hit(x, y, left, right) {
            DebugHit::Button(button) => self.debug_button_pressed(hwnd, button),
            DebugHit::Header(section) => self.toggle_debug_section(hwnd, section.index()),
            DebugHit::CollapseVariables => {
                self.debug_state.expanded.clear();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            DebugHit::RemoveBreakpoints => self.remove_all_breakpoints(hwnd),
            DebugHit::Variable(reference) => self.toggle_debug_variable(hwnd, reference),
            DebugHit::Frame(index) => {
                if let Some(frame) = self.debug_state.frames.get(index)
                    && let Some(path) = frame.path.clone()
                {
                    self.debug_show_line(hwnd, path, frame.line.saturating_sub(1) as usize);
                }
            }
            DebugHit::Breakpoint { tab, line } => {
                if tab < self.tabs.len() {
                    self.activate_tab(hwnd, tab);
                    self.debug_go_to_line(hwnd, line);
                }
            }
            DebugHit::RemoveBreakpoint { tab, line } => {
                if let Some(tab) = self.tabs.get_mut(tab) {
                    tab.document.toggle_breakpoint(line);
                }
                self.forget_debug_hot_row(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            DebugHit::Nothing => {}
        }
    }

    pub(super) fn debug_button_pressed(&mut self, hwnd: HWND, button: DebugButton) {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.debug_panel_layout(left, right);
        let origin = self.side_origin();
        let in_window = |rect: RECT| RECT {
            top: rect.top + origin,
            bottom: rect.bottom + origin,
            ..rect
        };
        match button {
            DebugButton::More => {
                self.toggle_debug_menu(hwnd, DebugMenu::Header, in_window(layout.more))
            }
            DebugButton::Go => {
                if self.effective_debug_config().is_some() {
                    self.start_debug_session(hwnd);
                } else {
                    self.status = "Open a Rust or Python file to debug it".into();
                }
            }
            DebugButton::Pick => self.show_debug_config_menu(hwnd),
            DebugButton::Tool(index) => {
                if !self.debug_tool_enabled(index) {
                    return;
                }
                match index {
                    0 if self.debug_state.running => self.debug_pause(hwnd),
                    0 => self.debug_continue(hwnd),
                    1 => self.debug_step_over(hwnd),
                    2 => self.debug_step_in(hwnd),
                    3 => self.debug_step_out(hwnd),
                    4 => self.debug_restart(hwnd),
                    _ => self.debug_stop(hwnd),
                }
            }
            DebugButton::Link => match self.debug_status().link {
                Some("Install") => self.install_debugpy(hwnd),
                Some(_) => self.show_debug_console(hwnd),
                None => {}
            },
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The configuration picker, dropped from the launch bar (opens the
    /// panel first when it's shut, as the Command Palette's entry does).
    pub(super) fn show_debug_config_menu(&mut self, hwnd: HWND) {
        if self.debug_session_shown() {
            self.status = "Stop the debug session to change its configuration".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if !(self.side_view == SideView::Debug && self.explorer_visible) {
            self.toggle_side_view(hwnd, SideView::Debug);
        }
        let layout = self.debug_panel_layout(self.scale(RAIL), self.sidebar_right());
        let origin = self.side_origin();
        self.toggle_debug_menu(
            hwnd,
            DebugMenu::Config,
            RECT {
                top: layout.pick.top + origin,
                bottom: layout.pick.bottom + origin,
                ..layout.pick
            },
        );
    }

    pub(super) fn remove_all_breakpoints(&mut self, hwnd: HWND) {
        let mut removed = 0;
        for tab in &mut self.tabs {
            let lines: Vec<usize> = tab.document.breakpoints().iter().copied().collect();
            for line in lines {
                tab.document.toggle_breakpoint(line);
                removed += 1;
            }
        }
        self.status = match removed {
            0 => "No breakpoints to remove".into(),
            1 => "Removed 1 breakpoint".into(),
            count => format!("Removed {count} breakpoints"),
        };
        self.forget_debug_hot_row(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Show `path` at zero-based `line` in the editor, keeping Run & Debug
    /// open in the side panel (opening a file would switch it to Files).
    pub(super) fn debug_show_line(&mut self, hwnd: HWND, path: PathBuf, line: usize) {
        let showing = self
            .doc()
            .path
            .as_deref()
            .is_some_and(|open| Self::same_path(open, &path));
        if !showing {
            let side_view = self.side_view;
            self.open(hwnd, Some(path));
            if side_view == SideView::Debug {
                self.side_view = SideView::Debug;
            }
        }
        self.debug_go_to_line(hwnd, line);
    }

    fn debug_go_to_line(&mut self, hwnd: HWND, line: usize) {
        let line = line.min(self.doc().line_count().saturating_sub(1));
        self.move_cursor(Pos { line, byte: 0 }, false);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
