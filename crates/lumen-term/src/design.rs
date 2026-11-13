//! The design system: the named tokens the terminal's look is built from.
//!
//! Colours are named by role, not by where they are painted, so a theme is a
//! choice of values for the same roles. [`Palette::light`] is the built-in
//! look and the one the terminal uses; [`theme`] maps it onto the renderer's
//! [`Theme`]. The renderer's own [`Theme::light`] stays as it is, because the
//! pinned conditions of the pixel evidence use it.
//!
//! Rules every palette keeps (the tests check them):
//!
//! - Every text role is at least 4.5:1 against every surface it is drawn on
//!   (WCAG AA for body text). The spec's floor for a visible run is 3:1, so
//!   no text the terminal draws in its own colours can fall under it.
//! - The run colour tokens of the spec (`fg`, `muted`, `accent`, `error`) map
//!   to text roles, so they keep the same floor.
//! - The ANSI colours a program uses for text (0 to 6 and 8 to 14) are at
//!   least 3:1 against the canvas. 7 and 15 are the light greys by convention.
//! - The control border (the editor's edge, and the edge of the container
//!   of unverified output) is at least 3:1 against the surfaces on both of
//!   its sides, the WCAG floor for the boundary of a control.

use crate::raster::{Rgb, Theme};

/// The colour roles of the terminal's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The page behind every block.
    pub canvas: Rgb,
    /// A raised surface: the container of a rich block and the strip.
    pub surface: Rgb,
    /// A sunken surface: the background of code.
    pub sunken: Rgb,
    /// The input editor's field.
    pub field: Rgb,
    /// The trust margin: the gutter and the header strip.
    pub chrome: Rgb,
    /// Body text.
    pub text: Rgb,
    /// Headings.
    pub text_strong: Rgb,
    /// Secondary text: labels, counts, hints.
    pub text_muted: Rgb,
    /// The terminal's own text in the trust margin.
    pub chrome_text: Rgb,
    /// Links, focus and the one accent of the look.
    pub accent: Rgb,
    /// Errors and failed exits.
    pub error: Rgb,
    /// The label of unverified output: a caution, not an error.
    pub caution: Rgb,
    /// Edges: the input editor and the container of unverified output.
    pub control_border: Rgb,
    /// A toast's box.
    pub notice: Rgb,
    /// A toast's border.
    pub notice_border: Rgb,
    /// The sixteen ANSI colours of grid blocks.
    pub ansi: [Rgb; 16],
}

impl Palette {
    /// The built-in light look.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            canvas: [255, 255, 255],
            surface: [247, 248, 250],
            sunken: [239, 241, 245],
            field: [255, 255, 255],
            chrome: [243, 244, 247],
            text: [24, 26, 32],
            text_strong: [17, 24, 39],
            text_muted: [88, 95, 110],
            chrome_text: [75, 83, 99],
            accent: [37, 84, 214],
            error: [190, 36, 36],
            caution: [140, 80, 0],
            control_border: [133, 140, 154],
            notice: [255, 249, 235],
            notice_border: [205, 150, 40],
            ansi: [
                [24, 26, 32],
                [190, 36, 36],
                [24, 128, 56],
                [150, 100, 0],
                [37, 84, 214],
                [150, 52, 178],
                [10, 118, 136],
                [204, 208, 216],
                [88, 95, 110],
                [214, 52, 52],
                [30, 140, 62],
                [166, 114, 0],
                [64, 112, 232],
                [172, 72, 196],
                [14, 132, 150],
                [238, 240, 244],
            ],
        }
    }

    /// The colour of one of the spec's run colour tokens.
    #[must_use]
    pub const fn run(&self, token: RunColour) -> Rgb {
        match token {
            RunColour::Fg => self.text,
            RunColour::Muted => self.text_muted,
            RunColour::Accent => self.accent,
            RunColour::Error => self.error,
        }
    }

    /// The renderer's theme for this palette.
    #[must_use]
    pub const fn theme(&self) -> Theme {
        Theme {
            foreground: self.text,
            background: self.canvas,
            ansi: self.ansi,
            container: self.surface,
            border: self.control_border,
            label: self.caution,
            code: self.sunken,
            heading: self.text_strong,
            margin: self.chrome,
            margin_text: self.chrome_text,
            editor: self.field,
            toast: self.notice,
            toast_border: self.notice_border,
        }
    }
}

/// The spec's semantic colour tokens for a text run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunColour {
    /// `fg`: body text.
    Fg,
    /// `muted`: secondary text.
    Muted,
    /// `accent`: emphasis and links.
    Accent,
    /// `error`: errors.
    Error,
}

/// The family role of a text run, as the spec names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Proportional body text.
    Body,
    /// Proportional heading text.
    Heading,
    /// Monospace: code, grids and the editor.
    Mono,
}

/// The type scale: pixel sizes at scale 1, by size step. Step 0 is the body
/// size; each step up is a ratio of about 1.2, rounded to a whole pixel.
pub const TYPE_SCALE: [u32; 5] = [12, 14, 17, 20, 24];

/// The index of step 0 in [`TYPE_SCALE`].
pub const BODY_STEP: usize = 1;

/// The pixel size of a size `step` (negative is smaller), at scale 1, clamped
/// to the scale.
#[must_use]
pub fn size_px(step: i8) -> u32 {
    let at = i64::try_from(BODY_STEP).unwrap_or(0) + i64::from(step);
    let last = i64::try_from(TYPE_SCALE.len() - 1).unwrap_or(0);
    TYPE_SCALE[usize::try_from(at.clamp(0, last)).unwrap_or(BODY_STEP)]
}

/// The default size step of a family's text.
#[must_use]
pub const fn default_step(family: Family) -> i8 {
    match family {
        Family::Body | Family::Mono => 0,
        Family::Heading => 2,
    }
}

/// The theme the terminal draws with: the built-in light look.
#[must_use]
pub const fn theme() -> Theme {
    Palette::light().theme()
}

/// WCAG relative luminance of a colour, from 0 to 1.
#[must_use]
pub fn luminance(colour: Rgb) -> f64 {
    let linear = |c: u8| {
        let s = f64::from(c) / 255.0;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.0722_f64.mul_add(
        linear(colour[2]),
        0.2126_f64.mul_add(linear(colour[0]), 0.7152 * linear(colour[1])),
    )
}

/// WCAG contrast ratio of two colours, from 1 to 21.
#[must_use]
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}
