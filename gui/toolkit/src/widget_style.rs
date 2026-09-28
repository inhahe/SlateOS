//! The widget-style theme axis: the shapes of the toolkit's controls, as a
//! theme chooses them.
//!
//! `roadmap-detailed.md` → *Tier 2 — Widget Styling* asks a theme to choose a
//! button's shape, how a text field is edged and shows that it has the
//! keyboard, a scrollbar's width and whether it stays on screen, and whether
//! an on/off setting is a pill or a box -- "the variables that define the
//! visual feel (flat modern vs. skeuomorphic vs. glassmorphism)". This module
//! is that choice as a value. A theme's `widget-style` section is read by
//! `appearance::themes`, the user picks which theme's to use as
//! `theme.widget_style` in `appearance.yaml` -- separately from the colours,
//! since every axis is its own choice -- and a [`Palette`] carries the result
//! to every draw site ([`Palette::widget_style`]), as it carries whether boxes
//! are outlined. `design-decisions.md` §1435 has the reasoning.
//!
//! # What a widget style does not choose
//!
//! - **Colours.** They are the colours axis's. A scrollbar's thumb is the
//!   palette's `surface2`, a text field's well is `crust`, and every mark that
//!   says "this has the keyboard" is the user's accent. A style chooses
//!   shapes; which colours fill them was chosen elsewhere.
//! - **Whether a control can be found and read.** Every choice here keeps the
//!   control's edge, its state and its focus visible. A focus mark is always
//!   drawn in the accent at the user's focus width (never a hairline); a field
//!   edged only underneath still has its well; and under high contrast the
//!   choices that trade legibility for looks are put back
//!   ([`WidgetStyle::for_high_contrast`]).
//! - **Where anything is.** A style does not move controls or resize the room
//!   they are given, the rule `roadmap-detailed.md` → *What Themes Do NOT
//!   Control* sets out. That is why a button's padding is not here yet: the
//!   width a button's label needs decides where the next button in a row
//!   begins, and the dialogs lay their rows out before they are drawn.
//!   `todo.txt` → *Judgment Calls* has the rest.
//!
//! # Whole pixels
//!
//! Every measure is a whole number of pixels at the user's scale of one. A
//! [`Palette`] is compared for equality -- to tell whether anything has to be
//! redrawn -- so what it carries must be `Eq`, and a theme's author writes
//! `radius: 4`, not `radius: 4.25`.
//!
//! [`Palette`]: crate::palette::Palette
//! [`Palette::widget_style`]: crate::palette::Palette::widget_style

/// How the toolkit's controls are shaped. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetStyle {
    /// Push buttons: `crate::button`.
    pub button: ButtonStyle,
    /// Text fields, and the drop-down fields drawn in the same well.
    pub field: FieldStyle,
    /// Check boxes: `crate::checkbox`.
    pub check: CheckStyle,
    /// On/off switches: `crate::switch`.
    pub toggle: ToggleStyle,
    /// Scrollbars.
    pub scrollbar: ScrollbarStyle,
}

impl WidgetStyle {
    /// The built-in theme's controls: `Aero Desktop (offline).html`'s.
    ///
    /// Each value is the one the reference's stylesheet gives, or -- where it
    /// gives none -- the one the toolkit drew before styles existed, so the
    /// built-in theme looks exactly as it did.
    pub const AERO: Self = Self {
        button: ButtonStyle {
            // `.aero-srch-btn { border-radius: 4px }`.
            radius: 4,
            // Its `linear-gradient(to bottom, #fbfdff, #e6eef6)`.
            gloss: true,
            // It has no `box-shadow`.
            shadow: false,
        },
        field: FieldStyle {
            // `.aero-srch-in { border-radius: 3px; border: 1px solid ... }`.
            radius: 3,
            border: FieldBorder::Box,
            // `.aero-srch-in:focus { border-color: #6aa6dc;
            //   box-shadow: ..., 0 0 0 2px rgba(120, 185, 235, 0.35) }`.
            focus: FocusMark::Glow,
        },
        // The reference leaves its check boxes to the browser; 2 is what the
        // toolkit's box has always had.
        check: CheckStyle { radius: 2 },
        // Every on/off setting the shell has ever drawn is a pill.
        toggle: ToggleStyle::Pill,
        // The reference leaves its scrollbars to the browser, whose bars stay
        // on screen; 10 pixels is what the toolkit has always drawn.
        scrollbar: ScrollbarStyle {
            width: ScrollbarWidth::Normal,
            visibility: ScrollbarVisibility::Always,
        },
    };

    /// This style as a high-contrast scheme draws it: each choice that trades
    /// how plainly a control shows for how it looks is put back, and every
    /// other choice kept.
    ///
    /// A high-contrast scheme is chosen for need, not taste, and it replaces
    /// the colours whole. The shapes are mostly not its business -- round
    /// corners or square, pill or box, a user who needs contrast can see
    /// either -- but four choices are:
    ///
    /// - **gloss**, a second shade across the upper half of a button, is a
    ///   second ground for its label to be read on;
    /// - **a shadow** is a soft edge, where this mode draws hard ones;
    /// - **a glow or an underline** says "this has the keyboard" less plainly
    ///   than a ring all the way round;
    /// - **a scrollbar that hides** is a control that cannot be seen until it
    ///   is found.
    #[must_use]
    pub const fn for_high_contrast(self) -> Self {
        Self {
            button: ButtonStyle {
                gloss: false,
                shadow: false,
                ..self.button
            },
            field: FieldStyle {
                focus: FocusMark::Ring,
                ..self.field
            },
            scrollbar: ScrollbarStyle {
                visibility: ScrollbarVisibility::Always,
                ..self.scrollbar
            },
            ..self
        }
    }
}

impl Default for WidgetStyle {
    fn default() -> Self {
        Self::AERO
    }
}

/// How a push button is shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ButtonStyle {
    /// The corners' radius, up to [`ButtonStyle::MAX_RADIUS`]. A button is
    /// never rounder than a pill: a radius past half its height is drawn as
    /// half its height.
    pub radius: u8,
    /// Whether the upper half of the face is a shade brighter -- the glass of
    /// the built-in theme. Without it a face is one flat colour.
    pub gloss: bool,
    /// Whether a soft shadow sits under the button, lifting it off the page.
    pub shadow: bool,
}

impl ButtonStyle {
    /// The roundest a theme may ask a button to be: half the height of the
    /// toolkit's button, which is a pill.
    pub const MAX_RADIUS: u8 = 14;
}

/// How a text field is shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldStyle {
    /// The well's corners' radius, up to [`FieldStyle::MAX_RADIUS`].
    pub radius: u8,
    /// Where the well's edge is drawn.
    pub border: FieldBorder,
    /// How a field shows that it has the keyboard.
    pub focus: FocusMark,
}

impl FieldStyle {
    /// The roundest a theme may ask a field to be: half the height of the
    /// reference's 26-pixel field.
    pub const MAX_RADIUS: u8 = 13;
}

/// Where a text field's edge is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FieldBorder {
    /// A line all the way round the well.
    #[default]
    Box,
    /// A line along the bottom of the well only: the flat look, where the
    /// well's own shade marks the field and the line says where to write.
    Underline,
}

/// How a control shows that it has the keyboard. Always in the user's accent,
/// always at least the user's focus width -- only the shape is the theme's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FocusMark {
    /// A ring round the control, outside its edge.
    #[default]
    Ring,
    /// The control's edge in the accent, with a soft halo of it outside: the
    /// built-in theme's, after the reference's `box-shadow` glow.
    Glow,
    /// A bar of the accent along the bottom of the control.
    Underline,
}

/// How a check box is shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckStyle {
    /// The box's corners' radius, up to [`CheckStyle::MAX_RADIUS`] -- which
    /// makes the box a circle.
    pub radius: u8,
}

impl CheckStyle {
    /// Half the toolkit's 14-pixel box: a circle.
    pub const MAX_RADIUS: u8 = 7;
}

/// How an on/off setting is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToggleStyle {
    /// A pill with a knob at the end that says which.
    #[default]
    Pill,
    /// A check box, ticked when on -- in the room the pill would have had, so
    /// what a click lands on does not move.
    Checkbox,
}

/// How a scrollbar is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollbarStyle {
    /// How wide it is.
    pub width: ScrollbarWidth,
    /// Whether it stays on screen.
    pub visibility: ScrollbarVisibility,
}

/// How wide a scrollbar is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollbarWidth {
    /// Six pixels: out of the way, and harder to take hold of.
    Thin,
    /// Ten pixels: what the toolkit has always drawn.
    #[default]
    Normal,
    /// Fourteen pixels: the easiest to take hold of.
    Wide,
}

impl ScrollbarWidth {
    /// The width in pixels, at the user's scale of one.
    #[must_use]
    pub const fn pixels(self) -> u8 {
        match self {
            Self::Thin => 6,
            Self::Normal => 10,
            Self::Wide => 14,
        }
    }
}

/// Whether a scrollbar stays on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollbarVisibility {
    /// Always drawn, in a column of its own beside what it scrolls.
    #[default]
    Always,
    /// Drawn over what it scrolls, and only while the pointer is over the
    /// scrolled view or the bar is held: the view has the whole width, and the
    /// bar is there when a hand goes to look for it.
    Overlay,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The built-in style is the reference's, and the default.**
    #[test]
    fn the_built_in_style_is_the_default() {
        assert_eq!(WidgetStyle::default(), WidgetStyle::AERO);
        assert_eq!(WidgetStyle::AERO.button.radius, 4);
        assert_eq!(WidgetStyle::AERO.field.radius, 3);
        assert_eq!(WidgetStyle::AERO.field.focus, FocusMark::Glow);
        assert_eq!(WidgetStyle::AERO.toggle, ToggleStyle::Pill);
    }

    /// **High contrast puts back what hides, and keeps what does not.** A
    /// style chosen for looks -- a flat, rounded, overlaid one -- keeps its
    /// corners, its pill and its width, and loses its gloss, its shadow, its
    /// soft focus mark and its hiding scrollbar.
    #[test]
    fn high_contrast_puts_back_only_what_hides() {
        let chosen = WidgetStyle {
            button: ButtonStyle {
                radius: 9,
                gloss: true,
                shadow: true,
            },
            field: FieldStyle {
                radius: 6,
                border: FieldBorder::Underline,
                focus: FocusMark::Underline,
            },
            check: CheckStyle { radius: 7 },
            toggle: ToggleStyle::Checkbox,
            scrollbar: ScrollbarStyle {
                width: ScrollbarWidth::Thin,
                visibility: ScrollbarVisibility::Overlay,
            },
        };
        let hc = chosen.for_high_contrast();
        assert!(!hc.button.gloss && !hc.button.shadow);
        assert_eq!(hc.field.focus, FocusMark::Ring);
        assert_eq!(hc.scrollbar.visibility, ScrollbarVisibility::Always);
        // Kept.
        assert_eq!(hc.button.radius, 9);
        assert_eq!(hc.field.radius, 6);
        assert_eq!(hc.field.border, FieldBorder::Underline);
        assert_eq!(hc.check, chosen.check);
        assert_eq!(hc.toggle, ToggleStyle::Checkbox);
        assert_eq!(hc.scrollbar.width, ScrollbarWidth::Thin);
    }

    /// **The widths are ordered and the normal one is the old constant**, so
    /// the built-in theme's scrollbars did not change width.
    #[test]
    fn the_scrollbar_widths_are_ordered() {
        assert!(ScrollbarWidth::Thin.pixels() < ScrollbarWidth::Normal.pixels());
        assert!(ScrollbarWidth::Normal.pixels() < ScrollbarWidth::Wide.pixels());
        #[allow(clippy::float_cmp, reason = "a small whole number, exactly")]
        {
            assert_eq!(
                f32::from(ScrollbarWidth::Normal.pixels()),
                crate::scrollbar::WIDTH
            );
        }
    }

    /// **The limits are the shapes they say they are**: half the height of
    /// the button, of the reference's field, and of the check box.
    #[test]
    fn the_limits_are_halves() {
        #[allow(clippy::float_cmp, reason = "small whole numbers, exactly")]
        {
            assert_eq!(
                f32::from(ButtonStyle::MAX_RADIUS),
                crate::button::HEIGHT / 2.0
            );
            assert_eq!(
                f32::from(CheckStyle::MAX_RADIUS),
                crate::checkbox::SIZE / 2.0
            );
        }
        assert_eq!(FieldStyle::MAX_RADIUS, 26 / 2);
    }
}
