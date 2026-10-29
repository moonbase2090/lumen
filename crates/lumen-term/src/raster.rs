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
use fontdue::{Font, FontSettings};
use lumen_track0::Block as Track0Block;
use prismattyc_core::{Color, Style, grapheme_display_width};
use std::cell::RefCell;
use std::collections::HashMap;

/// The bundled face.
const FACE: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
/// Pixel size of the face at scale 1.
pub const BASE_PX: f32 = 14.0;
/// The label every rich block's container carries.
pub const CONTAINER_LABEL: &str = "unverified output";

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
        }
    }
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
    font: Font,
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

fn indexed(n: u8, theme: &Theme) -> Rgb {
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
        let font = Font::from_bytes(FACE, FontSettings::default()).map_err(str::to_owned)?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a scale is a small whole number"
        )]
        let px = BASE_PX * scale.max(1) as f32;
        let lines = font
            .horizontal_line_metrics(px)
            .ok_or("the face has no line metrics")?;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "metrics of a 14 to 100 pixel face are small and positive"
        )]
        let (cell_w, cell_h, ascent) = (
            font.metrics('M', px).advance_width.round().max(1.0) as u32,
            (lines.ascent - lines.descent).ceil().max(1.0) as u32,
            lines.ascent.round() as i32,
        );
        Ok(Self {
            font,
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
        if ch == ' ' || ch == '\0' || self.font.lookup_glyph_index(ch) == 0 {
            return;
        }
        let mut cache = self.cache.borrow_mut();
        let g = cache.entry(ch).or_insert_with(|| {
            let (m, coverage) = self.font.rasterize(ch, self.px);
            Glyph {
                xmin: m.xmin,
                ymin: m.ymin,
                width: m.width,
                height: m.height,
                coverage,
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

/// Greedy word wrap to `width` cells, breaking a word that is longer than a line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let mut word = word.to_owned();
            loop {
                let need = width_of(&line) + usize::from(!line.is_empty()) + width_of(&word);
                if need <= width {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if line.is_empty() {
                    let head = clip(&word, width.max(1));
                    word = word[head.len().max(1)..].to_owned();
                    lines.push(head);
                    if word.is_empty() {
                        break;
                    }
                } else {
                    lines.push(std::mem::take(&mut line));
                }
            }
        }
        lines.push(line);
    }
    lines
}
