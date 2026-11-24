//! The rendered Markdown preview: a read-only tab that lays out the blocks
//! from `lightline::markdown` and paints them with GDI, the way the editor
//! paints code. There is no browser engine, so nothing in a file can run.

use super::*;
use lightline::image_cache;
use lightline::markdown::{self, Align, Block, BlockKind, Inline, LinkTarget, Marker, Style};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

// WM_TIMER id that repaints (and so re-parses) once typing pauses.
pub(super) const MARKDOWN_TIMER: usize = 10;
const REFRESH_DELAY: Duration = Duration::from_millis(150);
// Images larger than this on disk are described, not loaded.
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
// Web images one preview may download; any more are described instead.
const MAX_WEB_IMAGES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Download {
    Running,
    Done,
    Failed,
}

// Web image downloads, shared with the threads doing them. Each address is
// fetched at most once per preview; a finished one bumps `generation` so
// the next paint lays the document out again with the image in place.
#[derive(Default)]
struct Downloads {
    state: Mutex<HashMap<String, Download>>,
    generation: AtomicU64,
}

pub(super) struct MarkdownPreview {
    /// The Markdown file shown. Its open tab (if any) is the live source, so
    /// unsaved edits appear too; the preview never changes that document.
    pub(super) source: PathBuf,
    scroll: Cell<i32>,
    state: RefCell<ViewState>,
    downloads: Arc<Downloads>,
}

#[derive(Default)]
struct ViewState {
    blocks: Vec<Block>,
    // What `blocks` came from: the source tab's change serial, line count
    // and length; None until the first parse.
    parsed: Option<(u64, usize, usize)>,
    parsed_at: Option<Instant>,
    layout: Option<Layout>,
    fonts: Option<Fonts>,
    // Loaded once per local path ("file:...") or web address; None records
    // one that couldn't be decoded.
    images: HashMap<String, Option<image_view::ImageAsset>>,
}

impl MarkdownPreview {
    pub(super) fn new(source: PathBuf) -> Self {
        Self {
            source,
            scroll: Cell::new(0),
            state: RefCell::new(ViewState::default()),
            downloads: Arc::default(),
        }
    }

    /// Lays the document out again, e.g. once web images were allowed.
    pub(super) fn relayout(&self) {
        self.state.borrow_mut().layout = None;
    }

    /// Forgets the parsed text, e.g. after the file was reloaded from disk.
    pub(super) fn invalidate(&self) {
        let mut state = self.state.borrow_mut();
        state.parsed = None;
        state.layout = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Font {
    Body,
    Bold,
    Italic,
    BoldItalic,
    Code,
    H1,
    H2,
    H3,
    H4,
}

// Size in 96-dpi pixels, weight, italic, monospace.
const FONT_SPECS: [(Font, i32, i32, bool, bool); 9] = [
    (Font::Body, 15, 400, false, false),
    (Font::Bold, 15, 700, false, false),
    (Font::Italic, 15, 400, true, false),
    (Font::BoldItalic, 15, 700, true, false),
    (Font::Code, 14, 400, false, true),
    (Font::H1, 28, 600, false, false),
    (Font::H2, 22, 600, false, false),
    (Font::H3, 18, 600, false, false),
    (Font::H4, 16, 700, false, false),
];

// The same fonts in the AI chat, sized to the panel's 14 px text: body as
// the UI font, code a pixel smaller, and headings that stay modest.
const CHAT_FONT_SIZES: [i32; 9] = [14, 14, 14, 14, 13, 18, 16, 15, 14];

struct Fonts {
    scale: (u32, i32),
    handles: [HFONT; 9],
    heights: [i32; 9],
}

impl Fonts {
    // `chat`: the chat's smaller sizes (CHAT_FONT_SIZES) rather than the
    // preview's.
    fn new(dpi: u32, zoom: i32, chat: bool) -> Self {
        let mut handles: [HFONT; 9] = [null_mut(); 9];
        let mut heights = [0; 9];
        let code_family = wide(App::code_font_family());
        let ui_family = wide("Segoe UI");
        unsafe {
            let screen = GetDC(null_mut());
            for (index, (_, size, weight, italic, mono)) in FONT_SPECS.iter().enumerate() {
                let size = if chat { &CHAT_FONT_SIZES[index] } else { size };
                let family = if *mono { &code_family } else { &ui_family };
                handles[index] = CreateFontW(
                    -scaled(*size, dpi, zoom),
                    0,
                    0,
                    0,
                    *weight,
                    u32::from(*italic),
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    family.as_ptr(),
                );
                let old = SelectObject(screen, handles[index]);
                let mut metrics: TEXTMETRICW = zeroed();
                GetTextMetricsW(screen, &mut metrics);
                heights[index] = metrics.tmHeight.max(1);
                SelectObject(screen, old);
            }
            ReleaseDC(null_mut(), screen);
        }
        Self {
            scale: (dpi, zoom),
            handles,
            heights,
        }
    }

    fn index(font: Font) -> usize {
        FONT_SPECS
            .iter()
            .position(|spec| spec.0 == font)
            .unwrap_or(0)
    }

    fn handle(&self, font: Font) -> HFONT {
        self.handles[Self::index(font)]
    }

    fn height(&self, font: Font) -> i32 {
        self.heights[Self::index(font)]
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for handle in self.handles {
            if !handle.is_null() {
                unsafe { DeleteObject(handle) };
            }
        }
    }
}

// Colors are picked by role at paint time, so a theme change applies
// without laying the document out again.
#[derive(Clone, Copy)]
enum Tone {
    Text,
    Muted,
    Link,
    CodeBg,
    Edge,
    QuoteBar,
    TableHead,
    // Code in a fenced block, colored like the editor.
    Syntax(Color),
}

/// A fenced code block as laid out: its box (header strip included),
/// language tag and text.
#[derive(Clone)]
pub(super) struct CodeBlock {
    pub(super) rect: RECT,
    pub(super) language: String,
    pub(super) text: String,
}

enum Item {
    Text {
        x: i32,
        y: i32,
        width: i32,
        font: Font,
        tone: Tone,
        text: Vec<u16>,
        clip_right: i32,
        underline: bool,
        strike: bool,
    },
    Fill {
        rect: RECT,
        tone: Tone,
    },
    Image {
        rect: RECT,
        key: String,
    },
    Check {
        rect: RECT,
        checked: bool,
    },
}

// What a layout depends on besides the text: content width, dpi, zoom,
// finished downloads, and whether web images may load.
type LayoutKey = (i32, u32, i32, u64, bool);

struct Layout {
    key: LayoutKey,
    items: Vec<Item>,
    links: Vec<(RECT, String)>,
    anchors: Vec<(String, i32)>,
    // Each fenced code block (the chat's Copy, Insert and Replace buttons).
    code_blocks: Vec<CodeBlock>,
    height: i32,
    // Web images not shown because loading them hasn't been allowed.
    blocked: usize,
}

// One styled piece of a line being built.
struct Run {
    x: i32,
    width: i32,
    text: String,
    font: Font,
    tone: Tone,
    link: Option<String>,
    strike: bool,
    code: bool,
    // An inline image: its key in ViewState::images and its height.
    image: Option<(String, i32)>,
}

// Splits text into words that keep their trailing spaces, with each "\n"
// (a hard line break) as its own piece.
fn pieces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            if start < index {
                out.push(&text[start..index]);
            }
            out.push("\n");
            start = index + 1;
            in_space = false;
        } else if ch.is_whitespace() {
            in_space = true;
        } else if in_space {
            out.push(&text[start..index]);
            start = index;
            in_space = false;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

// How far a code line's continuation rows are indented, in columns: two
// past the line's own indent (a tab is four), at most half the box.
fn code_continuation_indent(line: &str, columns: usize) -> usize {
    let lead: usize = line
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum();
    (lead + 2).min(columns / 2)
}

// Where a code line breaks to fit `columns` characters (a tab is four):
// the byte offset each row starts at. Rows after the first have `indent`
// columns fewer; a break prefers the space before a word.
fn code_breaks(line: &str, columns: usize, indent: usize) -> Vec<usize> {
    let columns_of =
        |text: &str| -> usize { text.chars().map(|ch| if ch == '\t' { 4 } else { 1 }).sum() };
    let later_room = columns.saturating_sub(indent).max(4);
    let mut breaks = vec![0];
    let mut width = 0;
    let mut last_space = None;
    for (index, ch) in line.char_indices() {
        let room = if breaks.len() == 1 {
            columns
        } else {
            later_room
        };
        let step = if ch == '\t' { 4 } else { 1 };
        if width + step > room && index > *breaks.last().unwrap_or(&0) {
            // The word carried down must fit the next row with this
            // character; otherwise the row is cut here.
            let start = match last_space {
                Some(space)
                    if space > *breaks.last().unwrap_or(&0)
                        && columns_of(&line[space..index]) + step <= later_room =>
                {
                    space
                }
                _ => index,
            };
            breaks.push(start);
            width = columns_of(&line[start..index]);
            last_space = None;
        }
        width += step;
        if ch == ' ' {
            last_space = Some(index + 1);
        }
    }
    breaks
}

fn font_for(base: Font, style: Style) -> Font {
    if style.code {
        return Font::Code;
    }
    match base {
        Font::Body | Font::Bold | Font::Italic | Font::BoldItalic => {
            let bold = style.bold || matches!(base, Font::Bold | Font::BoldItalic);
            let italic = style.italic || matches!(base, Font::Italic | Font::BoldItalic);
            match (bold, italic) {
                (true, true) => Font::BoldItalic,
                (true, false) => Font::Bold,
                (false, true) => Font::Italic,
                (false, false) => Font::Body,
            }
        }
        heading => heading,
    }
}

fn plain_text(inlines: &[Inline]) -> String {
    inlines.iter().map(|inline| inline.text.as_str()).collect()
}

struct Layouter<'a> {
    app: &'a App,
    hdc: HDC,
    fonts: &'a Fonts,
    images: &'a mut HashMap<String, Option<image_view::ImageAsset>>,
    downloads: &'a Arc<Downloads>,
    // False for text that isn't a file (an AI answer): images are only
    // described, never read from disk or downloaded.
    load_images: bool,
    web_allowed: bool,
    blocked: usize,
    document_dir: PathBuf,
    root: PathBuf,
    y: i32,
    items: Vec<Item>,
    links: Vec<(RECT, String)>,
    anchors: Vec<(String, i32)>,
    code_blocks: Vec<CodeBlock>,
    // Extra space at the top of each code block for a toolbar (the chat's);
    // 0 in the preview.
    code_header: i32,
    // The chat's narrow panel: long code lines wrap instead of being cut at
    // the box's edge, and lines of text sit closer than in the preview.
    wrap_code: bool,
}

impl Layouter<'_> {
    fn s(&self, pixels: i32) -> i32 {
        self.app.scale(pixels)
    }

    fn measure(&self, font: Font, text: &str) -> i32 {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            SelectObject(self.hdc, self.fonts.handle(font));
            GetTextExtentPoint32W(self.hdc, utf16.as_ptr(), utf16.len() as i32, &mut size);
        }
        size.cx
    }

    // The longest prefix of `text` (at least one character) that fits in
    // `available`, as a byte index.
    fn fit(&self, font: Font, text: &str, available: i32) -> usize {
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(index, _)| index)
            .skip(1)
            .chain(Some(text.len()))
            .collect();
        let (mut low, mut high) = (0, bounds.len() - 1);
        while low < high {
            let mid = (low + high).div_ceil(2);
            if self.measure(font, &text[..bounds[mid]]) <= available {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        bounds[low]
    }

    fn finish_line(
        &mut self,
        line: &mut Vec<Run>,
        left: i32,
        width: i32,
        align: Align,
        base: Font,
    ) {
        let height = line
            .iter()
            .map(|run| match &run.image {
                Some((_, height)) => *height,
                None => self.fonts.height(run.font),
            })
            .max()
            .unwrap_or_else(|| self.fonts.height(base));
        let used = line.last().map_or(0, |run| run.x + run.width);
        let shift = match align {
            Align::Left => 0,
            Align::Center => (width - used) / 2,
            Align::Right => width - used,
        }
        .max(0);
        for run in line.drain(..) {
            let x = left + shift + run.x;
            if let Some((key, image_height)) = run.image {
                let rect = RECT {
                    left: x,
                    top: self.y + height - image_height,
                    right: x + run.width,
                    bottom: self.y + height,
                };
                if let Some(url) = run.link {
                    self.links.push((rect, url));
                }
                self.items.push(Item::Image { rect, key });
                continue;
            }
            let font_height = self.fonts.height(run.font);
            let y = self.y + height - font_height;
            if run.code {
                self.items.push(Item::Fill {
                    rect: RECT {
                        left: x - self.s(3),
                        top: y,
                        right: x + run.width + self.s(3),
                        bottom: y + font_height,
                    },
                    tone: Tone::CodeBg,
                });
            }
            if let Some(url) = &run.link {
                self.links.push((
                    RECT {
                        left: x,
                        top: y,
                        right: x + run.width,
                        bottom: y + font_height,
                    },
                    url.clone(),
                ));
            }
            self.items.push(Item::Text {
                x,
                y,
                width: run.width,
                font: run.font,
                tone: run.tone,
                text: run.text.encode_utf16().collect(),
                clip_right: left + width.max(run.x + run.width + shift),
                underline: run.link.is_some(),
                strike: run.strike,
            });
        }
        // The chat sets its lines closer than the preview does.
        self.y += height + self.s(if self.wrap_code { 2 } else { 5 });
    }

    fn place(&mut self, line: &mut Vec<Run>, x: &mut i32, text: String, width: i32, run: &Run) {
        match line.last_mut() {
            Some(last)
                if last.font == run.font
                    && matches!(
                        (last.tone, run.tone),
                        (Tone::Link, Tone::Link)
                            | (Tone::Text, Tone::Text)
                            | (Tone::Muted, Tone::Muted)
                    )
                    && last.link == run.link
                    && last.strike == run.strike
                    && last.code == run.code
                    && last.image.is_none()
                    && run.image.is_none() =>
            {
                last.text.push_str(&text);
                last.width += width;
            }
            _ => line.push(Run {
                x: *x,
                width,
                text,
                font: run.font,
                tone: run.tone,
                link: run.link.clone(),
                strike: run.strike,
                code: run.code,
                image: None,
            }),
        }
        *x += width;
    }

    // Lays out styled text wrapped to `width`, starting at `left`. Images in
    // the text flow like words; one that isn't available shows its
    // description instead.
    fn wrap(
        &mut self,
        inlines: &[Inline],
        base: Font,
        tone: Tone,
        left: i32,
        width: i32,
        align: Align,
    ) {
        let width = width.max(self.s(40));
        let mut line: Vec<Run> = Vec::new();
        let mut x = 0;
        for inline in inlines {
            let link_tone = if inline.link.is_some() {
                Tone::Link
            } else {
                tone
            };
            let mut style = Run {
                x: 0,
                width: 0,
                text: String::new(),
                font: font_for(base, inline.style),
                tone: link_tone,
                link: inline.link.clone(),
                strike: inline.style.strike,
                code: inline.style.code,
                image: None,
            };
            let Some(image) = &inline.image else {
                self.flow(
                    &mut line,
                    &mut x,
                    &inline.text,
                    &style,
                    (left, width, align, base),
                );
                continue;
            };
            match self.picture(&image.url, image.width, width) {
                Some((key, image_width, image_height)) => {
                    if x > 0 && x + image_width > width {
                        self.finish_line(&mut line, left, width, align, base);
                        x = 0;
                    }
                    style.x = x;
                    style.width = image_width;
                    style.image = Some((key, image_height));
                    line.push(style);
                    x += image_width;
                }
                None => {
                    let description = inline.text.trim();
                    let label = if description.is_empty() {
                        "[image]".to_string()
                    } else {
                        format!("[{description}]")
                    };
                    style.font = Font::Italic;
                    if inline.link.is_none() {
                        style.tone = Tone::Muted;
                    }
                    self.flow(
                        &mut line,
                        &mut x,
                        &label,
                        &style,
                        (left, width, align, base),
                    );
                }
            }
        }
        if !line.is_empty() {
            self.finish_line(&mut line, left, width, align, base);
        }
    }

    // Adds `text` to the line being built, word by word, starting new lines
    // as needed. `frame` is the wrap's left edge, width, alignment and base
    // font.
    fn flow(
        &mut self,
        line: &mut Vec<Run>,
        x: &mut i32,
        text: &str,
        style: &Run,
        frame: (i32, i32, Align, Font),
    ) {
        let (left, width, align, base) = frame;
        for piece in pieces(text) {
            if piece == "\n" {
                self.finish_line(line, left, width, align, base);
                *x = 0;
                continue;
            }
            let mut piece = if *x == 0 { piece.trim_start() } else { piece }.to_string();
            if piece.is_empty() {
                continue;
            }
            let mut piece_width = self.measure(style.font, &piece);
            if *x > 0 && *x + piece_width > width {
                self.finish_line(line, left, width, align, base);
                *x = 0;
                piece = piece.trim_start().to_string();
                if piece.is_empty() {
                    continue;
                }
                piece_width = self.measure(style.font, &piece);
            }
            // Longer than a whole line (a long URL): break it anywhere.
            while *x + piece_width > width && piece.chars().nth(1).is_some() {
                let at = self.fit(style.font, &piece, width - *x);
                let head = piece[..at].to_string();
                let head_width = self.measure(style.font, &head);
                self.place(line, x, head, head_width, style);
                self.finish_line(line, left, width, align, base);
                *x = 0;
                piece = piece[at..].to_string();
                piece_width = self.measure(style.font, &piece);
            }
            self.place(line, x, piece, piece_width, style);
        }
    }

    // The loaded picture for an image address, sized for display within
    // `max_width`: its key in `images`, width and height. None while a web
    // image is downloading, when loading web images isn't allowed (counted
    // in `blocked`), or when the image can't be read.
    fn picture(
        &mut self,
        url: &str,
        requested: Option<u32>,
        max_width: i32,
    ) -> Option<(String, i32, i32)> {
        if !self.load_images {
            return None;
        }
        let (dpi, zoom) = (self.app.dpi, self.app.zoom);
        let density = dpi as f32 * zoom as f32 / 9600.0;
        let key = match markdown::resolve_link(url, &self.document_dir, &self.root) {
            LinkTarget::File(path) => {
                let key = format!("file:{}", path.display());
                if !self.images.contains_key(&key) {
                    let loaded = std::fs::metadata(&path)
                        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_IMAGE_BYTES)
                        .then(|| image_view::load_picture(&path, density))
                        .flatten();
                    self.images.insert(key.clone(), loaded);
                }
                key
            }
            LinkTarget::Web(address) if address.to_ascii_lowercase().starts_with("http") => {
                if !self.images.contains_key(&address) {
                    // A cached copy needs no network, so it always shows.
                    if let Some(path) = image_cache::cached(&address) {
                        let loaded = image_view::load_picture(&path, density);
                        self.images.insert(address.clone(), loaded);
                    } else if self.web_allowed {
                        self.download(&address);
                        return None;
                    } else {
                        self.blocked += 1;
                        return None;
                    }
                }
                address
            }
            _ => return None,
        };
        let image = self.images.get(&key)?.as_ref()?;
        let logical_width = (image.width as f32 / image.density).round().max(1.0) as i32;
        let logical_height = (image.height as f32 / image.density).round().max(1.0) as i32;
        let mut draw_width = scaled(logical_width, dpi, zoom).max(1);
        let mut draw_height = scaled(logical_height, dpi, zoom).max(1);
        // An HTML width="..." sets the size, keeping the proportions.
        if let Some(requested) = requested.filter(|width| *width > 0) {
            let target = scaled(requested.min(4096) as i32, dpi, zoom).max(1);
            draw_height = (draw_height as i64 * target as i64 / draw_width as i64).max(1) as i32;
            draw_width = target;
        }
        if draw_width > max_width {
            draw_height = (draw_height as i64 * max_width as i64 / draw_width as i64).max(1) as i32;
            draw_width = max_width;
        }
        Some((key, draw_width, draw_height))
    }

    // Starts downloading a web image on its own thread, once per address.
    // When it ends, the window is asked to repaint and so lay out again.
    fn download(&self, address: &str) {
        {
            let mut state = self
                .downloads
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.contains_key(address) || state.len() >= MAX_WEB_IMAGES {
                return;
            }
            state.insert(address.to_string(), Download::Running);
        }
        let downloads = Arc::clone(self.downloads);
        let url = address.to_string();
        let hwnd = self.app.hwnd as isize;
        std::thread::spawn(move || {
            let outcome = match image_cache::download(&url) {
                Ok(_) => Download::Done,
                Err(_) => Download::Failed,
            };
            downloads
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(url, outcome);
            downloads.generation.fetch_add(1, Ordering::Release);
            unsafe { PostMessageW(hwnd as HWND, WM_TIMER, MARKDOWN_TIMER, 0) };
        });
    }

    fn fill(&mut self, left: i32, top: i32, right: i32, bottom: i32, tone: Tone) {
        self.items.push(Item::Fill {
            rect: RECT {
                left,
                top,
                right,
                bottom,
            },
            tone,
        });
    }

    // One line of a code block, as runs of text in their syntax colors
    // (later spans win, as in the editor). Tabs show as four spaces.
    fn code_line(
        &mut self,
        line: &str,
        spans: &[lightline::syntax::Span],
        x: i32,
        y: i32,
        clip_right: i32,
    ) {
        let mut colors: Vec<Option<Color>> = vec![None; line.len()];
        for span in spans {
            let end = span.end.min(line.len());
            for slot in colors.iter_mut().take(end).skip(span.start) {
                *slot = Some(span.color);
            }
        }
        let mut offset = 0;
        let mut start = 0;
        while start < line.len() {
            let color = colors[start];
            let mut end = start + 1;
            while end < line.len() && (colors[end] == color || !line.is_char_boundary(end)) {
                end += 1;
            }
            let piece = line[start..end].replace('\t', "    ");
            let width = self.measure(Font::Code, &piece);
            self.items.push(Item::Text {
                x: x + offset,
                y,
                width,
                font: Font::Code,
                tone: color.map_or(Tone::Text, Tone::Syntax),
                text: piece.encode_utf16().collect(),
                clip_right,
                underline: false,
                strike: false,
            });
            offset += width;
            start = end;
        }
    }

    fn block(&mut self, block: &Block, first: bool, width: i32) {
        let left = self.s(24) * block.indent as i32 + self.s(18) * block.quote as i32;
        let inner = (width - left).max(self.s(60));
        let top = self.y;
        let text_tone = if block.quote > 0 {
            Tone::Muted
        } else {
            Tone::Text
        };
        match &block.kind {
            BlockKind::Heading(level, inlines) => {
                if !first {
                    self.y += self.s(if *level <= 2 { 20 } else { 14 });
                }
                self.anchors
                    .push((markdown::heading_slug(&plain_text(inlines)), self.y));
                let font = match level {
                    1 => Font::H1,
                    2 => Font::H2,
                    3 => Font::H3,
                    _ => Font::H4,
                };
                self.wrap(inlines, font, Tone::Text, left, inner, Align::Left);
                if *level <= 2 {
                    let rule = self.y;
                    self.fill(
                        left,
                        rule,
                        left + inner,
                        rule + self.s(1).max(1),
                        Tone::Edge,
                    );
                    self.y += self.s(12);
                } else {
                    self.y += self.s(6);
                }
            }
            BlockKind::Paragraph(inlines) => {
                self.wrap(inlines, Font::Body, text_tone, left, inner, Align::Left);
                self.y += self.s(9);
            }
            BlockKind::ListItem(marker, inlines) => {
                let line_top = self.y;
                let body = self.fonts.height(Font::Body);
                match marker {
                    Marker::Bullet => {
                        let bullet = if block.indent > 1 {
                            "\u{25e6}"
                        } else {
                            "\u{2022}"
                        };
                        let bullet_width = self.measure(Font::Body, bullet);
                        self.items.push(Item::Text {
                            x: left - self.s(14),
                            y: line_top,
                            width: bullet_width,
                            font: Font::Body,
                            tone: Tone::Muted,
                            text: bullet.encode_utf16().collect(),
                            clip_right: left,
                            underline: false,
                            strike: false,
                        });
                    }
                    Marker::Number(number) => {
                        let label = format!("{number}.");
                        let label_width = self.measure(Font::Body, &label);
                        self.items.push(Item::Text {
                            x: left - self.s(6) - label_width,
                            y: line_top,
                            width: label_width,
                            font: Font::Body,
                            tone: Tone::Muted,
                            text: label.encode_utf16().collect(),
                            clip_right: left,
                            underline: false,
                            strike: false,
                        });
                    }
                    Marker::Task(checked) => {
                        let size = self.s(13);
                        let box_left = left - self.s(19);
                        let box_top = line_top + (body - size) / 2;
                        self.items.push(Item::Check {
                            rect: RECT {
                                left: box_left,
                                top: box_top,
                                right: box_left + size,
                                bottom: box_top + size,
                            },
                            checked: *checked,
                        });
                    }
                }
                self.wrap(inlines, Font::Body, text_tone, left, inner, Align::Left);
                self.y += self.s(3);
            }
            BlockKind::Code { language, text } => {
                let pad = self.s(12);
                let header = self.code_header;
                let line_height = self.fonts.height(Font::Code) + self.s(3);
                let lines: Vec<&str> = text.split('\n').collect();
                // Where each line breaks: byte offsets, one row per piece.
                // Continuation rows are indented past the line's indent.
                let char_width = (self.measure(Font::Code, "0000000000") / 10).max(1);
                let columns = ((inner - 2 * pad) / char_width).max(8) as usize;
                let rows: Vec<(Vec<usize>, usize)> = lines
                    .iter()
                    .map(|line| {
                        if self.wrap_code {
                            let indent = code_continuation_indent(line, columns);
                            (code_breaks(line, columns, indent), indent)
                        } else {
                            (vec![0], 0)
                        }
                    })
                    .collect();
                let row_count: usize = rows.iter().map(|(breaks, _)| breaks.len()).sum();
                let height = header + row_count as i32 * line_height + 2 * pad;
                let box_top = self.y;
                self.fill(left, box_top, left + inner, box_top + height, Tone::CodeBg);
                self.code_blocks.push(CodeBlock {
                    rect: RECT {
                        left,
                        top: box_top,
                        right: left + inner,
                        bottom: box_top + height,
                    },
                    language: language.clone(),
                    text: text.clone(),
                });
                let colors = lightline::syntax::highlight_snippet(language, text);
                let mut row = 0;
                for (index, line) in lines.iter().enumerate() {
                    let spans = colors
                        .as_ref()
                        .and_then(|lines| lines.get(index))
                        .map_or(&[][..], Vec::as_slice);
                    let (breaks, continuation) = &rows[index];
                    for (piece, &start) in breaks.iter().enumerate() {
                        let end = breaks.get(piece + 1).copied().unwrap_or(line.len());
                        let y = box_top + header + pad + row * line_height;
                        let indent = if piece == 0 {
                            0
                        } else {
                            *continuation as i32 * char_width
                        };
                        if piece > 0 {
                            // A small hook just before the row: it goes on
                            // from the one above.
                            let x = left + pad + indent - 2 * char_width + char_width / 2;
                            let mid = y + line_height / 2;
                            let thin = self.s(1).max(1);
                            self.fill(x, mid - line_height / 2, x + thin, mid + thin, Tone::Edge);
                            self.fill(x, mid, x + char_width, mid + thin, Tone::Edge);
                        }
                        let shifted: Vec<lightline::syntax::Span> = spans
                            .iter()
                            .filter(|span| span.end > start && span.start < end)
                            .map(|span| lightline::syntax::Span {
                                start: span.start.max(start) - start,
                                end: span.end.min(end) - start,
                                color: span.color,
                            })
                            .collect();
                        // Without wrapping, long lines are cut at the edge.
                        self.code_line(
                            &line[start..end],
                            &shifted,
                            left + pad + indent,
                            y,
                            left + inner - pad,
                        );
                        row += 1;
                    }
                }
                self.y = box_top + height + self.s(14);
            }
            BlockKind::Rule => {
                self.y += self.s(6);
                let rule = self.y;
                self.fill(
                    left,
                    rule,
                    left + inner,
                    rule + self.s(2).max(1),
                    Tone::Edge,
                );
                self.y += self.s(16);
            }
            BlockKind::Image { url, alt, width } => self.image(url, alt, *width, left, inner),
            BlockKind::Table {
                align,
                header,
                rows,
            } => self.table(align, header, rows, left, inner),
        }
        if block.quote > 0 {
            let bar_left = self.s(24) * block.indent as i32 + self.s(18) * (block.quote as i32 - 1);
            let bottom = (self.y - self.s(6)).max(top);
            self.fill(bar_left, top, bar_left + self.s(3), bottom, Tone::QuoteBar);
        }
    }

    // An image on its own line (an HTML <img>): a line holding just that
    // image, so it loads, sizes and falls back exactly like one in text.
    fn image(&mut self, url: &str, alt: &str, requested: Option<u32>, left: i32, width: i32) {
        let inline = Inline {
            text: alt.to_string(),
            style: Style::default(),
            link: None,
            image: Some(markdown::ImageRef {
                url: url.to_string(),
                width: requested,
            }),
        };
        self.wrap(&[inline], Font::Body, Tone::Text, left, width, Align::Left);
        self.y += self.s(7);
    }

    fn table(
        &mut self,
        align: &[Align],
        header: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        left: i32,
        width: i32,
    ) {
        let columns = rows
            .iter()
            .map(Vec::len)
            .chain([align.len(), header.len()])
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return;
        }
        let pad = self.s(8);
        let minimum = self.s(48);
        let mut widths = vec![minimum; columns];
        for (is_header, row) in
            std::iter::once((true, header)).chain(rows.iter().map(|row| (false, row.as_slice())))
        {
            for (column, cell) in row.iter().enumerate().take(columns) {
                let font = if is_header { Font::Bold } else { Font::Body };
                // Each run in its own font: inline code is wider than text.
                let natural = cell
                    .iter()
                    .map(|inline| {
                        let code_pad = if inline.style.code { self.s(6) } else { 0 };
                        self.measure(font_for(font, inline.style), &inline.text) + code_pad
                    })
                    .sum::<i32>()
                    + 2 * pad
                    + self.s(2);
                widths[column] = widths[column].max(natural);
            }
        }
        let total: i32 = widths.iter().sum();
        if total > width {
            for column_width in &mut widths {
                *column_width =
                    (*column_width as i64 * width as i64 / total as i64).max(minimum as i64) as i32;
            }
        }
        let table_width: i32 = widths.iter().sum();
        let table_top = self.y;
        let line = self.s(1).max(1);
        self.fill(
            left,
            table_top,
            left + table_width,
            table_top + line,
            Tone::Edge,
        );
        let empty: Vec<Inline> = Vec::new();
        for (is_header, row) in
            std::iter::once((true, header)).chain(rows.iter().map(|row| (false, row.as_slice())))
        {
            if is_header && row.is_empty() {
                continue;
            }
            let row_top = self.y;
            let first_item = self.items.len();
            let mut row_bottom = row_top;
            let mut x = left;
            for (column, &column_width) in widths.iter().enumerate() {
                let cell = row.get(column).unwrap_or(&empty);
                self.y = row_top + pad;
                let font = if is_header { Font::Bold } else { Font::Body };
                let cell_align = align.get(column).copied().unwrap_or(Align::Left);
                self.wrap(
                    cell,
                    font,
                    Tone::Text,
                    x + pad,
                    column_width - 2 * pad,
                    cell_align,
                );
                row_bottom = row_bottom.max(self.y);
                x += column_width;
            }
            let row_bottom =
                row_bottom.max(row_top + self.fonts.height(Font::Body) + 2 * pad) + pad / 2;
            if is_header {
                self.items.insert(
                    first_item,
                    Item::Fill {
                        rect: RECT {
                            left,
                            top: row_top,
                            right: left + table_width,
                            bottom: row_bottom,
                        },
                        tone: Tone::TableHead,
                    },
                );
            }
            self.fill(
                left,
                row_bottom,
                left + table_width,
                row_bottom + line,
                Tone::Edge,
            );
            self.y = row_bottom + line;
        }
        // Column lines: the left edge, then the right edge of every column.
        let bottom = self.y;
        let mut x = left;
        self.fill(x, table_top, x + line, bottom, Tone::Edge);
        for column_width in &widths {
            x += column_width;
            self.fill(x, table_top, x + line, bottom, Tone::Edge);
        }
        self.y += self.s(14);
    }
}

impl App {
    // Relative links and images may reach files inside the workspace, or
    // inside the document's own folder when it isn't in the workspace.
    fn markdown_folders(&self, source: &Path) -> (PathBuf, PathBuf) {
        let document_dir = source.parent().unwrap_or(Path::new(".")).to_path_buf();
        let root = self
            .workspace_root
            .clone()
            .filter(|root| source.starts_with(root))
            .unwrap_or_else(|| document_dir.clone());
        (document_dir, root)
    }

    /// Opens the rendered preview of the active Markdown file: as its own
    /// tab, or `beside` the source in a split so edits show up as you type.
    /// The tab index of the active Markdown file's preview when it is shown
    /// in the other pane, beside the file; or of the active tab when it is
    /// that preview, so the file's preview button stays while it has focus.
    pub(super) fn preview_beside(&self) -> Option<usize> {
        if !self.split_visible {
            return None;
        }
        let other = self.pane_tabs[1 - self.focused_pane];
        // The preview itself has the focus, with its file in the other pane.
        if let Some(preview) = &self.tab().markdown {
            let beside = self.tabs.get(other)?.document.path.as_ref() == Some(&preview.source);
            return beside.then_some(self.active);
        }
        let source = self.doc().path.as_ref()?;
        self.tabs
            .get(other)?
            .markdown
            .as_ref()
            .filter(|preview| &preview.source == source)
            .map(|_| other)
    }

    /// The tab bar's preview button: opens the preview beside the file, or
    /// closes it again when it is already there.
    pub(super) fn toggle_markdown_preview(&mut self, hwnd: HWND) {
        match self.preview_beside() {
            Some(index) => self.close_markdown_preview(hwnd, index),
            None => self.open_markdown_preview(hwnd, true),
        }
    }

    // Closes a preview shown beside its file and goes back to one pane
    // showing the file, where it was scrolled to. A preview is never
    // edited, so there is nothing to ask about saving.
    fn close_markdown_preview(&mut self, hwnd: HWND, index: usize) {
        // The file is in the pane the preview isn't, focused or not.
        let source_pane = usize::from(self.pane_tabs[0] == index);
        let mut source = self.pane_tabs[source_pane];
        if source_pane == 1 {
            self.tabs[source].views[0] = self.tabs[source].views[1].clone();
        }
        self.tabs.remove(index);
        if source > index {
            source -= 1;
        }
        self.cancel_transition(hwnd);
        self.split_visible = false;
        self.divider_dragging = false;
        self.focused_pane = 0;
        self.pane_tabs = [source, source];
        self.active = source;
        self.status = "Preview closed".into();
        self.show_active_tab(hwnd);
    }

    pub(super) fn open_markdown_preview(&mut self, hwnd: HWND, beside: bool) {
        let source = self
            .tab()
            .markdown
            .as_ref()
            .map(|preview| preview.source.clone())
            .or_else(|| self.doc().path.clone())
            .filter(|path| markdown::is_markdown_path(path));
        let Some(source) = source else {
            self.status = "Open a Markdown file (.md) to preview it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        if self.web_image_folders.is_none() {
            self.web_image_folders = Some(image_cache::allowed_folders());
        }
        let source_index = self.tabs.iter().position(|tab| {
            tab.markdown.is_none() && tab.document.path.as_deref() == Some(source.as_path())
        });
        let preview_index = match self.tabs.iter().position(|tab| {
            tab.markdown
                .as_ref()
                .is_some_and(|preview| preview.source == source)
        }) {
            Some(index) => index,
            None => {
                self.tabs.push(Tab::new_markdown_preview(source.clone()));
                self.tabs.len() - 1
            }
        };
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let fits_beside = rect.right - self.editor_left() >= self.scale(430);
        match source_index {
            Some(source_index) if beside && fits_beside => {
                self.cancel_transition(hwnd);
                self.split_visible = true;
                self.pane_tabs = [source_index, preview_index];
                self.focused_pane = 0;
                self.active = source_index;
                self.status = "Preview opened beside the file".into();
                self.show_active_tab(hwnd);
            }
            // Already shown in the other pane: go there instead of showing
            // the same preview in both.
            _ if self.split_visible && self.pane_tabs[1 - self.focused_pane] == preview_index => {
                self.focus_pane(hwnd, 1 - self.focused_pane);
            }
            _ => {
                if beside && !fits_beside {
                    self.status = "Widen the window to preview beside the file".into();
                }
                self.activate_tab(hwnd, preview_index);
            }
        }
    }

    // Parses the source again if it changed, at most once per
    // REFRESH_DELAY while typing continues.
    fn refresh_markdown(&self, preview: &MarkdownPreview) {
        let source = self
            .tabs
            .iter()
            .find(|tab| {
                tab.markdown.is_none()
                    && tab.document.path.as_deref() == Some(preview.source.as_path())
            })
            .map(|tab| &tab.document);
        let mut state = preview.state.borrow_mut();
        let text = match source {
            Some(document) => {
                let key = (
                    document.change_serial(),
                    document.line_count(),
                    document.byte_len(),
                );
                if state.parsed == Some(key) {
                    return;
                }
                if state.parsed.is_some()
                    && state
                        .parsed_at
                        .is_some_and(|at| at.elapsed() < REFRESH_DELAY)
                {
                    unsafe {
                        SetTimer(
                            self.hwnd,
                            MARKDOWN_TIMER,
                            REFRESH_DELAY.as_millis() as u32,
                            None,
                        )
                    };
                    return;
                }
                state.parsed = Some(key);
                document.text()
            }
            // The source tab was closed: keep showing what was last parsed.
            None if state.parsed.is_some() => return,
            None => {
                state.parsed = Some((u64::MAX, 0, 0));
                std::fs::read_to_string(&preview.source).unwrap_or_default()
            }
        };
        state.blocks = markdown::parse(&text);
        state.parsed_at = Some(Instant::now());
        state.layout = None;
    }

    fn markdown_padding(&self) -> i32 {
        self.scale(24)
    }

    fn tone(&self, tone: Tone) -> u32 {
        match tone {
            Tone::Text => self.theme.text,
            Tone::Muted => self.theme.muted,
            Tone::Link => self.theme.blue,
            Tone::CodeBg => self.theme.active_bg,
            Tone::Edge => self.theme.edge,
            Tone::QuoteBar => self.theme.violet,
            Tone::TableHead => self.theme.line_bg,
            Tone::Syntax(color) => self.theme.syntax(color),
        }
    }

    pub(in crate::windows_app) fn paint_markdown_pane(
        &self,
        hdc: HDC,
        preview: &MarkdownPreview,
        bounds: RECT,
    ) {
        Self::fill(hdc, bounds, self.theme.editor_bg);
        self.refresh_markdown(preview);
        let pad = self.markdown_padding();
        let width = (bounds.right - bounds.left - 2 * pad)
            .min(self.scale(920))
            .max(self.scale(80));
        let mut state = preview.state.borrow_mut();
        if state
            .fonts
            .as_ref()
            .is_none_or(|fonts| fonts.scale != (self.dpi, self.zoom))
        {
            state.fonts = Some(Fonts::new(self.dpi, self.zoom, false));
            state.layout = None;
            // SVGs were rasterized for the old scale.
            state.images.clear();
        }
        let (document_dir, root) = self.markdown_folders(&preview.source);
        let web_allowed = self.web_images_allowed(&root);
        let generation = preview.downloads.generation.load(Ordering::Acquire);
        let key = (width, self.dpi, self.zoom, generation, web_allowed);
        if state.layout.as_ref().is_none_or(|layout| layout.key != key) {
            let ViewState {
                blocks,
                fonts,
                images,
                layout,
                ..
            } = &mut *state;
            let fonts = fonts.as_ref().expect("fonts were just created");
            let mut layouter = Layouter {
                app: self,
                hdc,
                fonts,
                images,
                downloads: &preview.downloads,
                load_images: true,
                web_allowed,
                blocked: 0,
                document_dir,
                root,
                y: 0,
                items: Vec::new(),
                links: Vec::new(),
                anchors: Vec::new(),
                code_blocks: Vec::new(),
                code_header: 0,
                wrap_code: false,
            };
            for (index, block) in blocks.iter().enumerate() {
                layouter.block(block, index == 0, width);
            }
            *layout = Some(Layout {
                key,
                height: layouter.y,
                items: layouter.items,
                links: layouter.links,
                anchors: layouter.anchors,
                code_blocks: layouter.code_blocks,
                blocked: layouter.blocked,
            });
        }
        let state = &*state;
        let (Some(layout), Some(fonts)) = (state.layout.as_ref(), state.fonts.as_ref()) else {
            return;
        };
        // The bar asking to load web images stays put while the page scrolls.
        let banner = self.markdown_banner(bounds, layout.blocked);
        let banner_height = banner.map_or(0, |(bar, _)| bar.bottom - bar.top);
        let viewport = bounds.bottom - bounds.top - banner_height;
        let max_scroll = (layout.height + 2 * pad - viewport).max(0);
        let scroll = preview.scroll.get().clamp(0, max_scroll);
        preview.scroll.set(scroll);
        let (origin_x, origin_y) = (bounds.left + pad, bounds.top + banner_height + pad - scroll);
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, bounds.left, bounds.top, bounds.right, bounds.bottom);
            SetBkMode(hdc, TRANSPARENT as i32);
            self.paint_markdown_items(
                hdc,
                layout,
                fonts,
                &state.images,
                (origin_x, origin_y),
                bounds,
            );
            if let Some((bar, button)) = banner {
                self.paint_markdown_banner(hdc, bar, button, layout.blocked);
            }
            if state.blocks.is_empty() {
                SelectObject(hdc, self.ui_font);
                let message = "Nothing to preview yet";
                let x =
                    bounds.left + (bounds.right - bounds.left - self.text_width(hdc, message)) / 2;
                Self::label(
                    hdc,
                    message,
                    x.max(bounds.left),
                    bounds.top + (bounds.bottom - bounds.top) * 2 / 5,
                    self.theme.muted,
                    bounds,
                );
            }
            RestoreDC(hdc, saved);
        }
    }

    // Paints a laid-out document with its origin at `origin`, clipped to
    // `bounds`; only items inside `bounds` are drawn.
    fn paint_markdown_items(
        &self,
        hdc: HDC,
        layout: &Layout,
        fonts: &Fonts,
        images: &HashMap<String, Option<image_view::ImageAsset>>,
        (origin_x, origin_y): (i32, i32),
        bounds: RECT,
    ) {
        unsafe {
            let visible = |top: i32, bottom: i32| {
                bottom + origin_y >= bounds.top && top + origin_y <= bounds.bottom
            };
            let shift = |rect: &RECT| RECT {
                left: rect.left + origin_x,
                top: rect.top + origin_y,
                right: rect.right + origin_x,
                bottom: rect.bottom + origin_y,
            };
            for item in &layout.items {
                match item {
                    Item::Fill { rect, tone } if visible(rect.top, rect.bottom) => {
                        Self::fill(hdc, shift(rect), self.tone(*tone));
                    }
                    Item::Text {
                        x,
                        y,
                        width,
                        font,
                        tone,
                        text,
                        clip_right,
                        underline,
                        strike,
                    } if visible(*y, *y + fonts.height(*font)) => {
                        let (x, y) = (x + origin_x, y + origin_y);
                        let height = fonts.height(*font);
                        let color = self.tone(*tone);
                        SelectObject(hdc, fonts.handle(*font));
                        SetTextColor(hdc, color);
                        let clip = RECT {
                            left: bounds.left,
                            top: y,
                            right: (clip_right + origin_x).min(bounds.right),
                            bottom: y + height,
                        };
                        ExtTextOutW(
                            hdc,
                            x,
                            y,
                            ETO_CLIPPED,
                            &clip,
                            text.as_ptr(),
                            text.len() as u32,
                            null(),
                        );
                        let line = self.scale(1).max(1);
                        let right = (x + width).min(clip.right);
                        if *underline {
                            let under = y + height - line;
                            Self::fill(
                                hdc,
                                RECT {
                                    left: x,
                                    top: under,
                                    right,
                                    bottom: under + line,
                                },
                                color,
                            );
                        }
                        if *strike {
                            let middle = y + height / 2;
                            Self::fill(
                                hdc,
                                RECT {
                                    left: x,
                                    top: middle,
                                    right,
                                    bottom: middle + line,
                                },
                                color,
                            );
                        }
                    }
                    Item::Image { rect, key } if visible(rect.top, rect.bottom) => {
                        if let Some(Some(image)) = images.get(key) {
                            image.draw(hdc, shift(rect));
                        }
                    }
                    Item::Check { rect, checked } if visible(rect.top, rect.bottom) => {
                        let rect = shift(rect);
                        let line = self.scale(1).max(1);
                        let color = if *checked {
                            self.theme.blue
                        } else {
                            self.theme.muted
                        };
                        if *checked {
                            Self::fill(hdc, rect, color);
                            let size = rect.right - rect.left;
                            let pen = CreatePen(PS_SOLID, (size / 7).max(1), self.theme.editor_bg);
                            let old_pen = SelectObject(hdc, pen);
                            MoveToEx(hdc, rect.left + size / 5, rect.top + size / 2, null_mut());
                            LineTo(hdc, rect.left + size * 2 / 5, rect.bottom - size / 4);
                            LineTo(hdc, rect.right - size / 5, rect.top + size / 4);
                            SelectObject(hdc, old_pen);
                            DeleteObject(pen);
                        } else {
                            for edge in [
                                RECT {
                                    bottom: rect.top + line,
                                    ..rect
                                },
                                RECT {
                                    top: rect.bottom - line,
                                    ..rect
                                },
                                RECT {
                                    right: rect.left + line,
                                    ..rect
                                },
                                RECT {
                                    left: rect.right - line,
                                    ..rect
                                },
                            ] {
                                Self::fill(hdc, edge, color);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // The bar shown above a preview whose web images aren't loaded, and its
    // "Load images" button; None when nothing is blocked.
    fn markdown_banner(&self, bounds: RECT, blocked: usize) -> Option<(RECT, RECT)> {
        if blocked == 0 {
            return None;
        }
        let bar = RECT {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.top + self.scale(38),
        };
        let button = RECT {
            left: bar.right - self.scale(128),
            top: bar.top + self.scale(6),
            right: bar.right - self.scale(14),
            bottom: bar.bottom - self.scale(6),
        };
        Some((bar, button))
    }

    fn paint_markdown_banner(&self, hdc: HDC, bar: RECT, button: RECT, blocked: usize) {
        Self::fill(hdc, bar, self.theme.line_bg);
        Self::fill(
            hdc,
            RECT {
                top: bar.bottom - self.scale(1).max(1),
                ..bar
            },
            self.theme.edge,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let message = format!(
            "{blocked} web image{} not shown: LightLine only goes online when you allow it.",
            if blocked == 1 { " is" } else { "s are" }
        );
        let text_clip = RECT {
            right: button.left - self.scale(10),
            ..bar
        };
        self.label_ellipsis(
            hdc,
            &message,
            bar.left + self.scale(16),
            bar.top + self.scale(9),
            self.theme.muted,
            text_clip,
        );
        Self::rounded_fill(hdc, button, self.scale(5), self.theme.blue);
        let caption = "Load images";
        let caption_x =
            button.left + (button.right - button.left - self.text_width(hdc, caption)) / 2;
        Self::label(
            hdc,
            caption,
            caption_x,
            bar.top + self.scale(9),
            self.theme.editor_bg,
            button,
        );
    }

    fn web_images_allowed(&self, root: &Path) -> bool {
        self.settings.markdown_load_remote_images
            || self
                .web_image_folders
                .as_ref()
                .is_some_and(|folders| folders.iter().any(|folder| folder == root))
    }

    // Remembers that previews in this file's folder (its workspace, or its
    // own folder outside one) may load web images, then loads them.
    fn allow_web_images(&mut self, hwnd: HWND, source: &Path) {
        let (_, root) = self.markdown_folders(source);
        if let Err(error) = image_cache::allow_folder(&root) {
            self.status = format!("Web images load for now, but the choice wasn't saved: {error}");
        } else {
            self.status = "Loading web images...".into();
        }
        self.web_image_folders
            .get_or_insert_with(Vec::new)
            .push(root);
        for preview in self.tabs.iter().filter_map(|tab| tab.markdown.as_ref()) {
            preview.relayout();
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Scrolls the active preview by `pixels` (positive is down). Painting
    /// clamps it to the document.
    pub(super) fn scroll_markdown(&mut self, hwnd: HWND, pane: usize, pixels: i32) {
        if let Some(preview) = &self.tabs[self.tab_for_pane(pane)].markdown {
            let scroll = preview.scroll.get().saturating_add(pixels).max(0);
            preview.scroll.set(scroll);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    /// Keyboard scrolling in the preview. Returns true if `key` was used.
    pub(super) fn markdown_key(&mut self, hwnd: HWND, key: u32) -> bool {
        let page = (self.visible_lines(hwnd) as i32 * self.line_height - self.scale(40))
            .max(self.scale(40));
        let pixels = match key {
            k if k == VK_UP as u32 => -self.scale(40),
            k if k == VK_DOWN as u32 => self.scale(40),
            k if k == VK_PRIOR as u32 => -page,
            k if k == VK_NEXT as u32 => page,
            k if k == VK_HOME as u32 => i32::MIN / 2,
            k if k == VK_END as u32 => i32::MAX / 2,
            _ => return false,
        };
        self.scroll_markdown(hwnd, self.focused_pane, pixels);
        true
    }

    /// Follows the link under a click in a preview pane, if there is one.
    pub(super) fn click_markdown(&mut self, hwnd: HWND, pane: usize, x: i32, y: i32) {
        let tab = self.tab_for_pane(pane);
        let Some(preview) = &self.tabs[tab].markdown else {
            return;
        };
        let pad = self.markdown_padding();
        let blocked = preview
            .state
            .borrow()
            .layout
            .as_ref()
            .map_or(0, |layout| layout.blocked);
        let pane_bounds = RECT {
            left: self.pane_left(hwnd, pane),
            top: self.editor_top(),
            right: self.pane_right(hwnd, pane),
            bottom: i32::MAX,
        };
        let mut banner_height = 0;
        if let Some((bar, _)) = self.markdown_banner(pane_bounds, blocked) {
            if y >= bar.top && y < bar.bottom && x >= bar.left && x < bar.right {
                let source = preview.source.clone();
                self.allow_web_images(hwnd, &source);
                return;
            }
            banner_height = bar.bottom - bar.top;
        }
        let content_x = x - (pane_bounds.left + pad);
        let content_y = y - (self.editor_top() + banner_height + pad) + preview.scroll.get();
        let (url, anchors) = {
            let state = preview.state.borrow();
            let Some(layout) = state.layout.as_ref() else {
                return;
            };
            let url = layout
                .links
                .iter()
                .find(|(rect, _)| {
                    content_x >= rect.left
                        && content_x < rect.right
                        && content_y >= rect.top
                        && content_y < rect.bottom
                })
                .map(|(_, url)| url.clone());
            (url, layout.anchors.clone())
        };
        let Some(url) = url else {
            return;
        };
        let (document_dir, root) = self.markdown_folders(&preview.source);
        match markdown::resolve_link(&url, &document_dir, &root) {
            LinkTarget::Web(address) => {
                use windows_sys::Win32::UI::Shell::ShellExecuteW;
                let (operation, target) = (wide("open"), wide(&address));
                unsafe {
                    ShellExecuteW(
                        hwnd,
                        operation.as_ptr(),
                        target.as_ptr(),
                        null(),
                        null(),
                        SW_SHOWNORMAL,
                    );
                }
                self.status = format!("Opened {address} in your browser");
            }
            LinkTarget::Anchor(anchor) => match anchors.iter().find(|(slug, _)| *slug == anchor) {
                Some((_, top)) => preview.scroll.set(*top),
                None => self.status = format!("No heading #{anchor} in this file"),
            },
            LinkTarget::File(path) if path.is_file() => {
                self.open(hwnd, Some(path));
                return;
            }
            LinkTarget::File(path) => {
                self.status = format!("File not found: {}", display_path(&path));
            }
            LinkTarget::Refused => {
                self.status = format!("LightLine doesn't follow this link: {url}");
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

/// Height of the strip at the top of a snippet's code blocks, in 96-dpi
/// pixels.
pub(super) const SNIPPET_CODE_HEADER: i32 = 30;

/// Markdown that isn't a file -- an AI answer -- laid out once per width and
/// painted anywhere. Images are described rather than loaded, and nothing
/// is downloaded.
#[derive(Default)]
pub(super) struct MarkdownSnippet {
    state: RefCell<SnippetState>,
}

#[derive(Default)]
struct SnippetState {
    // The revision of the text `blocks` were parsed from.
    revision: Option<u64>,
    blocks: Vec<Block>,
    layout: Option<Layout>,
}

/// The fonts snippets are drawn with, shared by every snippet in a view and
/// made again when the scale changes.
#[derive(Default)]
pub(super) struct SnippetFonts(RefCell<Option<Fonts>>);

impl App {
    fn snippet_fonts<'a>(&self, fonts: &'a SnippetFonts) -> std::cell::Ref<'a, Option<Fonts>> {
        if fonts
            .0
            .borrow()
            .as_ref()
            .is_none_or(|current| current.scale != (self.dpi, self.zoom))
        {
            *fonts.0.borrow_mut() = Some(Fonts::new(self.dpi, self.zoom, true));
        }
        fonts.0.borrow()
    }

    /// Lays `text` out for `width` (again only when `revision`, the width or
    /// the scale changed) and returns its height.
    pub(super) fn snippet_height(
        &self,
        hdc: HDC,
        snippet: &MarkdownSnippet,
        fonts: &SnippetFonts,
        (text, revision): (&str, u64),
        width: i32,
    ) -> i32 {
        let fonts = self.snippet_fonts(fonts);
        let Some(fonts) = fonts.as_ref() else {
            return 0;
        };
        let mut state = snippet.state.borrow_mut();
        if state.revision != Some(revision) {
            state.blocks = markdown::parse(text);
            state.revision = Some(revision);
            state.layout = None;
        }
        let key = (width, self.dpi, self.zoom, 0, false);
        if state.layout.as_ref().is_none_or(|layout| layout.key != key) {
            let mut images = HashMap::new();
            let downloads = Arc::default();
            let mut layouter = Layouter {
                app: self,
                hdc,
                fonts,
                images: &mut images,
                downloads: &downloads,
                load_images: false,
                web_allowed: false,
                blocked: 0,
                document_dir: PathBuf::new(),
                root: PathBuf::new(),
                y: 0,
                items: Vec::new(),
                links: Vec::new(),
                anchors: Vec::new(),
                code_blocks: Vec::new(),
                // Room for the chat's toolbar (language, Insert, Copy...).
                code_header: self.scale(SNIPPET_CODE_HEADER),
                wrap_code: true,
            };
            for (index, block) in state.blocks.iter().enumerate() {
                layouter.block(block, index == 0, width);
            }
            state.layout = Some(Layout {
                key,
                height: layouter.y,
                items: layouter.items,
                links: layouter.links,
                anchors: layouter.anchors,
                code_blocks: layouter.code_blocks,
                blocked: 0,
            });
        }
        state.layout.as_ref().map_or(0, |layout| layout.height)
    }

    /// Paints a snippet laid out by snippet_height with its top-left corner
    /// at `origin`, clipped to `bounds`.
    pub(super) fn paint_snippet(
        &self,
        hdc: HDC,
        snippet: &MarkdownSnippet,
        fonts: &SnippetFonts,
        origin: (i32, i32),
        bounds: RECT,
    ) {
        let fonts = fonts.0.borrow();
        let state = snippet.state.borrow();
        let (Some(fonts), Some(layout)) = (fonts.as_ref(), state.layout.as_ref()) else {
            return;
        };
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, bounds.left, bounds.top, bounds.right, bounds.bottom);
            SetBkMode(hdc, TRANSPARENT as i32);
            self.paint_markdown_items(hdc, layout, fonts, &HashMap::new(), origin, bounds);
            RestoreDC(hdc, saved);
        }
    }
}

impl MarkdownSnippet {
    /// The fenced code blocks of the last layout: each one's box (relative
    /// to the snippet's top-left corner) and text.
    pub(super) fn code_blocks(&self) -> Vec<CodeBlock> {
        self.state
            .borrow()
            .layout
            .as_ref()
            .map(|layout| layout.code_blocks.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_code_lines_break_before_a_word() {
        assert_eq!(code_breaks("short", 20, 2), vec![0]);
        assert_eq!(code_breaks("", 20, 2), vec![0]);
        // 10 columns: "let sum = " fits, then "values" starts the next row,
        // which has 8 (two go to its indent).
        assert_eq!(
            code_breaks("let sum = values.iter()", 10, 2),
            vec![0, 10, 18]
        );
        // A word too long to carry down is cut where the row is full.
        assert_eq!(code_breaks("a bcdefghijk", 10, 2), vec![0, 10]);
        // No space to break at: the row is cut where it's full.
        assert_eq!(code_breaks("abcdefghijkl", 8, 2), vec![0, 8]);
        // A tab counts as four columns.
        assert_eq!(code_breaks("\t\tabcd", 8, 2), vec![0, 2]);
    }

    #[test]
    fn wrapped_code_continues_past_the_line_s_indent() {
        assert_eq!(code_continuation_indent("    let x", 40), 6);
        assert_eq!(code_continuation_indent("let x", 40), 2);
        assert_eq!(code_continuation_indent("\t\t\tx", 20), 10);
        // A deeper indent leaves continuation rows less room.
        assert_eq!(code_breaks("    abc defgh", 10, 6), vec![0, 8, 12]);
    }

    #[test]
    fn words_keep_their_spaces_and_hard_breaks_stand_alone() {
        assert_eq!(pieces("one two  three"), vec!["one ", "two  ", "three"]);
        assert_eq!(pieces("a\nb"), vec!["a", "\n", "b"]);
        assert_eq!(pieces("  lead"), vec!["  ", "lead"]);
    }

    #[test]
    fn inline_styles_pick_matching_fonts() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let italic = Style {
            italic: true,
            ..Style::default()
        };
        let code = Style {
            code: true,
            ..Style::default()
        };
        assert!(matches!(font_for(Font::Body, bold), Font::Bold));
        assert!(matches!(font_for(Font::Bold, italic), Font::BoldItalic));
        assert!(matches!(font_for(Font::Body, code), Font::Code));
        assert!(matches!(font_for(Font::H2, bold), Font::H2));
    }
}
