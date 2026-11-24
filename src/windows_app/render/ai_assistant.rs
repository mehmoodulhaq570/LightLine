use super::super::ai_chat::{AiAction, ChatEntry, SUGGESTED_MODEL, model_menu_models};
use super::super::ai_project::Permission;
use super::super::markdown_view::SNIPPET_CODE_HEADER;
use super::super::*;
use super::blend;
use lightline::ai::{self, Role};

// The AI Assistant panel. Until a model is chosen it explains the
// assistant and offers to connect to Ollama; then it shows the conversation
// (questions as bubbles, answers as Markdown) above the message box.
//
// Painting only lays out text the panel already holds, and each answer's
// layout is kept until its text or the panel's width changes, so an open
// panel costs nothing while it's idle.

// What the panel promises about AI, whatever the model.
const AI_PROMISES: [&str; 3] = [
    "Nothing runs in the background",
    "Nothing is sent until you ask",
    "Editing never depends on AI",
];

// Most lines the message box grows to before its text scrolls.
const COMPOSER_LINES: usize = 6;

// An attached file's chip: its icon (from the path), what it says, and
// whether it's amber.
struct ChipSpec {
    path: PathBuf,
    label: String,
    warn: bool,
}

// The permission card, laid out (`ai_permission_layout`).
struct PermissionLayout {
    title: String,
    lines: Vec<String>,
    buttons: Vec<(RECT, &'static str, Permission, bool)>,
    height: i32,
}

fn chip_specs(chips: &[super::super::ai_attach::Chip]) -> Vec<ChipSpec> {
    chips
        .iter()
        .map(|chip| ChipSpec {
            path: chip.path.clone(),
            label: chip.label.clone(),
            warn: chip.warn,
        })
        .collect()
}

impl App {
    pub(in crate::windows_app) fn paint_ai_assistant(&self, hdc: HDC, rect: RECT) {
        let s = |value: i32| self.scale(value);
        self.panel_card(
            hdc,
            rect,
            s(CARD_RADIUS),
            self.theme.card_edge,
            self.theme.sidebar_bg,
        );
        *self.ai.hits.borrow_mut() = Default::default();
        let header_bottom = rect.top + s(AI_HEADER);
        self.paint_ai_header(hdc, rect, header_bottom);

        let content = RECT {
            left: rect.left + s(16),
            top: header_bottom + s(14),
            right: rect.right - s(16),
            bottom: rect.bottom - s(14),
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        if !self.ai_ready() {
            let composer = RECT {
                top: content.bottom - s(50),
                ..content
            };
            self.paint_ai_setup_state(
                hdc,
                RECT {
                    top: content.top + s(10),
                    bottom: composer.top - s(12),
                    ..content
                },
            );
            self.paint_ai_composer(hdc, composer, &[], false);
            return;
        }

        let text_width = (content.right - content.left - s(64)).max(s(40));
        let lines = self.wrap_text(hdc, &self.ai.input, text_width);
        let shown = lines.len().clamp(1, COMPOSER_LINES);
        let composer = RECT {
            top: content.bottom - (shown as i32 * line_height + s(28)).max(s(50)),
            ..content
        };
        let mut conversation_bottom = composer.top - s(10);
        // Files waiting to go with the question, above the box.
        if !self.ai.attachments.is_empty() {
            let chips: Vec<ChipSpec> = self
                .ai
                .attachments
                .iter()
                .map(|attached| ChipSpec {
                    path: attached.path.clone(),
                    label: if attached.secret {
                        format!("{} \u{00b7} looks like secrets", attached.name)
                    } else {
                        attached.name.clone()
                    },
                    warn: attached.secret,
                })
                .collect();
            let (spans, height) =
                self.ai_chip_spans(hdc, &chips, composer.right - composer.left, true);
            let top = conversation_bottom - height;
            let mut hits = Vec::new();
            for (index, (chip, span)) in chips.iter().zip(&spans).enumerate() {
                let rect = RECT {
                    left: composer.left + span.left,
                    top: top + span.top,
                    right: composer.left + span.right,
                    bottom: top + span.bottom,
                };
                if let Some(close) = self.paint_ai_chip(hdc, rect, chip, true) {
                    hits.push((close, index));
                }
            }
            self.ai.hits.borrow_mut().chip_close = hits;
            conversation_bottom = top - s(8);
        }
        if let Some(label) = self.ai_selection_label() {
            let top = conversation_bottom - line_height;
            self.paint_ai_context(hdc, &label, RECT { top, ..composer });
            conversation_bottom = top - s(6);
        }
        self.paint_ai_conversation(
            hdc,
            RECT {
                bottom: conversation_bottom,
                ..content
            },
        );
        self.paint_ai_composer(
            hdc,
            composer,
            &lines[lines.len() - shown.min(lines.len())..],
            true,
        );
        if self.ai_mention_query().is_some() {
            self.paint_ai_mentions(hdc, composer, conversation_bottom, header_bottom);
        }
        if self.ai.model_menu_open {
            self.paint_ai_model_picker(hdc, rect, header_bottom);
        }
    }

    fn paint_ai_header(&self, hdc: HDC, rect: RECT, header_bottom: i32) {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.brand_font) };
        self.sparkle_glyph(
            hdc,
            rect.left + s(18),
            rect.top + s(15),
            s(20),
            ui(148, 102, 255),
        );
        let title = "AI Assistant";
        Self::label(
            hdc,
            title,
            rect.left + s(48),
            rect.top + s(16),
            self.theme.text,
            rect,
        );
        let title_right = rect.left + s(48) + self.text_width(hdc, title);
        // Close; its hit target is in ai_click.
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(
            hdc,
            "\u{00d7}",
            rect.right - s(25),
            rect.top + s(16),
            self.theme.muted,
            rect,
        );
        if let Some(model) = self.settings.ai_model.as_deref() {
            let new_chat = RECT {
                left: rect.right - s(60),
                top: rect.top + s(12),
                right: rect.right - s(36),
                bottom: rect.top + s(40),
            };
            unsafe { SelectObject(hdc, self.brand_font) };
            self.label_mid(
                hdc,
                "+",
                new_chat.left + s(7),
                (new_chat.top + new_chat.bottom) / 2,
                self.theme.muted,
                new_chat,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            self.ai.hits.borrow_mut().new_chat = Some(new_chat);

            let chip_left = title_right + s(12);
            let chip_right =
                (chip_left + self.text_width(hdc, model) + s(34)).min(new_chat.left - s(8));
            if chip_right - chip_left >= s(56) {
                let chip = RECT {
                    left: chip_left,
                    top: rect.top + s(13),
                    right: chip_right,
                    bottom: rect.top + s(39),
                };
                self.panel_card(hdc, chip, s(7), ui(66, 47, 126), ui(42, 28, 86));
                self.label_ellipsis(
                    hdc,
                    model,
                    chip.left + s(10),
                    chip.top + s(4),
                    ui(211, 195, 255),
                    RECT {
                        right: chip.right - s(20),
                        ..chip
                    },
                );
                Self::label(
                    hdc,
                    if self.ai.model_menu_open {
                        "\u{25b4}"
                    } else {
                        "\u{25be}"
                    },
                    chip.right - s(16),
                    chip.top + s(4),
                    ui(195, 165, 255),
                    chip,
                );
                self.ai.hits.borrow_mut().model = Some(chip);
            }
        }
        Self::fill(
            hdc,
            RECT {
                left: rect.left + s(1),
                top: header_bottom,
                right: rect.right - s(1),
                bottom: header_bottom + s(1).max(1),
            },
            self.theme.edge,
        );
    }

    /// The themed model picker. It deliberately stays within the assistant
    /// card and above the composer, unlike the old native popup menu.
    fn paint_ai_model_picker(&self, hdc: HDC, panel: RECT, header_bottom: i32) {
        let s = |value: i32| self.scale(value);
        let menu_top = header_bottom + s(8);
        let menu = RECT {
            left: panel.left + s(18),
            top: menu_top,
            right: panel.right - s(18),
            bottom: (menu_top + s(451)).min(panel.bottom - s(78)),
        };
        if menu.right - menu.left < s(210) || menu.bottom - menu.top < s(240) {
            return;
        }

        // A restrained shadow gives the popup elevation without the bright,
        // disconnected appearance of a platform context menu.
        Self::rounded_fill(
            hdc,
            RECT {
                left: menu.left + s(4),
                top: menu.top + s(5),
                right: menu.right + s(4),
                bottom: menu.bottom + s(5),
            },
            s(10),
            ui(3, 9, 19),
        );
        self.panel_card(hdc, menu, s(10), ui(69, 75, 169), ui(10, 25, 47));

        let search = RECT {
            left: menu.left + s(12),
            top: menu.top + s(12),
            right: menu.right - s(12),
            bottom: menu.top + s(54),
        };
        self.panel_card(hdc, search, s(8), ui(40, 75, 126), ui(13, 31, 57));
        self.rail_icon(
            hdc,
            1,
            search.left + s(12),
            search.top + s(12),
            ui(145, 174, 218),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let query_empty = self.ai.model_query.is_empty();
        let query = if query_empty {
            "Search models..."
        } else {
            &self.ai.model_query
        };
        let query_x = search.left + s(42);
        self.label_ellipsis(
            hdc,
            query,
            query_x,
            search.top + s(11),
            if query_empty {
                self.theme.muted
            } else {
                self.theme.text
            },
            RECT {
                left: query_x,
                right: search.right - s(10),
                ..search
            },
        );
        if !query_empty {
            let caret_x = (query_x + self.text_width(hdc, query)).min(search.right - s(10));
            Self::fill(
                hdc,
                RECT {
                    left: caret_x,
                    top: search.top + s(10),
                    right: caret_x + s(1).max(1),
                    bottom: search.bottom - s(10),
                },
                ui(116, 190, 250),
            );
        }

        let actions_top = menu.bottom - s(113);
        Self::fill(
            hdc,
            RECT {
                left: menu.left + s(1),
                top: actions_top,
                right: menu.right - s(1),
                bottom: actions_top + s(1).max(1),
            },
            ui(31, 59, 99),
        );
        let refresh = RECT {
            left: menu.left + s(8),
            top: actions_top + s(7),
            right: menu.right - s(8),
            bottom: actions_top + s(38),
        };
        let provider = RECT {
            top: refresh.bottom,
            bottom: refresh.bottom + s(31),
            ..refresh
        };
        let turn_off = RECT {
            top: provider.bottom,
            bottom: menu.bottom - s(7),
            ..refresh
        };
        Self::label(
            hdc,
            "\u{2699}",
            provider.left + s(10),
            provider.top + s(5),
            ui(145, 174, 218),
            provider,
        );
        self.label_ellipsis(
            hdc,
            &format!(
                "Change provider\u{2026} ({})",
                ai::host(&self.settings.ai_endpoint)
            ),
            provider.left + s(38),
            provider.top + s(5),
            self.theme.text,
            provider,
        );
        Self::label(
            hdc,
            "\u{21bb}",
            refresh.left + s(10),
            refresh.top + s(5),
            ui(145, 174, 218),
            refresh,
        );
        Self::label(
            hdc,
            if self.ai.connecting {
                "Refreshing model list..."
            } else {
                "Refresh model list"
            },
            refresh.left + s(38),
            refresh.top + s(5),
            self.theme.text,
            refresh,
        );
        Self::label(
            hdc,
            "\u{23fb}",
            turn_off.left + s(10),
            turn_off.top + s(5),
            ui(145, 174, 218),
            turn_off,
        );
        Self::label(
            hdc,
            "Turn off AI Assistant",
            turn_off.left + s(38),
            turn_off.top + s(5),
            self.theme.text,
            turn_off,
        );

        let models = model_menu_models(
            &self.ai.models,
            self.settings.ai_model.as_deref(),
            &self.ai.model_query,
        );
        let first = self.ai.model_menu_first.min(models.len().saturating_sub(1));
        let list = RECT {
            left: menu.left + s(8),
            top: search.bottom + s(9),
            right: menu.right - s(8),
            bottom: actions_top - s(5),
        };
        let row_height = s(36);
        let section_height = s(22);
        let current = self.settings.ai_model.as_deref();
        let mut y = list.top;
        let mut previous_cloud = None;
        let mut visible_rows = 0usize;
        let mut row_hits = Vec::new();
        for model in models.iter().skip(first) {
            if visible_rows >= 6 {
                break;
            }
            let cloud = ai::is_cloud_model(model);
            if previous_cloud != Some(cloud) {
                if y + section_height + row_height > list.bottom {
                    break;
                }
                let title = if cloud { "CLOUD" } else { "LOCAL" };
                Self::label(
                    hdc,
                    title,
                    list.left + s(8),
                    y + s(2),
                    ui(145, 174, 218),
                    list,
                );
                let title_width = self.text_width(hdc, title);
                Self::fill(
                    hdc,
                    RECT {
                        left: list.left + s(18) + title_width,
                        top: y + s(10),
                        right: list.right - s(12),
                        bottom: y + s(11),
                    },
                    ui(34, 64, 105),
                );
                y += section_height;
                previous_cloud = Some(cloud);
            }
            if y + row_height > list.bottom {
                break;
            }

            let row = RECT {
                left: list.left,
                top: y,
                right: list.right - s(6),
                bottom: y + row_height,
            };
            let selected = current == Some(model.as_str());
            if selected {
                self.panel_card(hdc, row, s(6), ui(73, 83, 170), ui(29, 37, 91));
                Self::label(
                    hdc,
                    "\u{2713}",
                    row.left + s(10),
                    row.top + s(7),
                    ui(71, 210, 250),
                    row,
                );
            }

            let icon = RECT {
                left: row.left + s(31),
                top: row.top + s(6),
                right: row.left + s(55),
                bottom: row.top + s(30),
            };
            self.paint_ai_model_icon(hdc, icon, model, cloud);

            let meta = if selected {
                "Selected"
            } else if cloud {
                "Cloud"
            } else {
                "Local"
            };
            let meta_width = self.text_width(hdc, meta);
            if cloud && !selected {
                let badge = RECT {
                    left: row.right - meta_width - s(22),
                    top: row.top + s(7),
                    right: row.right - s(7),
                    bottom: row.bottom - s(7),
                };
                Self::rounded_fill(hdc, badge, s(6), ui(24, 53, 92));
                Self::label(
                    hdc,
                    meta,
                    badge.left + s(8),
                    badge.top + s(3),
                    ui(157, 190, 232),
                    badge,
                );
            } else {
                Self::label(
                    hdc,
                    meta,
                    row.right - meta_width - s(9),
                    row.top + s(8),
                    if selected {
                        ui(71, 210, 250)
                    } else {
                        self.theme.muted
                    },
                    row,
                );
            }
            self.label_ellipsis(
                hdc,
                model,
                row.left + s(64),
                row.top + s(8),
                self.theme.text,
                RECT {
                    left: row.left + s(64),
                    right: row.right - meta_width - s(32),
                    ..row
                },
            );
            row_hits.push((row, model.clone()));
            y += row_height;
            visible_rows += 1;
        }

        if models.is_empty() {
            Self::label(
                hdc,
                "No matching models",
                list.left + s(10),
                list.top + s(12),
                self.theme.muted,
                list,
            );
        } else if models.len() > visible_rows {
            let track = RECT {
                left: list.right - s(3),
                top: list.top + s(4),
                right: list.right,
                bottom: list.bottom - s(4),
            };
            Self::rounded_fill(hdc, track, s(2), ui(20, 45, 78));
            let track_height = track.bottom - track.top;
            let thumb_height =
                ((track_height as usize * visible_rows) / models.len()).max(s(24) as usize) as i32;
            let max_first = models.len().saturating_sub(visible_rows).max(1);
            let offset = ((track_height - thumb_height) as usize * first / max_first) as i32;
            Self::rounded_fill(
                hdc,
                RECT {
                    top: track.top + offset,
                    bottom: track.top + offset + thumb_height,
                    ..track
                },
                s(2),
                ui(70, 112, 170),
            );
        }

        let mut hits = self.ai.hits.borrow_mut();
        hits.model_menu = Some(menu);
        hits.model_search = Some(search);
        hits.model_rows = row_hits;
        hits.model_refresh = Some(refresh);
        hits.model_turn_off = Some(turn_off);
        hits.model_provider = Some(provider);
    }

    /// Standalone provider marks matching the model-picker mockup: the logo
    /// itself carries the identity, without a colored tile behind every icon.
    /// They are vector-drawn so they stay sharp at every Windows DPI.
    fn paint_ai_model_icon(&self, hdc: HDC, rect: RECT, model: &str, cloud: bool) {
        let s = |value: i32| self.scale(value);
        let name = model.to_ascii_lowercase();
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        unsafe {
            if name.starts_with("qwen") {
                // Six faceted ribbons form Qwen's purple hexagonal knot.
                let outer = s(10) as f32;
                let inner = s(5) as f32;
                let colors = [
                    ui(143, 92, 246),
                    ui(126, 87, 238),
                    ui(108, 92, 231),
                    ui(126, 87, 238),
                    ui(154, 101, 255),
                    ui(139, 92, 246),
                ];
                let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                let point = |angle: f32, radius: f32| POINT {
                    x: cx + (angle.cos() * radius).round() as i32,
                    y: cy + (angle.sin() * radius).round() as i32,
                };
                for (index, color) in colors.into_iter().enumerate() {
                    let first =
                        -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 3.0;
                    let second = first + std::f32::consts::PI / 3.0;
                    let points = [
                        point(first, outer),
                        point(second, outer),
                        point(second, inner),
                        point(first, inner),
                    ];
                    let brush = CreateSolidBrush(color);
                    let old_brush = SelectObject(hdc, brush);
                    Polygon(hdc, points.as_ptr(), points.len() as i32);
                    SelectObject(hdc, old_brush);
                    DeleteObject(brush);
                }
                SelectObject(hdc, old_pen);
                return;
            }

            if name.starts_with("gemma") {
                // Compact multicolor Google G used by the Gemma family.
                let segments = [
                    (ui(66, 133, 244), (-7, -7, 7, -7)),
                    (ui(234, 67, 53), (-7, -7, -9, 1)),
                    (ui(251, 188, 5), (-9, 1, -4, 8)),
                    (ui(52, 168, 83), (-4, 8, 7, 5)),
                    (ui(66, 133, 244), (7, 5, 7, 0)),
                ];
                for (color, (x1, y1, x2, y2)) in segments {
                    let pen = CreatePen(PS_SOLID, s(4).max(2), color);
                    let old_pen = SelectObject(hdc, pen);
                    MoveToEx(hdc, cx + s(x1), cy + s(y1), null_mut());
                    LineTo(hdc, cx + s(x2), cy + s(y2));
                    SelectObject(hdc, old_pen);
                    DeleteObject(pen);
                }
                let blue = CreatePen(PS_SOLID, s(3).max(2), ui(66, 133, 244));
                let old_pen = SelectObject(hdc, blue);
                MoveToEx(hdc, cx, cy, null_mut());
                LineTo(hdc, cx + s(9), cy);
                SelectObject(hdc, old_pen);
                DeleteObject(blue);
                return;
            }

            let mark = if name.starts_with("nemotron") {
                ui(118, 196, 38)
            } else if name.starts_with("llama") {
                ui(66, 133, 244)
            } else if name.starts_with("gpt") {
                self.theme.text
            } else if cloud {
                self.theme.sky
            } else {
                self.theme.violet
            };
            let pen = CreatePen(PS_SOLID, s(2).max(2), mark);
            if pen.is_null() {
                return;
            }
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            if name.starts_with("gpt") {
                // White six-loop OpenAI knot, free of a surrounding tile.
                let r = s(3);
                for (dx, dy) in [
                    (0, -s(5)),
                    (s(5), -s(2)),
                    (s(5), s(3)),
                    (0, s(5)),
                    (-s(5), s(3)),
                    (-s(5), -s(2)),
                ] {
                    Ellipse(hdc, cx + dx - r, cy + dy - r, cx + dx + r, cy + dy + r);
                }
            } else if name.starts_with("nemotron") {
                // NVIDIA eye and iris, plus the squared terminal stroke seen
                // in the approved mockup.
                Arc(
                    hdc,
                    rect.left,
                    cy - s(8),
                    rect.right - s(2),
                    cy + s(8),
                    rect.right - s(2),
                    cy,
                    rect.left,
                    cy,
                );
                Ellipse(hdc, cx - s(5), cy - s(5), cx + s(5), cy + s(5));
                let pupil = CreateSolidBrush(mark);
                let prior = SelectObject(hdc, pupil);
                Ellipse(hdc, cx - s(2), cy - s(2), cx + s(2), cy + s(2));
                SelectObject(hdc, prior);
                DeleteObject(pupil);
                MoveToEx(hdc, rect.right - s(5), cy - s(6), null_mut());
                LineTo(hdc, rect.right - s(1), cy - s(6));
                LineTo(hdc, rect.right - s(1), cy + s(6));
                LineTo(hdc, rect.right - s(5), cy + s(6));
            } else if name.starts_with("llama") {
                // Meta/Llama infinity mark, also standalone.
                Ellipse(hdc, cx - s(10), cy - s(6), cx, cy + s(6));
                Ellipse(hdc, cx, cy - s(6), cx + s(10), cy + s(6));
                MoveToEx(hdc, cx - s(5), cy - s(4), null_mut());
                LineTo(hdc, cx + s(5), cy + s(4));
                MoveToEx(hdc, cx - s(5), cy + s(4), null_mut());
                LineTo(hdc, cx + s(5), cy - s(4));
            } else if cloud {
                // Generic cloud provider mark.
                Ellipse(hdc, cx - s(7), cy - s(2), cx + s(7), cy + s(6));
                Ellipse(hdc, cx - s(5), cy - s(7), cx + s(2), cy + s(3));
                Ellipse(hdc, cx, cy - s(5), cx + s(6), cy + s(3));
            } else {
                // Unknown local model: the same compact AI sparkle language
                // as the rest of the assistant, with no background tile.
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                DeleteObject(pen);
                self.sparkle_glyph(hdc, rect.left + s(3), rect.top + s(3), s(18), mark);
                return;
            }
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    // The sparkle tile and title that open the setup and empty-chat views;
    // returns the y below the title.
    fn paint_ai_intro(&self, hdc: HDC, area: RECT, title: &str) -> i32 {
        let s = |value: i32| self.scale(value);
        let tile = RECT {
            left: area.left,
            top: area.top,
            right: area.left + s(44),
            bottom: area.top + s(44),
        };
        Self::rounded_fill(hdc, tile, s(10), ui(38, 28, 84));
        self.sparkle_glyph(
            hdc,
            tile.left + s(11),
            tile.top + s(11),
            s(22),
            ui(180, 150, 255),
        );
        unsafe { SelectObject(hdc, self.brand_font) };
        let y = tile.bottom + s(16);
        Self::label(hdc, title, area.left, y, self.theme.text, area);
        let below = y + self.text_height(hdc) + s(10);
        unsafe { SelectObject(hdc, self.ui_font) };
        below
    }

    // The promises, one per line with a check mark; returns the y below.
    fn paint_ai_promises(&self, hdc: HDC, area: RECT, mut y: i32, line_height: i32) -> i32 {
        let s = |value: i32| self.scale(value);
        for promise in AI_PROMISES {
            Self::label(hdc, "\u{2713}", area.left, y, self.theme.green, area);
            self.label_ellipsis(hdc, promise, area.left + s(22), y, self.theme.muted, area);
            y += line_height + s(2);
        }
        y
    }

    // No model chosen yet: what the assistant does, and a button that asks
    // the server (Ollama by default) which models it has.
    fn paint_ai_setup_state(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        let mut y = self.paint_ai_intro(hdc, area, "Connect a model");
        let line_height = self.text_height(hdc) + s(4);
        let endpoint = &self.settings.ai_endpoint;
        let remote = !ai::is_local(endpoint);
        let host = ai::host(endpoint);
        let about = if remote {
            format!(
                "The assistant explains, fixes and writes code with a model from {host}. \
                 Your questions and the code you include are sent there."
            )
        } else {
            "The assistant explains, fixes and writes code with a model that runs \
             on your PC through Ollama: free, private, and it works offline."
                .to_string()
        };
        y = self.paint_wrapped(hdc, &about, area, y, line_height, self.theme.text);
        y = self.paint_ai_promises(hdc, area, y + s(12), line_height);

        y += s(12);
        if let Some(problem) = &self.ai.problem {
            y = self.paint_wrapped(hdc, problem, area, y, line_height, self.theme.error) + s(8);
        }
        let default_endpoint = self.settings.ai_endpoint == ai::DEFAULT_ENDPOINT;
        let key = if remote { self.ai_key_source() } else { None };
        // A service without a key: the button sets one first.
        let needs_key = remote && key.is_none();
        let label = if self.ai.connecting {
            "Connecting\u{2026}".to_string()
        } else if needs_key {
            "Set API Key".to_string()
        } else if self.ai.problem.is_some() {
            "Try again".to_string()
        } else if default_endpoint {
            "Connect to Ollama".to_string()
        } else if remote {
            format!("Connect to {host}")
        } else {
            "Connect".to_string()
        };
        let label = label.as_str();
        let button = RECT {
            left: area.left,
            top: y,
            right: (area.left + self.text_width(hdc, label) + s(32)).min(area.right),
            bottom: y + s(34),
        };
        if button.bottom <= area.bottom {
            let fill = if self.ai.connecting {
                self.theme.active_bg
            } else {
                self.theme.violet
            };
            Self::rounded_fill(hdc, button, s(7), fill);
            let text = if self.ai.connecting {
                self.theme.muted
            } else {
                label_on(fill, 255, 255, 255)
            };
            self.label_mid(
                hdc,
                label,
                button.left + s(16),
                (button.top + button.bottom) / 2,
                text,
                button,
            );
            if needs_key {
                self.ai.hits.borrow_mut().set_key = Some(button);
            } else if !self.ai.connecting {
                self.ai.hits.borrow_mut().connect = Some(button);
            }
            // Another service, or a key: the provider card.
            let other = "Change provider\u{2026}";
            let provider = RECT {
                left: button.right + s(10),
                right: button.right + s(10) + self.text_width(hdc, other) + s(28),
                ..button
            };
            if provider.right <= area.right {
                self.panel_card(hdc, provider, s(7), self.theme.edge, self.theme.editor_bg);
                self.label_mid(
                    hdc,
                    other,
                    provider.left + s(14),
                    (provider.top + provider.bottom) / 2,
                    self.theme.text,
                    provider,
                );
                self.ai.hits.borrow_mut().provider = Some(provider);
            }
        }
        y = button.bottom + s(12);
        let hint = if remote {
            let key = match key {
                Some(ai::KeySource::Saved) => {
                    "Its API key is saved in Windows Credential Manager.".to_string()
                }
                Some(ai::KeySource::Environment) => {
                    format!(
                        "Its API key comes from {}.",
                        ai::key_variable(&self.settings.ai_endpoint)
                    )
                }
                None => "It needs an API key: Set API Key above.".to_string(),
            };
            format!("Uses {}. {key}", self.settings.ai_endpoint)
        } else if default_endpoint {
            format!(
                "No Ollama yet? Get it from ollama.com, then run in a terminal: \
                 ollama pull {SUGGESTED_MODEL}. Or use a cloud service such as OpenAI \
                 with Change provider."
            )
        } else {
            format!("Uses the server at {}.", self.settings.ai_endpoint)
        };
        self.paint_wrapped(hdc, &hint, area, y, line_height, self.theme.muted);
    }

    // A model is chosen but nothing has been asked yet.
    fn paint_ai_empty_chat(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        let mut y = self.paint_ai_intro(hdc, area, "Ask about your code");
        let line_height = self.text_height(hdc) + s(4);
        let model = self.settings.ai_model.as_deref().unwrap_or_default();
        let local = ai::is_local(&self.settings.ai_endpoint);
        let source = if !local {
            format!(
                "Answers come from {model} via {}: your questions and the code you \
                 include are sent there.",
                ai::host(&self.settings.ai_endpoint)
            )
        } else if ai::is_cloud_model(model) {
            format!(
                "Answers come from {model}, a cloud model: your questions and the code \
                 you include are sent to ollama.com."
            )
        } else {
            format!("Answers come from {model}, running on your PC.")
        };
        y = self.paint_wrapped(
            hdc,
            &format!(
                "Select code in the editor to include it with your question, or type @ to \
                 attach files. {source}"
            ),
            area,
            y,
            line_height,
            self.theme.text,
        );
        self.paint_ai_promises(hdc, area, y + s(12), line_height);
    }

    // The line above the message box saying which code will be sent.
    fn paint_ai_context(&self, hdc: HDC, label: &str, row: RECT) {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let dot = RECT {
            left: row.left + s(2),
            top: row.top + s(7),
            right: row.left + s(9),
            bottom: row.top + s(14),
        };
        Self::rounded_fill(hdc, dot, s(3), self.theme.violet);
        self.label_ellipsis(
            hdc,
            &format!("Includes {label}"),
            row.left + s(16),
            row.top,
            self.theme.muted,
            row,
        );
    }

    fn paint_ai_conversation(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        if area.bottom <= area.top {
            return;
        }
        let entries = &self.ai.entries;
        if entries.is_empty() {
            self.paint_ai_empty_chat(hdc, area);
            return;
        }
        let width = (area.right - area.left).max(s(60));
        let gap = s(16);
        let heights: Vec<i32> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                self.ai_entry_height(hdc, entry, index + 1 == entries.len(), width)
            })
            .collect();
        let total = heights.iter().sum::<i32>() + gap * (entries.len() as i32 - 1);
        let max_scroll = (total - (area.bottom - area.top)).max(0);
        let scroll = if self.ai.follow.get() {
            max_scroll
        } else {
            self.ai.scroll.get().min(max_scroll)
        };
        // Back at the bottom: follow new text again.
        self.ai.follow.set(scroll >= max_scroll);
        self.ai.scroll.set(scroll);
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, area.left, area.top, area.right, area.bottom);
            let mut top = area.top - scroll;
            for (index, (entry, height)) in entries.iter().zip(&heights).enumerate() {
                let bottom = top + height;
                if bottom >= area.top && top <= area.bottom {
                    let bounds = RECT {
                        left: area.left,
                        top,
                        right: area.left + width,
                        bottom,
                    };
                    self.paint_ai_entry(hdc, index, entry, bounds, area);
                }
                top = bottom + gap;
            }
            RestoreDC(hdc, saved);
        }
    }

    // Whether `entry` is the answer still arriving.
    fn ai_receiving(&self, last: bool) -> bool {
        last && self.ai.busy()
    }

    fn ai_entry_height(&self, hdc: HDC, entry: &ChatEntry, last: bool, width: i32) -> i32 {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        if entry.role == Role::User {
            let lines = self.wrap_text(hdc, &entry.text, width - s(24)).len() as i32;
            let context = if entry.context.is_some() {
                line_height + s(2)
            } else {
                0
            };
            let chips = if entry.chips.is_empty() {
                0
            } else {
                let specs = chip_specs(&entry.chips);
                s(6) + self.ai_chip_spans(hdc, &specs, width - s(24), false).1
            };
            return s(20) + lines * line_height + context + chips;
        }
        let mut height = s(26) + self.ai_project_steps_height(hdc, entry, width);
        let (answer, thinking) = ai::visible_answer(&entry.text);
        if !answer.is_empty() {
            height += self.snippet_height(
                hdc,
                &entry.view,
                &self.ai.fonts,
                (answer, entry.revision),
                width,
            );
        } else if (thinking || self.ai_receiving(last)) && !entry.asking {
            height += line_height;
        }
        if let Some(error) = &entry.error {
            unsafe { SelectObject(hdc, self.ui_font) };
            height += s(6) + self.wrap_text(hdc, error, width).len() as i32 * line_height;
        }
        if let Some(note) = &entry.note {
            unsafe { SelectObject(hdc, self.ui_font) };
            height += s(6) + self.wrap_text(hdc, note, width).len() as i32 * line_height;
        }
        if !answer.is_empty() && !self.ai_receiving(last) {
            height += s(28);
        }
        height
    }

    // The permission card for a panel `width` wide: its title, its text's
    // lines, its buttons (placed from the card's inner left and the top of
    // the button area, wrapping when they don't fit) and its height.
    fn ai_permission_layout(&self, hdc: HDC, width: i32) -> PermissionLayout {
        let s = |value: i32| self.scale(value);
        let (title, text) = self.ai_permission_text();
        unsafe { SelectObject(hdc, self.small_font) };
        let lines = self.wrap_text(hdc, &text, width - s(24));
        let small = self.text_height(hdc) + s(3);
        unsafe { SelectObject(hdc, self.ui_font) };
        let title_height = self.text_height(hdc) + s(4);
        let room = width - s(24);
        let (mut x, mut y) = (0, 0);
        let mut buttons = Vec::new();
        for (label, answer, primary) in [
            ("Allow for This Chat", Permission::ForChat, true),
            ("Just This Once", Permission::Once, false),
            ("Don't Allow", Permission::Deny, false),
        ] {
            let w = self.text_width(hdc, label) + s(20);
            if x > 0 && x + w > room {
                x = 0;
                y += s(32);
            }
            buttons.push((
                RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + s(26),
                },
                label,
                answer,
                primary,
            ));
            x += w + s(6);
        }
        let buttons_height = y + s(26);
        let height =
            s(10) + title_height + lines.len() as i32 * small + s(10) + buttons_height + s(10);
        PermissionLayout {
            title,
            lines,
            buttons,
            height,
        }
    }

    // An answer's steps (one 22 px line each) and permission card.
    fn ai_project_steps_height(&self, hdc: HDC, entry: &ChatEntry, width: i32) -> i32 {
        let s = |value: i32| self.scale(value);
        let mut height = entry.steps.len() as i32 * s(22);
        if !entry.steps.is_empty() {
            height += s(8);
        }
        if entry.asking {
            height += self.ai_permission_layout(hdc, width).height + s(10);
        }
        height
    }

    // Paints an answer's steps and permission card from `top`; returns
    // where the answer's text starts.
    fn paint_ai_project_steps(
        &self,
        hdc: HDC,
        entry: &ChatEntry,
        bounds: RECT,
        top: i32,
        clip: RECT,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        let mut y = top;
        let row = s(22);
        unsafe { SelectObject(hdc, self.small_font) };
        let text_height = self.text_height(hdc);
        for step in &entry.steps {
            let rect = RECT {
                left: bounds.left,
                top: y,
                right: bounds.right,
                bottom: y + row,
            };
            Self::fill(
                hdc,
                RECT {
                    left: rect.left,
                    top: rect.top + s(3),
                    right: rect.left + s(2),
                    bottom: rect.bottom - s(3),
                },
                if step.error {
                    self.theme.warning
                } else {
                    self.theme.edge
                },
            );
            let color = if step.error {
                blend(self.theme.warning, self.theme.text, 0.3)
            } else if step.opened.is_some() {
                blend(self.theme.blue, self.theme.text, 0.35)
            } else {
                self.theme.muted
            };
            self.label_ellipsis(
                hdc,
                &step.summary,
                rect.left + s(11),
                (rect.top + rect.bottom - text_height) / 2,
                color,
                RECT {
                    left: rect.left,
                    right: rect.right,
                    ..clip
                },
            );
            if let Some((path, line)) = &step.opened
                && rect.bottom > clip.top
                && rect.top < clip.bottom
            {
                self.ai
                    .hits
                    .borrow_mut()
                    .steps
                    .push((rect, path.clone(), *line));
            }
            y += row;
        }
        if !entry.steps.is_empty() {
            y += s(8);
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        if entry.asking {
            let width = bounds.right - bounds.left;
            let PermissionLayout {
                title,
                lines,
                buttons,
                height,
            } = self.ai_permission_layout(hdc, width);
            let card = RECT {
                left: bounds.left,
                top: y,
                right: bounds.right,
                bottom: y + height,
            };
            self.panel_card(
                hdc,
                card,
                s(8),
                blend(self.theme.sidebar_bg, self.theme.blue, 0.4),
                blend(self.theme.sidebar_bg, self.theme.blue, 0.06),
            );
            let inner_left = card.left + s(12);
            let mut ty = card.top + s(10);
            let text_clip = RECT {
                left: card.left,
                right: card.right - s(12),
                ..clip
            };
            Self::label(hdc, &title, inner_left, ty, self.theme.text, text_clip);
            ty += self.text_height(hdc) + s(4);
            unsafe { SelectObject(hdc, self.small_font) };
            let small = self.text_height(hdc) + s(3);
            for line in &lines {
                Self::label(hdc, line, inner_left, ty, self.theme.muted, text_clip);
                ty += small;
            }
            unsafe { SelectObject(hdc, self.ui_font) };
            let button_top = ty + s(10);
            let mut hits = Vec::new();
            for (place, label, answer, primary) in buttons {
                let button = RECT {
                    left: inner_left + place.left,
                    top: button_top + place.top,
                    right: inner_left + place.right,
                    bottom: button_top + place.bottom,
                };
                if primary {
                    Self::rounded_fill(hdc, button, s(4), self.theme.violet);
                } else {
                    self.panel_card(hdc, button, s(4), self.theme.edge, self.theme.editor_bg);
                }
                let color = if primary {
                    ui(255, 255, 255)
                } else {
                    self.theme.text
                };
                self.label_mid(
                    hdc,
                    label,
                    button.left + s(10),
                    (button.top + button.bottom) / 2,
                    color,
                    button,
                );
                hits.push((button, answer));
            }
            self.ai.hits.borrow_mut().permission = hits;
            y = card.bottom + s(10);
        }
        y
    }

    // Paints entry `index` of the conversation within `bounds`.
    fn paint_ai_entry(&self, hdc: HDC, index: usize, entry: &ChatEntry, bounds: RECT, clip: RECT) {
        let s = |value: i32| self.scale(value);
        let last = index + 1 == self.ai.entries.len();
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        if entry.role == Role::User {
            self.panel_card(hdc, bounds, s(9), ui(42, 77, 133), ui(17, 35, 68));
            let inner = RECT {
                left: bounds.left + s(12),
                right: bounds.right - s(12),
                ..clip
            };
            let mut y = bounds.top + s(10);
            for line in self.wrap_text(hdc, &entry.text, inner.right - inner.left) {
                Self::label(hdc, &line, inner.left, y, self.theme.text, inner);
                y += line_height;
            }
            if let Some(context) = &entry.context {
                self.label_ellipsis(
                    hdc,
                    &format!("Includes {context}"),
                    inner.left,
                    y + s(2),
                    self.theme.muted,
                    inner,
                );
                y += line_height + s(2);
            }
            if !entry.chips.is_empty() {
                let specs = chip_specs(&entry.chips);
                let (spans, _) = self.ai_chip_spans(hdc, &specs, inner.right - inner.left, false);
                let top = y + s(6);
                for (chip, span) in specs.iter().zip(&spans) {
                    let rect = RECT {
                        left: inner.left + span.left,
                        top: top + span.top,
                        right: inner.left + span.right,
                        bottom: top + span.bottom,
                    };
                    self.paint_ai_chip(hdc, rect, chip, false);
                }
                unsafe { SelectObject(hdc, self.ui_font) };
            }
            return;
        }

        self.sparkle_glyph(
            hdc,
            bounds.left,
            bounds.top + s(3),
            s(14),
            ui(148, 102, 255),
        );
        self.label_ellipsis(
            hdc,
            self.settings.ai_model.as_deref().unwrap_or("Assistant"),
            bounds.left + s(22),
            bounds.top,
            self.theme.muted,
            RECT {
                left: bounds.left,
                right: bounds.right,
                ..clip
            },
        );
        let mut y = bounds.top + s(26);
        y = self.paint_ai_project_steps(hdc, entry, bounds, y, clip);
        let (answer, thinking) = ai::visible_answer(&entry.text);
        let visible = |rect: &RECT| rect.bottom > clip.top && rect.top < clip.bottom;
        if !answer.is_empty() {
            let height = self.snippet_height(
                hdc,
                &entry.view,
                &self.ai.fonts,
                (answer, entry.revision),
                bounds.right - bounds.left,
            );
            self.paint_snippet(hdc, &entry.view, &self.ai.fonts, (bounds.left, y), clip);
            if !self.ai_receiving(last) {
                self.paint_ai_code_toolbars(hdc, index, entry, (bounds.left, y), clip);
            }
            y += height;
        } else if (thinking || self.ai_receiving(last)) && !entry.asking {
            Self::label(
                hdc,
                "Thinking\u{2026}",
                bounds.left,
                y,
                self.theme.muted,
                clip,
            );
            y += line_height;
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        if let Some(error) = &entry.error {
            let color = if error == "Stopped" {
                self.theme.muted
            } else {
                self.theme.error
            };
            y = self.paint_wrapped(
                hdc,
                error,
                RECT {
                    left: bounds.left,
                    right: bounds.right,
                    ..clip
                },
                y + s(6),
                line_height,
                color,
            );
        }
        if let Some(note) = &entry.note {
            y = self.paint_wrapped(
                hdc,
                note,
                RECT {
                    left: bounds.left,
                    right: bounds.right,
                    ..clip
                },
                y + s(6),
                line_height,
                self.theme.muted,
            );
        }
        if !answer.is_empty() && !self.ai_receiving(last) {
            let label = "Copy answer";
            let button = RECT {
                left: bounds.left,
                top: y + s(6),
                right: bounds.left + self.text_width(hdc, label) + s(4),
                bottom: y + s(6) + line_height,
            };
            if visible(&button) {
                Self::label(hdc, label, button.left, button.top, self.theme.muted, clip);
                self.ai
                    .hits
                    .borrow_mut()
                    .actions
                    .push((button, AiAction::Copy(answer.to_string())));
            }
        }
    }

    // The strip at the top of each code block in answer `index`: its
    // language, and buttons that put the code into the editor (Insert at the
    // cursor, Replace what the question was about) or copy it.
    fn paint_ai_code_toolbars(
        &self,
        hdc: HDC,
        index: usize,
        entry: &ChatEntry,
        (left, top): (i32, i32),
        clip: RECT,
    ) {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let can_insert = self.ai_can_insert();
        let can_replace = self.ai_replace_target(index).is_some();
        for block in entry.view.code_blocks() {
            let strip = RECT {
                left: left + block.rect.left,
                top: top + block.rect.top,
                right: left + block.rect.right,
                bottom: top + block.rect.top + s(SNIPPET_CODE_HEADER),
            };
            if strip.bottom <= clip.top || strip.top >= clip.bottom {
                continue;
            }
            Self::fill(
                hdc,
                RECT {
                    top: strip.bottom - s(1).max(1),
                    ..strip
                },
                self.theme.edge,
            );
            let mut right = strip.right - s(6);
            let mut buttons = vec![("Copy", AiAction::Copy(block.text.clone()))];
            if can_replace {
                buttons.push(("Replace", AiAction::Replace(block.text.clone(), index)));
            }
            if can_insert {
                buttons.push(("Insert", AiAction::Insert(block.text.clone())));
            }
            if can_replace || can_insert {
                buttons.push(("Apply", AiAction::Apply(block.text.clone(), index)));
            }
            for (label, action) in buttons {
                let button = RECT {
                    left: right - self.text_width(hdc, label) - s(16),
                    top: strip.top + s(4),
                    right,
                    bottom: strip.bottom - s(5),
                };
                if button.left < strip.left + s(8) {
                    break;
                }
                Self::rounded_fill(hdc, button, s(4), self.theme.sidebar_bg);
                self.label_mid(
                    hdc,
                    label,
                    button.left + s(8),
                    (button.top + button.bottom) / 2,
                    self.theme.text,
                    button,
                );
                self.ai.hits.borrow_mut().actions.push((button, action));
                right = button.left - s(6);
            }
            if !block.language.is_empty() {
                self.label_mid(
                    hdc,
                    &block.language,
                    strip.left + s(12),
                    (strip.top + strip.bottom) / 2 - s(1),
                    self.theme.muted,
                    RECT {
                        right: right - s(4),
                        ..strip
                    },
                );
            }
        }
    }

    // The message box: `lines` of typed text (already wrapped), and the Send
    // button, which is Stop while an answer arrives. Disabled until a model
    // is chosen.
    fn paint_ai_composer(&self, hdc: HDC, input: RECT, lines: &[String], enabled: bool) {
        let s = |value: i32| self.scale(value);
        if input.bottom - input.top < s(30) {
            return;
        }
        let focused = enabled && self.ai_typing();
        let edge = if focused {
            self.theme.violet
        } else {
            self.theme.edge
        };
        self.panel_card(hdc, input, s(9), edge, self.theme.editor_bg);
        let send = RECT {
            left: input.right - s(42),
            top: input.bottom - s(43),
            right: input.right - s(7),
            bottom: input.bottom - s(8),
        };
        let text = RECT {
            left: input.left + s(14),
            top: input.top + s(14),
            right: send.left - s(8),
            bottom: input.bottom - s(10),
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        if !enabled || self.ai.input.is_empty() {
            let placeholder = if enabled {
                "Ask about your code\u{2026} (@ to attach files)".to_string()
            } else {
                "Connect a model to start chatting".to_string()
            };
            self.label_ellipsis(
                hdc,
                &placeholder,
                text.left,
                text.top,
                self.theme.muted,
                text,
            );
        } else {
            let mut y = text.top;
            for line in lines {
                Self::label(hdc, line, text.left, y, self.theme.text, text);
                y += line_height;
            }
        }
        if focused {
            let (x, y) = match lines.last() {
                Some(line) if !self.ai.input.is_empty() => (
                    text.left + self.text_width(hdc, line),
                    text.top + (lines.len() as i32 - 1) * line_height,
                ),
                _ => (text.left, text.top),
            };
            Self::fill(
                hdc,
                RECT {
                    left: x.min(text.right),
                    top: y,
                    right: x.min(text.right) + s(1).max(1),
                    bottom: y + self.text_height(hdc),
                },
                self.theme.text,
            );
        }

        let busy = enabled && self.ai.busy();
        let ready = enabled && !self.ai.input.trim().is_empty();
        let fill = if ready && !busy {
            self.theme.violet
        } else {
            self.theme.active_bg
        };
        Self::rounded_fill(hdc, send, s(9), fill);
        unsafe {
            if busy {
                // Stop: a square.
                let half = s(5);
                let (cx, cy) = ((send.left + send.right) / 2, (send.top + send.bottom) / 2);
                Self::fill(
                    hdc,
                    RECT {
                        left: cx - half,
                        top: cy - half,
                        right: cx + half,
                        bottom: cy + half,
                    },
                    self.theme.text,
                );
            } else {
                let color = if ready {
                    label_on(fill, 255, 255, 255)
                } else {
                    self.theme.muted
                };
                let brush = CreateSolidBrush(color);
                let previous_brush = SelectObject(hdc, brush);
                let previous_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                let middle = (send.top + send.bottom) / 2;
                let points = [
                    POINT {
                        x: send.left + s(10),
                        y: send.top + s(8),
                    },
                    POINT {
                        x: send.right - s(8),
                        y: middle,
                    },
                    POINT {
                        x: send.left + s(10),
                        y: send.bottom - s(8),
                    },
                    POINT {
                        x: send.left + s(14),
                        y: middle,
                    },
                ];
                Polygon(hdc, points.as_ptr(), points.len() as i32);
                SelectObject(hdc, previous_pen);
                SelectObject(hdc, previous_brush);
                DeleteObject(brush);
            }
        }
        if enabled {
            let mut hits = self.ai.hits.borrow_mut();
            hits.composer = Some(input);
            hits.send = Some(send);
        }
    }

    // Draws `text` word-wrapped to `area`'s width from `y`, stopping at its
    // bottom; returns the y below the last line.
    /// Where each chip goes, relative to the top left of a box `width`
    /// wide: left to right, wrapping to a new row, each at most the full
    /// width. Also the total height. `closable` chips leave room for a ✕.
    fn ai_chip_spans(
        &self,
        hdc: HDC,
        chips: &[ChipSpec],
        width: i32,
        closable: bool,
    ) -> (Vec<RECT>, i32) {
        let s = |value: i32| self.scale(value);
        let (height, gap) = (s(22), s(5));
        unsafe { SelectObject(hdc, self.small_font) };
        let mut spans = Vec::with_capacity(chips.len());
        let (mut x, mut y) = (0, 0);
        for chip in chips {
            let close = if closable { s(17) } else { 0 };
            let wanted = s(27) + self.text_width(hdc, &chip.label) + close + s(8);
            let w = wanted.min(width);
            if x > 0 && x + w > width {
                x = 0;
                y += height + gap;
            }
            spans.push(RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + height,
            });
            x += w + gap;
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        let total = if chips.is_empty() { 0 } else { y + height };
        (spans, total)
    }

    /// One file chip; with `closable`, also its ✕, whose rectangle is
    /// returned for clicks.
    fn paint_ai_chip(&self, hdc: HDC, rect: RECT, chip: &ChipSpec, closable: bool) -> Option<RECT> {
        let s = |value: i32| self.scale(value);
        let back = self.theme.sidebar_bg;
        let accent = if chip.warn {
            self.theme.warning
        } else {
            self.theme.blue
        };
        self.panel_card(
            hdc,
            rect,
            s(11),
            blend(back, accent, 0.45),
            blend(back, accent, 0.1),
        );
        let icon = s(13);
        let middle = (rect.top + rect.bottom) / 2;
        let icon_left = rect.left + s(7);
        let use_theme = self.has_extension("material-icon-theme");
        if !self.icons.draw_for_path(
            hdc,
            &chip.path,
            false,
            false,
            use_theme,
            icon_left,
            middle - icon / 2,
            icon,
        ) {
            Self::rounded_fill(
                hdc,
                RECT {
                    left: icon_left,
                    top: middle - icon / 2,
                    right: icon_left + icon,
                    bottom: middle + icon / 2,
                },
                s(3),
                accent,
            );
        }
        let close = closable.then(|| RECT {
            left: rect.right - s(20),
            top: rect.top,
            right: rect.right - s(3),
            bottom: rect.bottom,
        });
        let text_right = close.map_or(rect.right - s(8), |close| close.left);
        unsafe { SelectObject(hdc, self.small_font) };
        let text = if chip.warn {
            blend(self.theme.warning, self.theme.text, 0.3)
        } else {
            blend(self.theme.muted, self.theme.text, 0.55)
        };
        self.label_ellipsis(
            hdc,
            &chip.label,
            icon_left + icon + s(6),
            middle - self.text_height(hdc) / 2,
            text,
            RECT {
                right: text_right,
                ..rect
            },
        );
        if let Some(close) = close {
            self.glyph_in(hdc, DebugGlyph::Close, self.theme.muted, close, s(10));
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        close
    }

    /// The @ list: matching project files above the message box, over the
    /// conversation, never above `top_limit`.
    fn paint_ai_mentions(&self, hdc: HDC, composer: RECT, above: i32, top_limit: i32) {
        let s = |value: i32| self.scale(value);
        let matches = self.ai_mention_matches();
        let row = s(26);
        let footer = s(24);
        let rows = matches.len().max(1) as i32;
        let bottom = above.min(composer.top - s(6));
        let top = (bottom - rows * row - footer - s(8)).max(top_limit + s(4));
        let card = RECT {
            left: composer.left,
            top,
            right: composer.right,
            bottom,
        };
        self.panel_card(
            hdc,
            card,
            s(6),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.4),
            self.theme.editor_bg,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let use_theme = self.has_extension("material-icon-theme");
        let mut hits = Vec::new();
        let mut y = card.top + s(4);
        if matches.is_empty() {
            let note = if self.quick_loading {
                "Reading the project's files\u{2026}"
            } else if self.workspace_root.is_none() {
                "Open a folder to attach its files"
            } else {
                "No file matches"
            };
            self.label_mid(
                hdc,
                note,
                card.left + s(10),
                y + row / 2,
                self.theme.muted,
                card,
            );
            y += row;
        }
        for (index, path) in matches.iter().enumerate() {
            let rect = RECT {
                left: card.left + s(1),
                top: y,
                right: card.right - s(1),
                bottom: y + row,
            };
            if index == self.ai.mention_selected {
                Self::fill(hdc, rect, self.theme.select_bg);
            }
            let icon = s(15);
            let middle = (rect.top + rect.bottom) / 2;
            self.icons.draw_for_path(
                hdc,
                path,
                false,
                false,
                use_theme,
                rect.left + s(9),
                middle - icon / 2,
                icon,
            );
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = self.ai_file_name(path);
            let folder = folder
                .rsplit_once('/')
                .map(|(folder, _)| folder.to_string())
                .unwrap_or_default();
            let name_left = rect.left + s(32);
            self.label_mid(hdc, &name, name_left, middle, self.theme.text, rect);
            if !folder.is_empty() {
                unsafe { SelectObject(hdc, self.small_font) };
                let name_right = name_left
                    + {
                        unsafe { SelectObject(hdc, self.ui_font) };
                        let width = self.text_width(hdc, &name);
                        unsafe { SelectObject(hdc, self.small_font) };
                        width
                    }
                    + s(12);
                let width = self.text_width(hdc, &folder);
                let x = (rect.right - s(10) - width).max(name_right);
                self.label_ellipsis(
                    hdc,
                    &folder,
                    x,
                    middle - self.text_height(hdc) / 2,
                    self.theme.muted,
                    RECT {
                        right: rect.right - s(10),
                        ..rect
                    },
                );
                unsafe { SelectObject(hdc, self.ui_font) };
            }
            hits.push((rect, path.clone()));
            y += row;
        }
        unsafe { SelectObject(hdc, self.small_font) };
        let foot = RECT {
            top: card.bottom - footer,
            ..card
        };
        Self::fill(
            hdc,
            RECT {
                left: card.left + s(1),
                top: foot.top,
                right: card.right - s(1),
                bottom: foot.top + 1,
            },
            self.theme.edge,
        );
        self.label_mid(
            hdc,
            "Enter attaches \u{00b7} files .gitignore hides are left out \u{00b7} Esc",
            card.left + s(10),
            (foot.top + foot.bottom) / 2,
            self.theme.muted,
            foot,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        self.ai.hits.borrow_mut().mentions = hits;
    }

    pub(in crate::windows_app) fn paint_wrapped(
        &self,
        hdc: HDC,
        text: &str,
        area: RECT,
        mut y: i32,
        line_height: i32,
        color: u32,
    ) -> i32 {
        for line in self.wrap_text(hdc, text, area.right - area.left) {
            if y + line_height > area.bottom {
                break;
            }
            Self::label(hdc, &line, area.left, y, color, area);
            y += line_height;
        }
        y
    }

    // `text` wrapped to `width` pixels in the font selected into `hdc`:
    // at spaces where possible, inside a word only when it can't fit on a
    // line of its own. Line breaks in the text are kept.
    fn wrap_text(&self, hdc: HDC, text: &str, width: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.text_width(hdc, &candidate) <= width {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                // A word longer than the line is split between characters.
                for ch in word.chars() {
                    line.push(ch);
                    if line.chars().count() > 1 && self.text_width(hdc, &line) > width {
                        line.pop();
                        lines.push(std::mem::replace(&mut line, ch.to_string()));
                    }
                }
            }
            lines.push(line);
        }
        lines
    }

    pub(in crate::windows_app) fn sparkle_glyph(
        &self,
        hdc: HDC,
        x: i32,
        y: i32,
        size: i32,
        color: u32,
    ) {
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            let brush = CreateSolidBrush(color);
            let prev_pen = SelectObject(hdc, pen);
            let prev_brush = SelectObject(hdc, brush);

            let s = size;
            let cx = x + s / 2;
            let cy = y + s / 2;
            let points = [
                POINT { x: cx, y },
                POINT {
                    x: cx + s / 6,
                    y: cy - s / 6,
                },
                POINT { x: x + s, y: cy },
                POINT {
                    x: cx + s / 6,
                    y: cy + s / 6,
                },
                POINT { x: cx, y: y + s },
                POINT {
                    x: cx - s / 6,
                    y: cy + s / 6,
                },
                POINT { x, y: cy },
                POINT {
                    x: cx - s / 6,
                    y: cy - s / 6,
                },
            ];
            Polygon(hdc, points.as_ptr(), points.len() as i32);

            SelectObject(hdc, prev_brush);
            SelectObject(hdc, prev_pen);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }
}
