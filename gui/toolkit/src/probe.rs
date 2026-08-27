//! Driving a window from a test the way a user drives it: [`Probe`].
//!
//! [`crate::frame::Frame`] gives a test the geometry it needs — every control
//! the renderer painted, and the box it painted it in. What it does not give is
//! the four lines that turn that into an action:
//!
//! ```text
//! let rect = frame.rect_of(|t| *t == Target::Connect).expect("…");
//! let (cx, cy) = rect.centre();
//! app.handle_click(cx, cy, MouseButton::Left, SIZE)
//! ```
//!
//! Those four lines are the same in every application, and by the third one
//! they had been copied verbatim — helper names, doc comments and all — from
//! `apps/netmanager` into `apps/vpnmanager`. There are over a hundred more
//! programs waiting to be wired to a real window, so the choice is between one
//! copy here and a hundred and twenty-two copies there. This module is the one
//! copy.
//!
//! # What a program implements
//!
//! [`Probe`] is three methods over whatever the program already has: draw at a
//! size, deliver a click, deliver a keystroke.
//!
//! ```
//! use guitk::frame::{Frame, Rect};
//! use guitk::event::{KeyEvent, MouseButton};
//! use guitk::probe::{self, Probe};
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//! enum Target { Ok, Cancel }
//!
//! #[derive(Default)]
//! struct Dialog { accepted: bool, dismissed: bool }
//!
//! impl Dialog {
//!     fn render(&self) -> Frame<Target> {
//!         let mut frame = Frame::new(200.0, 100.0);
//!         frame.hit(Target::Ok, Rect::new(10.0, 60.0, 80.0, 24.0));
//!         frame.hit(Target::Cancel, Rect::new(110.0, 60.0, 80.0, 24.0));
//!         frame
//!     }
//! }
//!
//! impl Probe for Dialog {
//!     type Target = Target;
//!     type Outcome = ();
//!     const SIZE: (f32, f32) = (200.0, 100.0);
//!
//!     fn draw(&self, _size: (f32, f32)) -> Frame<Target> { self.render() }
//!
//!     fn click_at(&mut self, x: f32, y: f32, _b: MouseButton, size: (f32, f32)) {
//!         match self.draw(size).hit_test(x, y) {
//!             Some(Target::Ok) => self.accepted = true,
//!             Some(Target::Cancel) => self.dismissed = true,
//!             None => {}
//!         }
//!     }
//!
//!     fn key_at(&mut self, _key: &KeyEvent, _size: (f32, f32)) {}
//! }
//!
//! let mut dialog = Dialog::default();
//! probe::click(&mut dialog, Target::Ok);
//! assert!(dialog.accepted);
//! assert!(!dialog.dismissed, "the click landed on one button, not both");
//! ```
//!
//! # What it buys
//!
//! - **A control is named, never measured.** [`click`] asks the renderer where
//!   it drew the control and clicks the middle of that. A test written this way
//!   keeps testing the same button after the layout moves it, and starts
//!   failing the moment the button stops being drawn at all — which is the one
//!   thing a coordinate-literal test can never catch.
//! - **Absence is a failure, not a skip.** [`click`] panics naming the control
//!   and listing what *was* on screen. The alternative — an `if let Some(rect)`
//!   that quietly does nothing — is a test that passes while the feature is
//!   missing.
//! - **Topmost wins, once.** [`rect_of`] resolves through
//!   [`Frame::rect_of`](crate::frame::Frame::rect_of), so a control drawn twice
//!   resolves to the copy a real click would reach, and a control behind a
//!   modal resolves to nothing at all.
//!
//! # Why it is not `#[cfg(test)]`
//!
//! A `#[cfg(test)]` module is compiled only when *its own* crate is under
//! test, so it is invisible to the applications that need it — which is the
//! entire point of putting it here. The same reasoning already put
//! `oswindow::testing` in that crate's public API. The cost is a handful of
//! functions in the library: all but five are generic, so they are only
//! instantiated by a crate that calls them, and the five that are not
//! ([`typing`], [`press`], [`press_with`], [`ctrl`], [`shift`]) are each a
//! struct literal the linker drops when unused.

// A test harness's contract is to fail loudly the instant its subject is
// wrong, so panicking on a control that is not there is the feature and not
// the oversight: a `click` that silently did nothing would turn a missing
// button into a passing test. The panics are documented per-function under
// `# Panics`.
//
// Note what is *not* allowed here: `indexing_slicing` and
// `arithmetic_side_effects` stay on. Those would not be the harness reporting
// a fault in its subject — they would be faults in the harness itself, and a
// harness that panics on its own bug reports a failure the subject did not
// commit.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::fmt::Debug;

use crate::event::{Key, KeyEvent, Modifiers, MouseButton};
use crate::frame::{Frame, Rect};

/// A program a test can drive by naming its controls.
///
/// The three methods are the program's own render and input entry points,
/// which it has already written; implementing this trait is wiring, not new
/// behaviour. See the [module docs](self) for a worked example.
pub trait Probe {
    /// The renderer's control identifier — the `Target` enum the program
    /// records with [`Frame::hit`](crate::frame::Frame::hit).
    ///
    /// `Debug` is required because it is what a failed [`click`] prints, and a
    /// panic that named no control would be no better than an assertion with
    /// no message.
    type Target: Copy + PartialEq + Debug;

    /// Whatever the program's input handlers return — typically an `Action`
    /// enum saying what the window wants done next. `()` is fine for a program
    /// whose handlers return nothing.
    type Outcome;

    /// The size these helpers draw and click at unless told otherwise.
    ///
    /// Almost always the program's `(WINDOW_WIDTH, WINDOW_HEIGHT)`. The
    /// `*_sized` helpers exist for the tests that deliberately use a different
    /// one — a window too small for its own layout is a case worth a test, and
    /// it is not the case the other ninety tests want.
    const SIZE: (f32, f32);

    /// Draw the whole window at `size` and hand back the frame, hit boxes and
    /// all.
    ///
    /// Must *believe* `size` rather than any size the program remembers from a
    /// previous frame: the first frame a real window submits goes out before
    /// any resize event arrives, so a renderer that trusted its remembered
    /// size would draw that frame at the wrong one.
    fn draw(&self, size: (f32, f32)) -> Frame<Self::Target>;

    /// Deliver a click at window coordinates `(x, y)`.
    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome;

    /// Deliver a keystroke.
    ///
    /// `size` is passed for the programs whose key handling needs it — one
    /// that scrolls a selection into view has to know how tall the pane is.
    /// Programs whose handler takes no size ignore it.
    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome;
}

/// The box the renderer drew for `target`, or `None` if it drew nothing for it
/// in this state.
///
/// Topmost first — see [`Frame::rect_of`](crate::frame::Frame::rect_of) for
/// why that is the only defensible order.
#[must_use]
pub fn rect_of<P: Probe>(app: &P, target: P::Target) -> Option<Rect> {
    rect_of_sized(app, target, P::SIZE)
}

/// [`rect_of`] at a size other than [`Probe::SIZE`].
#[must_use]
pub fn rect_of_sized<P: Probe>(app: &P, target: P::Target, size: (f32, f32)) -> Option<Rect> {
    rect_matching_sized(app, |t| *t == target, size)
}

/// The box drawn for the topmost target satisfying `pred`, or `None`.
///
/// The predicate form is for targets that carry a payload the test does not
/// want to spell out: `ColumnHeader(Column::Size)` is worth naming exactly,
/// but "whichever row is selected" and "any `Row`" are not expressible as an
/// equality, and a test that had to reconstruct the payload to name the
/// control would be asserting on its own arithmetic.
#[must_use]
pub fn rect_matching<P: Probe>(app: &P, pred: impl Fn(&P::Target) -> bool) -> Option<Rect> {
    rect_matching_sized(app, pred, P::SIZE)
}

/// [`rect_matching`] at a size other than [`Probe::SIZE`].
#[must_use]
pub fn rect_matching_sized<P: Probe>(
    app: &P,
    pred: impl Fn(&P::Target) -> bool,
    size: (f32, f32),
) -> Option<Rect> {
    app.draw(size).rect_of(pred)
}

/// The topmost target satisfying `pred`, payload and all.
///
/// Useful when the test wants to *read* what was drawn — which row the
/// renderer put at the top of a scrolled list, say — rather than assert
/// against a value it computed itself.
#[must_use]
pub fn target_matching<P: Probe>(app: &P, pred: impl Fn(&P::Target) -> bool) -> Option<P::Target> {
    app.draw(P::SIZE)
        .hits()
        .iter()
        .rev()
        .find(|(target, _)| pred(target))
        .map(|(target, _)| *target)
}

/// Whether `target` is drawn at all in this state.
///
/// Note that "drawn" here means *reachable*: a control underneath a modal
/// sheet is not, because the sheet discards the hits recorded behind it.
#[must_use]
pub fn is_visible<P: Probe>(app: &P, target: P::Target) -> bool {
    rect_of(app, target).is_some()
}

/// Left-click the middle of whatever the renderer drew for `target`.
///
/// # Panics
///
/// If nothing was drawn for `target`, naming it and listing the controls that
/// *were* on screen. That is the point: a control that is not on screen cannot
/// be clicked, and a helper that silently skipped the click would let a test
/// pass while the button was missing.
pub fn click<P: Probe>(app: &mut P, target: P::Target) -> P::Outcome {
    click_with(app, target, MouseButton::Left)
}

/// [`click`] with a button other than the left one.
///
/// # Panics
///
/// As [`click`].
pub fn click_with<P: Probe>(app: &mut P, target: P::Target, button: MouseButton) -> P::Outcome {
    click_sized(app, target, button, P::SIZE)
}

/// [`click_with`] at a size other than [`Probe::SIZE`].
///
/// # Panics
///
/// As [`click`].
pub fn click_sized<P: Probe>(
    app: &mut P,
    target: P::Target,
    button: MouseButton,
    size: (f32, f32),
) -> P::Outcome {
    click_matching_sized(app, |t| *t == target, &format!("{target:?}"), button, size)
}

/// Left-click the middle of the topmost control satisfying `pred`.
///
/// `what` names it for the panic message — "the Size column header", "a
/// selected row" — since a predicate cannot describe itself.
///
/// # Panics
///
/// If nothing matching was drawn. As [`click`].
pub fn click_matching<P: Probe>(
    app: &mut P,
    pred: impl Fn(&P::Target) -> bool,
    what: &str,
) -> P::Outcome {
    click_matching_sized(app, pred, what, MouseButton::Left, P::SIZE)
}

/// [`click_matching`] with a button and a size chosen explicitly.
///
/// # Panics
///
/// As [`click`].
pub fn click_matching_sized<P: Probe>(
    app: &mut P,
    pred: impl Fn(&P::Target) -> bool,
    what: &str,
    button: MouseButton,
    size: (f32, f32),
) -> P::Outcome {
    let rect = rect_matching_sized(app, pred, size)
        .unwrap_or_else(|| panic!("{}", missing(app, what, size)));
    let (cx, cy) = rect.centre();
    app.click_at(cx, cy, button, size)
}

/// Click a point that nothing was drawn at, which is how a test says "the
/// user clicked the background".
///
/// Searches the window for a point outside every recorded box rather than
/// trusting a corner to be bare — a program that paints a full-window
/// backdrop target has no bare corner, and a test that assumed one would be
/// clicking that backdrop while believing it clicked nothing.
///
/// # Panics
///
/// If every point it looked at is covered, since there is then no such thing
/// as clicking the background in this state and the test is asking for
/// something that cannot happen.
pub fn click_background<P: Probe>(app: &mut P) -> P::Outcome {
    let size = P::SIZE;
    let (x, y) = bare_point(app, size)
        .unwrap_or_else(|| panic!("every point in the window is covered by some control"));
    app.click_at(x, y, MouseButton::Left, size)
}

/// A point in the window that no recorded box covers, or `None` if the sweep
/// found none.
///
/// Sweeps on a grid rather than testing every pixel: the gaps between controls
/// in a real layout are several pixels wide, and a sweep fine enough to find a
/// one-pixel gap would cost more than the answer is worth.
#[must_use]
pub fn bare_point<P: Probe>(app: &P, size: (f32, f32)) -> Option<(f32, f32)> {
    /// Coarse enough to be cheap, fine enough to find the gap between two
    /// controls in any layout a person would call a layout.
    const STEP: f32 = 4.0;

    let frame = app.draw(size);
    let (width, height) = size;
    let mut y = STEP / 2.0;
    while y < height {
        let mut x = STEP / 2.0;
        while x < width {
            if frame.hit_test_ref(x, y).is_none() {
                return Some((x, y));
            }
            x += STEP;
        }
        y += STEP;
    }
    None
}

/// A keystroke that types `text` and nothing else.
///
/// The `key` code is deliberately arbitrary: a handler that reacted to
/// [`Key::A`] rather than to the text would be reacting to the wrong thing,
/// and this is the shape of event that catches it.
#[must_use]
pub fn typing(text: &str) -> KeyEvent {
    KeyEvent {
        key: Key::A,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: text.to_string(),
    }
}

/// A keystroke that types nothing — an arrow, `Tab`, `Escape`, `Enter`.
#[must_use]
pub fn press(key: Key) -> KeyEvent {
    press_with(key, Modifiers::NONE)
}

/// [`press`] with modifiers held down.
#[must_use]
pub fn press_with(key: Key, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers,
        text: String::new(),
    }
}

/// [`press`] with Ctrl held down — the accelerator shape, `Ctrl+O`.
#[must_use]
pub fn ctrl(key: Key) -> KeyEvent {
    press_with(
        key,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    )
}

/// [`press`] with Shift held down, for the range-extending half of a
/// selection.
#[must_use]
pub fn shift(key: Key) -> KeyEvent {
    press_with(
        key,
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    )
}

/// Type `text` one character at a time, as a keyboard delivers it.
///
/// One event per `char`, not per byte: a handler that appended `text` to a
/// `String` would pass either way, but one that pushed a `u8` would corrupt
/// anything outside ASCII, and only the per-`char` form catches that.
pub fn type_str<P: Probe>(app: &mut P, text: &str) {
    for ch in text.chars() {
        let mut buffer = [0u8; 4];
        app.key_at(&typing(ch.encode_utf8(&mut buffer)), P::SIZE);
    }
}

/// Deliver one keystroke at [`Probe::SIZE`].
pub fn key<P: Probe>(app: &mut P, event: &KeyEvent) -> P::Outcome {
    app.key_at(event, P::SIZE)
}

/// Every control drawn in this state, by variant name, in paint order.
///
/// The name is truncated at the first `(`, so `Row(7)` and `Row(9)` both read
/// as `Row`. That is what a coverage test wants: it asks whether the program
/// can ever draw a `Row`, not which rows this particular state happens to
/// hold.
#[must_use]
pub fn control_names<P: Probe>(app: &P) -> Vec<String> {
    app.draw(P::SIZE)
        .hits()
        .iter()
        .map(|(target, _)| variant_name(*target))
        .collect()
}

/// The variant name of a target, truncated at the first `(`.
#[must_use]
pub fn variant_name<T: Debug>(target: T) -> String {
    let full = format!("{target:?}");
    match full.find('(') {
        Some(open) => full.get(..open).unwrap_or(&full).to_string(),
        None => full,
    }
}

/// The panic message for a control that is not on screen.
///
/// Lists what *is* on screen, because "nothing on screen for `Reconnect`" on
/// its own leaves the reader unable to tell a renamed control from a panel
/// that never opened — and the answer is right there in the frame.
fn missing<P: Probe>(app: &P, what: &str, size: (f32, f32)) -> String {
    let mut names: Vec<String> = app
        .draw(size)
        .hits()
        .iter()
        .map(|(t, _)| variant_name(*t))
        .collect();
    names.sort_unstable();
    names.dedup();
    format!(
        "nothing on screen for {what} at {size:?}; the frame drew: {}",
        if names.is_empty() {
            "no controls at all".to_string()
        } else {
            names.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is a
    // failure report, and an index out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use super::*;
    use crate::color::Color;
    use crate::render::RenderCommand;
    use crate::style::CornerRadii;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Target {
        Ok,
        Cancel,
        Row(usize),
        Buried,
    }

    /// A program just real enough to drive: it draws two buttons, a scrolling
    /// list of rows, and a control that is painted over by a later one.
    #[derive(Default)]
    struct Fake {
        clicked: Vec<Target>,
        typed: String,
        /// How far the row list is scrolled, in pixels.
        scroll: f32,
        /// When set, the frame is drawn with only the sheet's own controls, as
        /// a modal does.
        modal: bool,
    }

    impl Fake {
        fn render(&self, size: (f32, f32)) -> Frame<Target> {
            let (width, height) = size;
            let mut frame = Frame::new(width, height);

            // A control that a later one paints over, to prove `rect_of`
            // resolves to the copy a click would reach.
            frame.hit(Target::Buried, Rect::new(0.0, 0.0, width, 20.0));
            frame.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width,
                height: 20.0,
                color: Color::rgb(0, 0, 0),
                corner_radii: CornerRadii::ZERO,
            });

            // A scrolling list, clipped to the pane below the header.
            frame.push(RenderCommand::PushClip {
                x: 0.0,
                y: 20.0,
                width,
                height: 60.0,
            });
            frame.push(RenderCommand::PushTranslate {
                dx: 0.0,
                dy: 20.0 - self.scroll,
            });
            for row in 0..8 {
                frame.hit(
                    Target::Row(row),
                    Rect::new(0.0, row as f32 * 20.0, width, 20.0),
                );
            }
            frame.push(RenderCommand::PopTranslate);
            frame.push(RenderCommand::PopClip);

            if self.modal {
                frame.discard_hits();
            }

            frame.hit(Target::Ok, Rect::new(10.0, 100.0, 60.0, 24.0));
            frame.hit(Target::Cancel, Rect::new(80.0, 100.0, 60.0, 24.0));
            // The second copy of `Buried`, painted last and so the reachable
            // one.
            frame.hit(Target::Buried, Rect::new(150.0, 100.0, 40.0, 24.0));
            frame
        }
    }

    impl Probe for Fake {
        type Target = Target;
        type Outcome = Option<Target>;
        const SIZE: (f32, f32) = (200.0, 140.0);

        fn draw(&self, size: (f32, f32)) -> Frame<Target> {
            self.render(size)
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            _button: MouseButton,
            size: (f32, f32),
        ) -> Option<Target> {
            let hit = self.draw(size).hit_test(x, y);
            if let Some(target) = hit {
                self.clicked.push(target);
            }
            hit
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> Option<Target> {
            self.typed.push_str(&key.text);
            None
        }
    }

    #[test]
    fn a_click_aimed_by_name_lands_on_that_control() {
        let mut app = Fake::default();
        assert_eq!(click(&mut app, Target::Ok), Some(Target::Ok));
        assert_eq!(app.clicked, vec![Target::Ok]);
    }

    #[test]
    fn a_control_drawn_twice_resolves_to_the_copy_a_click_would_reach() {
        // The buried copy spans the whole header; the reachable one is a small
        // box at the bottom. If `rect_of` returned the buried one the click
        // would land on the header, and `click_at` — which hit-tests the same
        // frame back to front — would report whatever is painted there.
        let mut app = Fake::default();
        let rect = rect_of(&app, Target::Buried).expect("drawn twice, so certainly drawn");
        assert_eq!(rect.x, 150.0, "the topmost copy, not the header band");
        assert_eq!(
            click(&mut app, Target::Buried),
            Some(Target::Buried),
            "aiming at the returned rect must reach the control that was named"
        );
    }

    #[test]
    fn a_row_scrolled_out_of_its_pane_is_not_clickable() {
        let app = Fake::default();
        assert!(is_visible(&app, Target::Row(0)), "row 0 starts in view");
        assert!(
            !is_visible(&app, Target::Row(7)),
            "row 7 is below a 60px pane and must not be reachable"
        );
    }

    #[test]
    fn a_modal_hides_everything_behind_it_from_the_probe() {
        let app = Fake {
            modal: true,
            ..Fake::default()
        };
        assert!(!is_visible(&app, Target::Row(0)), "behind the sheet");
        assert!(is_visible(&app, Target::Ok), "on the sheet");
    }

    #[test]
    fn clicking_a_control_that_is_not_on_screen_says_what_was() {
        let mut app = Fake::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            click(&mut app, Target::Row(7));
        }))
        .expect_err("row 7 is off screen, so this must fail");
        let message = panic
            .downcast_ref::<String>()
            .expect("the helper panics with a String");
        assert!(message.contains("Row(7)"), "names the control: {message}");
        assert!(message.contains("Ok"), "lists what was drawn: {message}");
    }

    #[test]
    fn typing_delivers_one_event_per_character_not_per_byte() {
        let mut app = Fake::default();
        // A three-byte character: a handler that pushed bytes would mangle it.
        type_str(&mut app, "a\u{2603}b");
        assert_eq!(app.typed, "a\u{2603}b");
    }

    #[test]
    fn a_control_name_drops_its_payload_so_coverage_can_be_counted() {
        let app = Fake::default();
        let names = control_names(&app);
        assert!(
            names.iter().any(|name| name == "Row"),
            "Row(3) must read as Row: {names:?}"
        );
        assert!(
            !names.iter().any(|name| name.contains('(')),
            "no payload should survive: {names:?}"
        );
    }

    #[test]
    fn a_predicate_reaches_a_control_whose_payload_the_test_does_not_know() {
        // "whichever row is on top of the pane" is not an equality, and a test
        // that computed the index to name it would be asserting on its own
        // arithmetic rather than on the renderer.
        let app = Fake {
            scroll: 40.0,
            ..Fake::default()
        };
        assert_eq!(
            target_matching(&app, |t| matches!(t, Target::Row(_))),
            Some(Target::Row(4)),
            "row 2 and 3 scrolled out of the pane; 4 is the first still drawn"
        );
    }

    #[test]
    fn a_failed_predicate_click_names_what_the_test_was_looking_for() {
        let mut app = Fake::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            click_matching(&mut app, |t| matches!(t, Target::Row(99)), "row 99");
        }))
        .expect_err("there is no row 99");
        let message = panic
            .downcast_ref::<String>()
            .expect("the helper panics with a String");
        assert!(
            message.contains("row 99"),
            "a predicate cannot describe itself, so `what` must: {message}"
        );
    }

    #[test]
    fn ctrl_and_shift_hold_the_key_they_name_and_nothing_else() {
        let held = ctrl(Key::O);
        assert!(held.modifiers.ctrl && !held.modifiers.shift);
        assert!(held.text.is_empty(), "an accelerator types nothing");
        let held = shift(Key::Down);
        assert!(held.modifiers.shift && !held.modifiers.ctrl);
    }

    #[test]
    fn clicking_the_background_reaches_nothing() {
        let mut app = Fake::default();
        assert_eq!(click_background(&mut app), None);
        assert!(app.clicked.is_empty(), "the background is not a control");
    }

    #[test]
    fn the_bare_point_sweep_reports_a_fully_covered_window() {
        struct Covered;
        impl Probe for Covered {
            type Target = Target;
            type Outcome = ();
            const SIZE: (f32, f32) = (40.0, 40.0);
            fn draw(&self, size: (f32, f32)) -> Frame<Target> {
                let mut frame = Frame::new(size.0, size.1);
                frame.hit(Target::Ok, Rect::new(0.0, 0.0, size.0, size.1));
                frame
            }
            fn click_at(&mut self, _x: f32, _y: f32, _b: MouseButton, _size: (f32, f32)) {}
            fn key_at(&mut self, _key: &KeyEvent, _size: (f32, f32)) {}
        }
        assert_eq!(bare_point(&Covered, Covered::SIZE), None);
    }

    #[test]
    fn a_smaller_window_is_probed_at_the_size_it_is_given() {
        let app = Fake::default();
        let wide = rect_of_sized(&app, Target::Buried, (200.0, 140.0)).unwrap();
        let narrow = rect_of_sized(&app, Target::Buried, (100.0, 140.0)).unwrap();
        assert_eq!(wide, narrow, "this control does not move with the width");
        // The header copy does, which proves the size reached the renderer.
        let frame = app.draw((100.0, 140.0));
        let header = frame
            .hits()
            .iter()
            .find(|(t, _)| *t == Target::Buried)
            .map(|(_, rect)| *rect)
            .unwrap();
        assert_eq!(header.w, 100.0, "drawn at the size it was handed");
    }
}
