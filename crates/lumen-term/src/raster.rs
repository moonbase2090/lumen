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
//! asked), kerning, or any other text shaping.

use crate::design::{FontFile, Fonts};
use crate::document::{Block, Document, RichBlock};
use crate::grid::GridBlock;
use crate::image_draw;
use crate::layout::{GUTTER_COLUMNS, Rect, StripPlacement, layout, notification_area};
use crate::notify::Toast;
use lumen_track0::Block as Track0Block;
use prismattyc_core::{Color, Style, grapheme_display_width};
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
/// The label every rich block's container carries.
pub const CONTAINER_LABEL: &str = "unverified output";
pub use crate::image_draw::IMAGE_MAX_ROWS;
/// The most lines of text in a toast; the rest is cut.
pub const TOAST_MAX_LINES: usize = 3;
/// The terminal's own text in the header strip of the trust margin.
pub const MARGIN_LABEL: &str = "lumen";

pub use crate::frame::{Frame, Rgb};

pub use crate::design::Theme;

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
            FontFile::Data(d) => d.clone(),
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
        let screen = block.screen();
        for r in 0..screen.rows() {
            let Some(cells) = screen.row(r) else { continue };
            let top = y + u32::try_from(r).unwrap_or(0) * self.cell_h;
            let mut c = 0;
            while c < cells.len() {
                let cell = &cells[c];
                let span = 1 + usize::from(cells.get(c + 1).is_some_and(|n| n.wide_cont));
                if cell.wide_cont {
                    c += 1;
                    continue;
                }
                let (fg, bg) = self.colours(cell.style);
                let left = x + u32::try_from(c).unwrap_or(0) * self.cell_w;
                let w = u32::try_from(span).unwrap_or(1);
                frame.fill(left, top, w * self.cell_w, self.cell_h, bg);
                self.draw_glyph(frame, cell.character, left, top, w, fg);
                if cell.style.underline {
                    let line = top + self.cell_h - self.scale.min(self.cell_h);
                    frame.fill(left, line, w * self.cell_w, self.scale, fg);
                }
                c += span;
            }
        }
    }

    fn text_line(&self, frame: &mut Frame, x: u32, y: u32, text: &str, colour: Rgb) {
        let mut col = 0u32;
        for ch in text.chars() {
            let w = u32::try_from(grapheme_display_width(ch, &[])).unwrap_or(1);
            if w > 0 {
                self.draw_glyph(frame, ch, x + col * self.cell_w, y, w, colour);
            }
            col += w;
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
        limit: u32,
    ) {
        let (x, y) = at;
        let band = (i64::from(x), i64::from(x) + i64::from(limit));
        let mut pos = 0usize;
        for ch in text.chars() {
            let a = self.advance(ch);
            if a > 0 {
                let left = x + u32::try_from((pos + SUBPIXELS / 2) / SUBPIXELS).unwrap_or(0);
                let face = self.proportional_face(ch);
                self.draw_face(frame, face, ch, (left, y), band, colour);
            }
            pos += a;
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
        if kind == Style2::Code {
            self.text_line(frame, at.0, at.1, text, colour);
        } else {
            self.proportional_line(frame, at, text, colour, limit);
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

    /// Paint the content region: foreground blocks top down. The newest cycle's
    /// strip is below the editor; the strip of an older cycle sits where that
    /// cycle's output ended, in the flow: one row, collapsed, with the count and
    /// nothing the producer wrote.
    fn paint_content(
        &self,
        region: &mut Frame,
        doc: &Document,
        columns: usize,
        strips: &[crate::strip::Strip<'_>],
        newest_cycle: Option<u32>,
    ) {
        let mut y = 0;
        let mut placed: Vec<u32> = Vec::new();
        for b in doc.blocks() {
            match b {
                Block::Grid(g) => {
                    self.paint_grid(region, 0, y, g);
                    y += u32::try_from(g.size().1).unwrap_or(0) * self.cell_h;
                }
                Block::Rich(r) if !r.background => {
                    self.paint_rich(region, 0, y, columns, r);
                    y += self.rich_height(r, columns);
                }
                Block::Rich(r) => {
                    if Some(r.cycle) == newest_cycle || placed.contains(&r.cycle) {
                        continue;
                    }
                    if let Some(strip) = strips.iter().find(|s| s.cycle == r.cycle) {
                        placed.push(r.cycle);
                        y += self.paint_flow_strip(region, y, columns, strip);
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
        for line in &lines {
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
            self.body_line(frame, (inner, line_y), line, kind, colour, limit);
            line_y += self.cell_h;
        }
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
        let px = |r: &Rect| {
            (
                r.x * self.cell_w,
                r.y * self.cell_h,
                r.w * self.cell_w,
                r.h * self.cell_h,
            )
        };
        let mut frame = Frame::new(
            (win.cols * self.cell_w).max(1),
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
            let mut region = Frame::new(aw, ah, t.background);
            let columns = usize::try_from(area.w).unwrap_or(1);
            self.paint_content(&mut region, doc, columns, &strips, newest.map(|s| s.cycle));
            Self::blit(&mut frame, &region, ax, ay);
        }

        // Toasts: in the notification area only.
        let note = notification_area(&lay);
        self.paint_toasts(&mut frame, px(&note), note.w, toasts);

        // Editor: a fixed box with a prompt.
        let (ex, ey, ew, eh) = px(&lay.editor);
        frame.fill(ex, ey, ew, eh, t.border);
        frame.fill(
            ex + self.scale,
            ey + self.scale,
            ew.saturating_sub(2 * self.scale),
            eh.saturating_sub(2 * self.scale),
            t.editor,
        );
        self.text_line(&mut frame, ex + self.cell_w, ey, ">", t.prompt);

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
        self.text_line(
            &mut frame,
            self.cell_w * GUTTER_COLUMNS.min(win.cols),
            0,
            MARGIN_LABEL,
            t.margin_text,
        );
        frame
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
        let (nx, ny, nw, nh) = (
            note.x * self.cell_w,
            note.y * self.cell_h,
            note.w * self.cell_w,
            note.h * self.cell_h,
        );
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
