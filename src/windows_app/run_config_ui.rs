//! Run configuration editor painted inside the LightLine workbench.
use super::*;
use lightline::run_config::{Configuration, Configurations};

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

#[derive(Clone, Default)]
struct Field {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

impl Field {
    fn from(text: String) -> Self {
        Self {
            cursor: text.len(),
            text,
            ..Default::default()
        }
    }
    fn selection(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
    fn replace(&mut self, text: &str) {
        self.undo.push((self.text.clone(), self.cursor));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        let selection = self.selection();
        self.text.replace_range(selection.clone(), text);
        self.cursor = selection.start + text.len();
        self.anchor = None;
    }
    fn move_to(&mut self, cursor: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = cursor;
    }
    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .last()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
    }
    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }
    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }
    fn vertical(&self, down: bool) -> usize {
        let start = self.line_start();
        let column = self.text[start..self.cursor].chars().count();
        let target = if down {
            let end = self.line_end();
            if end == self.text.len() {
                return self.cursor;
            }
            end + 1
        } else {
            if start == 0 {
                return self.cursor;
            }
            self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1)
        };
        let line = self.text[target..].split('\n').next().unwrap_or("");
        target
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(i, _)| i)
    }
}

pub(super) struct RunConfigPanel {
    root: PathBuf,
    saved: Configurations,
    editing: Option<usize>,
    new: bool,
    fields: [Field; 6],
    // 0=list, 1=New, 2=Delete, 3..8=fields, 9=Cancel, 10=Save, 11=Close.
    focus: usize,
    scroll: i32,
    list_first: usize,
    error: String,
    hover: Option<usize>,
    scroll_grab: Option<i32>,
}

impl RunConfigPanel {
    fn load(&mut self, index: Option<usize>, new: bool) {
        self.editing = index;
        self.new = new;
        self.scroll = 0;
        self.error.clear();
        self.scroll_grab = None;
        let config = index
            .map(|i| self.saved.configurations[i].clone())
            .unwrap_or_default();
        self.fields = [
            config.name,
            config.entry_file,
            config.command,
            config.working_directory,
            config.arguments.join("\n"),
            config
                .environment
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ]
        .map(Field::from);
    }
    fn enabled(&self) -> bool {
        self.editing.is_some() || self.new
    }
    fn save(&mut self) -> Result<(), String> {
        let mut saved = self.saved.clone();
        if !self.enabled() {
            saved.selected = None;
        } else {
            let mut environment = std::collections::BTreeMap::new();
            for line in self.fields[5]
                .text
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                let (name, value) = line
                    .split_once('=')
                    .ok_or("Each environment line must be NAME=value")?;
                if environment
                    .insert(name.trim().to_string(), value.to_string())
                    .is_some()
                {
                    return Err(format!("Duplicate environment variable: {}", name.trim()));
                }
            }
            let config = Configuration {
                name: self.fields[0].text.trim().into(),
                entry_file: self.fields[1].text.trim().into(),
                command: self.fields[2].text.trim().into(),
                working_directory: self.fields[3].text.trim().into(),
                arguments: self.fields[4]
                    .text
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect(),
                environment,
            };
            saved.selected = Some(config.name.clone());
            if let Some(index) = self.editing {
                saved.configurations[index] = config;
            } else {
                saved.configurations.push(config);
            }
        }
        saved.save(&self.root)?;
        self.saved = saved;
        Ok(())
    }
}

struct Layout {
    card: RECT,
    list: RECT,
    body: RECT,
    fields: [RECT; 6],
    buttons: [(usize, RECT); 5],
    footer: RECT,
}

impl App {
    pub(super) fn show_run_configurations(&mut self, hwnd: HWND) {
        self.run_choice = None;
        let Some(root) = self.run_configuration_root() else {
            self.status = "Open a folder or save a file before creating Run configurations".into();
            self.refresh(hwnd);
            return;
        };
        let saved = match Configurations::load(&root) {
            Ok(saved) => saved,
            Err(error) => {
                self.status = error;
                self.refresh(hwnd);
                return;
            }
        };
        let index = saved
            .selected
            .as_ref()
            .and_then(|name| saved.configurations.iter().position(|c| &c.name == name));
        let mut panel = RunConfigPanel {
            root,
            saved,
            editing: None,
            new: false,
            fields: std::array::from_fn(|_| Field::default()),
            focus: 0,
            scroll: 0,
            list_first: 0,
            error: String::new(),
            hover: None,
            scroll_grab: None,
        };
        panel.load(index, false);
        self.run_config_panel = Some(panel);
        self.quick_open = false;
        self.editor_context = None;
        self.more_menu = None;
        self.terminal_focus = false;
        self.panel_focus = false;
        self.transition = None;
        self.pending_high_surrogate = None;
        self.refresh(hwnd);
    }

    fn run_config_layout(&self, hwnd: HWND) -> Layout {
        let s = |v| self.scale(v);
        let mut client = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut client);
        }
        let width = s(860).min((client.right - s(24)).max(s(300)));
        let height = s(680).min((client.bottom - s(90)).max(s(250)));
        let left = (client.right - width) / 2;
        let top = ((client.bottom - height) / 2).max(s(WORKBENCH_HEADER));
        let card = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let divider = left + (width / 4).clamp(s(130), s(220));
        let footer = RECT {
            left: divider + s(18),
            top: card.bottom - s(90),
            right: card.right - s(20),
            bottom: card.bottom - s(16),
        };
        let list = RECT {
            left: left + s(16),
            top: top + s(108),
            right: divider - s(12),
            bottom: card.bottom - s(20),
        };
        let body = RECT {
            left: divider + s(18),
            top: top + s(70),
            right: card.right - s(20),
            bottom: footer.top - s(10),
        };
        let scroll = self.run_config_panel.as_ref().map_or(0, |p| p.scroll);
        let fields = std::array::from_fn(|i| {
            let offset = if i == 5 { 328 } else { i as i32 * 58 };
            let y = body.top + s(25) + s(offset) - scroll;
            RECT {
                left: body.left,
                top: y,
                right: body.right,
                bottom: y + s(if i >= 4 { 64 } else { 34 }),
            }
        });
        let buttons = [
            (
                1,
                RECT {
                    left: list.left,
                    top: top + s(66),
                    right: list.left + s(74),
                    bottom: top + s(96),
                },
            ),
            (
                2,
                RECT {
                    left: list.right - s(76),
                    top: top + s(66),
                    right: list.right,
                    bottom: top + s(96),
                },
            ),
            (
                9,
                RECT {
                    left: footer.right - s(248),
                    top: footer.bottom - s(34),
                    right: footer.right - s(158),
                    bottom: footer.bottom,
                },
            ),
            (
                10,
                RECT {
                    left: footer.right - s(148),
                    top: footer.bottom - s(34),
                    right: footer.right,
                    bottom: footer.bottom,
                },
            ),
            (
                11,
                RECT {
                    left: card.right - s(48),
                    top: top + s(14),
                    right: card.right - s(16),
                    bottom: top + s(46),
                },
            ),
        ];
        Layout {
            card,
            list,
            body,
            fields,
            buttons,
            footer,
        }
    }

    fn run_config_reveal(&mut self, hwnd: HWND) {
        let layout = self.run_config_layout(hwnd);
        let max = (self.scale(420) - (layout.body.bottom - layout.body.top)).max(0);
        let label_height = self.scale(25);
        if let Some(panel) = &mut self.run_config_panel {
            if (3..9).contains(&panel.focus) {
                let field = layout.fields[panel.focus - 3];
                if field.top < layout.body.top {
                    panel.scroll -= layout.body.top - field.top + label_height;
                } else if field.bottom > layout.body.bottom {
                    panel.scroll += field.bottom - layout.body.bottom;
                }
            }
            panel.scroll = panel.scroll.clamp(0, max);
        }
    }

    fn run_config_action(&mut self, hwnd: HWND, action: usize) {
        let Some(panel) = &mut self.run_config_panel else {
            return;
        };
        match action {
            1 => {
                panel.load(None, true);
                panel.focus = 3;
            }
            2 => {
                if let Some(index) = panel.editing {
                    let mut saved = panel.saved.clone();
                    let removed = saved.configurations.remove(index);
                    if saved.selected.as_ref() == Some(&removed.name) {
                        saved.selected = None;
                    }
                    match saved.save(&panel.root) {
                        Ok(()) => {
                            panel.saved = saved;
                            panel.load(None, false);
                            panel.focus = 0;
                            panel.list_first = 0;
                        }
                        Err(error) => panel.error = error,
                    }
                }
            }
            9 | 11 => {
                self.run_config_panel = None;
                self.pending_high_surrogate = None;
            }
            10 => match panel.save() {
                Ok(()) => {
                    self.run_config_panel = None;
                    self.pending_high_surrogate = None;
                    self.status = "Run configuration saved. Use Run or Ctrl+Shift+R.".into();
                }
                Err(error) => panel.error = error,
            },
            _ => {}
        }
        self.refresh(hwnd);
    }

    pub(super) fn run_config_key(&mut self, hwnd: HWND, key: u32, ctrl: bool, shift: bool) {
        if key == VK_ESCAPE as u32 {
            self.run_config_action(hwnd, 9);
            return;
        }
        if ctrl && (key == 0x53 || key == VK_RETURN as u32) {
            self.run_config_action(hwnd, 10);
            return;
        }
        let Some(panel) = &mut self.run_config_panel else {
            return;
        };
        if key == VK_TAB as u32 {
            let order: Vec<_> = (0..12)
                .filter(|i| {
                    !((3..9).contains(i) && !panel.enabled())
                        && !(*i == 2 && panel.editing.is_none())
                })
                .collect();
            let at = order.iter().position(|i| *i == panel.focus).unwrap_or(0);
            panel.focus = order[if shift {
                (at + order.len() - 1) % order.len()
            } else {
                (at + 1) % order.len()
            }];
        } else if panel.focus == 0
            && (key == VK_UP as u32
                || key == VK_DOWN as u32
                || key == VK_HOME as u32
                || key == VK_END as u32)
        {
            let current = panel.editing.map_or(0, |i| i + 1);
            let next = if key == VK_HOME as u32 {
                0
            } else if key == VK_END as u32 {
                panel.saved.configurations.len()
            } else if key == VK_UP as u32 {
                current.saturating_sub(1)
            } else {
                (current + 1).min(panel.saved.configurations.len())
            };
            panel.load(next.checked_sub(1), false);
            panel.list_first = next.saturating_sub(3);
        } else if !(3..9).contains(&panel.focus) {
            if key == VK_RETURN as u32 || key == VK_SPACE as u32 {
                let action = panel.focus;
                if action == 0 {
                    panel.focus = if panel.enabled() { 3 } else { 1 };
                } else {
                    self.run_config_action(hwnd, action);
                    return;
                }
            }
        } else {
            let index = panel.focus - 3;
            let field = &mut panel.fields[index];
            if ctrl && key == 0x41 {
                field.anchor = Some(0);
                field.cursor = field.text.len();
            } else if ctrl && (key == 0x43 || key == 0x58) {
                let selection = field.selection();
                if !selection.is_empty() {
                    let _ = clipboard::copy(hwnd, &field.text[selection]);
                    if key == 0x58 {
                        field.replace("");
                    }
                }
            } else if ctrl && key == 0x56 {
                if let Ok(Some(text)) = clipboard::paste(hwnd) {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    field.replace(&if index >= 4 {
                        text
                    } else {
                        text.replace('\n', " ")
                    });
                }
            } else if ctrl && (key == 0x5a || key == 0x59) {
                let redo = key == 0x59 || shift;
                let source = if redo {
                    &mut field.redo
                } else {
                    &mut field.undo
                };
                if let Some((text, cursor)) = source.pop() {
                    let current = (std::mem::replace(&mut field.text, text), field.cursor);
                    if redo {
                        field.undo.push(current);
                    } else {
                        field.redo.push(current);
                    }
                    field.cursor = cursor;
                    field.anchor = None;
                }
            } else if key == VK_BACK as u32 || key == VK_DELETE as u32 {
                if field.selection().is_empty() {
                    field.anchor = Some(if key == VK_BACK as u32 {
                        field.previous()
                    } else {
                        field.next()
                    });
                }
                if !field.selection().is_empty() {
                    field.replace("");
                }
            } else if key == VK_RETURN as u32 {
                if index >= 4 {
                    field.replace("\n");
                } else {
                    panel.focus += 1;
                }
            } else {
                let selection = field.selection();
                let target = if key == VK_LEFT as u32 {
                    if !shift && !selection.is_empty() {
                        Some(selection.start)
                    } else {
                        Some(field.previous())
                    }
                } else if key == VK_RIGHT as u32 {
                    if !shift && !selection.is_empty() {
                        Some(selection.end)
                    } else {
                        Some(field.next())
                    }
                } else if key == VK_HOME as u32 {
                    Some(if ctrl { 0 } else { field.line_start() })
                } else if key == VK_END as u32 {
                    Some(if ctrl {
                        field.text.len()
                    } else {
                        field.line_end()
                    })
                } else if key == VK_UP as u32 {
                    Some(field.vertical(false))
                } else if key == VK_DOWN as u32 {
                    Some(field.vertical(true))
                } else {
                    None
                };
                if let Some(target) = target {
                    field.move_to(target, shift);
                }
            }
        }
        self.run_config_reveal(hwnd);
        self.refresh(hwnd);
    }

    pub(super) fn run_config_character(&mut self, hwnd: HWND, unit: u16) {
        if let Some(ch) = super::input::decode_utf16_input(&mut self.pending_high_surrogate, unit)
            && !ch.is_control()
            && let Some(panel) = &mut self.run_config_panel
            && (3..9).contains(&panel.focus)
            && panel.enabled()
        {
            panel.fields[panel.focus - 3].replace(&ch.to_string());
            self.refresh(hwnd);
        }
    }

    pub(super) fn run_config_scroll(&mut self, hwnd: HWND, delta: i32, x: i32) {
        let layout = self.run_config_layout(hwnd);
        let step = self.scale(44);
        let max = (self.scale(420) - (layout.body.bottom - layout.body.top)).max(0);
        let row_height = self.scale(36).max(1);
        if let Some(panel) = &mut self.run_config_panel {
            if x < layout.body.left {
                let visible = ((layout.list.bottom - layout.list.top) / row_height).max(1) as usize;
                let max = panel
                    .saved
                    .configurations
                    .len()
                    .saturating_add(1)
                    .saturating_sub(visible);
                panel.list_first = if delta > 0 {
                    panel.list_first.saturating_sub(3)
                } else {
                    (panel.list_first + 3).min(max)
                };
            } else {
                panel.scroll = (panel.scroll + if delta > 0 { -step } else { step }).clamp(0, max);
            }
        }
        self.refresh(hwnd);
    }

    fn run_config_field_position(
        &self,
        hwnd: HWND,
        field: &Field,
        rect: RECT,
        multiline: bool,
        active: bool,
        point: POINT,
    ) -> usize {
        let POINT { x, y } = point;
        let line_height = self.scale(21).max(1);
        let visible = ((rect.bottom - rect.top - self.scale(12)) / line_height).max(1) as usize;
        let cursor_line = field.text[..field.cursor]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let first = if multiline && active {
            cursor_line.saturating_sub(visible - 1)
        } else {
            0
        };
        let row = first + ((y - rect.top - self.scale(7)).max(0) / line_height) as usize;
        let dc = unsafe { GetDC(hwnd) };
        let previous = unsafe { SelectObject(dc, self.ui_font) };
        let caret_width = self.text_width(dc, &field.text[field.line_start()..field.cursor]);
        let offset = if active {
            (caret_width - (rect.right - rect.left - self.scale(24))).max(0)
        } else {
            0
        };
        let mut start = 0;
        let mut result = field.text.len();
        for (line_index, line) in field.text.split('\n').enumerate() {
            if line_index == row {
                let position = (x - rect.left - self.scale(10) + offset).max(0);
                result = start + line.len();
                let mut previous_width = 0;
                for (byte, ch) in line.char_indices() {
                    let width = self.text_width(dc, &line[..byte + ch.len_utf8()]);
                    if position < (previous_width + width) / 2 {
                        result = start + byte;
                        break;
                    }
                    previous_width = width;
                }
                break;
            }
            start += line.len() + 1;
        }
        unsafe {
            SelectObject(dc, previous);
            ReleaseDC(hwnd, dc);
        }
        result
    }

    pub(super) fn run_config_click(
        &mut self,
        hwnd: HWND,
        x: i32,
        y: i32,
        extend: bool,
        drag: bool,
    ) {
        let layout = self.run_config_layout(hwnd);
        let full = self.scale(420);
        let page = layout.body.bottom - layout.body.top;
        let thumb = (page * page / full).max(self.scale(20));
        let track_left = layout.card.right - self.scale(16);
        let on_track = x >= track_left
            && x < layout.card.right
            && y >= layout.body.top
            && y < layout.body.bottom;
        let panel = self.run_config_panel.as_mut().unwrap();
        if !drag {
            panel.scroll_grab = None;
        }
        if full > page && (on_track || drag && panel.scroll_grab.is_some()) {
            let top = layout.body.top + panel.scroll * (page - thumb) / (full - page);
            let grab = *panel
                .scroll_grab
                .get_or_insert(if y >= top && y < top + thumb {
                    y - top
                } else {
                    thumb / 2
                });
            panel.scroll = ((y - layout.body.top - grab) * (full - page) / (page - thumb).max(1))
                .clamp(0, full - page);
            self.dragging = true;
            unsafe {
                SetCapture(hwnd);
            }
            self.refresh(hwnd);
            return;
        }
        if !drag {
            if let Some((action, _)) = layout
                .buttons
                .iter()
                .find(|(_, rect)| contains(*rect, x, y))
            {
                self.run_config_action(hwnd, *action);
                return;
            }
            if contains(layout.list, x, y) {
                let row = ((y - layout.list.top) / self.scale(36).max(1)) as usize;
                if let Some(panel) = &mut self.run_config_panel {
                    let index = row + panel.list_first;
                    if index <= panel.saved.configurations.len() {
                        panel.load(index.checked_sub(1), false);
                        panel.focus = 0;
                    }
                }
                self.refresh(hwnd);
                return;
            }
        }
        if contains(layout.body, x, y) || drag {
            let panel = self.run_config_panel.as_ref().unwrap();
            let index = if drag {
                (3..9).contains(&panel.focus).then(|| panel.focus - 3)
            } else {
                layout.fields.iter().position(|rect| contains(*rect, x, y))
            };
            if let Some(index) = index
                && panel.enabled()
            {
                let position = self.run_config_field_position(
                    hwnd,
                    &panel.fields[index],
                    layout.fields[index],
                    index >= 4,
                    panel.focus == index + 3,
                    POINT { x, y },
                );
                let panel = self.run_config_panel.as_mut().unwrap();
                panel.focus = index + 3;
                panel.fields[index].move_to(position, extend || drag);
                if !drag {
                    panel.fields[index].anchor.get_or_insert(position);
                    self.dragging = true;
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                self.refresh(hwnd);
            }
        }
    }

    pub(super) fn run_config_hover(&mut self, hwnd: HWND, x: i32, y: i32) {
        let layout = self.run_config_layout(hwnd);
        let hover = layout
            .buttons
            .iter()
            .find(|(_, r)| contains(*r, x, y))
            .map(|(id, _)| *id);
        let field = contains(layout.body, x, y)
            && layout.fields.iter().any(|r| contains(*r, x, y))
            && self.run_config_panel.as_ref().is_some_and(|p| p.enabled());
        unsafe {
            SetCursor(LoadCursorW(
                null_mut(),
                if field { IDC_IBEAM } else { IDC_ARROW },
            ));
        }
        if let Some(panel) = &mut self.run_config_panel
            && panel.hover != hover
        {
            panel.hover = hover;
            self.refresh(hwnd);
        }
    }

    pub(super) fn run_config_end_drag(&mut self) {
        if let Some(panel) = &mut self.run_config_panel {
            panel.scroll_grab = None;
        }
    }

    pub(super) fn paint_run_configurations(&self, hwnd: HWND, hdc: HDC) {
        let Some(panel) = &self.run_config_panel else {
            return;
        };
        let layout = self.run_config_layout(hwnd);
        let s = |v| self.scale(v);
        let mut client = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut client);
            SelectObject(hdc, self.ui_font);
        }
        // Cover the workbench with its own palette; no native window or controls.
        Self::fill(
            hdc,
            RECT {
                top: self.chrome_top(),
                ..client
            },
            self.theme.shell_bg,
        );
        self.panel_card(
            hdc,
            layout.card,
            s(12),
            self.theme.edge,
            self.theme.sidebar_bg,
        );
        self.label_mid(
            hdc,
            "Run Configurations",
            layout.card.left + s(22),
            layout.card.top + s(28),
            self.theme.text,
            layout.card,
        );
        Self::label(
            hdc,
            &display_path(&panel.root),
            layout.card.left + s(22),
            layout.card.top + s(47),
            self.theme.muted,
            RECT {
                right: layout.card.right - s(60),
                ..layout.card
            },
        );
        for (id, rect) in layout.buttons {
            let enabled = id != 2 || panel.editing.is_some();
            let focused = panel.focus == id;
            let primary = id == 10;
            self.panel_card(
                hdc,
                rect,
                s(6),
                if focused {
                    self.theme.blue
                } else {
                    self.theme.edge
                },
                if primary || panel.hover == Some(id) && enabled {
                    self.theme.active_bg
                } else {
                    self.theme.editor_bg
                },
            );
            let label = match id {
                1 => "+ New",
                2 => "Delete",
                9 => "Cancel",
                10 => "Save & Select",
                _ => "×",
            };
            let x = (rect.left + rect.right - self.text_width(hdc, label)) / 2;
            self.label_mid(
                hdc,
                label,
                x,
                (rect.top + rect.bottom) / 2,
                if enabled {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                rect,
            );
        }
        let clipped = unsafe { SaveDC(hdc) };
        unsafe {
            IntersectClipRect(
                hdc,
                layout.list.left,
                layout.list.top,
                layout.list.right,
                layout.list.bottom,
            );
        }
        for index in panel.list_first..=panel.saved.configurations.len() {
            let y = layout.list.top + (index - panel.list_first) as i32 * s(36);
            if y >= layout.list.bottom {
                break;
            }
            let rect = RECT {
                top: y,
                bottom: y + s(32),
                ..layout.list
            };
            let selected = if index == 0 {
                !panel.enabled()
            } else {
                panel.editing == Some(index - 1)
            };
            if selected {
                self.panel_card(
                    hdc,
                    rect,
                    s(6),
                    if panel.focus == 0 {
                        self.theme.blue
                    } else {
                        self.theme.edge
                    },
                    self.theme.select_bg,
                );
            }
            let label = if index == 0 {
                "Automatic"
            } else {
                &panel.saved.configurations[index - 1].name
            };
            self.label_mid(
                hdc,
                label,
                rect.left + s(10),
                (rect.top + rect.bottom) / 2,
                self.theme.text,
                rect,
            );
        }
        unsafe {
            RestoreDC(hdc, clipped);
        }
        let clipped = unsafe { SaveDC(hdc) };
        unsafe {
            IntersectClipRect(
                hdc,
                layout.body.left,
                layout.body.top,
                layout.body.right,
                layout.body.bottom,
            );
        }
        let labels = [
            "Name",
            "Entry file · blank uses active file",
            "Command · blank detects language",
            "Working directory · blank uses detected folder",
            "Arguments · one per line, no quoting needed",
            "Environment · one NAME=value per line",
        ];
        for (index, rect) in layout.fields.iter().copied().enumerate() {
            if rect.bottom < layout.body.top || rect.top - s(25) > layout.body.bottom {
                continue;
            }
            Self::label(
                hdc,
                labels[index],
                rect.left,
                rect.top - s(23),
                self.theme.muted,
                layout.body,
            );
            let active = panel.enabled() && panel.focus == index + 3;
            self.panel_card(
                hdc,
                rect,
                s(6),
                if active {
                    self.theme.blue
                } else {
                    self.theme.edge
                },
                self.theme.editor_bg,
            );
            let field = &panel.fields[index];
            let inner = RECT {
                left: rect.left + s(10),
                top: rect.top + s(7),
                right: rect.right - s(10),
                bottom: rect.bottom - s(5),
            };
            let saved = unsafe { SaveDC(hdc) };
            unsafe {
                IntersectClipRect(hdc, inner.left, inner.top, inner.right, inner.bottom);
            }
            let line_height = s(21).max(1);
            let visible = ((inner.bottom - inner.top) / line_height).max(1) as usize;
            let cursor_line = field.text[..field.cursor]
                .bytes()
                .filter(|b| *b == b'\n')
                .count();
            let first = if active && index >= 4 {
                cursor_line.saturating_sub(visible - 1)
            } else {
                0
            };
            let offset = if active {
                (self.text_width(hdc, &field.text[field.line_start()..field.cursor])
                    - (inner.right - inner.left - s(4)))
                .max(0)
            } else {
                0
            };
            let selection = field.selection();
            let mut start = 0;
            for (row, line) in field.text.split('\n').enumerate() {
                if row >= first && row < first + visible {
                    let y = inner.top + (row - first) as i32 * line_height;
                    let x = inner.left - offset;
                    if active
                        && !selection.is_empty()
                        && selection.start <= start + line.len()
                        && selection.end >= start
                    {
                        let from = selection.start.saturating_sub(start).min(line.len());
                        let to = selection.end.saturating_sub(start).min(line.len());
                        Self::fill(
                            hdc,
                            RECT {
                                left: x + self.text_width(hdc, &line[..from]),
                                top: y,
                                right: x + self.text_width(hdc, &line[..to]),
                                bottom: y + line_height,
                            },
                            self.theme.select_bg,
                        );
                    }
                    Self::label(
                        hdc,
                        line,
                        x,
                        y,
                        if panel.enabled() {
                            self.theme.text
                        } else {
                            self.theme.muted
                        },
                        inner,
                    );
                    if active && row == cursor_line {
                        let caret = x + self.text_width(hdc, &line[..field.cursor - start]);
                        Self::fill(
                            hdc,
                            RECT {
                                left: caret,
                                top: y,
                                right: caret + s(1).max(1),
                                bottom: y + line_height,
                            },
                            self.theme.text,
                        );
                    }
                }
                start += line.len() + 1;
            }
            if field.text.is_empty() {
                let placeholder = match index {
                    0 => "Configuration name",
                    1 => "src/main.js",
                    2 => "node, npm, cargo, or executable path",
                    3 => "Workspace-relative folder",
                    4 => "hello world\n--verbose",
                    _ => "MODE=development",
                };
                for (row, line) in placeholder.lines().enumerate() {
                    Self::label(
                        hdc,
                        line,
                        inner.left,
                        inner.top + row as i32 * line_height,
                        self.theme.muted,
                        inner,
                    );
                }
            }
            unsafe {
                RestoreDC(hdc, saved);
            }
        }
        unsafe {
            RestoreDC(hdc, clipped);
        }
        if !panel.error.is_empty() {
            Self::label(
                hdc,
                &panel.error,
                layout.footer.left,
                layout.footer.top,
                self.theme.error,
                layout.footer,
            );
        } else {
            Self::label(
                hdc,
                if panel.enabled() {
                    "Paths are relative to this workspace."
                } else {
                    "Automatic follows the active file and detects its language."
                },
                layout.footer.left,
                layout.footer.top,
                self.theme.muted,
                layout.footer,
            );
            Self::label(
                hdc,
                "Tab to move · Ctrl+S to save · Esc to close",
                layout.footer.left,
                layout.footer.top + s(23),
                self.theme.muted,
                layout.footer,
            );
        }
        if self.scale(420) > layout.body.bottom - layout.body.top {
            let track = RECT {
                left: layout.card.right - s(12),
                top: layout.body.top,
                right: layout.card.right - s(8),
                bottom: layout.body.bottom,
            };
            Self::fill(hdc, track, self.theme.edge);
            let page = layout.body.bottom - layout.body.top;
            let full = s(420);
            let thumb = (page * page / full).max(s(20));
            let top = track.top + panel.scroll * (page - thumb) / (full - page).max(1);
            Self::rounded_fill(
                hdc,
                RECT {
                    top,
                    bottom: top + thumb,
                    ..track
                },
                s(3),
                self.theme.muted,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_selection_and_multiline_navigation() {
        let mut field = Field::from("a🙂b\nsecond".into());
        field.cursor = 5;
        assert_eq!(field.previous(), 1);
        assert_eq!(field.next(), 6);
        field.move_to(1, true);
        field.replace("é");
        assert_eq!(field.text, "aéb\nsecond");
        field.cursor = 3;
        assert_eq!(field.vertical(true), 7);
        field.move_to(0, false);
        field.move_to(field.text.len(), true);
        field.replace("replacement");
        assert_eq!(field.text, "replacement");
    }
}
