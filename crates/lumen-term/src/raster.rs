//! The headless rasteriser: blocks to an RGBA frame, with no window.
//!
//! One bundled face (IBM Plex Mono Regular, see `NOTICE.txt`), integer
//! blending and a fixed light theme, so the same document gives the same
//! pixels every time on a platform. That is the groundwork for the pixel
//! oracle of criterion 2.3: a baseline is taken twice and must match exactly,
//! and baselines are kept per platform.
//!
//! What it does not do: bold or italic faces (there is one face), glyphs the
//! face lacks (they are skipped), combining marks (the base glyph only), a
//! cursor (off unless asked), or any text shaping.

use crate::document::{Block, Document, RichBlock};
use crate::grid::GridBlock;
use crate::layout::{GUTTER_COLUMNS, Rect, StripPlacement, layout, notification_area};
use crate::notify::Toast;
use lumen_track0::Block as Track0Block;
use prismattyc_core::{Color, Style, grapheme_display_width};
use std::cell::RefCell;
use std::collections::HashMap;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

/// The bundled face.
const FACE: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
/// Pixel size of the face at scale 1.
pub const BASE_PX: f32 = 14.0;
/// The label every rich block's container carries.
pub const CONTAINER_LABEL: &str = "unverified output";
/// The most lines of text in a toast; the rest is cut.
pub const TOAST_MAX_LINES: usize = 3;
/// The terminal's own text in the header strip of the trust margin.
pub const MARGIN_LABEL: &str = "lumen";

/// A colour.
pub type Rgb = [u8; 3];

/// The colours of a render. The pinned conditions of 2.3 use the light theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Default foreground.
    pub foreground: Rgb,
    /// Default background.
    pub background: Rgb,
    /// The sixteen ANSI colours.
    pub ansi: [Rgb; 16],
    /// Background of a rich block's container.
    pub container: Rgb,
    /// Border of a rich block's container.
    pub border: Rgb,
    /// The container's label.
    pub label: Rgb,
    /// Background of a code block.
    pub code: Rgb,
    /// Foreground of a heading.
    pub heading: Rgb,
    /// The trust margin: the gutter and the header strip. Only the terminal paints it.
    pub margin: Rgb,
    /// The terminal's own text in the margin.
    pub margin_text: Rgb,
    /// The input editor.
    pub editor: Rgb,
    /// A toast's box.
    pub toast: Rgb,
    /// A toast's border.
    pub toast_border: Rgb,
}

impl Theme {
    /// The light theme.
    #[must_use]
    pub fn light() -> Self {
        Self {
            foreground: [28, 28, 28],
            background: [255, 255, 255],
            ansi: [
                [0, 0, 0],
                [194, 54, 33],
                [37, 188, 36],
                [173, 173, 39],
                [73, 46, 225],
                [211, 56, 211],
                [51, 187, 200],
                [203, 204, 205],
                [129, 131, 131],
                [252, 57, 31],
                [49, 231, 34],
                [234, 236, 35],
                [88, 51, 255],
                [249, 53, 248],
                [20, 240, 240],
                [233, 235, 235],
            ],
            container: [244, 244, 244],
            border: [176, 176, 176],
            label: [120, 64, 0],
            code: [232, 234, 240],
            heading: [20, 60, 120],
            margin: [226, 230, 238],
            margin_text: [60, 70, 90],
            editor: [250, 250, 250],
            toast: [255, 250, 230],
            toast_border: [190, 150, 60],
        }
    }
}

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

/// An RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, rows top to bottom, alpha always 255.
    pub rgba: Vec<u8>,
}

impl Frame {
    /// A frame filled with `colour`.
    #[must_use]
    pub fn new(width: u32, height: u32, colour: Rgb) -> Self {
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width as usize) * (height as usize) {
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| ((y as usize) * (self.width as usize) + x as usize) * 4)
    }

    /// The pixel at `x`, `y`, or `None` outside the frame.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        let i = self.index(x, y)?;
        Some([self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]])
    }

    fn put(&mut self, x: u32, y: u32, colour: Rgb) {
        if let Some(i) = self.index(x, y) {
            self.rgba[i..i + 3].copy_from_slice(&colour);
        }
    }

    fn blend(&mut self, x: u32, y: u32, colour: Rgb, coverage: u8) {
        let Some(i) = self.index(x, y) else { return };
        let a = u32::from(coverage);
        for (k, c) in colour.iter().enumerate() {
            let under = u32::from(self.rgba[i + k]);
            let mixed = (u32::from(*c) * a + under * (255 - a) + 127) / 255;
            self.rgba[i + k] = u8::try_from(mixed).unwrap_or(255);
        }
    }

    /// Fill a rectangle, clipped to the frame.
    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, colour: Rgb) {
        for yy in y..y.saturating_add(h).min(self.height) {
            for xx in x..x.saturating_add(w).min(self.width) {
                self.put(xx, yy, colour);
            }
        }
    }

    /// A `width` by `height` window onto this frame: when the frame is taller
    /// than the window, the bottom of it (the newest blocks), otherwise the
    /// frame at the top; the rest, and any width beyond the frame, is `colour`.
    #[must_use]
    pub fn viewport(&self, width: u32, height: u32, colour: Rgb) -> Frame {
        let mut out = Frame::new(width, height, colour);
        let skip = self.height.saturating_sub(height);
        let rows = self.height - skip;
        let copy = self.width.min(width) as usize * 4;
        for y in 0..rows.min(height) {
            let from = (((y + skip) as usize) * self.width as usize) * 4;
            let to = ((y as usize) * width as usize) * 4;
            out.rgba[to..to + copy].copy_from_slice(&self.rgba[from..from + copy]);
        }
        out
    }

    /// The frame as a PNG.
    ///
    /// # Errors
    ///
    /// Returns the encoder's error.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header()?;
            writer.write_image_data(&self.rgba)?;
        }
        Ok(out)
    }
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
    font: FontRef<'static>,
    scaler: RefCell<ScaleContext>,
    px: f32,
    scale: u32,
    cell_w: u32,
    cell_h: u32,
    ascent: i32,
    theme: Theme,
    cache: RefCell<HashMap<char, Glyph>>,
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
    /// A renderer for the bundled face at `scale` (the 2.3 conditions use 2).
    ///
    /// # Errors
    ///
    /// Returns a message if the face cannot be read, which would be a broken
    /// build, since it is compiled in.
    pub fn new(scale: u32, theme: Theme) -> Result<Self, String> {
        let font = FontRef::from_index(FACE, 0).ok_or("the face could not be read")?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a scale is a small whole number"
        )]
        let px = BASE_PX * scale.max(1) as f32;
        let lines = font.metrics(&[]).scale(px);
        let advance = font
            .glyph_metrics(&[])
            .scale(px)
            .advance_width(font.charmap().map('M'));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "metrics of a 14 to 100 pixel face are small and positive"
        )]
        let (cell_w, cell_h, ascent) = (
            advance.round().max(1.0) as u32,
            (lines.ascent + lines.descent).ceil().max(1.0) as u32,
            lines.ascent.round() as i32,
        );
        Ok(Self {
            font,
            scaler: RefCell::new(ScaleContext::new()),
            px,
            scale: scale.max(1),
            cell_w,
            cell_h,
            ascent,
            theme,
            cache: RefCell::new(HashMap::new()),
        })
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

    fn draw_glyph(&self, frame: &mut Frame, ch: char, x: u32, y: u32, cells: u32, colour: Rgb) {
        let glyph = self.font.charmap().map(ch);
        if ch == ' ' || ch == '\0' || glyph == 0 {
            return;
        }
        let mut cache = self.cache.borrow_mut();
        let g = cache.entry(ch).or_insert_with(|| {
            let mut context = self.scaler.borrow_mut();
            let mut scaler = context.builder(self.font).size(self.px).hint(false).build();
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
        let (band_start, band_end) = (i64::from(x), i64::from(x + cells * self.cell_w));
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

    /// The lines of a rich block's body, wrapped to `width` cells, with a flag
    /// for the code background.
    fn body(block: &Track0Block, width: usize) -> (Vec<String>, Style2) {
        match block {
            Track0Block::Paragraph(t) => (wrap(t, width), Style2::Plain),
            Track0Block::Heading { text, .. } => (wrap(text, width), Style2::Heading),
            Track0Block::Code { text, stripped, .. } => {
                let mut lines: Vec<String> = text.lines().map(|l| clip(l, width)).collect();
                if *stripped > 0 {
                    lines.push(format!("{stripped} bidi controls removed"));
                }
                (lines, Style2::Code)
            }
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
                let (lines, _) = Self::body(&block.content, inner);
                let first = lines.first().map_or("", String::as_str);
                let row = y + u32::try_from(i + 1).unwrap_or(u32::MAX) * self.cell_h;
                self.text_line(region, self.cell_w, row, &clip(first, inner), t.foreground);
            }
        }
        height
    }

    /// The height in pixels of a rich block's container.
    #[must_use]
    pub fn rich_height(&self, block: &RichBlock, columns: usize) -> u32 {
        let (lines, _) = Self::body(&block.content, columns.saturating_sub(2).max(1));
        2 * self.scale + (1 + u32::try_from(lines.len()).unwrap_or(0)) * self.cell_h
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
        let (lines, kind) = Self::body(&block.content, columns.saturating_sub(2).max(1));
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
            self.text_line(frame, inner, line_y, line, colour);
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
        self.text_line(&mut frame, ex + self.cell_w, ey, ">", t.foreground);

        // Strip: below the editor.
        if let (StripPlacement::Placed(r), Some(strip)) = (lay.strip, newest) {
            let (sx, sy, sw, sh) = px(&r);
            let mut region = Frame::new(sw, sh, t.container);
            let width = usize::try_from(r.w).unwrap_or(1).saturating_sub(2);
            self.text_line(&mut region, self.cell_w, 0, &strip.label(), t.label);
            if strip.open {
                for (i, block) in strip.blocks.iter().enumerate() {
                    let (lines, _) = Self::body(&block.content, width.max(1));
                    let first = lines.first().map_or("", String::as_str);
                    let row = u32::try_from(i + 1).unwrap_or(u32::MAX) * self.cell_h;
                    self.text_line(
                        &mut region,
                        self.cell_w,
                        row,
                        &clip(first, width),
                        t.foreground,
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
            let mut lines = wrap(&toast.text, inner);
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

fn width_of(s: &str) -> usize {
    s.chars().map(|c| grapheme_display_width(c, &[])).sum()
}

/// Cut `s` to `width` cells.
fn clip(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = grapheme_display_width(c, &[]);
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
fn cut_word(word: &str, width: usize) -> String {
    let head = clip(word, width);
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
    wrap(text, width)
}

/// Greedy word wrap to `width` cells, breaking a word that is longer than a line.
/// Characters that draw nothing take no cells and are kept in the line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for (segment, space_before) in segments(paragraph) {
            let mut word = segment;
            let mut space = space_before;
            loop {
                let gap = usize::from(space && !line.is_empty());
                if width_of(&line) + gap + width_of(&word) <= width {
                    if gap == 1 {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if line.is_empty() {
                    let head = cut_word(&word, width.max(1));
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
