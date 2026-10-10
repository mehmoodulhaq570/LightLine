//! LightLine's themed editor context menu.
//!
//! A native Win32 popup ignores most of the active workbench palette and
//! cannot carry the diagnostic summary or action badges this menu needs. The
//! menu therefore lives in the main backbuffer, like Quick Open and the AI
//! model picker, while retaining the usual click and keyboard behavior.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorContextAction {
    ExplainError,
    FixError,
    ExplainSelection,
    FixSelection,
    TestsForSelection,
    CommentsForSelection,
    QuickFix,
    RenameSymbol,
    Cut,
    Copy,
    Paste,
    SelectAll,
    CommandPalette,
    StageHunk,
    DiscardHunk,
    OpenDiffView,
}

#[derive(Clone)]
pub(super) struct EditorContextDiagnostic {
    title: String,
    detail: String,
    kind: &'static str,
    severity: u8,
    line: u32,
}

impl EditorContextDiagnostic {
    pub(super) fn from_lsp(diagnostic: &LspDiagnostic) -> Self {
        let kind = match diagnostic.severity {
            1 => "error",
            2 => "warning",
            _ => "problem",
        };
        let detail = diagnostic
            .message
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        Self {
            title: diagnostic_title(&detail, kind),
            detail,
            kind,
            severity: diagnostic.severity,
            line: diagnostic.range.start.line + 1,
        }
    }
}

fn diagnostic_title(message: &str, kind: &str) -> String {
    // Runtime-style names such as ZeroDivisionError or TypeError communicate
    // more than a generic "Error" when the language server includes one.
    message
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .find(|word| word.ends_with("Error") && word.len() > "Error".len())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} at cursor", title_case(kind)))
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[derive(Clone)]
pub(super) struct EditorContextMenu {
    anchor: POINT,
    diagnostic: Option<EditorContextDiagnostic>,
    has_selection: bool,
    has_hunk: bool,
    // The file has a language server to rename with.
    can_rename: bool,
    highlighted: Option<EditorContextAction>,
}

impl EditorContextMenu {
    pub(super) fn new(
        x: i32,
        y: i32,
        diagnostic: Option<EditorContextDiagnostic>,
        has_selection: bool,
        has_hunk: bool,
    ) -> Self {
        let highlighted = if diagnostic.is_some() {
            Some(EditorContextAction::FixError)
        } else if has_selection {
            Some(EditorContextAction::ExplainSelection)
        } else if has_hunk {
            Some(EditorContextAction::StageHunk)
        } else {
            Some(EditorContextAction::Paste)
        };
        Self {
            anchor: POINT { x, y },
            diagnostic,
            has_selection,
            has_hunk,
            can_rename: false,
            highlighted,
        }
    }

    pub(super) fn with_rename(mut self, can_rename: bool) -> Self {
        self.can_rename = can_rename;
        self
    }

    fn enabled_actions(&self) -> Vec<EditorContextAction> {
        let mut actions = Vec::new();
        if self.diagnostic.is_some() {
            actions.extend([
                EditorContextAction::ExplainError,
                EditorContextAction::FixError,
            ]);
        }
        if self.has_selection {
            actions.extend([
                EditorContextAction::ExplainSelection,
                EditorContextAction::FixSelection,
                EditorContextAction::TestsForSelection,
                EditorContextAction::CommentsForSelection,
                EditorContextAction::Cut,
                EditorContextAction::Copy,
            ]);
        }
        if self.has_hunk {
            actions.extend([
                EditorContextAction::StageHunk,
                EditorContextAction::DiscardHunk,
                EditorContextAction::OpenDiffView,
            ]);
        }
        if self.can_rename {
            actions.extend([
                EditorContextAction::QuickFix,
                EditorContextAction::RenameSymbol,
            ]);
        }
        actions.extend([
            EditorContextAction::Paste,
            EditorContextAction::SelectAll,
            EditorContextAction::CommandPalette,
        ]);
        actions
    }
}

#[derive(Clone, Copy)]
enum ContextElementKind {
    Header,
    Divider,
    Section(&'static str),
    Row {
        action: EditorContextAction,
        enabled: bool,
    },
}

#[derive(Clone, Copy)]
struct ContextElement {
    rect: RECT,
    kind: ContextElementKind,
}

struct ContextLayout {
    menu: RECT,
    elements: Vec<ContextElement>,
}

impl ContextLayout {
    fn hit(&self, x: i32, y: i32) -> Option<(EditorContextAction, bool)> {
        self.elements.iter().find_map(|element| match element.kind {
            ContextElementKind::Row { action, enabled } if in_rect(element.rect, x, y) => {
                Some((action, enabled))
            }
            _ => None,
        })
    }
}

fn in_rect(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn action_label(
    action: EditorContextAction,
    diagnostic: Option<&EditorContextDiagnostic>,
) -> String {
    match action {
        EditorContextAction::ExplainError => format!(
            "Explain this {}",
            diagnostic.map_or("problem", |problem| problem.kind)
        ),
        EditorContextAction::FixError => "Fix with AI".into(),
        EditorContextAction::ExplainSelection => "Explain selection".into(),
        EditorContextAction::FixSelection => "Fix selection".into(),
        EditorContextAction::TestsForSelection => "Write tests for selection".into(),
        EditorContextAction::CommentsForSelection => "Add comments to selection".into(),
        EditorContextAction::QuickFix => "Quick fix...".into(),
        EditorContextAction::RenameSymbol => "Rename symbol".into(),
        EditorContextAction::Cut => "Cut".into(),
        EditorContextAction::Copy => "Copy".into(),
        EditorContextAction::Paste => "Paste".into(),
        EditorContextAction::SelectAll => "Select all".into(),
        EditorContextAction::CommandPalette => "Command Palette...".into(),
        EditorContextAction::StageHunk => "Stage Hunk".into(),
        EditorContextAction::DiscardHunk => "Discard Hunk".into(),
        EditorContextAction::OpenDiffView => "Open Diff View".into(),
    }
}

fn action_shortcut(action: EditorContextAction) -> &'static str {
    match action {
        EditorContextAction::ExplainError => "Ctrl+Shift+E",
        EditorContextAction::QuickFix => "Ctrl+.",
        EditorContextAction::RenameSymbol => "F2",
        EditorContextAction::Cut => "Ctrl+X",
        EditorContextAction::Copy => "Ctrl+C",
        EditorContextAction::Paste => "Ctrl+V",
        EditorContextAction::SelectAll => "Ctrl+A",
        EditorContextAction::CommandPalette => "Ctrl+Shift+P",
        EditorContextAction::OpenDiffView => "F7",
        _ => "",
    }
}

fn is_ai_action(action: EditorContextAction) -> bool {
    matches!(
        action,
        EditorContextAction::ExplainError
            | EditorContextAction::FixError
            | EditorContextAction::ExplainSelection
            | EditorContextAction::FixSelection
            | EditorContextAction::TestsForSelection
            | EditorContextAction::CommentsForSelection
    )
}

impl App {
    fn editor_context_layout(&self, hwnd: HWND) -> Option<ContextLayout> {
        let menu = self.editor_context.as_ref()?;
        let s = |value: i32| self.scale(value);
        let mut specs = Vec::new();
        if menu.diagnostic.is_some() {
            specs.push((ContextElementKind::Header, s(64)));
            specs.push((ContextElementKind::Divider, s(7)));
            specs.push((
                ContextElementKind::Row {
                    action: EditorContextAction::ExplainError,
                    enabled: true,
                },
                s(34),
            ));
            specs.push((
                ContextElementKind::Row {
                    action: EditorContextAction::FixError,
                    enabled: true,
                },
                s(34),
            ));
        }
        if menu.has_selection {
            specs.push((ContextElementKind::Divider, s(7)));
            specs.push((ContextElementKind::Section("AI · SELECTION"), s(18)));
            for action in [
                EditorContextAction::ExplainSelection,
                EditorContextAction::FixSelection,
                EditorContextAction::TestsForSelection,
                EditorContextAction::CommentsForSelection,
            ] {
                specs.push((
                    ContextElementKind::Row {
                        action,
                        enabled: true,
                    },
                    s(30),
                ));
            }
        }
        specs.push((ContextElementKind::Divider, s(7)));
        specs.push((ContextElementKind::Section("EDIT"), s(18)));
        if menu.can_rename {
            for action in [
                EditorContextAction::QuickFix,
                EditorContextAction::RenameSymbol,
            ] {
                specs.push((
                    ContextElementKind::Row {
                        action,
                        enabled: true,
                    },
                    s(30),
                ));
            }
        }
        for action in [
            EditorContextAction::Cut,
            EditorContextAction::Copy,
            EditorContextAction::Paste,
            EditorContextAction::SelectAll,
        ] {
            specs.push((
                ContextElementKind::Row {
                    action,
                    enabled: menu.has_selection
                        || !matches!(action, EditorContextAction::Cut | EditorContextAction::Copy),
                },
                s(30),
            ));
        }
        specs.push((ContextElementKind::Divider, s(7)));
        specs.push((
            ContextElementKind::Row {
                action: EditorContextAction::CommandPalette,
                enabled: true,
            },
            s(32),
        ));
        if menu.has_hunk {
            specs.push((ContextElementKind::Divider, s(7)));
            specs.push((ContextElementKind::Section("GIT · CURRENT HUNK"), s(18)));
            for action in [
                EditorContextAction::StageHunk,
                EditorContextAction::DiscardHunk,
                EditorContextAction::OpenDiffView,
            ] {
                specs.push((
                    ContextElementKind::Row {
                        action,
                        enabled: true,
                    },
                    s(30),
                ));
            }
        }

        let padding = s(6);
        let height = specs.iter().map(|(_, height)| *height).sum::<i32>() + padding * 2;
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let min_left = self.editor_left() + s(12);
        let max_right = self.editor_right(hwnd) - s(12);
        let width = s(310).min((max_right - min_left).max(s(240)));
        let mut left = menu.anchor.x + s(8);
        if left + width > max_right {
            left = menu.anchor.x - width - s(8);
        }
        left = left.clamp(min_left, (max_right - width).max(min_left));
        let min_top = self.chrome_top() + s(10);
        let max_bottom = client.bottom - self.scale(STATUS) - s(10);
        let top = (menu.anchor.y + s(8)).min(max_bottom - height).max(min_top);
        let bounds = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };

        let mut y = bounds.top + padding;
        let elements = specs
            .into_iter()
            .map(|(kind, height)| {
                let inset = if matches!(kind, ContextElementKind::Row { .. }) {
                    s(7)
                } else {
                    0
                };
                let rect = RECT {
                    left: bounds.left + inset,
                    top: y,
                    right: bounds.right - inset,
                    bottom: y + height,
                };
                y += height;
                ContextElement { rect, kind }
            })
            .collect();
        Some(ContextLayout {
            menu: bounds,
            elements,
        })
    }

    pub(super) fn editor_context_key(
        &mut self,
        hwnd: HWND,
        key: u32,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        let Some(menu) = self.editor_context.as_ref() else {
            return false;
        };
        if ctrl || shift {
            self.editor_context = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return false;
        }
        if key == VK_ESCAPE as u32 {
            self.editor_context = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
        let actions = menu.enabled_actions();
        if key == VK_UP as u32 || key == VK_DOWN as u32 {
            let current = menu
                .highlighted
                .and_then(|highlighted| actions.iter().position(|action| *action == highlighted));
            let next = if key == VK_UP as u32 {
                current
                    .map(|index| (index + actions.len() - 1) % actions.len())
                    .unwrap_or(actions.len() - 1)
            } else {
                current.map_or(0, |index| (index + 1) % actions.len())
            };
            if let Some(menu) = &mut self.editor_context {
                menu.highlighted = actions.get(next).copied();
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
        if key == VK_RETURN as u32 {
            let action = menu.highlighted;
            self.editor_context = None;
            if let Some(action) = action.filter(|action| actions.contains(action)) {
                self.run_editor_context_action(hwnd, action);
            }
            return true;
        }
        self.editor_context = None;
        unsafe { InvalidateRect(hwnd, null(), 0) };
        false
    }

    pub(super) fn editor_context_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(layout) = self.editor_context_layout(hwnd) else {
            return false;
        };
        if !in_rect(layout.menu, x, y) {
            self.editor_context = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return false;
        }
        if let Some((action, true)) = layout.hit(x, y) {
            self.editor_context = None;
            self.run_editor_context_action(hwnd, action);
        }
        true
    }

    pub(super) fn editor_context_hover(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(layout) = self.editor_context_layout(hwnd) else {
            return false;
        };
        let next = layout
            .hit(x, y)
            .and_then(|(action, enabled)| enabled.then_some(action));
        if self
            .editor_context
            .as_ref()
            .is_some_and(|menu| menu.highlighted != next)
        {
            if let Some(menu) = &mut self.editor_context {
                menu.highlighted = next;
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        true
    }

    pub(super) fn dismiss_editor_context(&mut self, hwnd: HWND) -> bool {
        if self.editor_context.take().is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
            true
        } else {
            false
        }
    }

    fn run_editor_context_action(&mut self, hwnd: HWND, action: EditorContextAction) {
        match action {
            EditorContextAction::ExplainError => self.ai_run_task(hwnd, AiTask::ExplainError),
            EditorContextAction::FixError => self.ai_run_task(hwnd, AiTask::FixError),
            EditorContextAction::ExplainSelection => self.ai_run_task(hwnd, AiTask::Explain),
            EditorContextAction::FixSelection => self.ai_run_task(hwnd, AiTask::Fix),
            EditorContextAction::TestsForSelection => self.ai_run_task(hwnd, AiTask::Tests),
            EditorContextAction::CommentsForSelection => self.ai_run_task(hwnd, AiTask::Comments),
            EditorContextAction::QuickFix => self.request_code_actions(hwnd),
            EditorContextAction::RenameSymbol => self.start_rename(hwnd),
            EditorContextAction::Cut => {
                if self.copy_selection(hwnd) {
                    self.replace_selection("");
                }
            }
            EditorContextAction::Copy => {
                self.copy_selection(hwnd);
            }
            EditorContextAction::Paste => match clipboard::paste(hwnd) {
                Ok(Some(text)) => self.replace_selection(&text),
                Ok(None) => {}
                Err(error) => self.error(hwnd, &error),
            },
            EditorContextAction::SelectAll => {
                self.view_mut().selection_anchor = Some(Pos::default());
                let end = self.doc().end();
                self.view_mut().cursor = end;
            }
            EditorContextAction::CommandPalette => {
                self.show_quick_open(hwnd);
                self.quick_query = ">".into();
            }
            EditorContextAction::StageHunk => {
                self.stage_cursor_hunk(hwnd);
            }
            EditorContextAction::DiscardHunk => {
                self.discard_cursor_hunk(hwnd);
            }
            EditorContextAction::OpenDiffView => {
                self.review_cursor_file_diff(hwnd);
            }
        }
        self.refresh(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn paint_editor_context_menu(&self, hdc: HDC, hwnd: HWND) {
        let Some(menu) = self.editor_context.as_ref() else {
            return;
        };
        let Some(layout) = self.editor_context_layout(hwnd) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let shadow = RECT {
            left: layout.menu.left + s(5),
            top: layout.menu.top + s(6),
            right: layout.menu.right + s(5),
            bottom: layout.menu.bottom + s(6),
        };
        Self::rounded_fill(hdc, shadow, s(9), ui(3, 8, 18));
        self.panel_card(
            hdc,
            layout.menu,
            s(8),
            self.theme.violet,
            self.theme.sidebar_bg,
        );

        for element in layout.elements {
            match element.kind {
                ContextElementKind::Header => {
                    self.paint_context_diagnostic(hdc, element.rect, menu)
                }
                ContextElementKind::Divider => Self::fill(
                    hdc,
                    RECT {
                        left: layout.menu.left + s(11),
                        top: (element.rect.top + element.rect.bottom) / 2,
                        right: layout.menu.right - s(11),
                        bottom: (element.rect.top + element.rect.bottom) / 2 + s(1).max(1),
                    },
                    self.theme.edge,
                ),
                ContextElementKind::Section(label) => {
                    unsafe { SelectObject(hdc, self.ui_font) };
                    self.label_mid(
                        hdc,
                        label,
                        element.rect.left + s(14),
                        (element.rect.top + element.rect.bottom) / 2,
                        self.theme.muted,
                        element.rect,
                    );
                }
                ContextElementKind::Row { action, enabled } => {
                    self.paint_context_row(hdc, element.rect, menu, action, enabled)
                }
            }
        }
    }

    fn paint_context_diagnostic(&self, hdc: HDC, rect: RECT, menu: &EditorContextMenu) {
        let Some(problem) = menu.diagnostic.as_ref() else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let color = match problem.severity {
            1 => self.theme.error,
            2 => self.theme.warning,
            _ => self.theme.info,
        };
        let icon = RECT {
            left: rect.left + s(14),
            top: rect.top + s(13),
            right: rect.left + s(48),
            bottom: rect.top + s(47),
        };
        Self::rounded_fill(hdc, icon, s(17), color);
        unsafe { SelectObject(hdc, self.brand_font) };
        let mark = "!";
        self.label_mid(
            hdc,
            mark,
            icon.left + (icon.right - icon.left - self.text_width(hdc, mark)) / 2,
            (icon.top + icon.bottom) / 2 - s(1),
            label_on(color, 255, 255, 255),
            icon,
        );
        let text_left = icon.right + s(11);
        let clip = RECT {
            left: text_left,
            right: rect.right - s(16),
            ..rect
        };
        self.label_ellipsis(
            hdc,
            &problem.title,
            text_left,
            rect.top + s(9),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let detail = if problem.detail.is_empty() {
            format!("Line {}", problem.line)
        } else {
            format!("Line {} · {}", problem.line, problem.detail)
        };
        self.label_ellipsis(
            hdc,
            &detail,
            text_left,
            rect.top + s(34),
            self.theme.muted,
            clip,
        );
    }

    fn paint_context_row(
        &self,
        hdc: HDC,
        rect: RECT,
        menu: &EditorContextMenu,
        action: EditorContextAction,
        enabled: bool,
    ) {
        let s = |value: i32| self.scale(value);
        let highlighted = enabled && menu.highlighted == Some(action);
        if highlighted {
            if action == EditorContextAction::FixError {
                self.gradient_card(hdc, rect, s(6), ui(35, 39, 96), ui(29, 34, 79));
                self.card_outline(hdc, rect, s(6), ui(72, 65, 160));
            } else {
                Self::rounded_fill(hdc, rect, s(6), self.theme.active_bg);
            }
        }
        let icon_color = if enabled {
            if is_ai_action(action) {
                self.theme.violet
            } else {
                self.theme.text
            }
        } else {
            ui(78, 91, 116)
        };
        let center_y = (rect.top + rect.bottom) / 2;
        let icon_left = rect.left + s(11);
        if is_ai_action(action) {
            self.sparkle_glyph(hdc, icon_left, center_y - s(7), s(14), icon_color);
        } else {
            self.draw_context_action_icon(hdc, action, icon_left, center_y, icon_color);
        }

        unsafe { SelectObject(hdc, self.ui_font) };
        let label = action_label(action, menu.diagnostic.as_ref());
        let text_color = if enabled {
            self.theme.text
        } else {
            ui(81, 94, 119)
        };
        let label_x = rect.left + s(40);
        let shortcut = action_shortcut(action);
        let shortcut_width = if shortcut.is_empty() {
            0
        } else {
            self.text_width(hdc, shortcut) + s(14)
        };
        let label_clip = RECT {
            left: label_x,
            right: rect.right - shortcut_width - s(11),
            ..rect
        };
        self.label_mid(hdc, &label, label_x, center_y, text_color, label_clip);

        if action == EditorContextAction::FixError {
            let badge_text = "Recommended";
            let badge_width = self.text_width(hdc, badge_text) + s(14);
            let preferred = label_x + self.text_width(hdc, &label) + s(10);
            let badge = RECT {
                left: preferred.min(rect.right - badge_width - s(7)),
                top: center_y - s(9),
                right: (preferred + badge_width).min(rect.right - s(7)),
                bottom: center_y + s(9),
            };
            Self::rounded_fill(hdc, badge, s(5), self.theme.violet);
            self.label_mid(
                hdc,
                badge_text,
                badge.left + s(7),
                center_y,
                label_on(self.theme.violet, 255, 255, 255),
                badge,
            );
        }

        if !shortcut.is_empty() {
            let badge = RECT {
                left: rect.right - shortcut_width - s(6),
                top: center_y - s(10),
                right: rect.right - s(6),
                bottom: center_y + s(10),
            };
            self.panel_card(
                hdc,
                badge,
                s(5),
                self.theme.edge,
                if highlighted {
                    ui(27, 43, 75)
                } else {
                    self.theme.active_bg
                },
            );
            let width = self.text_width(hdc, shortcut);
            self.label_mid(
                hdc,
                shortcut,
                badge.left + (badge.right - badge.left - width) / 2,
                center_y,
                if enabled {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                badge,
            );
        }
    }

    fn draw_context_action_icon(
        &self,
        hdc: HDC,
        action: EditorContextAction,
        left: i32,
        center_y: i32,
        color: u32,
    ) {
        let s = |value: i32| self.scale(value);
        unsafe {
            let pen = CreatePen(PS_SOLID, s(1).max(1), color);
            if pen.is_null() {
                return;
            }
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            match action {
                EditorContextAction::Cut => {
                    Ellipse(hdc, left, center_y - s(7), left + s(7), center_y);
                    Ellipse(hdc, left, center_y + s(1), left + s(7), center_y + s(8));
                    MoveToEx(hdc, left + s(6), center_y - s(1), null_mut());
                    LineTo(hdc, left + s(17), center_y + s(7));
                    MoveToEx(hdc, left + s(6), center_y + s(1), null_mut());
                    LineTo(hdc, left + s(17), center_y - s(7));
                }
                EditorContextAction::Copy => {
                    Rectangle(
                        hdc,
                        left + s(2),
                        center_y - s(8),
                        left + s(14),
                        center_y + s(5),
                    );
                    Rectangle(
                        hdc,
                        left + s(6),
                        center_y - s(4),
                        left + s(18),
                        center_y + s(9),
                    );
                }
                EditorContextAction::Paste => {
                    Rectangle(
                        hdc,
                        left + s(3),
                        center_y - s(7),
                        left + s(17),
                        center_y + s(9),
                    );
                    Rectangle(
                        hdc,
                        left + s(7),
                        center_y - s(10),
                        left + s(13),
                        center_y - s(5),
                    );
                }
                EditorContextAction::SelectAll => {
                    SelectObject(hdc, old_pen);
                    DeleteObject(pen);
                    let dotted = CreatePen(PS_DOT, s(1).max(1), color);
                    let prior = SelectObject(hdc, dotted);
                    Rectangle(
                        hdc,
                        left + s(1),
                        center_y - s(9),
                        left + s(18),
                        center_y + s(9),
                    );
                    SelectObject(hdc, prior);
                    DeleteObject(dotted);
                    SelectObject(hdc, old_brush);
                    return;
                }
                EditorContextAction::CommandPalette => {
                    RoundRect(
                        hdc,
                        left,
                        center_y - s(9),
                        left + s(20),
                        center_y + s(9),
                        s(4),
                        s(4),
                    );
                    MoveToEx(hdc, left + s(4), center_y - s(3), null_mut());
                    LineTo(hdc, left + s(8), center_y);
                    LineTo(hdc, left + s(4), center_y + s(3));
                    MoveToEx(hdc, left + s(10), center_y + s(4), null_mut());
                    LineTo(hdc, left + s(16), center_y + s(4));
                }
                _ => {}
            }
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_title_prefers_runtime_error_names() {
        assert_eq!(
            diagnostic_title("ZeroDivisionError: division by zero", "error"),
            "ZeroDivisionError"
        );
        assert_eq!(
            diagnostic_title("Unsupported operator", "warning"),
            "Warning at cursor"
        );
    }

    #[test]
    fn menu_only_enables_selection_commands_for_a_selection() {
        let plain = EditorContextMenu::new(0, 0, None, false, false);
        assert!(!plain.enabled_actions().contains(&EditorContextAction::Cut));
        assert!(
            !plain
                .enabled_actions()
                .contains(&EditorContextAction::ExplainSelection)
        );
        assert!(
            plain
                .enabled_actions()
                .contains(&EditorContextAction::Paste)
        );

        let selected = EditorContextMenu::new(0, 0, None, true, false);
        assert!(
            selected
                .enabled_actions()
                .contains(&EditorContextAction::Cut)
        );
        assert!(
            selected
                .enabled_actions()
                .contains(&EditorContextAction::ExplainSelection)
        );

        let hunk_menu = EditorContextMenu::new(0, 0, None, false, true);
        assert!(
            hunk_menu
                .enabled_actions()
                .contains(&EditorContextAction::StageHunk)
        );
        assert!(
            hunk_menu
                .enabled_actions()
                .contains(&EditorContextAction::DiscardHunk)
        );
        assert!(
            hunk_menu
                .enabled_actions()
                .contains(&EditorContextAction::OpenDiffView)
        );
    }
}
