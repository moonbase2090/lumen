//! The headless rasteriser: blocks to an RGBA frame, with no window.
//!
//! Two bundled faces (IBM Plex Mono Regular and IBM Plex Sans Regular, see
//! `NOTICE.txt`), integer blending and a fixed light theme, so the same document gives the same
//! pixels every time on a platform. That is the groundwork for the pixel
//! oracle of criterion 2.3: a baseline is taken twice and must match exactly,
//! and baselines are kept per platform.
//!
//! Grids and code blocks are drawn on the monospace cell grid. Paragraph and
//! heading text in a rich block is laid out in the proportional face: each
//! glyph advances by its own width, and lines wrap at the container's pixel
//! width. A character the proportional face lacks is drawn from the monospace
//! face in whole cells.
//!
//! What it does not do: bold or italic faces, glyphs neither face has (they are
//! skipped), combining marks (the base glyph only), a cursor (off unless
//! asked, see [`CursorState`]), kerning, or any other text shaping.

use crate::copy::Kind as CopyKind;
use crate::design::{FontFile, Fonts};
use crate::document::{Block, Document, RichBlock};
use crate::grid::{CursorShape, GridBlock};
use crate::image_draw;
use crate::layout::{GUTTER_COLUMNS, HEADER_ROWS, Rect, StripPlacement, layout, notification_area};
use crate::notify::Toast;
use crate::scroll::Scroll;
use crate::search::{Mark, Overlay, SearchRow};
use crate::select::{FlowLine, Join, Selection};
use lumen_track0::Block as Track0Block;
use prismattyc_core::{Cell, Color, Style, grapheme_display_width};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

/// The bundled monospace face.
const FACE: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
/// The bundled proportional face, for text in rich blocks.
const SANS: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");
/// Proportional advances are measured in 64ths of a pixel, so a line is as
/// wide as its glyphs add up to and rounding does not build up along it.
const SUBPIXELS: usize = 64;
const SUBPIXELS_F: f32 = 64.0;
/// Pixel size of the face at scale 1.
pub const BASE_PX: f32 = 14.0;
/// Pixels of padding, at scale 1, between the gutter's divider and the text
/// column. It is scaled with the render scale, so it follows zoom and DPI.
pub const GUTTER_PAD_PX: u32 = 6;
/// The label every rich block's container carries.
pub const CONTAINER_LABEL: &str = "unverified output";
pub use crate::image_draw::IMAGE_MAX_ROWS;
/// The most lines of text in a toast; the rest is cut.
pub const TOAST_MAX_LINES: usize = 3;
/// The terminal's own text in the header strip of the trust margin.
pub const MARGIN_LABEL: &str = "Bright";

/// The terminal's mark, in the gutter, beside the first row of a line typed in
/// the input box for a sealed command. Only the terminal draws in the gutter,
/// so output cannot draw one.
pub const TYPED_MARK: &str = "kbd";

/// What the mark means, shown while the pointer is over it.
pub const TYPED_TIP: &str = "typed here";

pub use crate::frame::{Frame, Rgb};

pub use crate::design::Theme;

/// How the cursor of the live grid is drawn: whether the window has the focus, and, for a
/// blinking cursor, which half of its cycle this frame shows.
///
/// An unfocused window draws the cursor dimmed (a block as an outline, a bar or underline in a
/// blend of its colour and the cell's) and steady, whatever the program asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// The window has the focus.
    pub focused: bool,
    /// The on half of a blink cycle; ignored by a steady cursor and by an unfocused window.
    pub blink_on: bool,
}

/// The widest a link prompt is, in cells.
#[cfg(feature = "links-clipboard")]
const PROMPT_COLUMNS: u32 = 72;

/// Where a link prompt is drawn.
#[cfg(feature = "links-clipboard")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBox {
    /// The box: x, y, width and height in pixels.
    pub rect: (u32, u32, u32, u32),
    /// The text lines, wrapped to the box.
    pub lines: Vec<String>,
    /// Each button's label and its rectangle.
    pub buttons: Vec<(String, (u32, u32, u32, u32))>,
}

/// What a point on a link prompt is.
#[cfg(feature = "links-clipboard")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptHit {
    /// The button that opens the link.
    Open,
    /// The button that closes the prompt without opening.
    Cancel,
    /// The box, away from a button.
    Box,
    /// Outside the box.
    Outside,
}

/// Where a context menu is drawn: its box and each entry's rectangle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuBox {
    /// The box: x, y, width and height in pixels.
    pub rect: (u32, u32, u32, u32),
    /// Each entry's rectangle, in the menu's order (a separator too).
    pub rows: Vec<(u32, u32, u32, u32)>,
}

/// What a point on a context menu is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    /// The entry at this index of the menu's items (it may be turned off).
    Item(usize),
    /// The box, away from an entry (a separator or the border).
    Box,
    /// Outside the box.
    Outside,
}

/// Thickness of a bar or underline cursor, in pixels at scale 1.
const CURSOR_LINE_PX: u32 = 2;

/// What the input box shows: its text, or a one-row status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputView {
    /// The text of the box, one entry per line. Empty when it shows a status.
    pub lines: Vec<String>,
    /// The caret as (line, column in characters), when the box is edited.
    pub caret: Option<(usize, usize)>,
    /// The window has focus.
    pub focused: bool,
    /// A one-row status in place of the text: where keys go.
    pub status: Option<String>,
    /// A line under the text, inside the box: the choice a held line waits for.
    pub hint: Option<String>,
    /// Text being composed at the caret, drawn there with the rest of the line moved right.
    pub preedit: Option<PreeditView>,
}

impl InputView {
    /// The same caret as `caret`, with its column in display cells.
    pub(crate) fn caret_cell(&self) -> Option<(usize, u32)> {
        let (line, character) = self.caret?;
        let column = self.lines.get(line).map_or(0, |text| {
            Self::cell_offsets(text)
                .take(character.saturating_add(1))
                .last()
                .unwrap_or(0)
        });
        Some((line, u32::try_from(column).unwrap_or(u32::MAX)))
    }

    /// A click inside a wide character goes to its leading boundary.
    pub(crate) fn character_at_cell(&self, line: usize, column: usize) -> usize {
        self.lines.get(line).map_or(0, |text| {
            Self::cell_offsets(text)
                .take_while(|&offset| offset <= column)
                .count()
                .saturating_sub(1)
        })
    }

    fn cell_offsets(text: &str) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(0).chain(text.chars().map(cells).scan(0, |offset, width| {
            *offset += width;
            Some(*offset)
        }))
    }

    /// The first line shown when the box scrolls to keep the caret in view.
    pub(crate) fn visible_first_line(&self, rows: usize) -> usize {
        let caret_line = self.caret.map_or(0, |(line, _)| line);
        (caret_line + 1)
            .saturating_sub(rows)
            .min(self.lines.len().saturating_sub(rows))
    }
}

/// The text an input method is composing, to draw where it will go: at the caret of the box
/// or at the program's cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreeditView {
    /// The unfinished text.
    pub text: String,
    /// The caret or the selected clause, as byte offsets into `text`; `None` hides the caret.
    pub cursor: Option<(usize, usize)>,
}

/// Most lines of text the box shows before it scrolls.
pub const INPUT_MAX_LINES: usize = 5;

/// The size of a window and where its editor sits, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Columns.
    pub cols: u32,
    /// Rows.
    pub rows: u32,
    /// The row the editor starts at.
    pub editor_row: u32,
    /// Rows of the editor.
    pub editor_rows: u32,
}

/// One block of the flow of content, and where it sits.
struct Placed<'a> {
    /// Index of the block in the document (the first, for a strip).
    index: usize,
    top: u32,
    height: u32,
    part: Part<'a>,
}

/// The find marks of one block, by row.
#[derive(Clone, Copy, Default)]
struct Marks<'a> {
    map: Option<&'a std::collections::HashMap<(usize, usize), Vec<Mark>>>,
    block: usize,
}

impl<'a> Marks<'a> {
    fn of(overlay: Option<&'a Overlay>, block: usize) -> Self {
        Self {
            map: overlay.map(|o| &o.marks),
            block,
        }
    }

    fn row(&self, row: usize) -> &'a [Mark] {
        self.map
            .and_then(|m| m.get(&(self.block, row)))
            .map_or(&[], Vec::as_slice)
    }
}

enum Part<'a> {
    Grid(&'a GridBlock),
    Rich(&'a RichBlock),
    Strip(&'a crate::strip::Strip<'a>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Face {
    Mono,
    Sans,
}

struct Glyph {
    xmin: i32,
    ymin: i32,
    width: usize,
    height: usize,
    coverage: Vec<u8>,
}

/// Turns blocks into pixels.
pub struct Renderer {
    font: OwnedFace,
    sans: OwnedFace,
    scaler: RefCell<ScaleContext>,
    /// Pixel size of the monospace face.
    px: f32,
    /// Pixel size of the proportional face.
    sans_px: f32,
    fonts: Fonts,
    scale: u32,
    cell_w: u32,
    cell_h: u32,
    ascent: i32,
    theme: Theme,
    cache: RefCell<HashMap<(Face, char), Glyph>>,
    advances: RefCell<HashMap<char, usize>>,
}

/// A face the renderer owns: the bundled bytes or a user's font file.
struct OwnedFace {
    data: Arc<[u8]>,
    offset: u32,
    key: swash::CacheKey,
}

impl OwnedFace {
    fn new(file: &FontFile, bundled: &'static [u8]) -> Result<Self, String> {
        let data: Arc<[u8]> = match file {
            FontFile::Bundled => Arc::from(bundled),
            FontFile::Data(d) => d.bytes(),
        };
        let face = FontRef::from_index(&data, 0).ok_or("a face could not be read")?;
        let (offset, key) = (face.offset, face.key);
        Ok(Self { data, offset, key })
    }

    fn get(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }
}

fn resolve(colour: Color, default: Rgb, theme: &Theme) -> Rgb {
    match colour {
        Color::Default => default,
        Color::Ansi(n) => theme.ansi[usize::from(n.min(15))],
        Color::Indexed(n) => indexed(n, theme),
        Color::Rgb { r, g, b } => [r, g, b],
    }
}

pub(crate) fn indexed(n: u8, theme: &Theme) -> Rgb {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match n {
        0..=15 => theme.ansi[usize::from(n)],
        16..=231 => {
            let i = usize::from(n - 16);
            [LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6]]
        }
        232..=255 => {
            let v = 8 + 10 * (n - 232);
            [v, v, v]
        }
    }
}

impl Renderer {
    /// A renderer for the bundled faces at `scale` (the 2.3 conditions use 2).
    ///
    /// # Errors
    ///
    /// Returns a message if a face cannot be read, which would be a broken
    /// build, since they are compiled in.
    pub fn new(scale: u32, theme: Theme) -> Result<Self, String> {
        Self::with_fonts(scale, theme, &Fonts::default())
    }

    /// A renderer for `fonts` at `scale`. A cell is as wide as the monospace
    /// face's `M`, and a row is tall enough for a line of either face, so
    /// prose set larger than the grid is not cut.
    ///
    /// # Errors
    ///
    /// Returns a message if a face cannot be read.
    pub fn with_fonts(scale: u32, theme: Theme, fonts: &Fonts) -> Result<Self, String> {
        let font = OwnedFace::new(&fonts.mono, FACE)?;
        let sans = OwnedFace::new(&fonts.prose, SANS)?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a scale and a font size are small whole numbers"
        )]
        let (px, sans_px) = (
            fonts.mono_px() as f32 * scale.max(1) as f32,
            fonts.prose_px() as f32 * scale.max(1) as f32,
        );
        let (mono_ref, sans_ref) = (font.get(), sans.get());
        let lines = mono_ref.metrics(&[]).scale(px);
        let sans_lines = sans_ref.metrics(&[]).scale(sans_px);
        let advance = mono_ref
            .glyph_metrics(&[])
            .scale(px)
            .advance_width(mono_ref.charmap().map('M'));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "metrics of an 8 to 300 pixel face are small and positive"
        )]
        let (cell_w, cell_h, ascent) = (
            advance.round().max(1.0) as u32,
            (lines.ascent + lines.descent)
                .ceil()
                .max((sans_lines.ascent + sans_lines.descent).ceil())
                .max(1.0) as u32,
            lines.ascent.round().max(sans_lines.ascent.round()) as i32,
        );
        Ok(Self {
            font,
            sans,
            scaler: RefCell::new(ScaleContext::new()),
            px,
            sans_px,
            fonts: fonts.clone(),
            scale: scale.max(1),
            cell_w,
            cell_h,
            ascent,
            theme,
            cache: RefCell::new(HashMap::new()),
            advances: RefCell::new(HashMap::new()),
        })
    }

    /// The fonts in use.
    #[must_use]
    pub fn fonts(&self) -> &Fonts {
        &self.fonts
    }

    /// Pixels between the gutter's divider and the text column: the space the
    /// window is wider than its cells by.
    #[must_use]
    pub fn gutter_pad(&self) -> u32 {
        GUTTER_PAD_PX * self.scale
    }

    /// The render scale.
    #[must_use]
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// The bundled face at `scale` in the light theme.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn light(scale: u32) -> Result<Self, String> {
        Self::new(scale, Theme::light())
    }

    /// The size of a cell in pixels.
    #[must_use]
    pub fn cell(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// The theme in use.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Draw in `theme` from the next frame on. Glyph shapes are kept: only
    /// colours change.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    fn draw_glyph(&self, frame: &mut Frame, ch: char, x: u32, y: u32, cells: u32, colour: Rgb) {
        let band = (i64::from(x), i64::from(x + cells * self.cell_w));
        self.draw_face(frame, Face::Mono, ch, (x, y), band, colour);
    }

    /// Draw `ch` from `face` with its origin at `at`, clipped to the pixel
    /// columns `band` and to the row below `at.1`.
    fn draw_face(
        &self,
        frame: &mut Frame,
        face: Face,
        ch: char,
        at: (u32, u32),
        band: (i64, i64),
        colour: Rgb,
    ) {
        let (x, y) = at;
        let (font, size) = match face {
            Face::Mono => (self.font.get(), self.px),
            Face::Sans => (self.sans.get(), self.sans_px),
        };
        let glyph = font.charmap().map(ch);
        if ch == ' ' || ch == '\0' || glyph == 0 {
            return;
        }
        let mut cache = self.cache.borrow_mut();
        let g = cache.entry((face, ch)).or_insert_with(|| {
            let mut context = self.scaler.borrow_mut();
            let mut scaler = context.builder(font).size(size).hint(false).build();
            let Some(image) = Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .render(&mut scaler, glyph)
            else {
                return Glyph {
                    xmin: 0,
                    ymin: 0,
                    width: 0,
                    height: 0,
                    coverage: Vec::new(),
                };
            };
            let p = image.placement;
            // The image's top row is `p.top` above the baseline, so its bottom
            // is `p.top - height` above it: that is the glyph's `ymin`.
            Glyph {
                xmin: p.left,
                ymin: p.top - i32::try_from(p.height).unwrap_or(0),
                width: p.width as usize,
                height: p.height as usize,
                coverage: image.data,
            }
        });
        let rows = i64::try_from(g.height).unwrap_or(0);
        let top = i64::from(y) + i64::from(self.ascent) - i64::from(g.ymin) - rows;
        let left = i64::from(x) + i64::from(g.xmin);
        let (band_start, band_end) = band;
        let (cell_top, cell_bottom) = (i64::from(y), i64::from(y + self.cell_h));
        for row in 0..g.height {
            for col in 0..g.width {
                let px = left + i64::try_from(col).unwrap_or(0);
                let py = top + i64::try_from(row).unwrap_or(0);
                if px < band_start || px >= band_end || py < cell_top || py >= cell_bottom {
                    continue;
                }
                let a = g.coverage[row * g.width + col];
                if let (true, Ok(ux), Ok(uy)) = (a > 0, u32::try_from(px), u32::try_from(py)) {
                    frame.blend(ux, uy, colour, a);
                }
            }
        }
    }

    fn colours(&self, style: Style) -> (Rgb, Rgb) {
        let fg = resolve(style.foreground, self.theme.foreground, &self.theme);
        let bg = resolve(style.background, self.theme.background, &self.theme);
        if style.inverse { (bg, fg) } else { (fg, bg) }
    }

    /// Paint a grid block with its top left at `x`, `y`.
    pub fn paint_grid(&self, frame: &mut Frame, x: u32, y: u32, block: &GridBlock) {
        self.paint_grid_marked(frame, x, y, block, Marks::default());
    }

    fn paint_grid_marked(
        &self,
        frame: &mut Frame,
        x: u32,
        y: u32,
        block: &GridBlock,
        marks: Marks<'_>,
    ) {
        let screen = block.screen();
        let hist = block.history_rows();
        for r in 0..screen.rows() {
            let Some(cells) = screen.row(r) else { continue };
            let top = y + u32::try_from(r).unwrap_or(0) * self.cell_h;
            self.paint_cells(frame, x, top, cells, marks.row(hist + r));
            #[cfg(feature = "links-clipboard")]
            self.paint_link_marks(frame, x, top, &crate::linkhit::row_links(screen, r));
        }
    }

    /// Underline the links of one row in the accent colour, one pixel row
    /// above the bottom of the cell. The cells keep the program's own colours.
    #[cfg(feature = "links-clipboard")]
    fn paint_link_marks(&self, frame: &mut Frame, x: u32, top: u32, hits: &[crate::linkhit::Hit]) {
        let line = top + self.cell_h.saturating_sub(self.scale * 2);
        for hit in hits {
            let left = x + u32::try_from(hit.columns.start).unwrap_or(0) * self.cell_w;
            let cols = u32::try_from(hit.columns.len()).unwrap_or(0);
            frame.fill(
                left,
                line,
                cols * self.cell_w,
                self.scale,
                self.theme.prompt,
            );
        }
    }

    /// Paint one row of cells with its top left at `x`, `top`. The characters
    /// in `marks` are drawn as find matches.
    fn paint_cells(&self, frame: &mut Frame, x: u32, top: u32, cells: &[Cell], marks: &[Mark]) {
        let mut c = 0;
        let mut ci = 0usize;
        let mut boxes: Vec<(u32, u32, usize)> = Vec::new();
        while c < cells.len() {
            let cell = &cells[c];
            let span = 1 + usize::from(cells.get(c + 1).is_some_and(|n| n.wide_cont));
            if cell.wide_cont {
                c += 1;
                continue;
            }
            let (mut fg, mut bg) = self.colours(cell.style);
            let left = x + u32::try_from(c).unwrap_or(0) * self.cell_w;
            let w = u32::try_from(span).unwrap_or(1);
            if let Some((i, m)) = marks
                .iter()
                .enumerate()
                .find(|(_, m)| m.start <= ci && ci < m.end)
            {
                fg = self.theme.find_text;
                bg = self.mark_background(m);
                if m.current {
                    match boxes.iter_mut().find(|b| b.2 == i) {
                        Some(b) => b.1 = left + w * self.cell_w,
                        None => boxes.push((left, left + w * self.cell_w, i)),
                    }
                }
            }
            frame.fill(left, top, w * self.cell_w, self.cell_h, bg);
            self.draw_glyph(frame, cell.character, left, top, w, fg);
            if cell.style.underline {
                let line = top + self.cell_h - self.scale.min(self.cell_h);
                frame.fill(left, line, w * self.cell_w, self.scale, fg);
            }
            c += span;
            ci += 1;
        }
        for (l, r, _) in boxes {
            self.outline(frame, l, top, r - l);
        }
    }

    /// The background of a find match.
    fn mark_background(&self, m: &Mark) -> Rgb {
        if m.current {
            self.theme.find_current
        } else {
            self.theme.find_match
        }
    }

    /// The outline of the current find match, `width` wide, one cell high.
    fn outline(&self, frame: &mut Frame, x: u32, y: u32, width: u32) {
        let t = self.scale.min(self.cell_h).min(width).max(1);
        let c = self.theme.find_text;
        frame.fill(x, y, width, t, c);
        frame.fill(x, y + self.cell_h - t, width, t, c);
        frame.fill(x, y, t, self.cell_h, c);
        frame.fill(x + width - t, y, t, self.cell_h, c);
    }

    /// Paint the cursor of a grid block whose top left is at `x`, `y`: block, bar or underline as
    /// the program set it, nothing while the program has hidden it or a blinking cursor is in its
    /// off half. Call after [`paint_grid`](Self::paint_grid).
    #[allow(
        clippy::many_single_char_names,
        reason = "x, y, w, h and t are the usual pixel names"
    )]
    pub fn paint_cursor(
        &self,
        frame: &mut Frame,
        x: u32,
        y: u32,
        block: &GridBlock,
        state: CursorState,
    ) {
        let view = block.cursor();
        if !view.visible || (view.blink && state.focused && !state.blink_on) {
            return;
        }
        let (columns, rows) = block.size();
        let screen = block.screen();
        let Some(cells) = screen
            .row(view.row)
            .filter(|_| view.row < rows && columns > 0)
        else {
            return;
        };
        let mut col = view.column.min(columns - 1);
        if cells.get(col).is_some_and(|c| c.wide_cont) {
            col = col.saturating_sub(1);
        }
        let cell = &cells[col];
        let span = 1 + usize::from(cells.get(col + 1).is_some_and(|n| n.wide_cont));
        let w = u32::try_from(span).unwrap_or(1);
        let left = x + u32::try_from(col).unwrap_or(0) * self.cell_w;
        let top = y + u32::try_from(view.row).unwrap_or(0) * self.cell_h;
        let width = w * self.cell_w;
        let t = self.scale.max(1);
        let ink = self.theme.foreground;
        let under = self.colours(cell.style).1;
        let dimmed = [0, 1, 2].map(|i| ink[i] / 2 + under[i] / 2);
        let colour = if state.focused { ink } else { dimmed };
        match view.shape {
            CursorShape::Block if state.focused => {
                frame.fill(left, top, width, self.cell_h, ink);
                self.draw_glyph(frame, cell.character, left, top, w, self.theme.background);
            }
            CursorShape::Block => {
                // Unfocused: an outline, the cell's own glyph left as it was.
                let edge = t.min(width).min(self.cell_h);
                frame.fill(left, top, width, edge, ink);
                frame.fill(left, top + self.cell_h - edge, width, edge, ink);
                frame.fill(left, top, edge, self.cell_h, ink);
                frame.fill(left + width - edge, top, edge, self.cell_h, ink);
            }
            CursorShape::Bar => {
                frame.fill(
                    left,
                    top,
                    (CURSOR_LINE_PX * t).min(width),
                    self.cell_h,
                    colour,
                );
            }
            CursorShape::Underline => {
                let h = (CURSOR_LINE_PX * t).min(self.cell_h);
                frame.fill(left, top + self.cell_h - h, width, h, colour);
            }
        }
    }

    fn text_line(&self, frame: &mut Frame, x: u32, y: u32, text: &str, colour: Rgb) {
        self.text_line_marked(frame, x, y, text, colour, &[]);
    }

    /// Put `content` below a terminal-owned tab strip. Each tab gets the same
    /// share of the width; its number stays visible when its title is clipped.
    #[must_use]
    pub fn with_tabs(
        &self,
        content: &Frame,
        titles: &[String],
        active: usize,
        height: u32,
    ) -> Frame {
        let mut frame = Frame::new(content.width, height, self.theme.background);
        if titles.is_empty() {
            Self::blit(&mut frame, content, 0, 0);
            return frame;
        }
        let count = u32::try_from(titles.len()).unwrap_or(u32::MAX).max(1);
        for (index, title) in titles.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(count - 1);
            let left = u64::from(index) * u64::from(content.width) / u64::from(count);
            let right = u64::from(index + 1) * u64::from(content.width) / u64::from(count);
            let (left, right) = (
                u32::try_from(left).unwrap_or(0),
                u32::try_from(right).unwrap_or(content.width),
            );
            let width = right.saturating_sub(left);
            let chosen = usize::try_from(index).ok() == Some(active);
            frame.fill(
                left,
                0,
                width,
                self.cell_h,
                if chosen {
                    self.theme.editor
                } else {
                    self.theme.margin
                },
            );
            if right < content.width {
                frame.fill(
                    right.saturating_sub(1),
                    0,
                    1,
                    self.cell_h,
                    self.theme.divider,
                );
            }
            let cells = width / self.cell_w;
            if cells > 2 {
                let label = format!("{} {title}", index + 1);
                let shown = clip(&label, usize::try_from(cells - 2).unwrap_or(0));
                self.text_line(
                    &mut frame,
                    left + self.cell_w,
                    0,
                    &shown,
                    if chosen {
                        self.theme.foreground
                    } else {
                        self.theme.margin_text
                    },
                );
            }
        }
        frame.fill(
            0,
            self.cell_h.saturating_sub(1),
            content.width,
            1,
            self.theme.divider,
        );
        Self::blit(&mut frame, content, 0, self.cell_h);
        frame
    }

    /// As [`text_line`](Self::text_line), with the characters in `marks` drawn as
    /// find matches.
    fn text_line_marked(
        &self,
        frame: &mut Frame,
        x: u32,
        y: u32,
        text: &str,
        colour: Rgb,
        marks: &[Mark],
    ) {
        let mut col = 0u32;
        let mut boxes: Vec<(u32, u32, usize)> = Vec::new();
        for (ci, ch) in text.chars().enumerate() {
            let w = u32::try_from(grapheme_display_width(ch, &[])).unwrap_or(1);
            let left = x + col * self.cell_w;
            let mut ink = colour;
            if let Some((i, m)) = marks
                .iter()
                .enumerate()
                .find(|(_, m)| m.start <= ci && ci < m.end)
            {
                ink = self.theme.find_text;
                frame.fill(
                    left,
                    y,
                    w * self.cell_w,
                    self.cell_h,
                    self.mark_background(m),
                );
                if m.current {
                    match boxes.iter_mut().find(|b| b.2 == i) {
                        Some(b) => b.1 = left + w * self.cell_w,
                        None => boxes.push((left, left + w * self.cell_w, i)),
                    }
                }
            }
            if w > 0 {
                self.draw_glyph(frame, ch, left, y, w, ink);
            }
            col += w;
        }
        for (l, r, _) in boxes {
            self.outline(frame, l, y, r - l);
        }
    }

    /// The face a character of proportional text is drawn from: the
    /// proportional one when it has the glyph and the character takes one cell,
    /// else the monospace one.
    fn proportional_face(&self, ch: char) -> Face {
        if self.sans.get().charmap().map(ch) != 0 && grapheme_display_width(ch, &[]) == 1 {
            Face::Sans
        } else {
            Face::Mono
        }
    }

    /// The advance of `ch` in the proportional layout, in 64ths of a pixel. A
    /// character that draws nothing has none. One the proportional face lacks,
    /// or one that takes two cells, takes whole cells, as it does on the grid.
    fn advance(&self, ch: char) -> usize {
        let cells = grapheme_display_width(ch, &[]);
        if cells == 0 {
            return 0;
        }
        if let Some(a) = self.advances.borrow().get(&ch) {
            return *a;
        }
        let a = if self.proportional_face(ch) == Face::Sans {
            let sans = self.sans.get();
            let glyph = sans.charmap().map(ch);
            let w = sans
                .glyph_metrics(&[])
                .scale(self.sans_px)
                .advance_width(glyph);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "an advance of a 14 to 100 pixel face is small and positive"
            )]
            let units = (w * SUBPIXELS_F).round().max(0.0) as usize;
            units
        } else {
            cells * self.cell_w as usize * SUBPIXELS
        };
        self.advances.borrow_mut().insert(ch, a);
        a
    }

    /// The width of `text` in the proportional layout, in 64ths of a pixel.
    #[must_use]
    pub fn text_width(&self, text: &str) -> usize {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// Draw `text` in the proportional face with its top left at `x`, `y`,
    /// each glyph advancing by its own width, clipped to `limit` pixels from
    /// `x`.
    fn proportional_line(
        &self,
        frame: &mut Frame,
        at: (u32, u32),
        text: &str,
        colour: Rgb,
        (limit, marks): (u32, &[Mark]),
    ) {
        let (x, y) = at;
        let band = (i64::from(x), i64::from(x) + i64::from(limit));
        // Where each character starts, in 64ths of a pixel, and the end.
        let mut starts = Vec::new();
        let mut pos = 0usize;
        for ch in text.chars() {
            starts.push(pos);
            pos += self.advance(ch);
        }
        starts.push(pos);
        let px = |sub: usize| u32::try_from(sub.div_ceil(SUBPIXELS)).unwrap_or(u32::MAX);
        for m in marks {
            let end = m.end.min(starts.len() - 1);
            if m.start >= end {
                continue;
            }
            let left = x + u32::try_from(starts[m.start] / SUBPIXELS).unwrap_or(0);
            let right = (x + px(starts[end])).min(x + limit);
            if right > left {
                frame.fill(left, y, right - left, self.cell_h, self.mark_background(m));
                if m.current {
                    self.outline(frame, left, y, right - left);
                }
            }
        }
        for (ci, ch) in text.chars().enumerate() {
            let a = self.advance(ch);
            if a > 0 {
                let left = x + u32::try_from((starts[ci] + SUBPIXELS / 2) / SUBPIXELS).unwrap_or(0);
                let face = self.proportional_face(ch);
                let ink = if marks.iter().any(|m| m.start <= ci && ci < m.end) {
                    self.theme.find_text
                } else {
                    colour
                };
                self.draw_face(frame, face, ch, (left, y), band, ink);
            }
        }
    }

    /// Draw one line of a rich block's body: code on the cell grid, anything
    /// else proportional.
    fn body_line(
        &self,
        frame: &mut Frame,
        at: (u32, u32),
        text: &str,
        kind: Style2,
        colour: Rgb,
        limit: u32,
    ) {
        self.body_line_marked(frame, at, text, (kind, colour), (limit, &[]));
    }

    /// As [`body_line`](Self::body_line), with the characters in `marks` drawn as
    /// find matches.
    fn body_line_marked(
        &self,
        frame: &mut Frame,
        at: (u32, u32),
        text: &str,
        (kind, colour): (Style2, Rgb),
        (limit, marks): (u32, &[Mark]),
    ) {
        if kind == Style2::Code {
            self.text_line_marked(frame, at.0, at.1, text, colour, marks);
        } else {
            self.proportional_line(frame, at, text, colour, (limit, marks));
        }
    }

    /// The lines of a rich block's body, wrapped to `width` cells, with a flag
    /// for the code background. Code is cut at the cell; other text is wrapped
    /// by the widths of its glyphs.
    fn body(&self, block: &Track0Block, width: usize) -> (Vec<String>, Style2) {
        let limit = width * self.cell_w as usize * SUBPIXELS;
        let flow = |t: &str| wrap_by(t, limit, &|c| self.advance(c));
        match block {
            Track0Block::Paragraph(t) => (flow(t), Style2::Plain),
            Track0Block::Heading { text, .. } => (flow(text), Style2::Heading),
            Track0Block::Code { text, stripped, .. } => {
                let mut lines: Vec<String> = text.lines().map(|l| clip(l, width)).collect();
                if *stripped > 0 {
                    lines.push(format!("{stripped} bidi controls removed"));
                }
                (lines, Style2::Code)
            }
            // Until an image is decoded, and while images are locked, the alt
            // text stands in for it. The data frames have no row.
            Track0Block::ImageStart { alt, .. } => (flow(alt), Style2::Plain),
            Track0Block::ImageData { .. } => (Vec::new(), Style2::Plain),
        }
    }

    /// Where each foreground block sits in the flow of the content, top down:
    /// the pixel row it starts at and its height. A grid block is its history
    /// above its screen. The newest cycle's strip is below the editor, so it is
    /// not in the flow; the strip of an older cycle sits where that cycle's
    /// output ended: one row, collapsed, with the count and nothing the
    /// producer wrote.
    fn placement<'a>(
        &self,
        doc: &'a Document,
        columns: usize,
        strips: &'a [crate::strip::Strip<'a>],
        newest_cycle: Option<u32>,
    ) -> Vec<Placed<'a>> {
        let mut out = Vec::new();
        let mut y = 0u32;
        let mut placed: Vec<u32> = Vec::new();
        let live = doc.blocks().len().checked_sub(1);
        for (index, b) in doc.blocks().iter().enumerate() {
            match b {
                Block::Grid(g) => {
                    let rows = g.history_rows() + g.used_rows(Some(index) == live);
                    let h = u32::try_from(rows)
                        .unwrap_or(u32::MAX)
                        .saturating_mul(self.cell_h);
                    out.push(Placed {
                        index,
                        top: y,
                        height: h,
                        part: Part::Grid(g),
                    });
                    y = y.saturating_add(h);
                }
                Block::Rich(r) if !r.background => {
                    let h = self.rich_height(r, columns);
                    out.push(Placed {
                        index,
                        top: y,
                        height: h,
                        part: Part::Rich(r),
                    });
                    y = y.saturating_add(h);
                }
                Block::Rich(r) => {
                    if Some(r.cycle) == newest_cycle || placed.contains(&r.cycle) {
                        continue;
                    }
                    if let Some(strip) = strips.iter().find(|s| s.cycle == r.cycle) {
                        placed.push(r.cycle);
                        let rows = if strip.open {
                            1 + strip.blocks.len()
                        } else {
                            1
                        };
                        let h = u32::try_from(rows)
                            .unwrap_or(u32::MAX)
                            .saturating_mul(self.cell_h);
                        out.push(Placed {
                            index,
                            top: y,
                            height: h,
                            part: Part::Strip(strip),
                        });
                        y = y.saturating_add(h);
                    }
                }
            }
        }
        out
    }

    /// The rows a grid block takes in the flow, from its placement.
    fn placed_rows(&self, p: &Placed<'_>) -> usize {
        usize::try_from(p.height / self.cell_h.max(1)).unwrap_or(usize::MAX)
    }

    /// The height in pixels of the whole flow of foreground blocks, `columns`
    /// cells wide: what a view of the content area scrolls over.
    #[must_use]
    pub fn content_height(&self, doc: &Document, columns: usize) -> u32 {
        let strips = doc.strips();
        let newest = strips.last().map(|s| s.cycle);
        self.placement(doc, columns, &strips, newest)
            .last()
            .map_or(0, |p| p.top.saturating_add(p.height))
    }

    /// The text of the grid rows in view when the view starts `view_top` pixels
    /// down a flow `columns` cells wide and `area` pixels tall, top down, with
    /// trailing blanks removed. A row of a rich block or a strip is not text
    /// of a grid, so it is left out. This is what a search or a copy needs to
    /// read the rows the user sees, and what tests read.
    #[must_use]
    pub fn view_rows(
        &self,
        doc: &Document,
        columns: usize,
        view_top: u32,
        area: u32,
    ) -> Vec<String> {
        let strips = doc.strips();
        let newest = strips.last().map(|s| s.cycle);
        let end = view_top.saturating_add(area);
        let mut out = Vec::new();
        for p in self.placement(doc, columns, &strips, newest) {
            let Part::Grid(g) = p.part else { continue };
            let hist = g.history_rows();
            for k in 0..self.placed_rows(&p) {
                let y = p.top.saturating_add(
                    u32::try_from(k)
                        .unwrap_or(u32::MAX)
                        .saturating_mul(self.cell_h),
                );
                if y + self.cell_h <= view_top || y >= end {
                    continue;
                }
                let cells = if k < hist {
                    g.history_row(k)
                } else {
                    g.screen().row(k - hist)
                };
                let line: String = cells
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !c.wide_cont)
                    .map(|c| c.character)
                    .collect();
                out.push(line.trim_end().to_owned());
            }
        }
        out
    }

    /// Where the live grid block's screen starts in the flow, in pixels from the
    /// top of the flow: the newest block, when it is a grid. A pointer over
    /// the content area maps into that block's cells from here.
    #[must_use]
    pub fn live_screen_top(&self, doc: &Document, columns: usize) -> Option<u32> {
        let strips = doc.strips();
        let newest = strips.last().map(|s| s.cycle);
        let flow = self.placement(doc, columns, &strips, newest);
        let last = flow.last()?;
        let Part::Grid(g) = last.part else {
            return None;
        };
        if !std::ptr::eq(
            doc.blocks().last().and_then(|b| match b {
                Block::Grid(x) => Some(&**x),
                Block::Rich(_) => None,
            })?,
            g,
        ) {
            return None;
        }
        let hist = u32::try_from(g.history_rows()).unwrap_or(u32::MAX);
        Some(last.top.saturating_add(hist.saturating_mul(self.cell_h)))
    }

    /// The content region, `size` pixels, `cells` cells wide: the part of the flow
    /// at `scroll`, the live cursor, and the scroll bar when the flow is taller.
    fn content_region(
        &self,
        size: (u32, u32),
        doc: &Document,
        cells: u32,
        scroll: Scroll,
        selection: Option<&Selection>,
        (cursor, overlay): (Option<CursorState>, Option<&Overlay>),
    ) -> Frame {
        let (aw, ah) = size;
        let strips = doc.strips();
        let mut region = Frame::new(aw, ah, self.theme.background);
        let columns = usize::try_from(cells).unwrap_or(1);
        let newest_cycle = strips.last().map(|s| s.cycle);
        let total = self.content_height(doc, columns);
        let view_top = scroll.top(total, ah);
        self.paint_content(
            &mut region,
            doc,
            columns,
            (&strips, newest_cycle),
            (view_top, cursor, overlay),
        );
        if let Some(sel) = selection {
            self.paint_selection(&mut region, &self.flow(doc, columns), sel, view_top);
        }
        if let Some((top, height)) = scroll.thumb(total, ah) {
            let bar = (2 * self.scale).min(aw);
            region.fill(aw - bar, top, bar, height, self.theme.border);
        }
        if let Some(o) = overlay {
            self.paint_find_bar(&mut region, o);
        }
        region
    }

    /// The find bar, across the top row of the content area: the terminal's own
    /// text, drawn after the content, so nothing a program writes reaches it.
    fn paint_find_bar(&self, region: &mut Frame, o: &Overlay) {
        let t = &self.theme;
        let (w, h) = (region.width, self.cell_h.min(region.height));
        region.fill(0, 0, w, h, t.editor);
        region.fill(
            0,
            h.saturating_sub(self.scale),
            w,
            self.scale.min(h),
            t.border,
        );
        let cell = self.cell_w;
        let label = "Find:";
        self.text_line(region, cell, 0, label, t.prompt);
        let status_w = u32::try_from(o.status.chars().count()).unwrap_or(0) * cell;
        let status_x = w.saturating_sub(status_w + cell);
        let colour = if o.missing { t.label } else { t.margin_text };
        self.text_line(region, status_x, 0, &o.status, colour);
        // The end of the query is the part in view when it is longer than the room.
        let left = cell * (u32::try_from(label.len()).unwrap_or(5) + 2);
        let room = usize::try_from(status_x.saturating_sub(left) / cell.max(1))
            .unwrap_or(0)
            .saturating_sub(2);
        let count = o.query.chars().count();
        let shown: String = o.query.chars().skip(count.saturating_sub(room)).collect();
        self.text_line(region, left, 0, &shown, t.foreground);
        let caret = left + u32::try_from(shown.chars().count()).unwrap_or(0) * cell;
        region.fill(
            caret,
            self.scale,
            self.scale * 2,
            h.saturating_sub(self.scale * 3),
            t.foreground,
        );
    }

    /// The rows of text in the flow that can be searched, top down: every row
    /// of a grid block (its history and its screen), and every line of a rich
    /// block as it is drawn. A grid row has its trailing blanks removed, except a
    /// row the terminal wrapped, which keeps them so the next row joins on at the
    /// right column. A strip's label is
    /// the terminal's own text and an image has no text, so neither is a row.
    #[must_use]
    pub fn search_rows(&self, doc: &Document, columns: usize) -> Vec<SearchRow> {
        let strips = doc.strips();
        let newest = strips.last().map(|s| s.cycle);
        let mut out = Vec::new();
        for p in self.placement(doc, columns, &strips, newest) {
            match p.part {
                Part::Grid(g) => {
                    let hist = g.history_rows();
                    for k in 0..self.placed_rows(&p) {
                        let cells = if k < hist {
                            g.history_row(k)
                        } else {
                            g.screen().row(k - hist)
                        };
                        let text: String = cells
                            .unwrap_or_default()
                            .iter()
                            .filter(|c| !c.wide_cont)
                            .map(|c| c.character)
                            .collect();
                        let wraps = g.row_wraps(k);
                        out.push(SearchRow {
                            block: p.index,
                            row: k,
                            top: p.top.saturating_add(
                                u32::try_from(k)
                                    .unwrap_or(u32::MAX)
                                    .saturating_mul(self.cell_h),
                            ),
                            height: self.cell_h,
                            text: if wraps {
                                text
                            } else {
                                text.trim_end().to_owned()
                            },
                            wraps,
                        });
                    }
                }
                Part::Rich(r) if r.image.is_none() => {
                    let (lines, _) = self.body(&r.content, columns.saturating_sub(2).max(1));
                    let first = p.top.saturating_add(self.scale).saturating_add(self.cell_h);
                    for (i, text) in lines.into_iter().enumerate() {
                        out.push(SearchRow {
                            block: p.index,
                            row: i,
                            top: first.saturating_add(
                                u32::try_from(i)
                                    .unwrap_or(u32::MAX)
                                    .saturating_mul(self.cell_h),
                            ),
                            height: self.cell_h,
                            text,
                            wraps: false,
                        });
                    }
                }
                Part::Rich(_) | Part::Strip(_) => {}
            }
        }
        out
    }

    /// Paint the content region: the part of the flow of foreground blocks that
    /// starts `view_top` pixels down, so the region is the view.
    fn paint_content(
        &self,
        region: &mut Frame,
        doc: &Document,
        columns: usize,
        (strips, newest_cycle): (&[crate::strip::Strip<'_>], Option<u32>),
        (view_top, cursor, overlay): (u32, Option<CursorState>, Option<&Overlay>),
    ) {
        let view_end = view_top.saturating_add(region.height);
        // Only the newest grid is the program's live screen, so only it can show a cursor.
        let live = doc.blocks().iter().rev().find_map(|b| match b {
            Block::Grid(g) => Some(&**g),
            Block::Rich(_) => None,
        });
        for p in self.placement(doc, columns, strips, newest_cycle) {
            let end = p.top.saturating_add(p.height);
            if end <= view_top || p.top >= view_end {
                continue;
            }
            match p.part {
                Part::Grid(g) => {
                    let hist = g.history_rows();
                    let first = (view_top.saturating_sub(p.top)) / self.cell_h;
                    let last = (view_end - p.top).div_ceil(self.cell_h);
                    let rows = self.placed_rows(&p);
                    for k in first..last.min(u32::try_from(rows).unwrap_or(u32::MAX)) {
                        let k = k as usize;
                        let cells = if k < hist {
                            g.history_row(k)
                        } else {
                            g.screen().row(k - hist)
                        };
                        let Some(cells) = cells else { continue };
                        #[cfg(feature = "links-clipboard")]
                        let hits = if k < hist {
                            crate::linkhit::history_links(g.screen(), k)
                        } else {
                            crate::linkhit::row_links(g.screen(), k - hist)
                        };
                        let y = i64::from(p.top)
                            + i64::try_from(k).unwrap_or(0) * i64::from(self.cell_h)
                            - i64::from(view_top);
                        self.paint_row_at(
                            region,
                            y,
                            cells,
                            #[cfg(feature = "links-clipboard")]
                            &hits,
                            Marks::of(overlay, p.index).row(k),
                        );
                    }
                    if let (Some(state), Some(l)) = (cursor, live)
                        && std::ptr::eq(l, g)
                    {
                        // The screen is drawn again with the cursor on it, clipped to the view.
                        let screen_h = u32::try_from(g.size().1)
                            .unwrap_or(0)
                            .saturating_mul(self.cell_h);
                        let mut screen =
                            Frame::new(region.width, screen_h.max(1), self.theme.background);
                        self.paint_grid_marked(&mut screen, 0, 0, g, Marks::of(overlay, p.index));
                        self.paint_cursor(&mut screen, 0, 0, g, state);
                        let top = i64::from(p.top)
                            + i64::try_from(hist).unwrap_or(0) * i64::from(self.cell_h)
                            - i64::from(view_top);
                        Self::blit_clipped(region, &screen, top);
                    }
                }
                Part::Rich(r) => {
                    let mut part = Frame::new(region.width, p.height.max(1), self.theme.background);
                    self.paint_rich_marked(
                        &mut part,
                        (0, 0),
                        columns,
                        r,
                        Marks::of(overlay, p.index),
                    );
                    Self::blit_clipped(region, &part, i64::from(p.top) - i64::from(view_top));
                }
                Part::Strip(s) => {
                    let mut part = Frame::new(region.width, p.height.max(1), self.theme.background);
                    self.paint_flow_strip(&mut part, 0, columns, s);
                    Self::blit_clipped(region, &part, i64::from(p.top) - i64::from(view_top));
                }
            }
        }
    }

    /// Paint one row of cells at pixel row `y` of `region`, which may be partly
    /// above or below it: a row that is not wholly in view is cut.
    fn paint_row_at(
        &self,
        region: &mut Frame,
        y: i64,
        cells: &[Cell],
        #[cfg(feature = "links-clipboard")] hits: &[crate::linkhit::Hit],
        marks: &[Mark],
    ) {
        let mut line = Frame::new(region.width, self.cell_h, self.theme.background);
        self.paint_cells(&mut line, 0, 0, cells, marks);
        #[cfg(feature = "links-clipboard")]
        self.paint_link_marks(&mut line, 0, 0, hits);
        Self::blit_clipped(region, &line, y);
    }

    /// Copy `src` into `region` with its top at row `y`, which may be negative or
    /// past the bottom: only the rows that land inside are copied.
    fn blit_clipped(region: &mut Frame, src: &Frame, y: i64) {
        for row in 0..src.height {
            let to = y + i64::from(row);
            let Ok(to) = u32::try_from(to) else { continue };
            if to >= region.height {
                break;
            }
            for col in 0..src.width.min(region.width) {
                if let Some(c) = src.pixel(col, row) {
                    region.put(col, to, c);
                }
            }
        }
    }

    /// The selectable lines of the content flow, `columns` cells wide, in the
    /// places [`paint_content`](Self::paint_content) puts them: each grid row,
    /// and each wrapped line of a rich block's text. Strips and images have no
    /// line, and the container's label and the note on removed bidi controls are
    /// the terminal's own and are not text to copy.
    #[must_use]
    pub fn flow(&self, doc: &Document, columns: usize) -> Vec<FlowLine> {
        let mut out = Vec::new();
        let inner = columns.saturating_sub(2).max(1);
        let strips = doc.strips();
        let newest_cycle = strips.last().map(|s| s.cycle);
        for p in self.placement(doc, columns, &strips, newest_cycle) {
            match p.part {
                Part::Grid(g) => {
                    let hist = g.history_rows();
                    for k in 0..self.placed_rows(&p) {
                        let cells = if k < hist {
                            g.history_row(k)
                        } else {
                            g.screen().row(k - hist)
                        };
                        let text: String = cells
                            .unwrap_or_default()
                            .iter()
                            .filter(|c| !c.wide_cont)
                            .map(|c| c.character)
                            .collect();
                        out.push(FlowLine {
                            block: p.index,
                            top: p.top + u32::try_from(k).unwrap_or(0) * self.cell_h,
                            height: self.cell_h,
                            left: 0,
                            text: text.trim_end().to_owned(),
                            grid: true,
                            kind: CopyKind::Command,
                            join: Join::Break,
                        });
                    }
                }
                Part::Rich(r) => {
                    if r.image.is_none() {
                        let first = p.top + self.scale + self.cell_h;
                        self.flow_rich(&mut out, p.index, &r.content, inner, first);
                    }
                }
                // A strip of an older cycle takes its rows and has no text of
                // its own to select.
                Part::Strip(_) => {}
            }
        }
        out
    }

    fn flow_rich(
        &self,
        out: &mut Vec<FlowLine>,
        block: usize,
        content: &Track0Block,
        inner: usize,
        first: u32,
    ) {
        let limit = inner * self.cell_w as usize * SUBPIXELS;
        let mut rows: Vec<(String, bool, CopyKind, Join)> = Vec::new();
        let mut flowed = |text: &str| {
            for paragraph in text.split('\n') {
                let lines = wrap_by(paragraph, limit, &|c| self.advance(c));
                let last = lines.len().saturating_sub(1);
                let mut seen = 0usize;
                for (i, line) in lines.into_iter().enumerate() {
                    seen += line.chars().filter(|c| !c.is_whitespace()).count();
                    let join = if i == last {
                        Join::Break
                    } else if next_after_nonspace(paragraph, seen).is_some_and(char::is_whitespace)
                    {
                        Join::Space
                    } else {
                        Join::Cut
                    };
                    rows.push((line, false, CopyKind::Text, join));
                }
            }
        };
        match content {
            Track0Block::Paragraph(t) => flowed(t),
            Track0Block::Heading { text, .. } => flowed(text),
            Track0Block::ImageStart { alt, .. } => flowed(alt),
            Track0Block::Code { text, .. } => {
                for l in text.lines() {
                    rows.push((clip(l, inner), true, CopyKind::Code, Join::Break));
                }
            }
            Track0Block::ImageData { .. } => {}
        }
        for (i, (text, grid, kind, join)) in rows.into_iter().enumerate() {
            out.push(FlowLine {
                block,
                top: first + u32::try_from(i).unwrap_or(0) * self.cell_h,
                height: self.cell_h,
                left: self.cell_w,
                text,
                grid,
                kind,
                join,
            });
        }
    }

    /// Where, in pixels from the line's left edge, the character boundary
    /// `index` of `line` is.
    #[must_use]
    pub fn char_left(&self, line: &FlowLine, index: usize) -> u32 {
        let mut pos = 0usize;
        for c in line.text.chars().take(index) {
            pos += if line.grid {
                cells(c) * self.cell_w as usize * SUBPIXELS
            } else {
                self.advance(c)
            };
        }
        u32::try_from((pos + SUBPIXELS / 2) / SUBPIXELS).unwrap_or(u32::MAX)
    }

    /// The character boundary of `line` nearest `x` pixels from its left edge.
    #[must_use]
    pub fn char_at(&self, line: &FlowLine, x: u32) -> usize {
        let target = x as usize * SUBPIXELS;
        let mut pos = 0usize;
        for (i, c) in line.text.chars().enumerate() {
            let a = if line.grid {
                cells(c) * self.cell_w as usize * SUBPIXELS
            } else {
                self.advance(c)
            };
            if target < pos + a / 2 + a % 2 {
                return i;
            }
            pos += a;
        }
        line.text.chars().count()
    }

    /// Tint the characters `sel` covers: each pixel moves a third of the way
    /// towards the foreground colour, so text drawn in the foreground colour is
    /// unchanged and what is behind it is lifted. The selection is the
    /// terminal's, not the program's, so it does not come from the program's
    /// colours.
    fn paint_selection(
        &self,
        region: &mut Frame,
        lines: &[FlowLine],
        sel: &Selection,
        view_top: u32,
    ) {
        let fg = self.theme.foreground;
        let last = sel.ordered().1.line;
        for (i, line) in lines.iter().enumerate() {
            let Some((from, to)) = sel.span_in(lines, i) else {
                continue;
            };
            let start = line.left + self.char_left(line, from);
            let mut end = line.left + self.char_left(line, to);
            if i < last && to >= line.text.chars().count() {
                // A line the selection runs past is tinted a little beyond its end.
                end += self.cell_w / 2;
            }
            if line.top + line.height <= view_top {
                continue;
            }
            let top = line.top.saturating_sub(view_top);
            let bottom = line.top + line.height - view_top;
            for y in top..bottom {
                for x in start..end {
                    if let Some(p) = region.pixel(x, y) {
                        let mixed: Rgb = std::array::from_fn(|k| {
                            u8::try_from((2 * u16::from(p[k]) + u16::from(fg[k])) / 3)
                                .unwrap_or(p[k])
                        });
                        region.put(x, y, mixed);
                    }
                }
            }
        }
    }

    /// Paint the strip of an older prompt cycle at row `y` of the content
    /// region, `columns` cells wide, and return its height in pixels: one row
    /// when collapsed, a header and a row per block when the user opened it.
    fn paint_flow_strip(
        &self,
        region: &mut Frame,
        y: u32,
        columns: usize,
        strip: &crate::strip::Strip<'_>,
    ) -> u32 {
        let t = &self.theme;
        let rows = if strip.open {
            1 + strip.blocks.len()
        } else {
            1
        };
        let height = u32::try_from(rows).unwrap_or(u32::MAX) * self.cell_h;
        let width = u32::try_from(columns).unwrap_or(1) * self.cell_w;
        region.fill(0, y, width, height, t.container);
        let inner = columns.saturating_sub(2).max(1);
        self.text_line(region, self.cell_w, y, &strip.label(), t.label);
        if strip.open {
            for (i, block) in strip.blocks.iter().enumerate() {
                let (lines, kind) = self.body(&block.content, inner);
                let first = lines.first().map_or("", String::as_str);
                let row = y + u32::try_from(i + 1).unwrap_or(u32::MAX) * self.cell_h;
                let limit = u32::try_from(inner).unwrap_or(1) * self.cell_w;
                self.body_line(region, (self.cell_w, row), first, kind, t.foreground, limit);
            }
        }
        height
    }

    /// The height in pixels of a rich block's container.
    #[must_use]
    pub fn rich_height(&self, block: &RichBlock, columns: usize) -> u32 {
        if let Some(pixels) = &block.image {
            return image_draw::block_height(
                pixels,
                columns,
                (self.cell_w, self.cell_h),
                self.scale,
            );
        }
        let (lines, _) = self.body(&block.content, columns.saturating_sub(2).max(1));
        2 * self.scale + (1 + u32::try_from(lines.len()).unwrap_or(0)) * self.cell_h
    }

    /// The size a decoded image is drawn at in a container `columns` wide; see
    /// [`image_draw::size`].
    #[must_use]
    pub fn image_size(&self, pixels: &crate::images::Pixels, columns: usize) -> (u32, u32) {
        image_draw::size(pixels, columns, (self.cell_w, self.cell_h))
    }

    /// Paint a rich block in its container, `columns` cells wide, with its top
    /// left at `x`, `y`. The container always carries [`CONTAINER_LABEL`].
    pub fn paint_rich(&self, frame: &mut Frame, x: u32, y: u32, columns: usize, block: &RichBlock) {
        self.paint_rich_marked(frame, (x, y), columns, block, Marks::default());
    }

    fn paint_rich_marked(
        &self,
        frame: &mut Frame,
        (x, y): (u32, u32),
        columns: usize,
        block: &RichBlock,
        marks: Marks<'_>,
    ) {
        let t = &self.theme;
        let width = u32::try_from(columns).unwrap_or(1) * self.cell_w;
        let height = self.rich_height(block, columns);
        frame.fill(x, y, width, height, t.border);
        frame.fill(
            x + self.scale,
            y + self.scale,
            width.saturating_sub(2 * self.scale),
            height.saturating_sub(2 * self.scale),
            t.container,
        );
        let inner = x + self.cell_w;
        let mut line_y = y + self.scale;
        self.text_line(frame, inner, line_y, CONTAINER_LABEL, t.label);
        line_y += self.cell_h;
        if let Some(pixels) = &block.image {
            image_draw::paint_block(
                frame,
                (inner, line_y),
                columns,
                (self.cell_w, self.cell_h),
                pixels,
            );
            return;
        }
        let (lines, kind) = self.body(&block.content, columns.saturating_sub(2).max(1));
        let limit = u32::try_from(columns.saturating_sub(2).max(1)).unwrap_or(1) * self.cell_w;
        for (i, line) in lines.iter().enumerate() {
            let colour = match kind {
                Style2::Heading => t.heading,
                _ => t.foreground,
            };
            if kind == Style2::Code {
                frame.fill(
                    x + self.scale,
                    line_y,
                    width.saturating_sub(2 * self.scale),
                    self.cell_h,
                    t.code,
                );
            }
            self.body_line_marked(
                frame,
                (inner, line_y),
                line,
                (kind, colour),
                (limit, marks.row(i)),
            );
            line_y += self.cell_h;
        }
    }

    /// The pixel box `(x, y, w, h)` of a layout rectangle. What lies right of
    /// the gutter is moved right by the gutter padding, so the text column
    /// starts clear of the divider; the header runs on to the window's edge.
    fn rect_px(&self, r: &Rect) -> (u32, u32, u32, u32) {
        let pad = self.gutter_pad();
        let edge = |col: u32| col * self.cell_w + if col > GUTTER_COLUMNS { pad } else { 0 };
        let left = if r.x >= GUTTER_COLUMNS {
            r.x * self.cell_w + pad
        } else {
            r.x * self.cell_w
        };
        let right = edge(r.x + r.w);
        (
            left,
            r.y * self.cell_h,
            right.saturating_sub(left),
            r.h * self.cell_h,
        )
    }

    /// Copy `src` into `frame` with its top left at `x`, `y`. `src` is the
    /// whole of one region, so what was painted into it cannot reach outside.
    fn blit(frame: &mut Frame, src: &Frame, x: u32, y: u32) {
        for row in 0..src.height {
            for col in 0..src.width {
                if let Some(c) = src.pixel(col, row) {
                    frame.put(x + col, y + row, c);
                }
            }
        }
    }

    /// The text of `text` that fits in `cells` columns, and the columns it takes. A two-column
    /// character that would cross the edge is left out.
    fn clip_cells(text: &str, cells: usize) -> (String, usize) {
        let mut used = 0usize;
        let mut out = String::new();
        for ch in text.chars() {
            let w = grapheme_display_width(ch, &[]);
            if used + w > cells {
                break;
            }
            used += w;
            out.push(ch);
        }
        (out, used)
    }

    /// The columns `text` takes.
    fn cells_of(text: &str) -> usize {
        text.chars().map(|c| grapheme_display_width(c, &[])).sum()
    }

    /// Draw composing text from `(x, y)`, at most `room` columns wide: the text on the editor's
    /// background, underlined across what is drawn (a thicker line under the clause the input
    /// method has selected), and a bar caret where the input method puts it. Returns the
    /// columns used.
    fn paint_preedit_run(
        &self,
        frame: &mut Frame,
        (x, y): (u32, u32),
        room: usize,
        (preedit, focused, inset): (&PreeditView, bool, u32),
    ) -> usize {
        let t = &self.theme;
        let (shown, used) = Self::clip_cells(&preedit.text, room);
        let width = u32::try_from(used).unwrap_or(0) * self.cell_w;
        let top = inset.min(self.cell_h / 2);
        frame.fill(x, y + top, width, self.cell_h - top, t.editor);
        self.text_line(frame, x, y, &shown, t.foreground);
        let line = if focused { t.prompt } else { t.muted };
        let thin = self.scale.max(1).min(self.cell_h);
        let thick = (2 * self.scale).max(2).min(self.cell_h);
        let base = y + self.cell_h;
        let selected = preedit
            .cursor
            .filter(|&(a, b)| a < b)
            .map(|(a, b)| (a.min(preedit.text.len()), b.min(preedit.text.len())));
        frame.fill(x, base - thin, width, thin, line);
        if let Some((a, b)) = selected {
            let before = Self::clip_cells(preedit.text.get(..a).unwrap_or(""), used).1;
            let inside = Self::clip_cells(preedit.text.get(a..b).unwrap_or(""), used - before).1;
            frame.fill(
                x + u32::try_from(before).unwrap_or(0) * self.cell_w,
                base - thick,
                u32::try_from(inside).unwrap_or(0) * self.cell_w,
                thick,
                line,
            );
        }
        if let Some((a, _)) = preedit.cursor.filter(|&(a, b)| a == b && focused) {
            let before = Self::clip_cells(preedit.text.get(..a).unwrap_or(""), used).1;
            frame.fill(
                x + u32::try_from(before).unwrap_or(0) * self.cell_w,
                y,
                self.scale.max(2),
                self.cell_h,
                t.prompt,
            );
        }
        used
    }

    /// Draw the line of the box the caret is on while text is being composed at `col`: the
    /// text before the caret, the composing text, then the rest of the line moved to the right,
    /// all clipped to `room` columns.
    fn paint_composing_line(
        &self,
        frame: &mut Frame,
        (x, y): (u32, u32),
        room: usize,
        (line, col): (&str, usize),
        preedit: (&PreeditView, bool),
    ) {
        let split = line
            .char_indices()
            .nth(col)
            .map_or(line.len(), |(at, _)| at);
        let (before, after) = line.split_at(split);
        let (before, before_cells) = Self::clip_cells(before, room);
        self.text_line(frame, x, y, &before, self.theme.foreground);
        let used = self.paint_preedit_run(
            frame,
            (
                x + u32::try_from(before_cells).unwrap_or(0) * self.cell_w,
                y,
            ),
            room - before_cells,
            (preedit.0, preedit.1, self.scale),
        );
        let left = before_cells + used;
        let (after, _) = Self::clip_cells(after, room - left);
        self.text_line(
            frame,
            x + u32::try_from(left).unwrap_or(0) * self.cell_w,
            y,
            &after,
            self.theme.foreground,
        );
    }

    /// Draw composing text over the grid, from pixel `(x, y)` (the program's cursor) to no
    /// further than pixel `right`. The program's cells under it are covered while it is there.
    pub fn paint_preedit(
        &self,
        frame: &mut Frame,
        (x, y): (u32, u32),
        right: u32,
        preedit: &PreeditView,
        focused: bool,
    ) {
        let room = (right.saturating_sub(x) / self.cell_w.max(1)) as usize;
        if room == 0 {
            return;
        }
        self.paint_preedit_run(frame, (x, y), room, (preedit, focused, 0));
    }

    /// How wide `text` is drawn, in columns.
    #[must_use]
    pub fn preedit_columns(text: &str) -> u32 {
        u32::try_from(Self::cells_of(text)).unwrap_or(u32::MAX)
    }

    /// Paint the input box into `frame` at `(ex, ey, ew, eh)` pixels. Its text is
    /// the user's own and its status is fixed text: no program output.
    fn paint_input_box(
        &self,
        frame: &mut Frame,
        (ex, ey, ew, eh): (u32, u32, u32, u32),
        editor_rows: u32,
        input: Option<&InputView>,
    ) {
        let t = &self.theme;
        // A status is one row of muted text: no field, border, mark or caret,
        // so nothing looks typeable while the keys go elsewhere.
        if let Some(InputView {
            status: Some(s), ..
        }) = input
        {
            let room = (ew / self.cell_w.max(1)).saturating_sub(2) as usize;
            let shown: String = s.chars().take(room.max(1)).collect();
            self.text_line(frame, ex + self.cell_w, ey, &shown, t.muted);
            return;
        }
        // The box stops a gutter padding short of the right edge, as the text
        // column starts one right of the gutter.
        let ew = ew.saturating_sub(self.gutter_pad());
        let focused = input.is_some_and(|i| i.focused);
        let edge = if focused { t.prompt } else { t.border };
        frame.fill(ex, ey, ew, eh, edge);
        frame.fill(
            ex + self.scale,
            ey + self.scale,
            ew.saturating_sub(2 * self.scale),
            eh.saturating_sub(2 * self.scale),
            t.editor,
        );
        let text_x = ex + 3 * self.cell_w;
        let room = (ew / self.cell_w.max(1)).saturating_sub(4) as usize;
        match input {
            Some(view) => {
                self.text_line(frame, ex + self.cell_w, ey, ">", t.prompt);
                let rows = (editor_rows.saturating_sub(1) as usize).max(1);
                let first = view.visible_first_line(rows);
                let composing = view.preedit.as_ref().zip(view.caret);
                for (i, line) in view.lines.iter().skip(first).take(rows).enumerate() {
                    let y = ey + u32::try_from(i).unwrap_or(0) * self.cell_h;
                    if let Some((preedit, (_, col))) =
                        composing.filter(|&(_, (at, _))| at == first + i)
                    {
                        self.paint_composing_line(
                            frame,
                            (text_x, y),
                            room.max(1),
                            (line, col),
                            (preedit, focused),
                        );
                    } else {
                        let shown: String = line.chars().take(room.max(1)).collect();
                        self.text_line(frame, text_x, y, &shown, t.foreground);
                    }
                }
                let caret = view
                    .caret_cell()
                    .filter(|_| view.preedit.is_none())
                    .filter(|&(line, _)| focused && (first..first + rows).contains(&line));
                if let Some(hint) = &view.hint {
                    let shown: String = hint.chars().take(room.max(1)).collect();
                    let y = ey + eh.saturating_sub(self.cell_h + self.scale);
                    self.text_line(frame, text_x, y, &shown, t.muted);
                }
                if let Some((line, column)) = caret {
                    let y = ey + u32::try_from(line - first).unwrap_or(0) * self.cell_h;
                    let x = text_x.saturating_add(column.saturating_mul(self.cell_w));
                    frame.fill(x, y, self.scale.max(2), self.cell_h, t.prompt);
                }
            }
            None => self.text_line(frame, ex + self.cell_w, ey, ">", t.prompt),
        }
    }

    /// Paint a window: the trust margin, the content above the editor, the
    /// editor, and the background strip below it.
    ///
    /// Every part has a rectangle from [`layout`], and a producer's output
    /// reaches only two of them: the content area and the strip. Each is
    /// painted into a frame of its own size and copied in, so nothing it holds
    /// can spill. The margin is painted last, from the terminal's own text and
    /// colours, as well: nothing a producer writes is an input to a margin
    /// pixel. The editor is fixed (a prompt and no producer text).
    ///
    /// The strip drawn is the newest cycle's: collapsed it is one row with
    /// `background output (N)`, and open it is one row per block, each the
    /// kind and the first line, clipped. Background blocks are never part of
    /// the content flow.
    #[must_use]
    pub fn render_window(&self, doc: &Document, win: &Window) -> Frame {
        self.render_window_with(doc, win, &[])
    }

    /// Paint a window with the toasts the notifier is showing.
    ///
    /// Toasts are drawn only in the notification area, top down, newest last.
    /// Each is painted into a frame of its own box size and copied in, so no
    /// glyph overflows into neighbouring chrome, and a toast that does not
    /// fit in what is left of the area is not drawn. An unsigned toast carries
    /// the unverified-output label above its text. The text is what the
    /// notifier composed: the fixed prefix, the pane label and the program's
    /// cleaned text.
    #[must_use]
    pub fn render_window_with(&self, doc: &Document, win: &Window, toasts: &[Toast]) -> Frame {
        self.render_window_cursor(doc, win, toasts, None)
    }

    /// Paint a window as [`render_window_with`](Self::render_window_with) does, with the cursor of
    /// the live grid drawn when `cursor` is given. The cursor is clipped to the content area, so
    /// it never reaches the margin, the editor or a toast.
    #[must_use]
    pub fn render_window_cursor(
        &self,
        doc: &Document,
        win: &Window,
        toasts: &[Toast],
        cursor: Option<CursorState>,
    ) -> Frame {
        self.render_window_scrolled(doc, win, toasts, Scroll::Follow, (cursor, None))
    }

    /// Paint a window with the content scrolled to `scroll`. At the bottom
    /// (`Scroll::Follow`) it is what [`render_window_cursor`](Self::render_window_cursor)
    /// draws. A scroll bar is drawn at the right edge of the content area when
    /// the flow is taller than the area; it is the terminal's own and nothing a
    /// producer writes is an input to it.
    #[must_use]
    pub fn render_window_scrolled(
        &self,
        doc: &Document,
        win: &Window,
        toasts: &[Toast],
        scroll: Scroll,
        (cursor, overlay): (Option<CursorState>, Option<&Overlay>),
    ) -> Frame {
        self.render_window_full(doc, win, toasts, None, scroll, (cursor, overlay))
    }

    /// Paint a window with its toasts and a text selection tinted in the
    /// content (see [`flow`](Self::flow) for the lines it is made of).
    #[must_use]
    pub fn render_window_selected(
        &self,
        doc: &Document,
        win: &Window,
        toasts: &[Toast],
        selection: Option<&Selection>,
    ) -> Frame {
        self.render_window_full(doc, win, toasts, selection, Scroll::Follow, (None, None))
    }

    /// Paint a window with its toasts, a tinted selection, the scroll and the
    /// live cursor.
    #[must_use]
    #[allow(clippy::too_many_lines, reason = "one pass over the window's areas")]
    pub fn render_window_full(
        &self,
        doc: &Document,
        win: &Window,
        toasts: &[Toast],
        selection: Option<&Selection>,
        scroll: Scroll,
        (cursor, overlay): (Option<CursorState>, Option<&Overlay>),
    ) -> Frame {
        self.render_window_input(doc, win, toasts, selection, scroll, (cursor, overlay), None)
    }

    /// Paint a window as [`render_window_full`](Self::render_window_full) does,
    /// with the input box showing `input`: the user's own text and caret or a
    /// fixed status line, never program output.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "one argument per part of the window"
    )]
    pub fn render_window_input(
        &self,
        doc: &Document,
        win: &Window,
        toasts: &[Toast],
        selection: Option<&Selection>,
        scroll: Scroll,
        (cursor, overlay): (Option<CursorState>, Option<&Overlay>),
        input: Option<&InputView>,
    ) -> Frame {
        let t = &self.theme;
        let strips = doc.strips();
        let newest = strips.last();
        let lay = layout(
            win.cols,
            win.rows,
            win.editor_row,
            win.editor_rows,
            newest.map(|s| (s.open, s.count())),
        );
        let px = |r: &Rect| self.rect_px(r);
        let mut frame = Frame::new(
            (win.cols * self.cell_w + self.gutter_pad()).max(1),
            (win.rows * self.cell_h).max(1),
            t.background,
        );

        // Content: foreground blocks only, top down, clipped to the area above the editor.
        let area = Rect {
            x: lay.content.x,
            y: lay.content.y,
            w: lay.content.w,
            h: lay.editor.y.saturating_sub(lay.content.y),
        };
        let (ax, ay, aw, ah) = px(&area);
        if aw > 0 && ah > 0 {
            let region =
                self.content_region((aw, ah), doc, area.w, scroll, selection, (cursor, overlay));
            Self::blit(&mut frame, &region, ax, ay);
        }

        // Toasts: in the notification area only.
        let note = notification_area(&lay);
        self.paint_toasts(&mut frame, px(&note), note.w, toasts);

        let (ex, ey, ew, eh) = px(&lay.editor);
        self.paint_input_box(&mut frame, (ex, ey, ew, eh), win.editor_rows, input);

        // Strip: below the editor.
        if let (StripPlacement::Placed(r), Some(strip)) = (lay.strip, newest) {
            let (sx, sy, sw, sh) = px(&r);
            let mut region = Frame::new(sw, sh, t.container);
            let width = usize::try_from(r.w).unwrap_or(1).saturating_sub(2);
            self.text_line(&mut region, self.cell_w, 0, &strip.label(), t.label);
            if strip.open {
                for (i, block) in strip.blocks.iter().enumerate() {
                    let (lines, kind) = self.body(&block.content, width.max(1));
                    let first = lines.first().map_or("", String::as_str);
                    let row = u32::try_from(i + 1).unwrap_or(u32::MAX) * self.cell_h;
                    let limit = u32::try_from(width.max(1)).unwrap_or(1) * self.cell_w;
                    self.body_line(
                        &mut region,
                        (self.cell_w, row),
                        first,
                        kind,
                        t.foreground,
                        limit,
                    );
                }
            }
            Self::blit(&mut frame, &region, sx, sy);
        }

        // Margin: last, from the terminal's own colours and text.
        for m in &lay.margin {
            let (mx, my, mw, mh) = px(m);
            frame.fill(mx, my, mw, mh, t.margin);
        }
        // The divider: one line along the inner edges of the margin, inside
        // it, under the header and right of the gutter below the header.
        let line = self.scale;
        let [gutter, header] = &lay.margin;
        let (gx, _, gw, gh) = px(gutter);
        let (_, hy, hw, hh) = px(header);
        if hh >= line {
            frame.fill(0, hy + hh - line, hw, line, t.divider);
        }
        if gw >= line {
            frame.fill(
                gx + gw - line,
                hy + hh,
                line,
                gh.saturating_sub(hh),
                t.divider,
            );
        }
        self.text_line(
            &mut frame,
            self.cell_w * GUTTER_COLUMNS.min(win.cols) + self.gutter_pad(),
            0,
            MARGIN_LABEL,
            t.margin_text,
        );
        frame
    }

    /// Paint the typed-line mark in the gutter of a window `frame`, on the row
    /// whose top is pixel `y`.
    pub fn paint_typed_mark(&self, frame: &mut Frame, y: u32) {
        self.text_line(frame, self.cell_w / 2, y, TYPED_MARK, self.theme.muted);
    }

    /// Whether pixel `(x, y)` is on the typed-line mark of the row whose top is
    /// pixel `top`: the gutter, short of its divider, on that row.
    #[must_use]
    pub fn on_typed_mark(&self, top: u32, (x, y): (u32, u32)) -> bool {
        let gutter = self.cell_w * GUTTER_COLUMNS;
        x < gutter.saturating_sub(self.scale) && y >= top && y < top + self.cell_h
    }

    /// Paint what the mark on the row whose top is pixel `row_top` means, in a
    /// box of the terminal's own on that row, starting `after` columns into the
    /// content (just past the typed text, so the tip does not cover it). When
    /// the row has no room left for it, the tip goes wholly above the row, or
    /// below it when there is no row above: never over the typed text's row,
    /// and never into the header strip of the trust margin.
    pub fn paint_typed_tip(&self, frame: &mut Frame, row_top: u32, after: u32) {
        let theme = &self.theme;
        let chars = u32::try_from(TYPED_TIP.chars().count()).unwrap_or(0);
        let width = (chars + 2) * self.cell_w;
        let height = self.cell_h + 2 * self.scale;
        let header = self.cell_h * HEADER_ROWS;
        let content = self.cell_w * GUTTER_COLUMNS + self.gutter_pad();
        let wanted = content + after * self.cell_w;
        let (left, top) = if wanted + width <= frame.width {
            (wanted, row_top.saturating_sub(self.scale).max(header))
        } else {
            let left = frame.width.saturating_sub(width).max(content);
            let above = row_top.checked_sub(height).filter(|t| *t >= header);
            (left, above.unwrap_or(row_top + self.cell_h))
        };
        let mut tip = Frame::new(width, height, theme.toast);
        tip.fill(0, 0, width, self.scale, theme.toast_border);
        tip.fill(
            0,
            height.saturating_sub(self.scale),
            width,
            self.scale,
            theme.toast_border,
        );
        tip.fill(0, 0, self.scale, height, theme.toast_border);
        tip.fill(
            width.saturating_sub(self.scale),
            0,
            self.scale,
            height,
            theme.toast_border,
        );
        self.text_line(
            &mut tip,
            self.cell_w,
            self.scale,
            TYPED_TIP,
            theme.foreground,
        );
        Self::blit(frame, &tip, left, top);
    }

    /// The toasts that fit in a notification area `columns` cells wide and `nh`
    /// pixels high, top down: each one's height in pixels and its wrapped lines.
    /// A toast that does not fit in what is left is not in the list, and neither
    /// is any after it. Painting and hit testing both use this, so a click lands
    /// on exactly what was drawn.
    fn toast_boxes(&self, toasts: &[Toast], columns: u32, nh: u32) -> Vec<(u32, Vec<String>)> {
        let inner = usize::try_from(columns)
            .unwrap_or(1)
            .saturating_sub(2)
            .max(1);
        let mut used = 0;
        let mut out = Vec::new();
        for toast in toasts {
            let mut lines = wrap_by(&toast.text, inner, &cells);
            lines.truncate(TOAST_MAX_LINES);
            let rows = u32::try_from(lines.len()).unwrap_or(0) + u32::from(toast.unsigned);
            let height = rows * self.cell_h + 2 * self.scale;
            if used + height > nh {
                break;
            }
            used += height;
            out.push((height, lines));
        }
        out
    }

    /// The toast drawn at pixel `(x, y)` of a window `win` showing `toasts`, as
    /// its index in `toasts`, or `None` if the point is on no toast.
    #[must_use]
    pub fn toast_at(&self, win: &Window, toasts: &[Toast], x: u32, y: u32) -> Option<usize> {
        let lay = layout(win.cols, win.rows, win.editor_row, win.editor_rows, None);
        let note = notification_area(&lay);
        let (nx, ny, nw, nh) = self.rect_px(&note);
        if nw == 0 || x < nx || x >= nx + nw {
            return None;
        }
        let mut top = ny;
        for (index, (height, _)) in self.toast_boxes(toasts, note.w, nh).iter().enumerate() {
            if y >= top && y < top + height {
                return Some(index);
            }
            top += height;
        }
        None
    }

    /// Paint `toasts` top down into the notification area `(x, y, w, h)` in
    /// pixels, `columns` cells wide. A toast that does not fit in what is left
    /// is not drawn, and neither is any after it.
    fn paint_toasts(
        &self,
        frame: &mut Frame,
        (nx, ny, nw, nh): (u32, u32, u32, u32),
        columns: u32,
        toasts: &[Toast],
    ) {
        if nw == 0 || nh == 0 {
            return;
        }
        let t = &self.theme;
        let mut used = 0;
        for (toast, (height, lines)) in toasts.iter().zip(self.toast_boxes(toasts, columns, nh)) {
            let mut box_frame = Frame::new(nw, height, t.toast);
            box_frame.fill(0, 0, nw, self.scale, t.toast_border);
            box_frame.fill(
                0,
                height.saturating_sub(self.scale),
                nw,
                self.scale,
                t.toast_border,
            );
            box_frame.fill(0, 0, self.scale, height, t.toast_border);
            box_frame.fill(
                nw.saturating_sub(self.scale),
                0,
                self.scale,
                height,
                t.toast_border,
            );
            let mut y = self.scale;
            if toast.unsigned {
                self.text_line(&mut box_frame, self.cell_w, y, CONTAINER_LABEL, t.label);
                y += self.cell_h;
            }
            for line in &lines {
                self.text_line(&mut box_frame, self.cell_w, y, line, t.foreground);
                y += self.cell_h;
            }
            Self::blit(frame, &box_frame, nx, ny + used);
            used += height;
        }
    }

    /// Where a link prompt sits in a window frame of `width` by `height`
    /// pixels: its box, the text lines wrapped to it, and the buttons. Painting
    /// and hit testing both use this, so a click lands on what was drawn.
    #[cfg(feature = "links-clipboard")]
    #[must_use]
    pub fn link_prompt_box(
        &self,
        width: u32,
        height: u32,
        prompt: &crate::linkprompt::Prompt,
    ) -> PromptBox {
        let cw = self.cell_w.max(1);
        let columns = (width / cw).max(1);
        let box_cols =
            usize::try_from(columns.saturating_sub(2).clamp(1, PROMPT_COLUMNS)).unwrap_or(1);
        let inner = box_cols.saturating_sub(2).max(1);
        let mut lines = Vec::new();
        for line in prompt.lines() {
            if line.is_empty() {
                lines.push(String::new());
            } else {
                lines.extend(wrap_by(&line, inner, &cells));
            }
        }
        let rows = u32::try_from(lines.len()).unwrap_or(0) + 2;
        let box_w = u32::try_from(box_cols).unwrap_or(1) * cw;
        let box_h = rows * self.cell_h + self.cell_h + 4 * self.scale;
        let x = width.saturating_sub(box_w) / 2;
        let y = height.saturating_sub(box_h) / 2;
        let buttons_y = y + 2 * self.scale + self.cell_h / 2 + (rows - 1) * self.cell_h;
        let labels: &[&str] = if prompt.target().is_some() {
            &[" Open (Enter) ", " Cancel (Esc) "]
        } else {
            &[" Close (Esc) "]
        };
        let mut col = 1u32;
        let mut buttons = Vec::new();
        for label in labels {
            let n = u32::try_from(label.chars().count()).unwrap_or(0);
            buttons.push((
                label.to_string(),
                (x + col * cw, buttons_y, n * cw, self.cell_h),
            ));
            col += n + 2;
        }
        PromptBox {
            rect: (x, y, box_w, box_h),
            lines,
            buttons,
        }
    }

    /// Draw a link prompt over `frame`, the whole window. It is the last thing
    /// painted, so nothing a program drew can cover it or pass for it.
    #[cfg(feature = "links-clipboard")]
    pub fn paint_link_prompt(&self, frame: &mut Frame, prompt: &crate::linkprompt::Prompt) {
        let theme = &self.theme;
        let placed = self.link_prompt_box(frame.width, frame.height, prompt);
        let (left, top, width, height) = placed.rect;
        frame.fill(left, top, width, height, theme.toast_border);
        let edge = 2 * self.scale;
        frame.fill(
            left + edge,
            top + edge,
            width.saturating_sub(2 * edge),
            height.saturating_sub(2 * edge),
            theme.toast,
        );
        let mut line_top = top + edge + self.cell_h / 2;
        for (i, line) in placed.lines.iter().enumerate() {
            let colour = if i == 0 {
                theme.heading
            } else if line.starts_with("Warning:") {
                theme.label
            } else {
                theme.foreground
            };
            self.text_line(frame, left + self.cell_w, line_top, line, colour);
            line_top += self.cell_h;
        }
        for (label, (bx, by, bw, bh)) in &placed.buttons {
            frame.fill(*bx, *by, *bw, *bh, theme.toast_border);
            self.text_line(frame, *bx, *by, label, theme.foreground);
        }
    }

    /// What is at pixel `(px, py)` of a link prompt drawn over a window frame
    /// of `width` by `height`.
    #[cfg(feature = "links-clipboard")]
    #[must_use]
    pub fn link_prompt_at(
        &self,
        width: u32,
        height: u32,
        prompt: &crate::linkprompt::Prompt,
        px: u32,
        py: u32,
    ) -> PromptHit {
        let b = self.link_prompt_box(width, height, prompt);
        let inside =
            |(x, y, w, h): (u32, u32, u32, u32)| px >= x && px < x + w && py >= y && py < y + h;
        for (label, rect) in &b.buttons {
            if inside(*rect) {
                return if label.contains("Open") {
                    PromptHit::Open
                } else {
                    PromptHit::Cancel
                };
            }
        }
        if inside(b.rect) {
            PromptHit::Box
        } else {
            PromptHit::Outside
        }
    }

    /// Where a context menu sits in a window frame of `width` by `height`
    /// pixels: it opens at the point it was asked for and is moved back inside
    /// the window if it would run past an edge. Painting and hit testing both
    /// use this, so a click lands on what was drawn.
    #[must_use]
    pub fn context_menu_box(
        &self,
        width: u32,
        height: u32,
        menu: &crate::contextmenu::ContextMenu,
    ) -> MenuBox {
        let cw = self.cell_w.max(1);
        let edge = 2 * self.scale;
        let entry_h = self.cell_h + 2 * self.scale;
        let rule_h = self.cell_h / 2 + self.scale;
        let labels = menu
            .items()
            .iter()
            .flatten()
            .map(|(_, label, _)| u32::try_from(label.chars().count()).unwrap_or(0))
            .max()
            .unwrap_or(0);
        let box_w = (labels + 4) * cw + 2 * edge;
        let inner_h: u32 = menu
            .items()
            .iter()
            .map(|e| if e.is_some() { entry_h } else { rule_h })
            .sum();
        let box_h = inner_h + 2 * edge;
        let x = menu.at.0.min(width.saturating_sub(box_w));
        let y = menu.at.1.min(height.saturating_sub(box_h));
        let mut top = y + edge;
        let mut rows = Vec::new();
        for entry in menu.items() {
            let h = if entry.is_some() { entry_h } else { rule_h };
            rows.push((x + edge, top, box_w - 2 * edge, h));
            top += h;
        }
        MenuBox {
            rect: (x, y, box_w, box_h),
            rows,
        }
    }

    /// Draw a context menu over `frame`, the whole window. It is painted after
    /// everything else, so nothing a program drew can cover it or pass for it.
    pub fn paint_context_menu(&self, frame: &mut Frame, menu: &crate::contextmenu::ContextMenu) {
        let theme = &self.theme;
        let placed = self.context_menu_box(frame.width, frame.height, menu);
        let (left, top, width, height) = placed.rect;
        frame.fill(left, top, width, height, theme.toast_border);
        let edge = 2 * self.scale;
        frame.fill(
            left + edge,
            top + edge,
            width.saturating_sub(2 * edge),
            height.saturating_sub(2 * edge),
            theme.toast,
        );
        for (i, (entry, (rx, ry, rw, rh))) in menu.items().iter().zip(&placed.rows).enumerate() {
            if let Some((_, label, on)) = entry {
                if menu.hover() == Some(i) {
                    frame.fill(*rx, *ry, *rw, *rh, theme.toast_border);
                }
                let colour = if *on { theme.foreground } else { theme.muted };
                self.text_line(frame, rx + 2 * self.cell_w, ry + self.scale, label, colour);
            } else {
                let line = (*rh / 2).max(1);
                frame.fill(
                    rx + self.cell_w,
                    ry + line,
                    rw.saturating_sub(2 * self.cell_w),
                    self.scale.max(1),
                    theme.toast_border,
                );
            }
        }
    }

    /// What is at pixel `(px, py)` of a context menu drawn over a window frame
    /// of `width` by `height`.
    #[must_use]
    pub fn context_menu_at(
        &self,
        width: u32,
        height: u32,
        menu: &crate::contextmenu::ContextMenu,
        px: u32,
        py: u32,
    ) -> MenuHit {
        let b = self.context_menu_box(width, height, menu);
        let inside =
            |(x, y, w, h): (u32, u32, u32, u32)| px >= x && px < x + w && py >= y && py < y + h;
        for (i, (entry, rect)) in menu.items().iter().zip(&b.rows).enumerate() {
            if entry.is_some() && inside(*rect) {
                return MenuHit::Item(i);
            }
        }
        if inside(b.rect) {
            MenuHit::Box
        } else {
            MenuHit::Outside
        }
    }

    /// Paint a whole document, blocks stacked top to bottom.
    ///
    /// # Panics
    ///
    /// Never; the name is the usual one for a function that cannot fail.
    #[must_use]
    pub fn render(&self, doc: &Document) -> Frame {
        let columns = doc.columns();
        let width = u32::try_from(columns).unwrap_or(1) * self.cell_w;
        let height: u32 = doc
            .blocks()
            .iter()
            .map(|b| match b {
                Block::Grid(g) => u32::try_from(g.size().1).unwrap_or(0) * self.cell_h,
                Block::Rich(r) => self.rich_height(r, columns),
            })
            .sum();
        let mut frame = Frame::new(width.max(1), height.max(1), self.theme.background);
        let mut y = 0;
        for b in doc.blocks() {
            match b {
                Block::Grid(g) => {
                    self.paint_grid(&mut frame, 0, y, g);
                    y += u32::try_from(g.size().1).unwrap_or(0) * self.cell_h;
                }
                Block::Rich(r) => {
                    self.paint_rich(&mut frame, 0, y, columns, r);
                    y += self.rich_height(r, columns);
                }
            }
        }
        frame
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style2 {
    Plain,
    Heading,
    Code,
}

/// How wide a character is in the unit a layout counts in.
type Measure<'a> = &'a dyn Fn(char) -> usize;

fn cells(c: char) -> usize {
    grapheme_display_width(c, &[])
}

/// The character right after the `n`th character of `s` that is not whitespace.
fn next_after_nonspace(s: &str, n: usize) -> Option<char> {
    let mut seen = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if !c.is_whitespace() {
            seen += 1;
            if seen == n {
                return chars.next();
            }
        }
    }
    None
}

fn width_by(s: &str, m: Measure<'_>) -> usize {
    s.chars().map(m).sum()
}

/// Cut `s` to `width` cells.
fn clip(s: &str, width: usize) -> String {
    clip_by(s, width, &cells)
}

/// Cut `s` to `width` units of `m`.
fn clip_by(s: &str, width: usize, m: Measure<'_>) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = m(c);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// A character that joins its neighbours: a line must not end after it or
/// before it, or an emoji sequence or a no-break would be torn apart.
fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200d}' | '\u{2060}')
}

/// `paragraph` as segments, each with whether a space comes before it. A space
/// ends a segment and the next one has a space before it; a zero width space
/// (U+200B) ends a segment too, and the next one has none, so it is a place a
/// line may break and never draws anything.
fn segments(paragraph: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut space_before = false;
    for c in paragraph.chars() {
        match c {
            ' ' => {
                out.push((std::mem::take(&mut current), space_before));
                space_before = true;
            }
            '\u{200b}' if cfg!(feature = "zero-width-text") => {
                current.push(c);
                out.push((std::mem::take(&mut current), space_before));
                space_before = false;
            }
            _ => current.push(c),
        }
    }
    out.push((current, space_before));
    out
}

/// Cut `word` to `width` cells without leaving a joiner at either side of the
/// cut: while the last character kept or the next one is a joiner, the last
/// character kept goes to the next line. If the joined run is too long for the
/// line to hold at all, the plain cut stands.
fn cut_word(word: &str, width: usize, m: Measure<'_>) -> String {
    let head = clip_by(word, width, m);
    if !cfg!(feature = "zero-width-text") {
        return head;
    }
    let mut chars: Vec<char> = head.chars().collect();
    let mut next = word[head.len()..].chars().next();
    while let Some(&last) = chars.last() {
        if next.is_none() || !(is_joiner(last) || next.is_some_and(is_joiner)) {
            break;
        }
        next = chars.pop();
    }
    if chars.is_empty() {
        head
    } else {
        chars.into_iter().collect()
    }
}

/// The lines `text` is wrapped to in `width` cells: at spaces, after a zero
/// width space (a break opportunity that draws nothing), and, for a word longer
/// than a line, between characters but never next to a joiner (U+200D, U+2060)
/// unless the joined run is wider than the line. Every character is kept.
#[must_use]
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_by(text, width, &cells)
}

/// Greedy word wrap to `width` units of `m`, breaking a word that is longer
/// than a line. Characters that draw nothing take no room and are kept in the
/// line.
fn wrap_by(text: &str, width: usize, m: Measure<'_>) -> Vec<String> {
    let blank = m(' ');
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for (segment, space_before) in segments(paragraph) {
            let mut word = segment;
            let mut space = space_before;
            loop {
                let gap = if space && !line.is_empty() { blank } else { 0 };
                if width_by(&line, m) + gap + width_by(&word, m) <= width {
                    if gap > 0 {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if line.is_empty() {
                    let head = cut_word(&word, width.max(1), m);
                    // A character wider than the line leaves `head` empty: drop
                    // that whole character, never part of it, so the cut stays on
                    // a character boundary and the loop still makes progress.
                    let cut = if head.is_empty() {
                        word.chars().next().map_or(1, char::len_utf8)
                    } else {
                        head.len()
                    };
                    word = word[cut..].to_owned();
                    if !head.is_empty() {
                        lines.push(head);
                    }
                    if word.is_empty() {
                        break;
                    }
                } else {
                    lines.push(std::mem::take(&mut line));
                    space = false;
                }
            }
        }
        lines.push(line);
    }
    lines
}
