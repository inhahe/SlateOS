//! Pop-up notifications ("toasts"): each notification the desktop files,
//! shown for a few seconds at the bottom-right corner, above the taskbar, the
//! moment it arrives (design-decisions §1447, open question C-Q32).
//!
//! The notification pane ([`crate::notif_pane`]) is the *history*: every
//! notification is filed there and stays until dismissed. A toast is the
//! *attention*: the user is shown the notification without opening anything,
//! and it goes by itself. [`crate::DesktopShell::notify`] does both -- and a
//! notification focus assist silenced ([`Notification::silent`]) is filed and
//! not shown, since "do not show me" is exactly what Do Not Disturb asks.
//! While the pane is open nothing is shown here: what arrives is in front of
//! the user already, in the pane.
//!
//! # How long one stays
//!
//! By priority ([`stays_for`]): low four seconds, normal six, high ten, and an
//! urgent one until it is closed. The pointer over the stack holds every
//! toast in it where it is -- a toast that leaves while it is being read is
//! one that could not be read. That time is a wait, not motion: the user's
//! animation speed does not change it (§1446).
//!
//! # Where
//!
//! Stacked upward from just above the taskbar at the screen's right edge --
//! beside the tray's bell, which opens the pane holding the same notifications
//! -- the newest at the bottom, nearest the bell. At most [`MAX_SHOWN`] at
//! once: a new one pushes the oldest out early, since the pane keeps it; only
//! urgent ones are never pushed out, and a newcomer waits for one of those to
//! be closed.
//!
//! # Motion
//!
//! A toast slides in from the screen's edge along the desktop's arriving
//! curve and out along its leaving one, as long as the motion makes
//! [`SLIDE_MS`], and the ones above close up the gap it leaves along the same
//! curve. An arrival stops at its place rather than springing past it: the
//! toast is drawn on a surface that ends there, and would be cut. Under a
//! still motion every toast is simply there, and simply gone.
//!
//! # One layout
//!
//! Where each toast is drawn and where a press lands on it are both read from
//! [`ToastStack::placed`], one description of the stack -- the daemon this
//! replaces drew a leaving toast in the stack and skipped it when hit-testing,
//! so for a quarter of a second a press on any toast above it landed on the
//! wrong one.

use crate::notif_pane::{NotifPriority, Notification};
use appearance::{Palette, Surface};
use guitk::color::Color;
use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::motion::Motion;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use std::collections::VecDeque;

/// A toast's width.
pub const TOAST_WIDTH: f32 = 360.0;

/// Room between the stack and the screen's edge, and between it and the
/// taskbar.
pub const MARGIN: f32 = 12.0;

/// Room between two toasts.
const GAP: f32 = 8.0;

/// Room inside a toast, round its text.
const PADDING: f32 = 14.0;

/// A toast's corner radius.
const RADIUS: f32 = 8.0;

/// Room round the stack for the toasts' shadows, on the sides that are not
/// the screen's edge: without it the surface they are drawn on would cut the
/// shadow off in a straight line.
pub const SHADOW_ROOM: f32 = 16.0;

/// The close button's size.
const CLOSE_SIZE: f32 = 20.0;

/// The most toasts shown at once.
pub const MAX_SHOWN: usize = 3;

/// The priority stripe's width, down the toast's left edge.
const STRIPE: f32 = 4.0;

/// The app name's font size.
const APP_SIZE: f32 = 11.0;

/// The title's font size.
const TITLE_SIZE: f32 = 13.0;

/// The body's font size.
const BODY_SIZE: f32 = 12.0;

/// Baseline-to-baseline spacing of the body.
const BODY_LINE: f32 = 16.0;

/// Most body lines shown before the rest is cut with a mark.
const BODY_MAX_LINES: usize = 3;

/// A toast's height with no body.
const BASE_HEIGHT: f32 = PADDING + 16.0 + 18.0 + PADDING;

/// How long a toast takes to slide in or out, as designed: against the
/// standard transition ([`Motion::STANDARD_MS`]), so the desktop's motion
/// scales it.
pub const SLIDE_MS: u32 = 250;

/// How long the stack takes to close up the gap a toast leaves, as designed.
const SETTLE_MS: u32 = 200;

/// How long a toast of `priority` stays on screen, in milliseconds; `None`
/// for one that stays until it is closed.
#[must_use]
pub const fn stays_for(priority: NotifPriority) -> Option<u64> {
    match priority {
        NotifPriority::Low => Some(4_000),
        NotifPriority::Normal => Some(6_000),
        NotifPriority::High => Some(10_000),
        NotifPriority::Urgent => None,
    }
}

/// What a press on a toast asked for, for the shell to act on.
#[derive(Clone, Debug, PartialEq)]
pub enum ToastEvent {
    /// The toast itself was pressed: open what the notification is about, as
    /// a press on its card in the pane does.
    Opened(u64),
    /// Its close button was pressed: the toast goes, and the notification
    /// stays in the pane, unread.
    Closed(u64),
    /// The toast was pressed with the secondary button, at `(x, y)` on the
    /// screen: offer what can be done about the program it came from, as a
    /// secondary press on its card in the pane does. The toast stays.
    MenuAsked {
        /// The program the notification came from.
        app: String,
        /// Where the press was, in screen coordinates.
        x: f32,
        /// Where the press was, in screen coordinates.
        y: f32,
    },
}

/// Where a toast is in its life.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Sliding in, this far through the slide.
    Arriving(f32),
    /// In its place, this many milliseconds of its time spent.
    Shown(u64),
    /// Sliding out, this far through the slide.
    Leaving(f32),
}

/// One toast.
#[derive(Clone, Debug)]
struct Toast {
    /// The notification's id -- the same id the pane files it under.
    id: u64,
    app_name: String,
    title: String,
    /// The body, wrapped to the toast's width when it arrived.
    body: Vec<String>,
    priority: NotifPriority,
    phase: Phase,
    /// Its height, fixed at arrival: the body is wrapped once.
    height: f32,
    /// How far above the stack's bottom its bottom edge is drawn: easing
    /// from `from` to `to`, `progress` of the way.
    settle: Settle,
}

/// A vertical place easing toward a new one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settle {
    from: f32,
    to: f32,
    progress: f32,
}

impl Settle {
    /// At `place`, not moving.
    const fn at(place: f32) -> Self {
        Self {
            from: place,
            to: place,
            progress: 1.0,
        }
    }

    /// Where it is drawn under `motion`.
    fn now(self, motion: Motion) -> f32 {
        let along = motion.arriving(self.progress).min(1.0);
        self.from + (self.to - self.from) * along
    }

    fn is_moving(self) -> bool {
        self.progress < 1.0
    }

    /// The highest it is drawn between now and the end of its easing: both
    /// ends while it eases, where it rests once it does not.
    fn highest(self) -> f32 {
        if self.is_moving() {
            self.from.max(self.to)
        } else {
            self.to
        }
    }
}

/// A toast placed: where it is drawn, in screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The notification's id.
    pub id: u64,
    /// The toast's rectangle.
    pub rect: Rect,
    /// Its close button's rectangle.
    pub close: Rect,
}

/// The toasts on screen, and those waiting for room. See the module docs.
#[derive(Clone, Debug)]
pub struct ToastStack {
    /// On screen, oldest first: the last is at the bottom.
    shown: Vec<Toast>,
    /// Waiting for room, oldest first -- only while every toast shown is
    /// urgent, since anything else is pushed out to make room.
    waiting: VecDeque<Toast>,
    /// How toasts move.
    motion: Motion,
    /// Whether the pointer is over the stack, holding every toast in it.
    held: bool,
    /// Whether a menu opened from a toast is up, holding every toast as the
    /// pointer does: the pointer goes from the toast to the menu, off the
    /// stack, and the toast the menu is about must not leave while its menu
    /// is being read. See [`hold_for_menu`](Self::hold_for_menu).
    held_for_menu: bool,
    /// What presses asked for since [`take_events`](Self::take_events).
    events: Vec<ToastEvent>,
    /// The screen's width, which the stack is placed against.
    screen_width: f32,
    /// Where the stack's bottom is: the taskbar's top, less the margin.
    bottom: f32,
}

impl Default for ToastStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastStack {
    /// No toasts, on a 1920 by 1080 screen with nothing below them, until
    /// [`place`](Self::place) says otherwise.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shown: Vec::new(),
            waiting: VecDeque::new(),
            motion: Motion::STANDARD,
            held: false,
            held_for_menu: false,
            events: Vec::new(),
            screen_width: 1920.0,
            bottom: 1080.0 - MARGIN,
        }
    }

    /// Put the stack against the right edge of a screen `screen_width` wide,
    /// its bottom a margin above `floor` -- the taskbar's top.
    pub fn place(&mut self, screen_width: f32, floor: f32) {
        self.screen_width = screen_width;
        self.bottom = floor - MARGIN;
    }

    /// Move as `motion` says from now on. Under a still motion every slide
    /// and every settling is where it was going at the next tick.
    pub fn set_motion(&mut self, motion: Motion) {
        self.motion = motion;
    }

    /// Show `notif`, sliding in at the bottom of the stack.
    ///
    /// When the stack is full the oldest toast that is not urgent starts to
    /// leave early, to make room -- the pane keeps it. When every toast shown
    /// is urgent, the newcomer waits for one to be closed.
    pub fn show(&mut self, notif: &Notification) {
        // Showing a notification that is already on screen again (an update
        // under the same id) replaces it in place rather than stacking twice.
        self.shown.retain(|t| t.id != notif.id);
        self.waiting.retain(|t| t.id != notif.id);
        let toast = Toast::new(notif);
        let motion = self.motion;
        if self.staying_count() >= MAX_SHOWN {
            let oldest = self
                .shown
                .iter_mut()
                .find(|t| !matches!(t.phase, Phase::Leaving(_)) && stays_for(t.priority).is_some());
            match oldest {
                Some(t) => t.leave(motion),
                None => {
                    self.waiting.push_back(toast);
                    return;
                }
            }
        }
        self.shown.push(toast);
        self.restack();
    }

    /// Take away every toast at once, with no slide: the pane is opening, and
    /// what they showed is in front of the user there.
    pub fn clear(&mut self) {
        self.shown.clear();
        self.waiting.clear();
        self.held = false;
    }

    /// Stop showing the notification `id`, wherever it is: it was read or
    /// dismissed in the pane.
    pub fn forget(&mut self, id: u64) {
        self.waiting.retain(|t| t.id != id);
        let motion = self.motion;
        if let Some(t) = self.shown.iter_mut().find(|t| t.id == id) {
            t.leave(motion);
        }
    }

    /// Stop showing everything from `app`: the user has just said not to
    /// show them notifications from it. What is on screen slides out, what
    /// was waiting for room is dropped; the pane keeps them all.
    pub fn forget_app(&mut self, app: &str) {
        self.waiting.retain(|t| t.app_name != app);
        let motion = self.motion;
        for t in self.shown.iter_mut().filter(|t| t.app_name == app) {
            t.leave(motion);
        }
    }

    /// Hold every toast where it is while a menu opened from one is up, or
    /// let them go on when it closes -- the pointer's hold, for the time the
    /// pointer is on the menu instead. The shell's, to set and to clear: the
    /// stack cannot see its menu close.
    pub fn hold_for_menu(&mut self, held: bool) {
        self.held_for_menu = held;
    }

    /// Whether anything holds the toasts where they are: the pointer over
    /// them, or a menu opened from one.
    const fn is_held(&self) -> bool {
        self.held || self.held_for_menu
    }

    /// How many toasts are on screen and not leaving.
    fn staying_count(&self) -> usize {
        self.shown
            .iter()
            .filter(|t| !matches!(t.phase, Phase::Leaving(_)))
            .count()
    }

    /// Give every toast its place: each one's bottom is the heights of those
    /// below it, and a toast whose place changed eases to the new one from
    /// where it is drawn. (A newcomer's place is the bottom, where it starts:
    /// it slides in from the side, never from above.)
    fn restack(&mut self) {
        let motion = self.motion;
        let mut below = 0.0_f32;
        for toast in self.shown.iter_mut().rev() {
            if (toast.settle.to - below).abs() > f32::EPSILON {
                toast.settle = Settle {
                    from: toast.settle.now(motion),
                    to: below,
                    progress: 0.0,
                };
            }
            below += toast.height + GAP;
        }
    }

    /// Advance every toast by `dt_ms` of wall time: slides, settling, and --
    /// unless the pointer or a menu holds them -- their time on screen.
    pub fn tick(&mut self, dt_ms: u64) {
        let motion = self.motion;
        let held = self.is_held();
        let mut gone = false;
        for toast in &mut self.shown {
            toast.phase = match toast.phase {
                Phase::Arriving(p) => {
                    let next = advance(p, dt_ms, motion, SLIDE_MS);
                    if next >= 1.0 {
                        Phase::Shown(0)
                    } else {
                        Phase::Arriving(next)
                    }
                }
                Phase::Shown(ms) => {
                    let ms = if held { ms } else { ms.saturating_add(dt_ms) };
                    match stays_for(toast.priority) {
                        Some(stay) if ms >= stay => Phase::Leaving(0.0),
                        _ => Phase::Shown(ms),
                    }
                }
                Phase::Leaving(p) => {
                    let next = advance(p, dt_ms, motion, SLIDE_MS);
                    if next >= 1.0 {
                        gone = true;
                        Phase::Leaving(1.0)
                    } else {
                        Phase::Leaving(next)
                    }
                }
            };
            toast.settle.progress = advance(toast.settle.progress, dt_ms, motion, SETTLE_MS);
        }
        if gone {
            self.shown
                .retain(|t| !matches!(t.phase, Phase::Leaving(p) if p >= 1.0));
            while self.staying_count() < MAX_SHOWN {
                let Some(next) = self.waiting.pop_front() else {
                    break;
                };
                self.shown.push(next);
            }
            self.restack();
        }
    }

    /// Whether anything is sliding or settling, and so whether another frame
    /// is owed.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.shown.iter().any(|t| {
            matches!(t.phase, Phase::Arriving(_) | Phase::Leaving(_)) || t.settle.is_moving()
        })
    }

    /// When the next toast's time is up, in milliseconds from now -- for a
    /// wake-up at that moment rather than a frame every sixtieth of a second
    /// while toasts sit still. `None` when nothing is waiting to go: no
    /// toasts, only urgent ones, or the pointer or a menu holding them.
    #[must_use]
    pub fn next_due_in(&self) -> Option<u64> {
        if self.is_held() {
            return None;
        }
        self.shown
            .iter()
            .filter_map(|t| match t.phase {
                Phase::Shown(ms) => stays_for(t.priority).map(|stay| stay.saturating_sub(ms)),
                _ => None,
            })
            .min()
    }

    /// Whether any toast is on screen.
    #[must_use]
    pub fn is_showing(&self) -> bool {
        !self.shown.is_empty()
    }

    /// The ids of the toasts on screen, top to bottom -- for a test, or an
    /// accessibility reader.
    #[must_use]
    pub fn ids(&self) -> Vec<u64> {
        self.shown.iter().map(|t| t.id).collect()
    }

    /// Where each toast is drawn, top to bottom, in screen coordinates. What
    /// is drawn and what a press lands on both come from here.
    #[must_use]
    pub fn placed(&self) -> Vec<Placed> {
        let rest_x = self.screen_width - MARGIN - TOAST_WIDTH;
        // Far enough to be off the screen: the width and the margin.
        let away = TOAST_WIDTH + MARGIN;
        self.shown
            .iter()
            .map(|t| {
                let out = match t.phase {
                    // Never past its place: the surface ends there.
                    Phase::Arriving(p) => 1.0 - self.motion.arriving(p).min(1.0),
                    Phase::Shown(_) => 0.0,
                    Phase::Leaving(p) => self.motion.leaving(p),
                };
                let x = rest_x + away * out;
                let bottom = self.bottom - t.settle.now(self.motion);
                let rect = Rect::new(x, bottom - t.height, TOAST_WIDTH, t.height);
                let close = Rect::new(
                    x + TOAST_WIDTH - PADDING - CLOSE_SIZE + 6.0,
                    rect.y + PADDING - 6.0,
                    CLOSE_SIZE,
                    CLOSE_SIZE,
                );
                Placed {
                    id: t.id,
                    rect,
                    close,
                }
            })
            .collect()
    }

    /// The rectangle the stack needs on screen -- every toast wherever its
    /// settling takes it, their shadows, and the strip to the screen's edge
    /// they slide through -- or `None` when nothing is shown. The surface the
    /// toasts are drawn on is this.
    ///
    /// Steady while the stack moves: a toast easing from one place to
    /// another is covered at both ends for the whole of the easing, so the
    /// surface is resized once when the stack changes, not every frame of the
    /// change.
    #[must_use]
    pub fn extent(&self) -> Option<Rect> {
        let highest = self
            .shown
            .iter()
            .map(|t| t.settle.highest() + t.height)
            .reduce(f32::max)?;
        let top = self.bottom - highest;
        let left = self.screen_width - MARGIN - TOAST_WIDTH - SHADOW_ROOM;
        let top = (top - SHADOW_ROOM).max(0.0);
        Some(Rect::new(
            left,
            top,
            self.screen_width - left,
            self.bottom + MARGIN - top,
        ))
    }

    /// A pointer event, in screen coordinates. Answers whether it landed on a
    /// toast -- a press there is the stack's, and asks for something through
    /// [`take_events`](Self::take_events).
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        let placed = self.placed();
        let under = placed
            .iter()
            .find(|p| contains(p.rect, event.x, event.y))
            .copied();
        match event.kind {
            MouseEventKind::Leave => {
                self.held = false;
                false
            }
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(hit) = under else {
                    return false;
                };
                let event = if contains(hit.close, event.x, event.y) {
                    ToastEvent::Closed(hit.id)
                } else {
                    ToastEvent::Opened(hit.id)
                };
                let motion = self.motion;
                if let Some(t) = self.shown.iter_mut().find(|t| t.id == hit.id) {
                    t.leave(motion);
                }
                self.events.push(event);
                true
            }
            // The secondary button asks what can be done about the toast's
            // program -- on the close button too, which is still the toast --
            // and the toast stays: the menu is about it.
            MouseEventKind::Press(MouseButton::Right) => {
                let Some(hit) = under else {
                    return false;
                };
                let Some(app) = self
                    .shown
                    .iter()
                    .find(|t| t.id == hit.id)
                    .map(|t| t.app_name.clone())
                else {
                    return false;
                };
                self.events.push(ToastEvent::MenuAsked {
                    app,
                    x: event.x,
                    y: event.y,
                });
                true
            }
            _ => {
                self.held = under.is_some();
                under.is_some()
            }
        }
    }

    /// What presses asked for since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<ToastEvent> {
        std::mem::take(&mut self.events)
    }

    /// Draw the stack, in screen coordinates.
    #[must_use]
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        for (placed, toast) in self.placed().iter().zip(&self.shown) {
            toast.render(p, placed, &mut cmds);
        }
        cmds
    }
}

impl Toast {
    fn new(notif: &Notification) -> Self {
        let body = body_lines(&notif.body);
        #[expect(clippy::cast_precision_loss, reason = "at most BODY_MAX_LINES lines")]
        let height = BASE_HEIGHT + BODY_LINE * body.len() as f32;
        Self {
            id: notif.id,
            app_name: notif.app_name.clone(),
            title: notif.title.clone(),
            body,
            priority: notif.priority,
            phase: Phase::Arriving(0.0),
            height,
            settle: Settle::at(0.0),
        }
    }

    /// Start sliding out from wherever it is drawn: an arrival turned round
    /// leaves from the place it had reached, which under a curve that
    /// arrives and leaves at different paces is not where the arrival's
    /// clock stood ([`Motion::when_leaving_at`]).
    fn leave(&mut self, motion: Motion) {
        self.phase = match self.phase {
            Phase::Leaving(p) => Phase::Leaving(p),
            Phase::Shown(_) => Phase::Leaving(0.0),
            Phase::Arriving(p) => {
                let out = 1.0 - motion.arriving(p).min(1.0);
                Phase::Leaving(motion.when_leaving_at(out))
            }
        };
    }

    fn render(&self, p: &Palette, placed: &Placed, cmds: &mut Vec<RenderCommand>) {
        let r = placed.rect;
        cmds.push(RenderCommand::BoxShadow {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 0.0,
            // Black: a shadow is an absence of light, not a colour.
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(RADIUS),
        });
        p.push_surface(cmds, r.x, r.y, r.w, r.h, RADIUS, Surface::Panel);
        cmds.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y + RADIUS,
            width: STRIPE,
            height: (r.h - 2.0 * RADIUS).max(0.0),
            color: self.priority.accent_color(p),
            corner_radii: CornerRadii::all(STRIPE / 2.0),
        });
        let text_x = r.x + PADDING + STRIPE;
        let text_width = TOAST_WIDTH - 2.0 * PADDING - STRIPE - CLOSE_SIZE;
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: r.y + PADDING,
            text: self.app_name.clone(),
            color: p.subtext0,
            font_size: APP_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_width),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: r.y + PADDING + 16.0,
            text: self.title.clone(),
            color: p.text,
            font_size: TITLE_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(text_width),
            overflow: TextOverflow::Ellipsis,
        });
        let mut y = r.y + PADDING + 16.0 + 18.0;
        for line in &self.body {
            cmds.push(RenderCommand::Text {
                x: text_x,
                y,
                text: line.clone(),
                color: p.subtext1,
                font_size: BODY_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(body_width()),
                overflow: TextOverflow::Ellipsis,
            });
            y += BODY_LINE;
        }
        let c = placed.close;
        cmds.push(RenderCommand::Text {
            x: text::center_x("x", c.x + c.w / 2.0, 12.0, FontWeightHint::Bold),
            y: c.y + 3.0,
            text: String::from("x"),
            color: p.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// How wide the body runs: the toast, less its padding and stripe.
fn body_width() -> f32 {
    TOAST_WIDTH - 2.0 * PADDING - STRIPE
}

/// `body` wrapped to the toast's width, at most [`BODY_MAX_LINES`] lines, the
/// last cut with a mark when there was more.
fn body_lines(body: &str) -> Vec<String> {
    if body.trim().is_empty() {
        return Vec::new();
    }
    let width = body_width();
    let mut lines = text::wrap(body, width, BODY_SIZE, FontWeightHint::Regular);
    if lines.len() > BODY_MAX_LINES {
        lines.truncate(BODY_MAX_LINES);
        if let Some(last) = lines.last_mut() {
            *last = text::elide(
                &format!("{last}\u{2026}"),
                width,
                "\u{2026}",
                BODY_SIZE,
                FontWeightHint::Regular,
            );
        }
    }
    lines
}

/// `progress` through a transition designed at `stated_ms`, advanced by
/// `dt_ms` under `motion` -- and at its end at once under a still motion,
/// however short the step (a step of nothing included).
fn advance(progress: f32, dt_ms: u64, motion: Motion, stated_ms: u32) -> f32 {
    let duration = motion.duration_ms(stated_ms);
    if duration == 0 {
        return 1.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame's and a transition's milliseconds are far inside f32's exact range"
    )]
    let step = dt_ms as f32 / duration as f32;
    (progress + step).min(1.0)
}

/// Whether `(x, y)` is inside `r`.
fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::motion::Curve;

    fn notif(id: u64, priority: NotifPriority) -> Notification {
        Notification {
            id,
            app_name: String::from("Mail"),
            title: format!("Message {id}"),
            body: String::from("Something arrived."),
            timestamp: 0,
            priority,
            read: false,
            action: None,
            silent: false,
        }
    }

    /// A stack on a 1920-wide screen with the taskbar's top at 1040.
    fn stack() -> ToastStack {
        let mut s = ToastStack::new();
        s.place(1920.0, 1040.0);
        s
    }

    /// Long enough for every slide and settling to finish.
    const SETTLED: u64 = 1_000;

    fn press(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }
    }

    fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// **A toast arrives, stays as long as its priority says, and goes.**
    #[test]
    fn a_toast_stays_as_long_as_its_priority_says() {
        for (priority, stay) in [
            (NotifPriority::Low, 4_000),
            (NotifPriority::Normal, 6_000),
            (NotifPriority::High, 10_000),
        ] {
            let mut s = stack();
            s.show(&notif(1, priority));
            assert!(s.is_moving(), "{priority:?} did not slide in");
            s.tick(SETTLED);
            assert!(!s.is_moving());
            assert_eq!(s.next_due_in(), Some(stay), "{priority:?}");
            s.tick(stay - 1);
            assert_eq!(s.ids(), [1], "{priority:?} left early");
            s.tick(1);
            assert!(s.is_moving(), "{priority:?} did not slide out");
            s.tick(SETTLED);
            assert!(!s.is_showing(), "{priority:?} never left");
        }
    }

    /// **An urgent toast stays until it is closed**, and nothing is due.
    #[test]
    fn an_urgent_toast_stays_until_closed() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Urgent));
        s.tick(SETTLED);
        s.tick(600_000);
        assert_eq!(s.ids(), [1]);
        // Not leaving, either: a toast sliding out is still listed.
        assert!(!s.is_moving(), "an urgent toast started to leave");
        assert_eq!(s.next_due_in(), None);
        let close = s.placed()[0].close;
        assert!(s.handle_mouse(&press(close.x + 2.0, close.y + 2.0)));
        assert_eq!(s.take_events(), [ToastEvent::Closed(1)]);
        s.tick(SETTLED);
        assert!(!s.is_showing());
    }

    /// **The pointer over the stack holds every toast in it**, and leaving
    /// lets them go on from where they were.
    #[test]
    fn the_pointer_holds_them() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Low));
        s.tick(SETTLED);
        let (x, y) = centre(s.placed()[0].rect);
        assert!(s.handle_mouse(&at(x, y, MouseEventKind::Move)));
        assert_eq!(s.next_due_in(), None, "held, nothing is due");
        s.tick(60_000);
        assert_eq!(s.ids(), [1], "a held toast left");
        assert!(!s.handle_mouse(&at(x, y, MouseEventKind::Leave)));
        assert_eq!(s.next_due_in(), Some(4_000));
        s.tick(4_000);
        s.tick(SETTLED);
        assert!(!s.is_showing());
    }

    /// **The newest is at the bottom, and they stack upward** a gap apart,
    /// the lowest a margin above the taskbar and at the screen's edge.
    #[test]
    fn the_newest_is_at_the_bottom() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Normal));
        s.tick(SETTLED);
        s.show(&notif(2, NotifPriority::Normal));
        s.tick(SETTLED);
        let placed = s.placed();
        assert_eq!(placed.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
        let (upper, lower) = (placed[0].rect, placed[1].rect);
        assert!((lower.y + lower.h - (1040.0 - MARGIN)).abs() < 0.01);
        assert!((upper.y + upper.h + GAP - lower.y).abs() < 0.01);
        assert!((lower.x + lower.w - (1920.0 - MARGIN)).abs() < 0.01);
    }

    /// **A full stack pushes the oldest out early** -- the pane keeps it --
    /// but never an urgent one: a newcomer waits for one of those to close.
    #[test]
    fn a_full_stack_pushes_the_oldest_out_but_not_an_urgent_one() {
        let mut s = stack();
        for id in 1..=4 {
            s.show(&notif(id, NotifPriority::Normal));
        }
        s.tick(SETTLED);
        assert_eq!(s.ids(), [2, 3, 4]);

        let mut urgent = stack();
        for id in 1..=3 {
            urgent.show(&notif(id, NotifPriority::Urgent));
        }
        urgent.show(&notif(4, NotifPriority::Normal));
        urgent.tick(SETTLED);
        assert_eq!(urgent.ids(), [1, 2, 3], "an urgent toast was pushed out");
        let close = urgent.placed()[0].close;
        urgent.handle_mouse(&press(close.x + 2.0, close.y + 2.0));
        urgent.tick(SETTLED);
        urgent.tick(SETTLED);
        assert_eq!(urgent.ids(), [2, 3, 4], "the newcomer did not get its turn");
    }

    /// **A press on a toast opens it; one on its close button closes it** --
    /// and either way the toast goes.
    #[test]
    fn a_press_opens_or_closes() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Normal));
        s.show(&notif(2, NotifPriority::Normal));
        s.tick(SETTLED);
        let placed = s.placed();
        let (x, y) = centre(placed[0].rect);
        assert!(s.handle_mouse(&press(x, y)));
        let close = placed[1].close;
        assert!(s.handle_mouse(&press(close.x + 1.0, close.y + 1.0)));
        assert_eq!(
            s.take_events(),
            [ToastEvent::Opened(1), ToastEvent::Closed(2)]
        );
        assert!(s.take_events().is_empty(), "events are taken once");
        assert!(!s.handle_mouse(&press(10.0, 10.0)), "a press elsewhere");
        s.tick(SETTLED);
        s.tick(SETTLED);
        assert!(!s.is_showing());
    }

    /// **One layout draws and is pressed**: while the lower toast slides
    /// out, a press on the one above lands on it where it is drawn.
    #[test]
    fn one_layout_draws_and_is_pressed() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Normal));
        s.show(&notif(2, NotifPriority::Normal));
        s.tick(SETTLED);
        s.forget(2);
        s.tick(60); // part-way out
        let placed = s.placed();
        let upper = placed.iter().find(|p| p.id == 1).unwrap().rect;
        let drawn = s.render(&Palette::for_mode(false));
        assert!(drawn.iter().any(|c| matches!(
            c,
            RenderCommand::Text { text, y, .. }
                if text == "Message 1" && *y > upper.y && *y < upper.y + upper.h
        )));
        let (x, y) = centre(upper);
        assert!(s.handle_mouse(&press(x, y)));
        assert_eq!(s.take_events(), [ToastEvent::Opened(1)]);
    }

    /// **The stack closes up along the curve** when a toast below leaves,
    /// from where the toast above is drawn, never jumping.
    #[test]
    fn the_stack_closes_up_along_the_curve() {
        let mut s = stack();
        s.set_motion(Motion::new(200, Curve::Linear));
        s.show(&notif(1, NotifPriority::Normal));
        s.show(&notif(2, NotifPriority::Normal));
        s.tick(SETTLED);
        let before = s.placed()[0].rect.y;
        s.forget(2);
        s.tick(250); // the slide out ends; the gap starts to close
        assert_eq!(s.ids(), [1]);
        let start = s.placed()[0].rect.y;
        assert!((start - before).abs() < 0.01, "the upper toast jumped");
        s.tick(100); // half the settling
        let mid = s.placed()[0].rect.y;
        assert!(mid > before && s.is_moving());
        s.tick(SETTLED);
        let end = s.placed()[0].rect;
        assert!((end.y + end.h - (1040.0 - MARGIN)).abs() < 0.01);
        assert!(
            (mid - f32::midpoint(before, end.y)).abs() < 0.5,
            "not linear half-way"
        );
    }

    /// **A toast turned round leaves from where it is drawn**, under every
    /// curve.
    #[test]
    fn a_toast_turned_round_leaves_from_where_it_is_drawn() {
        for curve in Curve::ALL {
            let mut s = stack();
            s.set_motion(Motion::new(200, curve));
            s.show(&notif(1, NotifPriority::Normal));
            s.tick(40); // before a spring reaches its place
            let before = s.placed()[0].rect.x;
            s.forget(1);
            let after = s.placed()[0].rect.x;
            assert!(
                (before - after).abs() < 0.05,
                "{curve:?}: {before} then {after}"
            );
        }
    }

    /// **An arrival stops at its place**: a spring does not carry a toast
    /// past it, off the surface it is drawn on.
    #[test]
    fn an_arrival_stops_at_its_place() {
        let mut s = stack();
        s.set_motion(Motion::new(200, Curve::Spring));
        s.show(&notif(1, NotifPriority::Normal));
        let rest = 1920.0 - MARGIN - TOAST_WIDTH;
        for _ in 0..60 {
            s.tick(5);
            assert!(s.placed()[0].rect.x >= rest - 0.01);
        }
    }

    /// **Under a still motion a toast is there at once and gone at once.**
    #[test]
    fn under_a_still_motion_they_come_and_go_at_once() {
        let mut s = stack();
        s.set_motion(Motion::STILL);
        s.show(&notif(1, NotifPriority::Low));
        s.tick(0);
        assert!(!s.is_moving());
        assert_eq!(s.next_due_in(), Some(4_000));
        s.tick(4_000);
        s.tick(0);
        assert!(!s.is_showing());
    }

    /// **The soonest time up is the one due**, across priorities; nothing
    /// shown, nothing due.
    #[test]
    fn the_soonest_time_up_is_due() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::High));
        s.show(&notif(2, NotifPriority::Low));
        s.tick(SETTLED);
        assert_eq!(s.next_due_in(), Some(4_000));
        s.tick(1_500);
        assert_eq!(s.next_due_in(), Some(2_500));
        assert_eq!(stack().next_due_in(), None);
    }

    /// **The stack's extent covers every toast and its way in**: the strip
    /// to the screen's edge, the taskbar's top, and the shadows' room.
    #[test]
    fn the_extent_covers_the_stack_and_its_way_in() {
        let mut s = stack();
        assert_eq!(s.extent(), None);
        s.show(&notif(1, NotifPriority::Normal));
        s.show(&notif(2, NotifPriority::Normal));
        for step in [0, 30, 60, SETTLED] {
            s.tick(step);
            let e = s.extent().unwrap();
            assert!((e.x + e.w - 1920.0).abs() < 0.01);
            assert!((e.y + e.h - 1040.0).abs() < 0.01);
            for p in s.placed() {
                assert!(p.rect.y >= e.y + SHADOW_ROOM - 0.01, "{step}");
                assert!(p.rect.x >= e.x + SHADOW_ROOM - 0.01, "{step}");
            }
        }
    }

    /// **The extent is steady while the stack moves**: from the moment a
    /// toast arrives or leaves until the stack has closed up, the surface is
    /// the same size -- resized once per change, not every frame of it.
    #[test]
    fn the_extent_is_steady_while_the_stack_moves() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Normal));
        s.tick(SETTLED);
        s.show(&notif(2, NotifPriority::Normal));
        let growing = s.extent().unwrap();
        for _ in 0..20 {
            s.tick(10);
            assert_eq!(s.extent().unwrap(), growing);
        }
        s.tick(SETTLED);
        s.forget(2);
        s.tick(250); // gone; the upper one starts down
        assert!(s.is_moving());
        let shrinking = s.extent().unwrap();
        for _ in 0..10 {
            s.tick(10);
            assert_eq!(s.extent().unwrap(), shrinking);
        }
        s.tick(SETTLED);
        assert!(s.extent().unwrap().h < shrinking.h, "never shrank back");
    }

    /// **Showing an update in place, forgetting, and clearing**: the same id
    /// shown again is one toast; `forget` slides it out; `clear` takes every
    /// toast away at once.
    #[test]
    fn update_forget_and_clear() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Normal));
        s.show(&notif(1, NotifPriority::Normal));
        assert_eq!(s.ids(), [1]);
        s.show(&notif(2, NotifPriority::Normal));
        s.tick(SETTLED);
        s.forget(1);
        assert!(s.is_moving());
        s.tick(SETTLED);
        assert_eq!(s.ids(), [2]);
        s.clear();
        assert!(!s.is_showing());
        assert!(!s.is_moving());
    }

    fn from(id: u64, app: &str, priority: NotifPriority) -> Notification {
        Notification {
            app_name: app.to_owned(),
            ..notif(id, priority)
        }
    }

    /// **The secondary button asks for the toast's program's menu, and the
    /// toast stays** -- on its body and on its close button alike, since
    /// both are the toast. Beside the stack it asks for nothing.
    #[test]
    fn a_secondary_press_asks_for_the_menu_and_the_toast_stays() {
        let mut s = stack();
        s.show(&from(1, "Mail", NotifPriority::Normal));
        s.show(&from(2, "Calendar", NotifPriority::Normal));
        s.tick(SETTLED);
        let placed = s.placed();
        let right = |x: f32, y: f32| at(x, y, MouseEventKind::Press(MouseButton::Right));

        let (x, y) = centre(placed[0].rect);
        assert!(s.handle_mouse(&right(x, y)));
        assert_eq!(
            s.take_events(),
            [ToastEvent::MenuAsked {
                app: "Mail".to_owned(),
                x,
                y
            }]
        );
        let close = placed[1].close;
        assert!(s.handle_mouse(&right(close.x + 2.0, close.y + 2.0)));
        assert_eq!(
            s.take_events(),
            [ToastEvent::MenuAsked {
                app: "Calendar".to_owned(),
                x: close.x + 2.0,
                y: close.y + 2.0
            }]
        );
        assert!(!s.is_moving(), "a toast started to leave");
        assert_eq!(s.ids(), [1, 2]);

        assert!(!s.handle_mouse(&right(placed[0].rect.x - 5.0, y)));
        assert!(s.take_events().is_empty());
    }

    /// **A menu opened from a toast holds the stack as the pointer does**,
    /// with the pointer gone from the stack to the menu, and letting go
    /// lets them go on from where they were.
    #[test]
    fn a_menu_holds_them_with_the_pointer_gone() {
        let mut s = stack();
        s.show(&notif(1, NotifPriority::Low));
        s.tick(SETTLED);
        let (x, y) = centre(s.placed()[0].rect);
        s.handle_mouse(&at(x, y, MouseEventKind::Move));
        s.hold_for_menu(true);
        s.handle_mouse(&at(x, y, MouseEventKind::Leave));
        assert_eq!(s.next_due_in(), None, "held, nothing is due");
        s.tick(60_000);
        assert_eq!(s.ids(), [1], "a toast left under its menu");
        assert!(!s.is_moving());
        s.hold_for_menu(false);
        assert_eq!(s.next_due_in(), Some(4_000));
        s.tick(4_000);
        s.tick(SETTLED);
        assert!(!s.is_showing());
    }

    /// **Forgetting a program takes away its toasts and only its**: the
    /// ones on screen slide out, one waiting for room is dropped, and the
    /// rest stay where they are.
    #[test]
    fn forgetting_a_program_takes_its_toasts_only() {
        let mut s = stack();
        s.show(&from(1, "Mail", NotifPriority::Urgent));
        s.show(&from(2, "Calendar", NotifPriority::Urgent));
        s.show(&from(3, "Mail", NotifPriority::Urgent));
        // Every toast shown is urgent, so these two wait for room.
        s.show(&from(4, "Mail", NotifPriority::Normal));
        s.show(&from(5, "Calendar", NotifPriority::Normal));
        s.tick(SETTLED);
        assert_eq!(s.ids(), [1, 2, 3]);

        s.forget_app("Mail");
        assert!(s.is_moving(), "nothing started to leave");
        s.tick(SETTLED);
        // Mail's two went, making room for the one waiting that is not
        // Mail's; Mail's waiting one never came.
        assert_eq!(s.ids(), [2, 5]);
        s.tick(SETTLED);
        assert_eq!(s.ids(), [2, 5]);
    }

    /// **A long body is wrapped and cut at three lines**, with a mark; an
    /// empty one takes no room.
    #[test]
    fn a_long_body_is_wrapped_and_cut() {
        let long = "word ".repeat(200);
        let lines = body_lines(&long);
        assert_eq!(lines.len(), BODY_MAX_LINES);
        assert!(lines[2].ends_with('\u{2026}'), "{lines:?}");
        for line in &lines {
            assert!(text::measure(line, BODY_SIZE, FontWeightHint::Regular) <= body_width() + 0.5);
        }
        assert!(body_lines("   ").is_empty());
        let mut s = stack();
        let mut bare = notif(1, NotifPriority::Normal);
        bare.body = String::new();
        s.show(&bare);
        s.tick(SETTLED);
        assert!((s.placed()[0].rect.h - BASE_HEIGHT).abs() < 0.01);
    }

    /// **Everything is drawn from the palette**, in both modes and for every
    /// priority.
    #[test]
    fn everything_is_drawn_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mut s = stack();
            for (id, priority) in (1_u64..).zip([
                NotifPriority::Low,
                NotifPriority::Normal,
                NotifPriority::High,
                NotifPriority::Urgent,
            ]) {
                s.show(&notif(id, priority));
            }
            s.tick(SETTLED);
            let cmds = s.render(&p);
            assert!(cmds.len() > 12);
            appearance::palette_check::assert_drawn_from(&p, &cmds, &[], "the toasts");
        }
    }
}
