//! The design system: the named tokens the terminal's look is built from.
//!
//! Colours are named by role, not by where they are painted, so a theme is a
//! choice of values for the same roles. There are three built-in palettes
//! ([`Builtin`]): light, dark and high contrast. [`Palette::theme`] maps a
//! palette onto the renderer's [`Theme`]. [`Theme::light`] stays as it was,
//! because the pinned conditions of the pixel evidence use it.
//!
//! Each built-in palette keeps the [`Floors`] of its kind (the tests check
//! them, and [`Palette::check`] reports what a palette misses):
//!
//! - Every text role is at least 4.5:1 against every surface it is drawn on
//!   (WCAG AA for body text), 7:1 in high contrast. The spec's floor for a
//!   visible run is 3:1, so no text the terminal draws in its own colours can
//!   fall under it.
//! - The run colour tokens of the spec (`fg`, `muted`, `accent`, `error`) map
//!   to text roles, so they keep the same floor.
//! - The ANSI colours a program uses for text are at least 3:1 against the
//!   canvas and the surface (4.5:1 in high contrast). The black and white
//!   slots that sit next to the background by convention (7 and 15 on a light
//!   palette, 0 on a dark one) are exempt.
//! - The control border (the editor's edge, and the edge of the container
//!   of unverified output) is at least 3:1 against the surfaces on both of
//!   its sides, the WCAG floor for the boundary of a control.
//!
//! A palette from a theme file only has to keep the spec's floor for text,
//! 3:1 ([`Floors::USER`]), since a run under it would be hidden.

use crate::frame::Rgb;

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

    /// The built-in dark look.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            canvas: [22, 24, 29],
            surface: [30, 33, 40],
            sunken: [36, 40, 48],
            field: [26, 28, 34],
            chrome: [28, 31, 38],
            text: [226, 229, 235],
            text_strong: [243, 245, 248],
            text_muted: [160, 167, 180],
            chrome_text: [170, 177, 190],
            accent: [122, 162, 255],
            error: [255, 117, 117],
            caution: [230, 176, 80],
            control_border: [110, 118, 134],
            notice: [44, 38, 24],
            notice_border: [190, 140, 50],
            ansi: [
                [36, 40, 48],
                [255, 110, 110],
                [110, 210, 130],
                [230, 190, 90],
                [122, 162, 255],
                [210, 140, 240],
                [90, 205, 215],
                [200, 205, 214],
                [125, 133, 148],
                [255, 140, 140],
                [140, 230, 155],
                [245, 210, 120],
                [150, 185, 255],
                [225, 165, 250],
                [125, 225, 235],
                [243, 245, 248],
            ],
        }
    }

    /// The built-in high-contrast look: black on white, text at 7:1 or more.
    #[must_use]
    pub const fn high_contrast() -> Self {
        Self {
            canvas: [255, 255, 255],
            surface: [250, 250, 250],
            sunken: [240, 240, 240],
            field: [255, 255, 255],
            chrome: [235, 235, 235],
            text: [0, 0, 0],
            text_strong: [0, 0, 0],
            text_muted: [55, 55, 55],
            chrome_text: [30, 30, 30],
            accent: [0, 51, 170],
            error: [160, 0, 0],
            caution: [100, 55, 0],
            control_border: [0, 0, 0],
            notice: [255, 255, 255],
            notice_border: [0, 0, 0],
            ansi: [
                [0, 0, 0],
                [160, 0, 0],
                [0, 100, 0],
                [110, 70, 0],
                [0, 51, 170],
                [120, 0, 140],
                [0, 90, 110],
                [200, 200, 200],
                [60, 60, 60],
                [190, 0, 0],
                [0, 120, 0],
                [130, 85, 0],
                [0, 70, 200],
                [140, 0, 160],
                [0, 105, 125],
                [245, 245, 245],
            ],
        }
    }

    /// Whether the canvas is dark: its luminance is under that of mid grey.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        luminance(self.canvas) < luminance([128, 128, 128])
    }

    /// The surfaces text is drawn on, by role name.
    #[must_use]
    pub const fn surfaces(&self) -> [(&'static str, Rgb); 6] {
        [
            ("canvas", self.canvas),
            ("surface", self.surface),
            ("sunken", self.sunken),
            ("field", self.field),
            ("chrome", self.chrome),
            ("notice", self.notice),
        ]
    }

    /// The text roles, by role name.
    #[must_use]
    pub const fn text_roles(&self) -> [(&'static str, Rgb); 7] {
        [
            ("text", self.text),
            ("text_strong", self.text_strong),
            ("text_muted", self.text_muted),
            ("chrome_text", self.chrome_text),
            ("accent", self.accent),
            ("error", self.error),
            ("caution", self.caution),
        ]
    }

    /// What this palette misses of `floors`, one line each; empty if none.
    #[must_use]
    pub fn check(&self, floors: &Floors) -> Vec<String> {
        let mut misses = Vec::new();
        for (name, fg) in self.text_roles() {
            for (surface, bg) in self.surfaces() {
                let ratio = contrast(fg, bg);
                if ratio < floors.text {
                    misses.push(format!(
                        "{name} on {surface} is {ratio:.2}:1, under {}:1",
                        floors.text
                    ));
                }
            }
        }
        if let Some(floor) = floors.ansi {
            let exempt: &[usize] = if self.is_dark() { &[0] } else { &[7, 15] };
            for (i, colour) in self.ansi.iter().enumerate() {
                if exempt.contains(&i) {
                    continue;
                }
                for (surface, bg) in [("canvas", self.canvas), ("surface", self.surface)] {
                    let ratio = contrast(*colour, bg);
                    if ratio < floor {
                        misses.push(format!(
                            "ANSI {i} on {surface} is {ratio:.2}:1, under {floor}:1"
                        ));
                    }
                }
            }
        }
        if let Some(floor) = floors.border {
            for (surface, bg) in [
                ("canvas", self.canvas),
                ("field", self.field),
                ("surface", self.surface),
            ] {
                let ratio = contrast(self.control_border, bg);
                if ratio < floor {
                    misses.push(format!(
                        "control_border on {surface} is {ratio:.2}:1, under {floor}:1"
                    ));
                }
            }
        }
        misses
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
            prompt: self.accent,
            toast: self.notice,
            toast_border: self.notice_border,
        }
    }
}

/// The colours of a render, by where they are painted.
///
/// [`Theme::light`] is the fixed theme the pinned conditions of the pixel
/// evidence (criterion 2.3) use, kept as it was so baselines stay comparable.
/// The terminal draws with [`theme`], the light [`Palette`] mapped by role.
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
    /// The editor's prompt mark.
    pub prompt: Rgb,
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
            prompt: [28, 28, 28],
            toast: [255, 250, 230],
            toast_border: [190, 150, 60],
        }
    }
}

/// The contrast floors a palette is held to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floors {
    /// Every text role against every surface.
    pub text: f64,
    /// The ANSI colours used for text against the canvas and the surface;
    /// `None` checks nothing.
    pub ansi: Option<f64>,
    /// The control border against the surfaces beside it; `None` checks
    /// nothing.
    pub border: Option<f64>,
}

impl Floors {
    /// The built-in light and dark palettes.
    pub const STANDARD: Self = Self {
        text: 4.5,
        ansi: Some(3.0),
        border: Some(3.0),
    };
    /// The built-in high-contrast palette.
    pub const HIGH_CONTRAST: Self = Self {
        text: 7.0,
        ansi: Some(4.5),
        border: Some(3.0),
    };
    /// A palette from a theme file: the spec's visibility floor for text.
    pub const USER: Self = Self {
        text: 3.0,
        ansi: None,
        border: None,
    };
}

/// The built-in palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// [`Palette::light`].
    Light,
    /// [`Palette::dark`].
    Dark,
    /// [`Palette::high_contrast`].
    HighContrast,
}

impl Builtin {
    /// Every built-in palette.
    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::HighContrast];

    /// The palette.
    #[must_use]
    pub const fn palette(self) -> Palette {
        match self {
            Self::Light => Palette::light(),
            Self::Dark => Palette::dark(),
            Self::HighContrast => Palette::high_contrast(),
        }
    }

    /// The floors it keeps.
    #[must_use]
    pub const fn floors(self) -> Floors {
        match self {
            Self::Light | Self::Dark => Floors::STANDARD,
            Self::HighContrast => Floors::HIGH_CONTRAST,
        }
    }

    /// Its name in settings and theme files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::HighContrast => "high-contrast",
        }
    }

    /// The built-in palette called `name`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
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
