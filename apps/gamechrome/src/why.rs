//! Why a game's greyed button is greyed, said while the pointer rests on it.
//!
//! `design.txt` asks every program for "a hover tooltip for any menu option
//! or button that's currently disabled, explaining why it's disabled/how to
//! enable it", and the toolkit says it with one `guitk::disabled::WhyDisabled`
//! per window (`requests/c-e-say-why-a-control-is-disabled.md`). A game's
//! window holds a [`Reasons`], which is that and the little a game needs
//! round it:
//!
//! - a game's frame records a click box for every button it draws, greyed
//!   or not; [`greyed`] picks out the boxes of those greyed now, each with
//!   the game's reason, and [`Reasons::drawn`] takes them when the frame is
//!   done -- before the frame draws [`Reasons::render`] last, over
//!   everything. A frame with something over the buttons (the list of keys,
//!   a dialog) hands over none;
//! - every event goes to [`Reasons::event`] first: a pointer event says
//!   where the pointer is, its leaving that it has gone, and a tick lets
//!   time pass;
//! - the window's `tick_interval` asks [`Reasons::due_in`] beside its own,
//!   so a reason appears when its delay is up rather than with whatever is
//!   drawn next.
//!
//! The clock is the ticks' own: the sum of their `elapsed_ms`. A game that
//! ticks only while a reason waits has a clock that stands still between
//! waits, which measures each wait all the same. Read when the pointer comes
//! to rest, it has not counted the time since the last tick, so moved from
//! one greyed button to another before the first's reason appeared, the
//! pointer sees the second's as soon as the first's was due -- early, never
//! late (design-decisions §1244 weighs this against the real clock).

use guitk::disabled::{Disabled, WhyDisabled};
use guitk::event::{Event, MouseEventKind};
use guitk::frame::Rect;
use guitk::palette::Palette;
use guitk::render::RenderCommand;
use std::time::Duration;

/// One greyed control as a frame drew it: its box, `(x, y, width, height)`,
/// and why it is greyed -- a sentence for the player.
pub type Greyed = ((f32, f32, f32, f32), String);

/// The greyed controls among a frame's click boxes: each box whose target
/// `why` gives a reason for -- the game's own answer to "is this greyed now,
/// and why?". A game's frame records a box for every button it draws, greyed
/// or not, so the boxes the reasons are given for are the ones drawn.
pub fn greyed<T>(hits: &[(T, Rect)], why: impl Fn(&T) -> Option<&'static str>) -> Vec<Greyed> {
    hits.iter()
        .filter_map(|(target, r)| why(target).map(|why| ((r.x, r.y, r.w, r.h), why.to_owned())))
        .collect()
}

/// A game window's reasons for its greyed buttons. See the module's doc.
#[derive(Default)]
pub struct Reasons {
    /// The greyed buttons the last frame drew.
    drawn: Vec<Greyed>,
    /// The screen the last frame was drawn for, where the reason is kept.
    screen: (f32, f32),
    /// Where the pointer is; `None` once it has left, or while something
    /// covers the buttons.
    pointer: Option<(f32, f32)>,
    /// The reason the pointer rests on, and its delay.
    why: WhyDisabled,
    /// The window's clock, in milliseconds: every tick's `elapsed_ms`.
    clock_ms: u64,
}

impl Reasons {
    /// Nothing drawn and nothing to say.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The frame just drawn, on a screen `screen` big, drew these greyed
    /// buttons. The pointer is asked about again -- a button may have been
    /// greyed, freed or moved under a pointer that has not -- before the
    /// frame draws [`render`](Self::render).
    pub fn drawn(&mut self, greyed: Vec<Greyed>, screen: (f32, f32)) {
        self.drawn = greyed;
        self.screen = screen;
        self.settle();
    }

    /// Hand over one of the window's events: any pointer event says where
    /// the pointer is (its leaving, that it is gone), and a tick lets time
    /// pass. Returns whether what [`render`](Self::render) draws changed --
    /// a reason appeared, or one shown went: the moment to draw.
    pub fn event(&mut self, event: &Event) -> bool {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Leave => self.pointer_left(),
                _ => self.pointer_at(mouse.x, mouse.y),
            },
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            _ => false,
        }
    }

    /// The pointer is at (`x`, `y`). Returns whether what
    /// [`render`](Self::render) draws changed: a reason shown went.
    pub fn pointer_at(&mut self, x: f32, y: f32) -> bool {
        self.pointer = Some((x, y));
        self.settle()
    }

    /// The pointer left the window, or something was drawn over the buttons:
    /// any reason shown or waiting goes. Returns whether one shown went.
    pub fn pointer_left(&mut self) -> bool {
        self.pointer = None;
        self.why.pointer_left()
    }

    /// `elapsed_ms` passed. Returns whether a reason appeared: the moment
    /// to draw.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        self.clock_ms = self.clock_ms.saturating_add(elapsed_ms);
        self.why.tick(self.clock_ms)
    }

    /// How long until a reason appears, for the window's `tick_interval`;
    /// `None` when none is waiting to. Never nought: a wait starts at the
    /// clock's present reading, and the clock moves only in
    /// [`tick`](Self::tick), which shows the reason the moment it is due.
    #[must_use]
    pub fn due_in(&self) -> Option<Duration> {
        self.why.due_in(self.clock_ms).map(Duration::from_millis)
    }

    /// What the window's `tick_interval` answers: the sooner of the game's
    /// own tick, `own`, and the one a reason is waiting for.
    #[must_use]
    pub fn sooner(&self, own: Option<Duration>) -> Option<Duration> {
        match (own, self.due_in()) {
            (Some(own), Some(reason)) => Some(own.min(reason)),
            (own, reason) => own.or(reason),
        }
    }

    /// The reason showing, if one is.
    #[must_use]
    pub fn showing(&self) -> Option<&str> {
        self.why.showing()
    }

    /// What to draw over everything else: the reason showing, if one is.
    #[must_use]
    pub fn render(&self, palette: &Palette) -> Vec<RenderCommand> {
        self.why.render(palette)
    }

    /// Ask what the pointer rests on, against the buttons last drawn.
    fn settle(&mut self) -> bool {
        let Some(at) = self.pointer else {
            return self.why.pointer_left();
        };
        let boxes: Vec<Disabled<'_>> = self
            .drawn
            .iter()
            .map(|(bounds, why)| (*bounds, why.as_str()))
            .collect();
        self.why.pointer_at(at, self.clock_ms, self.screen, &boxes)
    }
}

/// A copy keeps what was drawn, where the pointer is and the clock; a reason
/// waiting or showing starts its wait again in the copy. The toolkit's
/// `WhyDisabled` cannot be copied (`requests/e-c-whydisabled-and-tooltip-
/// could-be-clone.md`), and a game is copied to try a line of play or to
/// branch its history, where what a tooltip was doing does not matter --
/// but the game's own state must be copyable whole, reasons and all.
impl Clone for Reasons {
    fn clone(&self) -> Self {
        let mut copy = Self {
            drawn: self.drawn.clone(),
            screen: self.screen,
            pointer: self.pointer,
            why: WhyDisabled::new(),
            clock_ms: self.clock_ms,
        };
        copy.settle();
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (800.0, 600.0);
    const NEW_GAME: (f32, f32, f32, f32) = (10.0, 10.0, 100.0, 30.0);
    const WHY: &str = "A game is under way: finish it, or resign it first.";

    fn greyed() -> Vec<Greyed> {
        vec![(NEW_GAME, WHY.to_owned())]
    }

    #[test]
    fn a_greyed_button_says_why_after_the_delay_and_asks_for_the_tick() {
        let mut reasons = Reasons::new();
        reasons.drawn(greyed(), SCREEN);
        assert!(!reasons.pointer_at(50.0, 20.0));
        assert_eq!(reasons.showing(), None, "the reason came at once");
        assert_eq!(reasons.due_in(), Some(Duration::from_millis(500)));
        assert!(!reasons.tick(499));
        assert_eq!(reasons.due_in(), Some(Duration::from_millis(1)));
        assert!(reasons.tick(1), "the tick that shows it asks for no frame");
        assert_eq!(reasons.showing(), Some(WHY));
        assert!(!reasons.render(&Palette::for_mode(false)).is_empty());
        assert_eq!(reasons.due_in(), None, "the window ticks on for nothing");
    }

    #[test]
    fn the_reason_goes_with_the_pointer_and_with_the_button() {
        let mut reasons = Reasons::new();
        reasons.drawn(greyed(), SCREEN);
        reasons.pointer_at(50.0, 20.0);
        reasons.tick(500);
        assert!(!reasons.pointer_at(60.0, 20.0), "moving within it hid it");
        assert_eq!(reasons.showing(), Some(WHY));
        assert!(reasons.pointer_at(300.0, 300.0), "moving off it kept it");
        assert_eq!(reasons.showing(), None);

        reasons.pointer_at(50.0, 20.0);
        reasons.tick(500);
        assert!(reasons.pointer_left(), "leaving kept it");
        assert_eq!(reasons.showing(), None);
        assert_eq!(reasons.due_in(), None);

        // The button freed under a pointer that stays: the next frame drawn
        // has no greyed button there, and the reason goes with it.
        reasons.pointer_at(50.0, 20.0);
        reasons.tick(500);
        reasons.drawn(Vec::new(), SCREEN);
        assert_eq!(reasons.showing(), None, "the reason outlived its button");
    }

    #[test]
    fn the_window_s_events_move_the_pointer_and_the_clock() {
        use guitk::event::MouseEvent;
        let mouse = |x, y, kind| Event::Mouse(MouseEvent { x, y, kind });
        let mut reasons = Reasons::new();
        reasons.drawn(greyed(), SCREEN);
        assert!(!reasons.event(&mouse(50.0, 20.0, MouseEventKind::Move)));
        assert!(!reasons.event(&Event::Tick { elapsed_ms: 499 }));
        assert!(
            reasons.event(&Event::Tick { elapsed_ms: 1 }),
            "a tick is not time"
        );
        assert_eq!(reasons.showing(), Some(WHY));
        assert!(
            !reasons.event(&Event::FocusIn),
            "an event of no pointer moved it"
        );
        assert!(
            reasons.event(&mouse(-1.0, -1.0, MouseEventKind::Leave)),
            "leaving is not the pointer gone"
        );
        assert_eq!(reasons.showing(), None);
    }

    #[test]
    fn the_greyed_boxes_are_those_the_game_gives_a_reason_for() {
        let hits = [
            ("new game", Rect::new(10.0, 10.0, 100.0, 30.0)),
            ("undo", Rect::new(120.0, 10.0, 60.0, 30.0)),
        ];
        let found = super::greyed(&hits, |target| {
            (*target == "undo").then_some("Nothing to undo.")
        });
        assert_eq!(
            found,
            vec![((120.0, 10.0, 60.0, 30.0), "Nothing to undo.".to_owned())]
        );
    }

    #[test]
    fn the_window_ticks_for_the_sooner_of_its_own_and_a_reason() {
        let mut reasons = Reasons::new();
        let own = Some(Duration::from_millis(100));
        assert_eq!(
            reasons.sooner(None),
            None,
            "a window with nothing to time ticks"
        );
        assert_eq!(reasons.sooner(own), own);
        reasons.drawn(greyed(), SCREEN);
        reasons.pointer_at(50.0, 20.0);
        assert_eq!(reasons.sooner(None), Some(Duration::from_millis(500)));
        assert_eq!(reasons.sooner(own), own, "the game's own tick was put off");
        assert_eq!(
            reasons.sooner(Some(Duration::from_secs(1))),
            Some(Duration::from_millis(500)),
            "the reason waits for the game's slower tick"
        );
    }

    #[test]
    fn a_copy_keeps_the_buttons_and_the_pointer_and_waits_again() {
        let mut reasons = Reasons::new();
        reasons.drawn(greyed(), SCREEN);
        reasons.pointer_at(50.0, 20.0);
        reasons.tick(500);
        let mut copy = reasons.clone();
        assert_eq!(copy.showing(), None, "the copy's wait did not start again");
        assert_eq!(copy.due_in(), Some(Duration::from_millis(500)));
        copy.tick(500);
        assert_eq!(
            copy.showing(),
            Some(WHY),
            "the copy lost the button or the pointer"
        );
        assert_eq!(reasons.showing(), Some(WHY), "copying changed the original");
    }

    #[test]
    fn a_pointer_that_has_left_starts_no_reason_when_a_frame_is_drawn() {
        let mut reasons = Reasons::new();
        reasons.pointer_at(50.0, 20.0);
        reasons.pointer_left();
        reasons.drawn(greyed(), SCREEN);
        assert_eq!(
            reasons.due_in(),
            None,
            "a reason waits for a pointer that left"
        );
    }

    #[test]
    fn a_button_greyed_under_a_resting_pointer_explains_itself() {
        let mut reasons = Reasons::new();
        reasons.drawn(Vec::new(), SCREEN);
        reasons.pointer_at(50.0, 20.0);
        assert_eq!(reasons.due_in(), None);
        reasons.drawn(greyed(), SCREEN);
        assert_eq!(reasons.due_in(), Some(Duration::from_millis(500)));
        reasons.tick(500);
        assert_eq!(reasons.showing(), Some(WHY));
    }
}
