//! The pixel buffer every render paints into: an RGBA frame with integer
//! blending. Part of the image path's build identity, because decoded image
//! pixels are written through it.

/// A colour.
pub type Rgb = [u8; 3];

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

    pub(crate) fn put(&mut self, x: u32, y: u32, colour: Rgb) {
        if let Some(i) = self.index(x, y) {
            self.rgba[i..i + 3].copy_from_slice(&colour);
        }
    }

    pub(crate) fn blend(&mut self, x: u32, y: u32, colour: Rgb, coverage: u8) {
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
