//! taskscheduler -- Slate OS Task Scheduler
//!
//! A cron-like task scheduling application with a GUI built on guitk.
//! Supports one-shot and recurring schedules including daily, weekly,
//! monthly, hourly, every-N-minutes, and full cron expressions.
//!
//! # Architecture
//!
//! ```text
//! CronExpr          -- parsed cron expression (minute, hour, dom, month, dow)
//!       |
//!       v
//! ScheduleFrequency -- enum of all schedule types (Once, Daily, Cron, etc.)
//!       |
//!       v
//! ScheduledTask     -- a single task with schedule, retry policy, run history
//!       |
//!       v
//! TaskScheduler     -- manages collection of tasks, checks due, calculates next run
//!       |
//!       v
//! TaskHistory       -- log of past executions
//!       |
//!       v
//! TaskSchedulerConfig -- persistence in simple text format
//!       |
//!       v
//! SchedulerUI       -- guitk-based GUI with task list, add/edit, history
//! ```

#![allow(clippy::too_many_arguments)]

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 600.0;
const HEADER_HEIGHT: f32 = 48.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 32.0;
/// The status bar is drawn *over* the bottom of the content area when a
/// message is showing, so a list has to stop above it, not at the window
/// edge.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height reserved under a list for its "N more" line. Reserved whether
/// or not the line is drawn, so how many rows fit does not depend on
/// whether any are hidden.
const LIST_MORE_HEIGHT: f32 = 16.0;
/// How much history the History tab offers to scroll through. Not a viewport
/// bound -- that is `scroll_window::visible`'s job -- just a limit on how far
/// back the tab reaches.
const HISTORY_ROWS_OFFERED: usize = 100;
const PADDING: f32 = 12.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_WIDTH: f32 = 90.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 16.0;
/// A dialog text field's height, and the width of the value column in the
/// add/edit dialog. Named because the hit box and the drawing both need them
/// and a second copy of either number is a control that misses its own paint.
const FIELD_HEIGHT: f32 = 24.0;
const FIELD_WIDTH: f32 = 280.0;
/// The largest "every N minutes" the form will accept: a little over a year.
/// Not a policy about what is a sensible schedule -- it is a bound that keeps
/// the next-run arithmetic well away from overflow, and stops a held-down
/// digit key from growing the value without limit.
const MAX_INTERVAL_MINUTES: u32 = 600_000;
/// How wide a tab's clickable strip is. The two tabs are laid out 80px apart,
/// so this makes them abut exactly: no gap that swallows a click, and no
/// overlap where one tab would answer for the other.
const TAB_HIT_WIDTH: f32 = 80.0;

// ============================================================================
// Targets -- every control a click can land on
// ============================================================================

/// Everything in this window that can be clicked.
///
/// The enum is the contract between the two halves that must not disagree: a
/// variant no renderer records is a control that cannot be reached, and a
/// variant the click handler does not match is a control that does nothing.
/// Naming them here puts both failures in one list where they can be read off.
///
/// Rows carry a **task id**, not a row index. The lists scroll, so an index
/// means "third row on screen" one moment and a different task the next; the
/// id means the same task regardless of where it was drawn or whether the list
/// has since been re-sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The tab strip: which of the two views to show.
    Tab(UiTab),
    /// Toolbar: open the Add Task dialog.
    Add,
    /// Toolbar: open the Edit dialog for the selected task.
    Edit,
    /// Toolbar: open the delete confirmation for the selected task.
    Remove,
    /// Toolbar: flip the selected task between enabled and disabled.
    ToggleEnabled,
    /// Task list: the body of the row for task `id` -- selects it.
    TaskRow(u64),
    /// Task list: the enabled checkbox on task `id`'s row.
    ///
    /// Separate from [`Target::TaskRow`] because "look at this task" and
    /// "stop this task running" are not the same intention, and on a list of
    /// scheduled jobs the difference is the difference between reading a row
    /// and silently switching off a backup.
    TaskCheckbox(u64),
    /// Add/edit dialog: a text field, focused by clicking it.
    Field(FormField),
    /// Add/edit dialog: the frequency selector, which cycles on click.
    FrequencyCycle,
    /// Add/edit dialog: the frequency's parameter when it is *picked* from a
    /// fixed set rather than typed -- currently only the day of the week.
    ///
    /// Separate from [`Target::Field`] because "Monday" is not a string the
    /// user is editing: a field that accepted typing there would let the form
    /// hold a day name no calendar has.
    ParamCycle,
    /// Add/edit dialog: the Enabled checkbox.
    FormEnabled,
    /// Add/edit dialog: Save.
    DialogSave,
    /// Add/edit dialog, and the delete confirmation: Cancel.
    DialogCancel,
    /// Delete confirmation: the button that actually deletes.
    DeleteConfirm,
    /// The dimmed backdrop behind an open dialog.
    ///
    /// Recorded so that a modal is actually modal. Without it a click at the
    /// coordinates of the Remove button would delete a task through a dialog
    /// whose entire purpose is to ask whether to.
    Scrim,
}

/// Which text field in the add/edit dialog has the caret.
///
/// [`FormField::Param`] is the frequency-specific one -- day of month, minutes,
/// or the cron expression -- which exists for three of the seven frequencies
/// and is absent for the rest. It is one variant rather than three because the
/// dialog only ever shows one of them, and three would let the focus name a
/// field that is not on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    /// The task's name.
    Name,
    /// The command to run.
    Command,
    /// The frequency's parameter, when the chosen frequency has a typed one.
    Param,
}

/// What the frequency-specific control on the form is, for a given frequency.
///
/// Three frequencies take a typed parameter, one takes a picked one, and three
/// take none at all. Deriving that here rather than at each use is what keeps
/// the renderer, the click handler and the focus rules from disagreeing about
/// whether the control on screen is a text field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParamKind {
    /// This frequency takes no parameter, so no control is drawn.
    None,
    /// Picked from a fixed set: clicking advances it.
    Cycle,
    /// Typed in: clicking focuses it.
    Text,
}

/// Which kind of parameter control the frequency at `index` needs.
///
/// The indices are into [`FREQUENCY_LABELS`]; keep the two in step.
fn param_kind(index: usize) -> ParamKind {
    match index {
        // Weekly -- day of week.
        2 => ParamKind::Cycle,
        // Monthly day, every-N-minutes, cron expression.
        3 | 5 | 6 => ParamKind::Text,
        _ => ParamKind::None,
    }
}

pub type Frame = guitk::frame::Frame<Target>;

// ============================================================================
// Layout
// ============================================================================

/// A window size that can be laid out against: never negative, never NaN.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Take a band of height `want` off the top of what is left below `y`, and
/// advance `y` past it.
///
/// The band **shrinks** to what remains rather than being clamped, and keeps
/// its `y` even when it shrinks to nothing. Both halves matter. [`Frame`] does
/// not clip to the window, so a band clamped to a minimum height in a window
/// too short for it would still record hit boxes -- controls that cannot be
/// seen but can be pressed. And a band with no height still has a *place*: a
/// zero-height strip that reported `y == 0` would put everything below it back
/// at the top of the window, on top of the header.
fn take_top(y: &mut f32, limit: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let band = Rect::new(0.0, *y, width, h);
    *y += h;
    band
}

/// The top of one line of `size`, centred down `band` -- or `None` when `band`
/// is too short to hold the line.
///
/// Centring is not a bound. `band.y + (band.h - size) / 2.0` sits *above* the
/// band's top edge the moment the band is shorter than the line, and hangs the
/// same distance below its bottom, so a strip squeezed by a small window puts
/// its run outside the strip in both directions at once. Every run in this file
/// was placed by that expression and nothing bounded any of them: [`take_top`]
/// deliberately shrinks a band to what remains, so a window a few points tall
/// gives the header a strip shorter than its own heading and the title is
/// drawn over the toolbar below it.
///
/// The refusal lives here, in one place every caller goes through, rather than
/// as eight copies of the same comparison -- which is how the rule would have
/// come to hold in seven places and not the eighth.
fn centre_line(band: Rect, size: f32) -> Option<f32> {
    (band.h + 0.01 >= size).then(|| band.y + (band.h - size) / 2.0)
}

/// The horizontal counterpart of [`centre_line`]: the part of `x .. x + want`
/// that lies inside `band`, as `(x, width)` -- or `None` when none of it does.
///
/// Every run in this file is placed at a constant inset and given a constant
/// `max_width`, and a constant is not a bound either: `PADDING` is left of a
/// band narrower than the padding, and a 190-point count pinned 200 points from
/// the right edge runs off it as soon as the window is narrower than the two
/// numbers together. Returning `None` rather than a zero-width span is what
/// keeps the run out of the command list entirely -- a zero-width run still has
/// an `x`, and an `x` outside the band is still ink outside the band.
fn span(band: Rect, x: f32, want: f32) -> Option<(f32, f32)> {
    let left = x.max(band.x);
    let right = (x + want.max(0.0)).min(band.right());
    (right > left).then_some((left, right - left))
}

/// One line of text to place inside a band: everything about it except which
/// band it goes in and what it says.
#[derive(Clone, Copy)]
struct Run {
    /// Left edge, before clipping to the band.
    x: f32,
    /// How much width it would like, before clipping to the band.
    w: f32,
    size: f32,
    color: Color,
    weight: FontWeightHint,
}

/// Push `text` as one line inside `band`, or push nothing at all when the band
/// has no room for it.
///
/// The single door every list cell goes through, so that "a cell stays inside
/// its row" is one rule rather than one rule per column -- the task list has
/// six columns and the history list four, and a rule stated ten times is a rule
/// that will hold nine times.
fn run_in(frame: &mut Frame, band: Rect, run: Run, text: String) {
    if let (Some(y), Some((x, w))) = (centre_line(band, run.size), span(band, run.x, run.w)) {
        frame.push(RenderCommand::Text {
            x,
            y,
            text,
            color: run.color,
            font_size: run.size,
            font_weight: run.weight,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// A strip of at most `want` points along the bottom edge of `band`, as
/// `(y, height)`.
///
/// The fill counterpart of [`centre_line`]. A separator or an underline written
/// as `band.bottom() - want` with a literal height is above the band's top edge
/// whenever the band is thinner than the rule itself, and a fill is pushed
/// exactly as asked whatever the clip -- so it really does paint on whatever
/// sits above.
fn bottom_strip(band: Rect, want: f32) -> (f32, f32) {
    let h = want.min(band.h).max(0.0);
    (band.bottom() - h, h)
}

/// Where every band goes, derived from the live window size.
///
/// Built fresh on every frame and never stored. The size a window *is* and the
/// size it was last told to be are two different things for exactly one frame
/// -- the first one, which arrives before any `Event::Resize` -- and that is
/// the frame in which a remembered layout is wrong.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// The whole window.
    window: Rect,
    /// The title strip across the top.
    header: Rect,
    /// The Add / Edit / Remove / Enable strip below it.
    toolbar: Rect,
    /// The Tasks / History strip below that.
    tab_bar: Rect,
    /// Whatever is left for the list, above the status bar.
    content: Rect,
    /// The status message strip along the bottom. Zero-height when there is no
    /// message, which is why the content is measured against it rather than
    /// against a constant.
    status: Rect,
}

impl Layout {
    /// `status` is present only when there is a message to put in it.
    fn new(width: f32, height: f32, status: bool) -> Self {
        let width = sane(width);
        let height = sane(height);
        let window = Rect::new(0.0, 0.0, width, height);

        // The status bar is taken off the bottom first, so a window too short
        // for everything loses list rows rather than losing the bar off the
        // bottom edge.
        let status_h = if status {
            STATUS_BAR_HEIGHT.min(height)
        } else {
            0.0
        };
        let body_bottom = (height - status_h).max(0.0);

        let mut y = 0.0;
        let header = take_top(&mut y, body_bottom, width, HEADER_HEIGHT);
        let toolbar = take_top(&mut y, body_bottom, width, TOOLBAR_HEIGHT);
        let tab_bar = take_top(&mut y, body_bottom, width, TAB_BAR_HEIGHT);
        let content = take_top(&mut y, body_bottom, width, f32::INFINITY);

        Self {
            window,
            header,
            toolbar,
            tab_bar,
            content,
            status: Rect::new(0.0, body_bottom, width, status_h),
        }
    }
}

// ============================================================================
// DayOfWeek
// ============================================================================

/// Day of the week (0 = Sunday through 6 = Saturday), matching cron convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DayOfWeek {
    Sunday = 0,
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
}

impl DayOfWeek {
    /// Parse a numeric day-of-week value (0..=6).
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Sunday),
            1 => Some(Self::Monday),
            2 => Some(Self::Tuesday),
            3 => Some(Self::Wednesday),
            4 => Some(Self::Thursday),
            5 => Some(Self::Friday),
            6 => Some(Self::Saturday),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Tuesday => "Tuesday",
            Self::Wednesday => "Wednesday",
            Self::Thursday => "Thursday",
            Self::Friday => "Friday",
            Self::Saturday => "Saturday",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Sunday => "Sun",
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
        }
    }
}

// ============================================================================
// CronExpr — simple cron expression parser
// ============================================================================

/// A single cron field that can match specific values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronField {
    /// Wildcard: matches any value.
    Any,
    /// Matches a single specific value.
    Value(u8),
    /// Matches any value in the list.
    List(Vec<u8>),
    /// Matches values in a range (inclusive).
    Range(u8, u8),
    /// Matches every Nth value starting from the base (base/step).
    Step(u8, u8),
}

impl CronField {
    /// Check whether this field matches a given value.
    pub fn matches(&self, val: u8) -> bool {
        match self {
            Self::Any => true,
            Self::Value(v) => *v == val,
            Self::List(vs) => vs.contains(&val),
            Self::Range(lo, hi) => val >= *lo && val <= *hi,
            Self::Step(base, step) => {
                if *step == 0 {
                    return val == *base;
                }
                // A value below the base is not on the sequence at all, and
                // `checked_sub` says so without a second comparison that could
                // drift out of step with the subtraction it guards.
                val.checked_sub(*base)
                    .is_some_and(|offset| offset.is_multiple_of(*step))
            }
        }
    }

    /// Parse a single cron field string.
    ///
    /// Supported formats:
    /// - `*` — wildcard
    /// - `5` — single value
    /// - `1,3,5` — list
    /// - `1-5` — range
    /// - `*/15` — step from 0
    /// - `5/10` — step from base
    pub fn parse(s: &str) -> Result<Self, CronParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(CronParseError::EmptyField);
        }

        // Wildcard
        if s == "*" {
            return Ok(Self::Any);
        }

        // Step: */N or base/step. `split_once` rather than `find` plus two
        // slices: the split is the search and both halves in one operation, so
        // there is no index to get wrong and no byte offset to walk into the
        // middle of a multi-byte character.
        if let Some((base_part, step_part)) = s.split_once('/') {
            let step: u8 = step_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(step_part.to_string()))?;
            let base: u8 = if base_part == "*" {
                0
            } else {
                base_part
                    .parse()
                    .map_err(|_| CronParseError::InvalidNumber(base_part.to_string()))?
            };
            return Ok(Self::Step(base, step));
        }

        // Range: lo-hi. Same reasoning as the step split above.
        if let Some((lo_part, hi_part)) = s.split_once('-') {
            let lo: u8 = lo_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(lo_part.to_string()))?;
            let hi: u8 = hi_part
                .parse()
                .map_err(|_| CronParseError::InvalidNumber(hi_part.to_string()))?;
            if lo > hi {
                return Err(CronParseError::InvalidRange(lo, hi));
            }
            return Ok(Self::Range(lo, hi));
        }

        // List: a,b,c
        if s.contains(',') {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let v: u8 = part
                    .trim()
                    .parse()
                    .map_err(|_| CronParseError::InvalidNumber(part.to_string()))?;
                vals.push(v);
            }
            vals.sort_unstable();
            vals.dedup();
            return Ok(Self::List(vals));
        }

        // Single value
        let v: u8 = s
            .parse()
            .map_err(|_| CronParseError::InvalidNumber(s.to_string()))?;
        Ok(Self::Value(v))
    }
}

/// Errors from parsing cron expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronParseError {
    /// Not enough fields (expected 5).
    WrongFieldCount(usize),
    /// An empty field was encountered.
    EmptyField,
    /// A numeric value could not be parsed.
    InvalidNumber(String),
    /// Range lo > hi.
    InvalidRange(u8, u8),
    /// A field value is out of the allowed range.
    OutOfRange {
        field: &'static str,
        value: u8,
        max: u8,
    },
}

impl core::fmt::Display for CronParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongFieldCount(n) => write!(f, "expected 5 fields, got {n}"),
            Self::EmptyField => write!(f, "empty cron field"),
            Self::InvalidNumber(s) => write!(f, "invalid number: {s}"),
            Self::InvalidRange(lo, hi) => write!(f, "invalid range: {lo}-{hi}"),
            Self::OutOfRange { field, value, max } => {
                write!(f, "{field} value {value} out of range 0-{max}")
            }
        }
    }
}

/// A parsed cron expression with five fields: minute, hour, day-of-month, month,
/// day-of-week.
///
/// Format: `minute hour day_of_month month day_of_week`
///
/// Ranges:
/// - minute: 0-59
/// - hour: 0-23
/// - day_of_month: 1-31
/// - month: 1-12
/// - day_of_week: 0-6 (0 = Sunday)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronExpr {
    pub minute: CronField,
    pub hour: CronField,
    pub day_of_month: CronField,
    pub month: CronField,
    pub day_of_week: CronField,
}

impl CronExpr {
    /// Parse a cron expression string (5 space-separated fields).
    pub fn parse(expr: &str) -> Result<Self, CronParseError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        // Destructured rather than length-checked and then indexed: the
        // pattern *is* the count check, so the two cannot drift apart and a
        // four-field expression cannot reach a read that assumes five.
        let [minute, hour, day_of_month, month, day_of_week] = fields.as_slice() else {
            return Err(CronParseError::WrongFieldCount(fields.len()));
        };

        let minute = CronField::parse(minute)?;
        let hour = CronField::parse(hour)?;
        let day_of_month = CronField::parse(day_of_month)?;
        let month = CronField::parse(month)?;
        let day_of_week = CronField::parse(day_of_week)?;

        let cron = Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        };
        cron.validate()?;
        Ok(cron)
    }

    /// Validate that field values are within the allowed ranges.
    fn validate(&self) -> Result<(), CronParseError> {
        validate_field_range(&self.minute, "minute", 59)?;
        validate_field_range(&self.hour, "hour", 23)?;
        validate_field_range_min(&self.day_of_month, "day_of_month", 1, 31)?;
        validate_field_range_min(&self.month, "month", 1, 12)?;
        validate_field_range(&self.day_of_week, "day_of_week", 6)?;
        Ok(())
    }

    /// Check whether a given time matches this cron expression.
    ///
    /// Arguments are decomposed time fields (not a timestamp) so this stays
    /// pure and testable without a clock.
    pub fn matches(
        &self,
        minute: u8,
        hour: u8,
        day_of_month: u8,
        month: u8,
        day_of_week: u8,
    ) -> bool {
        self.minute.matches(minute)
            && self.hour.matches(hour)
            && self.day_of_month.matches(day_of_month)
            && self.month.matches(month)
            && self.day_of_week.matches(day_of_week)
    }

    /// Format this cron expression back to string form.
    pub fn to_string_repr(&self) -> String {
        format!(
            "{} {} {} {} {}",
            format_cron_field(&self.minute),
            format_cron_field(&self.hour),
            format_cron_field(&self.day_of_month),
            format_cron_field(&self.month),
            format_cron_field(&self.day_of_week),
        )
    }
}

/// Validate that all concrete values in a field are within 0..=max.
fn validate_field_range(
    field: &CronField,
    name: &'static str,
    max: u8,
) -> Result<(), CronParseError> {
    validate_field_range_min(field, name, 0, max)
}

/// Validate that all concrete values in a field are within min..=max.
fn validate_field_range_min(
    field: &CronField,
    name: &'static str,
    min: u8,
    max: u8,
) -> Result<(), CronParseError> {
    match field {
        CronField::Any => Ok(()),
        CronField::Value(v) => {
            if *v < min || *v > max {
                Err(CronParseError::OutOfRange {
                    field: name,
                    value: *v,
                    max,
                })
            } else {
                Ok(())
            }
        }
        CronField::List(vs) => {
            for v in vs {
                if *v < min || *v > max {
                    return Err(CronParseError::OutOfRange {
                        field: name,
                        value: *v,
                        max,
                    });
                }
            }
            Ok(())
        }
        CronField::Range(lo, hi) => {
            if *lo < min || *hi > max {
                return Err(CronParseError::OutOfRange {
                    field: name,
                    value: if *lo < min { *lo } else { *hi },
                    max,
                });
            }
            Ok(())
        }
        CronField::Step(base, _step) => {
            if *base < min || *base > max {
                Err(CronParseError::OutOfRange {
                    field: name,
                    value: *base,
                    max,
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Format a CronField back to string.
fn format_cron_field(field: &CronField) -> String {
    match field {
        CronField::Any => String::from("*"),
        CronField::Value(v) => format!("{v}"),
        CronField::List(vs) => vs
            .iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(","),
        CronField::Range(lo, hi) => format!("{lo}-{hi}"),
        CronField::Step(base, step) => {
            if *base == 0 {
                format!("*/{step}")
            } else {
                format!("{base}/{step}")
            }
        }
    }
}

// ============================================================================
// ScheduleFrequency
// ============================================================================

/// How often a task should run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleFrequency {
    /// Run exactly once at the specified timestamp.
    Once,
    /// Run every day at the same time.
    Daily,
    /// Run on a specific day of the week.
    Weekly(DayOfWeek),
    /// Run on a specific day of the month (1-31).
    Monthly(u8),
    /// Run every hour.
    Hourly,
    /// Run every N minutes.
    EveryNMinutes(u32),
    /// Run according to a cron expression.
    Cron(CronExpr),
}

impl ScheduleFrequency {
    /// Human-readable description of this frequency.
    pub fn display_name(&self) -> String {
        match self {
            Self::Once => String::from("Once"),
            Self::Daily => String::from("Daily"),
            Self::Weekly(day) => format!("Weekly ({})", day.display_name()),
            Self::Monthly(day) => format!("Monthly (day {day})"),
            Self::Hourly => String::from("Hourly"),
            Self::EveryNMinutes(n) => format!("Every {n} min"),
            Self::Cron(expr) => format!("Cron: {}", expr.to_string_repr()),
        }
    }
}

// ============================================================================
// TaskResult
// ============================================================================

/// Outcome of a single task execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task failed with an error message.
    Error(String),
}

impl TaskResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn display_str(&self) -> &str {
        match self {
            Self::Ok => "OK",
            Self::Error(msg) => msg.as_str(),
        }
    }
}

// ============================================================================
// ScheduledTask
// ============================================================================

/// A single scheduled task.
#[derive(Clone, Debug)]
pub struct ScheduledTask {
    /// Unique identifier.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Command to execute.
    pub command: String,
    /// How often to run.
    pub frequency: ScheduleFrequency,
    /// Whether the task is active.
    pub enabled: bool,
    /// Timestamp (unix epoch seconds) of the next scheduled run.
    pub next_run_timestamp: u64,
    /// Timestamp of the last run (0 if never run).
    pub last_run_timestamp: u64,
    /// Result of the last execution.
    pub last_result: Option<TaskResult>,
    /// Whether to retry on failure.
    pub retry_on_failure: bool,
    /// Maximum number of retries (0 = no retries).
    pub max_retries: u32,
    /// Current retry count for the current attempt.
    pub current_retries: u32,
    /// Timestamp when this task was created.
    pub created_at: u64,
}

impl ScheduledTask {
    /// Create a new task with sensible defaults.
    pub fn new(id: u64, name: &str, command: &str, frequency: ScheduleFrequency, now: u64) -> Self {
        Self {
            id,
            name: name.to_string(),
            command: command.to_string(),
            frequency,
            enabled: true,
            next_run_timestamp: now,
            last_run_timestamp: 0,
            last_result: None,
            retry_on_failure: false,
            max_retries: 0,
            current_retries: 0,
            created_at: now,
        }
    }

    /// Whether this task has ever been executed.
    pub fn has_run(&self) -> bool {
        self.last_run_timestamp > 0
    }

    /// Whether the last execution succeeded.
    pub fn last_succeeded(&self) -> bool {
        self.last_result.as_ref().is_some_and(|r| r.is_ok())
    }

    /// Whether the last execution failed.
    pub fn last_failed(&self) -> bool {
        self.last_result.as_ref().is_some_and(|r| !r.is_ok())
    }

    /// Whether this task can retry after its current failure.
    pub fn can_retry(&self) -> bool {
        self.retry_on_failure && self.current_retries < self.max_retries
    }

    /// Display text for the last result column.
    pub fn result_display(&self) -> &str {
        match &self.last_result {
            None => "Never run",
            Some(TaskResult::Ok) => "OK",
            Some(TaskResult::Error(msg)) => msg.as_str(),
        }
    }
}

// ============================================================================
// TaskHistory
// ============================================================================

/// A single entry in the execution history log.
#[derive(Clone, Debug)]
pub struct TaskHistoryEntry {
    /// ID of the task that was executed.
    pub task_id: u64,
    /// Name of the task (snapshot at time of execution).
    pub task_name: String,
    /// Unix timestamp when execution started.
    pub timestamp: u64,
    /// Whether execution succeeded.
    pub success: bool,
    /// Duration of execution in milliseconds.
    pub duration_ms: u64,
    /// Error message if execution failed.
    pub error: Option<String>,
}

/// Persistent log of task executions.
#[derive(Clone, Debug, Default)]
pub struct TaskHistory {
    entries: Vec<TaskHistoryEntry>,
    /// Maximum number of entries to retain.
    max_entries: usize,
}

impl TaskHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 1000,
        }
    }

    #[must_use]
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Record a successful execution.
    pub fn record_success(
        &mut self,
        task_id: u64,
        task_name: &str,
        timestamp: u64,
        duration_ms: u64,
    ) {
        self.add_entry(TaskHistoryEntry {
            task_id,
            task_name: task_name.to_string(),
            timestamp,
            success: true,
            duration_ms,
            error: None,
        });
    }

    /// Record a failed execution.
    pub fn record_failure(
        &mut self,
        task_id: u64,
        task_name: &str,
        timestamp: u64,
        duration_ms: u64,
        error: &str,
    ) {
        self.add_entry(TaskHistoryEntry {
            task_id,
            task_name: task_name.to_string(),
            timestamp,
            success: false,
            duration_ms,
            error: Some(error.to_string()),
        });
    }

    fn add_entry(&mut self, entry: TaskHistoryEntry) {
        self.entries.push(entry);
        // Trim to max_entries if needed.
        let excess = self.entries.len().saturating_sub(self.max_entries);
        if excess > 0 {
            self.entries.drain(..excess);
        }
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> &[TaskHistoryEntry] {
        &self.entries
    }

    /// Entries for a specific task, oldest first.
    pub fn entries_for_task(&self, task_id: u64) -> Vec<&TaskHistoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.task_id == task_id)
            .collect()
    }

    /// Most recent entries, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Vec<&TaskHistoryEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    /// Total number of recorded executions.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Number of successful executions.
    pub fn success_count(&self) -> usize {
        self.entries.iter().filter(|e| e.success).count()
    }

    /// Number of failed executions.
    pub fn failure_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.success).count()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// TaskScheduler
// ============================================================================

/// Why a task run failed, and how long it took to fail.
///
/// The duration is carried alongside the message rather than being dropped
/// because a command that fails after ninety seconds and one that fails
/// immediately are different problems, and the history column that would tell
/// them apart is already there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRunError {
    /// What to show in the task's Last Result column and the history.
    pub message: String,
    /// How long the attempt took, in milliseconds.
    pub duration_ms: u64,
}

/// What actually executes a scheduled command.
///
/// Given the command line, returns how long a successful run took, or a
/// [`TaskRunError`]. This is the seam where a process spawner will attach; see
/// `known-issues.md` → `C-TASKSCHEDULER-HAS-NO-EXECUTOR`.
pub type RunFn = fn(&str) -> Result<u64, TaskRunError>;

/// Manages a collection of scheduled tasks.
pub struct TaskScheduler {
    /// All tasks, keyed by ID.
    tasks: BTreeMap<u64, ScheduledTask>,
    /// Next ID to assign.
    next_id: u64,
    /// Execution history.
    pub history: TaskHistory,
    /// How to actually run a command, if anything can.
    ///
    /// `None` in every build so far: nothing in the tree can start a process
    /// yet. Held as an installable seam rather than a `todo!()` so that the
    /// scheduling half is finished, tested and callable now, and attaching a
    /// spawner later is one line rather than a rewrite.
    runner: Option<RunFn>,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 1,
            history: TaskHistory::new(),
            runner: None,
        }
    }

    /// Add a new task. Returns the assigned task ID.
    pub fn add_task(
        &mut self,
        name: &str,
        command: &str,
        frequency: ScheduleFrequency,
        now: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut task = ScheduledTask::new(id, name, command, frequency, now);
        task.next_run_timestamp = calculate_next_run(&task.frequency, now);
        self.tasks.insert(id, task);
        id
    }

    /// Remove a task by ID. Returns true if it existed.
    pub fn remove_task(&mut self, id: u64) -> bool {
        self.tasks.remove(&id).is_some()
    }

    /// Enable a task.
    pub fn enable_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a task.
    pub fn disable_task(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.enabled = false;
            true
        } else {
            false
        }
    }

    /// Get a task by ID.
    pub fn get_task(&self, id: u64) -> Option<&ScheduledTask> {
        self.tasks.get(&id)
    }

    /// Get a mutable reference to a task by ID.
    pub fn get_task_mut(&mut self, id: u64) -> Option<&mut ScheduledTask> {
        self.tasks.get_mut(&id)
    }

    /// List all tasks sorted by next run time.
    pub fn list_tasks(&self) -> Vec<&ScheduledTask> {
        let mut tasks: Vec<&ScheduledTask> = self.tasks.values().collect();
        tasks.sort_by_key(|t| t.next_run_timestamp);
        tasks
    }

    /// List only enabled tasks sorted by next run time.
    pub fn list_enabled_tasks(&self) -> Vec<&ScheduledTask> {
        let mut tasks: Vec<&ScheduledTask> = self.tasks.values().filter(|t| t.enabled).collect();
        tasks.sort_by_key(|t| t.next_run_timestamp);
        tasks
    }

    /// Check which tasks are due to run at or before the given timestamp.
    pub fn check_due(&self, now_timestamp: u64) -> Vec<&ScheduledTask> {
        let mut due: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.enabled && t.next_run_timestamp <= now_timestamp)
            .collect();
        due.sort_by_key(|t| t.next_run_timestamp);
        due
    }

    /// Run everything due at `now`, and report which task ids were run.
    ///
    /// **Nothing runs unless a runner has been installed** — see
    /// [`TaskScheduler::set_runner`]. Without one this returns empty and, in
    /// particular, does *not* advance any task's `next_run_timestamp`: a
    /// scheduler with no way to execute a command must leave the task overdue
    /// rather than quietly rescheduling it for tomorrow as though it had run.
    /// That distinction is the whole reason this is a seam and not a stub.
    ///
    /// Due tasks are run oldest-first, which is `check_due`'s order, so a
    /// backlog is worked through in the order it accumulated.
    pub fn run_due_tasks(&mut self, now: u64) -> Vec<u64> {
        let Some(runner) = self.runner else {
            return Vec::new();
        };
        // Collected first: running a task writes back through `&mut self`, so
        // the borrow taken by `check_due` cannot still be alive. Ids rather
        // than references for the same reason — and because a task that a
        // runner somehow removed must be skipped, not followed to a dangling
        // place in the map.
        let due: Vec<(u64, String)> = self
            .check_due(now)
            .into_iter()
            .map(|t| (t.id, t.command.clone()))
            .collect();

        let mut ran = Vec::with_capacity(due.len());
        for (id, command) in due {
            let outcome = runner(&command);
            match outcome {
                Ok(duration_ms) => self.mark_completed(id, now, duration_ms),
                Err(TaskRunError {
                    message,
                    duration_ms,
                }) => self.mark_failed(id, &message, now, duration_ms),
            }
            ran.push(id);
        }
        ran
    }

    /// Install the thing that actually executes a command.
    ///
    /// A `fn` pointer, not a closure: the scheduler is serialised and compared
    /// in tests, and a captured environment would be state that round-trips
    /// through neither. What a real runner needs — a process spawner — does
    /// not exist yet, so in this build the only callers are the tests.
    pub fn set_runner(&mut self, runner: RunFn) {
        self.runner = Some(runner);
    }

    /// Whether anything would actually run when a task falls due.
    #[must_use]
    pub fn can_run(&self) -> bool {
        self.runner.is_some()
    }

    /// Mark a task as completed successfully.
    pub fn mark_completed(&mut self, id: u64, now: u64, duration_ms: u64) {
        if let Some(task) = self.tasks.get_mut(&id) {
            let task_name = task.name.clone();
            task.last_run_timestamp = now;
            task.last_result = Some(TaskResult::Ok);
            task.current_retries = 0;

            // For one-shot tasks, disable after completion.
            if task.frequency == ScheduleFrequency::Once {
                task.enabled = false;
                task.next_run_timestamp = u64::MAX;
            } else {
                task.next_run_timestamp = calculate_next_run(&task.frequency, now);
            }

            self.history
                .record_success(id, &task_name, now, duration_ms);
        }
    }

    /// Mark a task as failed.
    pub fn mark_failed(&mut self, id: u64, error_msg: &str, now: u64, duration_ms: u64) {
        if let Some(task) = self.tasks.get_mut(&id) {
            let task_name = task.name.clone();
            task.last_run_timestamp = now;
            task.last_result = Some(TaskResult::Error(error_msg.to_string()));

            if task.can_retry() {
                task.current_retries = task.current_retries.saturating_add(1);
                // Schedule retry in 60 seconds.
                task.next_run_timestamp = now.saturating_add(60);
            } else {
                task.current_retries = 0;
                if task.frequency == ScheduleFrequency::Once {
                    task.enabled = false;
                    task.next_run_timestamp = u64::MAX;
                } else {
                    task.next_run_timestamp = calculate_next_run(&task.frequency, now);
                }
            }

            self.history
                .record_failure(id, &task_name, now, duration_ms, error_msg);
        }
    }

    /// Total number of tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Number of enabled tasks.
    pub fn enabled_count(&self) -> usize {
        self.tasks.values().filter(|t| t.enabled).count()
    }

    /// Update a task's name, command, and frequency.
    pub fn update_task(
        &mut self,
        id: u64,
        name: &str,
        command: &str,
        frequency: ScheduleFrequency,
        now: u64,
    ) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.name = name.to_string();
            task.command = command.to_string();
            task.frequency = frequency;
            task.next_run_timestamp = calculate_next_run(&task.frequency, now);
            true
        } else {
            false
        }
    }

    /// Set retry policy on a task.
    pub fn set_retry_policy(&mut self, id: u64, retry_on_failure: bool, max_retries: u32) -> bool {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.retry_on_failure = retry_on_failure;
            task.max_retries = max_retries;
            true
        } else {
            false
        }
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// calculate_next_run — schedule calculation
// ============================================================================

/// Calculate the next run timestamp for a given frequency, based on the
/// current time.
///
/// This uses a simplified model where timestamps are unix epoch seconds.
/// For daily/weekly/monthly, it adds the appropriate number of seconds.
/// For cron expressions, it scans forward minute-by-minute (up to a
/// bounded limit) to find the next matching time.
pub fn calculate_next_run(frequency: &ScheduleFrequency, now: u64) -> u64 {
    const SECS_PER_MINUTE: u64 = 60;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_DAY: u64 = 86400;

    match frequency {
        ScheduleFrequency::Once => now,
        ScheduleFrequency::Daily => now.saturating_add(SECS_PER_DAY),
        ScheduleFrequency::Weekly(_day) => now.saturating_add(SECS_PER_DAY * 7),
        ScheduleFrequency::Monthly(_day) => {
            // Approximate month as 30 days.
            now.saturating_add(SECS_PER_DAY * 30)
        }
        ScheduleFrequency::Hourly => now.saturating_add(SECS_PER_HOUR),
        ScheduleFrequency::EveryNMinutes(n) => {
            now.saturating_add((*n as u64).saturating_mul(SECS_PER_MINUTE))
        }
        ScheduleFrequency::Cron(expr) => {
            // Scan forward minute by minute, up to ~2 years, to find the next
            // matching minute.
            let max_scan = SECS_PER_MINUTE * 60 * 24 * 366 * 2; // ~2 years
            let mut candidate = now.saturating_add(SECS_PER_MINUTE);
            // Align to the start of the minute: cron matches on whole minutes,
            // and a candidate a few seconds into one would be compared against
            // the same minute twice.
            let into_minute = candidate.checked_rem(SECS_PER_MINUTE).unwrap_or(0);
            candidate = candidate.saturating_sub(into_minute);
            let end = now.saturating_add(max_scan);

            while candidate <= end {
                let time = decompose_timestamp(candidate);
                if expr.matches(time.minute, time.hour, time.day, time.month, time.weekday) {
                    return candidate;
                }
                candidate = candidate.saturating_add(SECS_PER_MINUTE);
            }
            // If no match found within scan window, return far future.
            u64::MAX
        }
    }
}

/// The calendar fields a cron expression matches against.
///
/// Not a `DateTime` directly because a cron expression matches on `u8` fields
/// and on a weekday numbered from Sunday; this is the adapter between the two
/// shapes, and it exists so `CronExpr::matches` keeps a signature that says
/// what it compares.
struct DecomposedTime {
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    weekday: u8,
}

/// Decompose a unix epoch timestamp into the fields cron matches on.
///
/// This program used to decompose an instant twice by two different routes —
/// here for cron matching, and again in `format_timestamp` for display — each
/// with its own `secs % 86400` and its own transcription of Howard Hinnant's
/// `civil_from_days`. The display half now renders through `guitk::datetime`;
/// this half reads the same type's accessors, so there is one calendar in this
/// file and not two. The old code also derived the weekday from
/// `(days + 4) % 7`, a correct-but-separate fact about 1970-01-01 being a
/// Thursday that `Date::weekday` already knows.
///
/// UTC, and that is a live bug, not a decision: a user who schedules a task
/// for 03:00 means 03:00 where they are. It stays UTC because there is no
/// per-process zone to read yet (known-issues
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and because cron-under-DST has
/// genuine semantics to settle first — see known-issues
/// `TD-CRON-MATCHES-UTC-FIELDS`. Written as an explicit `Tz::utc()` so the
/// grep that finds the zoneless callers finds this one too.
fn decompose_timestamp(ts: u64) -> DecomposedTime {
    let dt = guitk::datetime::DateTime::at(
        i64::try_from(ts).unwrap_or(i64::MAX),
        &guitk::tzrules::Tz::utc(),
    );
    let d = dt.date();
    DecomposedTime {
        // Every field below is in a range that fits `u8` by construction —
        // minutes and hours from a seconds-of-day count, month and day from a
        // calendar date, weekday from `0..=6` — so the fallbacks are
        // unreachable rather than chosen.
        minute: u8::try_from(dt.minute()).unwrap_or(0),
        hour: u8::try_from(dt.hour()).unwrap_or(0),
        day: u8::try_from(d.day()).unwrap_or(1),
        month: u8::try_from(d.month()).unwrap_or(1),
        weekday: u8::try_from(d.weekday().index()).unwrap_or(0),
    }
}

// ============================================================================
// TaskSchedulerConfig — simple text-based persistence
// ============================================================================

/// Serialization/deserialization for task scheduler state using a simple
/// line-based text format.
///
/// Format:
/// ```text
/// TASK|id|name|command|frequency_type|frequency_param|enabled|next_run|last_run|retry|max_retries|created_at
/// ```
pub struct TaskSchedulerConfig;

impl TaskSchedulerConfig {
    /// Serialize all tasks to a text config string.
    pub fn serialize(scheduler: &TaskScheduler) -> String {
        let mut lines = Vec::new();
        lines.push(String::from("# Slate OS Task Scheduler Config"));
        lines.push("VERSION|1".to_string());

        for task in scheduler.tasks.values() {
            let freq_str = serialize_frequency(&task.frequency);
            let enabled_str = if task.enabled { "1" } else { "0" };
            let retry_str = if task.retry_on_failure { "1" } else { "0" };
            let last_result_str = match &task.last_result {
                None => String::from("none"),
                Some(TaskResult::Ok) => String::from("ok"),
                Some(TaskResult::Error(msg)) => format!("error:{}", msg.replace('|', "\\|")),
            };

            lines.push(format!(
                "TASK|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                task.id,
                task.name.replace('|', "\\|"),
                task.command.replace('|', "\\|"),
                freq_str,
                enabled_str,
                task.next_run_timestamp,
                task.last_run_timestamp,
                last_result_str,
                retry_str,
                task.max_retries,
                task.created_at,
            ));
        }

        lines.join("\n")
    }

    /// Deserialize tasks from a config string into a scheduler.
    pub fn deserialize(text: &str) -> Result<TaskScheduler, ConfigError> {
        let mut scheduler = TaskScheduler::new();
        let mut max_id: u64 = 0;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("VERSION|") {
                continue;
            }
            if line.starts_with("TASK|") {
                let task = parse_task_line(line)?;
                if task.id >= max_id {
                    max_id = task.id.saturating_add(1);
                }
                scheduler.tasks.insert(task.id, task);
            }
        }

        scheduler.next_id = max_id;
        Ok(scheduler)
    }
}

/// Errors from config parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Wrong number of fields in a TASK line.
    InvalidFieldCount(usize),
    /// A numeric field could not be parsed.
    InvalidNumber(String),
    /// An invalid frequency type was encountered.
    InvalidFrequency(String),
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidFieldCount(n) => write!(f, "expected 12 fields in TASK line, got {n}"),
            Self::InvalidNumber(s) => write!(f, "invalid number in config: {s}"),
            Self::InvalidFrequency(s) => write!(f, "invalid frequency: {s}"),
        }
    }
}

/// Serialize a ScheduleFrequency to string.
fn serialize_frequency(freq: &ScheduleFrequency) -> String {
    match freq {
        ScheduleFrequency::Once => String::from("once"),
        ScheduleFrequency::Daily => String::from("daily"),
        ScheduleFrequency::Weekly(day) => format!("weekly:{}", *day as u8),
        ScheduleFrequency::Monthly(day) => format!("monthly:{day}"),
        ScheduleFrequency::Hourly => String::from("hourly"),
        ScheduleFrequency::EveryNMinutes(n) => format!("every_n_min:{n}"),
        ScheduleFrequency::Cron(expr) => format!("cron:{}", expr.to_string_repr()),
    }
}

/// Deserialize a ScheduleFrequency from string.
fn deserialize_frequency(s: &str) -> Result<ScheduleFrequency, ConfigError> {
    if s == "once" {
        return Ok(ScheduleFrequency::Once);
    }
    if s == "daily" {
        return Ok(ScheduleFrequency::Daily);
    }
    if s == "hourly" {
        return Ok(ScheduleFrequency::Hourly);
    }
    if let Some(rest) = s.strip_prefix("weekly:") {
        let day_num: u8 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        let day = DayOfWeek::from_u8(day_num)
            .ok_or_else(|| ConfigError::InvalidFrequency(s.to_string()))?;
        return Ok(ScheduleFrequency::Weekly(day));
    }
    if let Some(rest) = s.strip_prefix("monthly:") {
        let day: u8 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        return Ok(ScheduleFrequency::Monthly(day));
    }
    if let Some(rest) = s.strip_prefix("every_n_min:") {
        let n: u32 = rest
            .parse()
            .map_err(|_| ConfigError::InvalidNumber(rest.to_string()))?;
        return Ok(ScheduleFrequency::EveryNMinutes(n));
    }
    if let Some(rest) = s.strip_prefix("cron:") {
        let expr =
            CronExpr::parse(rest).map_err(|_| ConfigError::InvalidFrequency(s.to_string()))?;
        return Ok(ScheduleFrequency::Cron(expr));
    }
    Err(ConfigError::InvalidFrequency(s.to_string()))
}

/// Parse a single TASK line from the config file.
fn parse_task_line(line: &str) -> Result<ScheduledTask, ConfigError> {
    let parts: Vec<&str> = line.splitn(12, '|').collect();
    // Destructured rather than length-checked and then indexed twelve times:
    // the pattern *is* the field-count check, and it names each field at the
    // point it is read instead of leaving a bare `parts[10]` for a later
    // reader to count out on their fingers. `_tag` is the literal "TASK".
    let [
        _tag,
        id,
        name,
        command,
        frequency,
        enabled,
        next_run,
        last_run,
        last_result,
        retry,
        max_retries,
        created_at,
    ] = parts.as_slice()
    else {
        return Err(ConfigError::InvalidFieldCount(parts.len()));
    };

    let id: u64 = id
        .parse()
        .map_err(|_| ConfigError::InvalidNumber((*id).to_string()))?;
    let name = name.replace("\\|", "|");
    let command = command.replace("\\|", "|");
    let frequency = deserialize_frequency(frequency)?;
    let enabled = *enabled == "1";
    let next_run: u64 = next_run
        .parse()
        .map_err(|_| ConfigError::InvalidNumber((*next_run).to_string()))?;
    let last_run: u64 = last_run
        .parse()
        .map_err(|_| ConfigError::InvalidNumber((*last_run).to_string()))?;

    let last_result = match *last_result {
        "ok" => Some(TaskResult::Ok),
        // "none" -- and anything this version does not recognise -- means no
        // recorded result, which is what a task that has never run has.
        s => s
            .strip_prefix("error:")
            .map(|message| TaskResult::Error(message.replace("\\|", "|"))),
    };

    let retry = *retry == "1";
    let max_retries: u32 = max_retries
        .parse()
        .map_err(|_| ConfigError::InvalidNumber((*max_retries).to_string()))?;
    let created_at: u64 = created_at
        .trim()
        .parse()
        .map_err(|_| ConfigError::InvalidNumber((*created_at).to_string()))?;

    Ok(ScheduledTask {
        id,
        name,
        command,
        frequency,
        enabled,
        next_run_timestamp: next_run,
        last_run_timestamp: last_run,
        last_result,
        retry_on_failure: retry,
        max_retries,
        current_retries: 0,
        created_at,
    })
}

// ============================================================================
// SchedulerUI — GUI view state
// ============================================================================

/// Which tab the UI is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTab {
    /// Main task list.
    Tasks,
    /// Execution history.
    History,
}

/// Which dialog is open (if any).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiDialog {
    None,
    /// Add task dialog.
    AddTask,
    /// Edit task dialog (contains task ID).
    EditTask(u64),
    /// Confirm delete dialog (contains task ID).
    ConfirmDelete(u64),
}

/// Form state for the add/edit task dialog.
#[derive(Clone, Debug)]
pub struct TaskFormState {
    pub name: String,
    pub command: String,
    pub frequency_index: usize,
    pub enabled: bool,
    pub cron_expr: String,
    pub weekly_day: u8,
    pub monthly_day: u8,
    pub interval_minutes: u32,
}

impl TaskFormState {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            frequency_index: 0,
            enabled: true,
            cron_expr: String::from("0 * * * *"),
            weekly_day: 1,
            monthly_day: 1,
            interval_minutes: 30,
        }
    }

    /// Populate from an existing task.
    pub fn from_task(task: &ScheduledTask) -> Self {
        let (freq_index, cron_expr, weekly_day, monthly_day, interval_minutes) =
            match &task.frequency {
                ScheduleFrequency::Once => (0, String::new(), 1u8, 1u8, 30u32),
                ScheduleFrequency::Daily => (1, String::new(), 1, 1, 30),
                ScheduleFrequency::Weekly(day) => (2, String::new(), *day as u8, 1, 30),
                ScheduleFrequency::Monthly(d) => (3, String::new(), 1, *d, 30),
                ScheduleFrequency::Hourly => (4, String::new(), 1, 1, 30),
                ScheduleFrequency::EveryNMinutes(n) => (5, String::new(), 1, 1, *n),
                ScheduleFrequency::Cron(expr) => (6, expr.to_string_repr(), 1, 1, 30),
            };

        Self {
            name: task.name.clone(),
            command: task.command.clone(),
            frequency_index: freq_index,
            enabled: task.enabled,
            cron_expr,
            weekly_day,
            monthly_day,
            interval_minutes,
        }
    }

    /// Build the ScheduleFrequency from the current form state.
    pub fn build_frequency(&self) -> Option<ScheduleFrequency> {
        match self.frequency_index {
            0 => Some(ScheduleFrequency::Once),
            1 => Some(ScheduleFrequency::Daily),
            2 => DayOfWeek::from_u8(self.weekly_day).map(ScheduleFrequency::Weekly),
            // A day of 0, or of 32, is not a date. It is reachable: the form
            // is typed into digit by digit, and "0" is how somebody starts
            // typing "5". Rejecting it here rather than clamping it is what
            // makes Save grey out while the field is still half-typed, instead
            // of quietly scheduling the job for a day the user did not choose.
            3 => (1..=31)
                .contains(&self.monthly_day)
                .then_some(ScheduleFrequency::Monthly(self.monthly_day)),
            4 => Some(ScheduleFrequency::Hourly),
            // Same reasoning: "every 0 minutes" is not an interval, and it is
            // what the field reads for as long as it is empty.
            5 => (self.interval_minutes > 0)
                .then_some(ScheduleFrequency::EveryNMinutes(self.interval_minutes)),
            6 => CronExpr::parse(&self.cron_expr)
                .ok()
                .map(ScheduleFrequency::Cron),
            _ => None,
        }
    }
}

impl Default for TaskFormState {
    fn default() -> Self {
        Self::new()
    }
}

/// Frequency type labels for the UI dropdown.
const FREQUENCY_LABELS: &[&str] = &[
    "Once",
    "Daily",
    "Weekly",
    "Monthly",
    "Hourly",
    "Every N Minutes",
    "Cron Expression",
];

/// Complete UI state for the task scheduler application.
pub struct SchedulerUI {
    /// Current tab.
    pub tab: UiTab,
    /// Current dialog.
    pub dialog: UiDialog,
    /// The scheduler engine.
    pub scheduler: TaskScheduler,
    /// Selected task ID (if any).
    pub selected_task_id: Option<u64>,
    /// Form state for add/edit dialog.
    pub form: TaskFormState,
    /// First task row drawn, counted in rows rather than pixels: the list
    /// only ever scrolls a whole row at a time, so a pixel offset could only
    /// express positions the renderer then rounds away. A value past the end
    /// is not an error, and shows the last page.
    pub task_list_scroll: usize,
    /// First history row drawn. Same units and same tolerance as
    /// `task_list_scroll`.
    pub history_scroll: usize,
    /// Status message displayed temporarily.
    pub status_message: Option<String>,
    /// Which dialog field has the caret, if a dialog is open at all.
    ///
    /// Cleared whenever a dialog opens or closes, so a caret cannot survive
    /// into a dialog that does not have the field it names.
    pub focus: Option<FormField>,
    /// The last window size the event path was told about.
    ///
    /// Only the *event* path reads this. The renderer takes the size as a
    /// parameter, because it is handed the live one and this is at best a
    /// frame behind it. Its job is to let `target_at` re-draw at the size the
    /// user was looking at when they clicked.
    window_width: f32,
    window_height: f32,
    /// Seconds since the Unix epoch, as of the last tick.
    ///
    /// Held rather than read on demand so that every "what time is it" in one
    /// frame gives the same answer, and so the whole scheduler can be driven
    /// from a test at a chosen instant. [`SchedulerUI::refresh_clock`] is the
    /// only thing that consults the host clock.
    pub now: u64,
    /// Rows earned from the wheel but not yet delivered, for whichever list is
    /// showing.
    ///
    /// One accumulator, not two, because only one list is on screen at a time
    /// and the tab switch resets it -- a fraction earned while scrolling the
    /// task list has no business delivering a row in the history.
    wheel: wheel::Accumulator,
}

impl SchedulerUI {
    pub fn new() -> Self {
        Self {
            tab: UiTab::Tasks,
            dialog: UiDialog::None,
            scheduler: TaskScheduler::new(),
            selected_task_id: None,
            form: TaskFormState::new(),
            task_list_scroll: 0,
            history_scroll: 0,
            status_message: None,
            focus: None,
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            now: 0,
            wheel: wheel::Accumulator::default(),
        }
    }

    // -- tab navigation -------------------------------------------------------

    pub fn switch_to_tasks(&mut self) {
        self.switch_to(UiTab::Tasks);
    }

    pub fn switch_to_history(&mut self) {
        self.switch_to(UiTab::History);
    }

    /// Show `tab`, forgetting the wheel's outstanding fraction if the view
    /// actually changed.
    ///
    /// Guarded on the change so that clicking the tab you are already on is
    /// not a way to cancel a slow trackpad drag half way through it.
    fn switch_to(&mut self, tab: UiTab) {
        if self.tab != tab {
            self.tab = tab;
            self.wheel.reset();
        }
    }

    // -- task selection -------------------------------------------------------

    pub fn select_task(&mut self, id: u64) {
        self.selected_task_id = Some(id);
    }

    pub fn deselect_task(&mut self) {
        self.selected_task_id = None;
    }

    // -- dialog management ----------------------------------------------------

    /// Show `dialog`, and put the caret where that dialog's typing should go.
    ///
    /// Every change of dialog goes through here, which is what keeps the caret
    /// from outliving the field it names: a focus left on `Param` while the
    /// delete confirmation is up would send the next keystroke into a text
    /// field that is not on screen. The two form dialogs open focused on Name
    /// so that a dialog opened from the keyboard can be filled in from the
    /// keyboard, without a click nobody would know to make.
    fn set_dialog(&mut self, dialog: UiDialog) {
        self.focus = match dialog {
            UiDialog::AddTask | UiDialog::EditTask(_) => Some(FormField::Name),
            UiDialog::None | UiDialog::ConfirmDelete(_) => None,
        };
        self.dialog = dialog;
    }

    pub fn open_add_dialog(&mut self) {
        self.form = TaskFormState::new();
        self.set_dialog(UiDialog::AddTask);
    }

    pub fn open_edit_dialog(&mut self, id: u64) {
        if let Some(task) = self.scheduler.get_task(id) {
            self.form = TaskFormState::from_task(task);
            self.set_dialog(UiDialog::EditTask(id));
        }
    }

    pub fn open_delete_dialog(&mut self, id: u64) {
        self.set_dialog(UiDialog::ConfirmDelete(id));
    }

    pub fn close_dialog(&mut self) {
        self.set_dialog(UiDialog::None);
    }

    // -- actions --------------------------------------------------------------

    /// Commit the add-task form: create a new task.
    pub fn commit_add_task(&mut self, now: u64) -> Option<u64> {
        if self.form.name.is_empty() || self.form.command.is_empty() {
            self.status_message = Some(String::from("Name and command are required"));
            return None;
        }
        let freq = match self.form.build_frequency() {
            Some(f) => f,
            None => {
                self.status_message = Some(String::from("Invalid frequency"));
                return None;
            }
        };
        let id = self
            .scheduler
            .add_task(&self.form.name, &self.form.command, freq, now);
        if self.form.enabled {
            self.scheduler.enable_task(id);
        } else {
            self.scheduler.disable_task(id);
        }
        self.set_dialog(UiDialog::None);
        self.status_message = Some(format!("Task '{}' added", self.form.name));
        Some(id)
    }

    /// Commit the edit-task form: update the existing task.
    pub fn commit_edit_task(&mut self, id: u64, now: u64) -> bool {
        if self.form.name.is_empty() || self.form.command.is_empty() {
            self.status_message = Some(String::from("Name and command are required"));
            return false;
        }
        let freq = match self.form.build_frequency() {
            Some(f) => f,
            None => {
                self.status_message = Some(String::from("Invalid frequency"));
                return false;
            }
        };
        let updated =
            self.scheduler
                .update_task(id, &self.form.name, &self.form.command, freq, now);
        if updated {
            if self.form.enabled {
                self.scheduler.enable_task(id);
            } else {
                self.scheduler.disable_task(id);
            }
            self.set_dialog(UiDialog::None);
            self.status_message = Some(format!("Task '{}' updated", self.form.name));
        }
        updated
    }

    /// Delete the selected task.
    pub fn confirm_delete_task(&mut self, id: u64) -> bool {
        let removed = self.scheduler.remove_task(id);
        if removed {
            if self.selected_task_id == Some(id) {
                self.selected_task_id = None;
            }
            self.status_message = Some(String::from("Task deleted"));
        }
        self.set_dialog(UiDialog::None);
        removed
    }

    /// Toggle enabled/disabled on the selected task.
    pub fn toggle_selected_task(&mut self) {
        if let Some(id) = self.selected_task_id
            && let Some(task) = self.scheduler.get_task(id)
        {
            if task.enabled {
                self.scheduler.disable_task(id);
            } else {
                self.scheduler.enable_task(id);
            }
        }
    }

    // -- form editing ---------------------------------------------------------

    /// Whether the open form would be accepted if it were submitted now.
    ///
    /// Deliberately the same three conditions `commit_add_task` and
    /// `commit_edit_task` apply, and no others: this decides how Save is
    /// *drawn*, so a condition here that the commit does not share would grey
    /// out a button that works, or offer one that does not.
    #[must_use]
    pub fn form_is_valid(&self) -> bool {
        !self.form.name.is_empty()
            && !self.form.command.is_empty()
            && self.form.build_frequency().is_some()
    }

    /// Advance the frequency selector to the next of [`FREQUENCY_LABELS`].
    ///
    /// Wraps, because there is no "last" frequency to stop at and a selector
    /// that stuck on Cron would need a second control to get back.
    pub fn cycle_frequency(&mut self) {
        let next = self
            .form
            .frequency_index
            .saturating_add(1)
            .checked_rem(FREQUENCY_LABELS.len())
            .unwrap_or(0);
        self.form.frequency_index = next;
        // The parameter row is replaced wholesale by whatever the new
        // frequency needs, and three of the seven need none. A caret left on
        // `Param` would then point at a field that is no longer drawn, and the
        // next keystroke would be typed into a value nobody can see. Dropping
        // the caret is the honest outcome: there is nothing to move it to that
        // the user asked for.
        if self.focus == Some(FormField::Param) && param_kind(next) != ParamKind::Text {
            self.focus = None;
        }
    }

    /// Advance the frequency's picked parameter -- currently only the day of
    /// the week -- to its next value.
    ///
    /// Does nothing for a frequency whose parameter is typed or absent, so a
    /// stale click that arrives after the frequency changed cannot rewrite a
    /// field it was not aimed at.
    pub fn cycle_param(&mut self) {
        if param_kind(self.form.frequency_index) != ParamKind::Cycle {
            return;
        }
        // Sunday..=Saturday is 0..=6; anything else came from a form built
        // before this was the range, and lands on Sunday rather than staying
        // unrepresentable.
        self.form.weekly_day = match self.form.weekly_day {
            0..=5 => self.form.weekly_day.saturating_add(1),
            _ => 0,
        };
    }

    /// Flip the form's Enabled checkbox.
    pub fn toggle_form_enabled(&mut self) {
        self.form.enabled = !self.form.enabled;
    }

    /// Put the caret in `field`, if that field is on screen.
    ///
    /// The guard is what stops a click on a control the renderer is no longer
    /// drawing -- a `Param` field for a frequency that has none -- from
    /// leaving the caret somewhere invisible.
    pub fn focus_field(&mut self, field: FormField) {
        let showing = match field {
            FormField::Name | FormField::Command => {
                matches!(self.dialog, UiDialog::AddTask | UiDialog::EditTask(_))
            }
            FormField::Param => param_kind(self.form.frequency_index) == ParamKind::Text,
        };
        if showing {
            self.focus = Some(field);
        }
    }

    /// Move the caret to the next field, wrapping.
    ///
    /// `Param` is in the cycle only when it is a *typed* field, so Tab visits
    /// exactly the fields that can take a keystroke.
    pub fn focus_next(&mut self) {
        if !matches!(self.dialog, UiDialog::AddTask | UiDialog::EditTask(_)) {
            return;
        }
        let has_param = param_kind(self.form.frequency_index) == ParamKind::Text;
        self.focus = Some(match self.focus {
            Some(FormField::Name) => FormField::Command,
            Some(FormField::Command) if has_param => FormField::Param,
            // From an unfocused form Tab lands on Name, which is also where
            // it lands from the last field: one cycle, entered at the top.
            _ => FormField::Name,
        });
    }

    /// Append `ch` to the focused field.
    ///
    /// The numeric fields take digits only and are parsed rather than stored
    /// as text, so the form can never hold a "day of month" that is not a
    /// number. A digit that would push the value past what the field can hold
    /// is dropped rather than wrapping the value round to something small --
    /// silently rescheduling a monthly job from the 31st to the 3rd is the
    /// kind of edit a user would not notice until it had already run.
    pub fn type_char(&mut self, ch: char) -> bool {
        let Some(field) = self.focus else {
            return false;
        };
        match field {
            FormField::Name => {
                self.form.name.push(ch);
                true
            }
            FormField::Command => {
                self.form.command.push(ch);
                true
            }
            FormField::Param => match self.form.frequency_index {
                3 => {
                    let mut value = u32::from(self.form.monthly_day);
                    let changed = Self::push_digit(&mut value, ch, 31);
                    self.form.monthly_day = u8::try_from(value).unwrap_or(self.form.monthly_day);
                    changed
                }
                5 => {
                    let mut value = self.form.interval_minutes;
                    // A minute count past a year is not a schedule anybody
                    // means; the cap is there to stop the multiply below from
                    // having to think about overflow, not to be a policy.
                    let changed = Self::push_digit(&mut value, ch, MAX_INTERVAL_MINUTES);
                    self.form.interval_minutes = value;
                    changed
                }
                6 => {
                    self.form.cron_expr.push(ch);
                    true
                }
                _ => false,
            },
        }
    }

    /// Append `ch` to a numeric field, refusing anything that is not a digit
    /// or that would take the value past `max`.
    fn push_digit(value: &mut u32, ch: char, max: u32) -> bool {
        let Some(digit) = ch.to_digit(10) else {
            return false;
        };
        let Some(next) = value.checked_mul(10).and_then(|v| v.checked_add(digit)) else {
            return false;
        };
        if next > max {
            return false;
        }
        *value = next;
        true
    }

    /// Delete the last character of the focused field.
    pub fn backspace(&mut self) -> bool {
        let Some(field) = self.focus else {
            return false;
        };
        match field {
            FormField::Name => self.form.name.pop().is_some(),
            FormField::Command => self.form.command.pop().is_some(),
            FormField::Param => match self.form.frequency_index {
                3 => {
                    let was = self.form.monthly_day;
                    self.form.monthly_day = was / 10;
                    was != self.form.monthly_day
                }
                5 => {
                    let was = self.form.interval_minutes;
                    self.form.interval_minutes = was / 10;
                    was != self.form.interval_minutes
                }
                6 => self.form.cron_expr.pop().is_some(),
                _ => false,
            },
        }
    }

    // -- rendering ------------------------------------------------------------

    /// Draw the whole window, recording a hit box for everything clickable.
    ///
    /// The geometry comes from [`Layout`], which is derived from the size
    /// passed in and never stored, so a resize cannot leave a stale rectangle
    /// behind for the hit test to consult. Hit boxes are recorded by the same
    /// code that paints, which is what keeps a control's clickable area and
    /// its visible area from drifting apart.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let layout = Layout::new(width, height, self.status_message.is_some());
        let mut frame = Frame::new(layout.window.w, layout.window.h);

        // Window background.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: layout.window.w,
            height: layout.window.h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        self.render_header(&mut frame, &layout);
        self.render_toolbar(&mut frame, &layout);
        self.render_tab_bar(&mut frame, &layout);

        // Clipping the content also clips its hit boxes: `Frame::hit` trims
        // to the innermost clip and drops a box left with no area, so a row
        // scrolled off the bottom stops being clickable without the click
        // handler needing a bounds check of its own.
        frame.clip(layout.content);
        match self.tab {
            UiTab::Tasks => self.render_task_list(&mut frame, &layout),
            UiTab::History => self.render_history(&mut frame, &layout),
        }
        frame.unclip();

        if let Some(msg) = &self.status_message {
            self.render_status_bar(&mut frame, &layout, msg);
        }

        if !matches!(self.dialog, UiDialog::None) {
            // Everything drawn above stays on screen but must stop being
            // clickable: a modal that can be clicked past is not modal. The
            // scrim then claims the whole window, so a click outside the
            // dialog lands on a target that knows to swallow it rather than
            // on whatever happens to sit underneath.
            frame.discard_hits();
            frame.hit(Target::Scrim, layout.window);
        }
        match self.dialog {
            UiDialog::None => {}
            UiDialog::AddTask => self.render_add_edit_dialog(&mut frame, &layout, "Add Task"),
            UiDialog::EditTask(_) => self.render_add_edit_dialog(&mut frame, &layout, "Edit Task"),
            UiDialog::ConfirmDelete(id) => {
                self.render_confirm_delete_dialog(&mut frame, &layout, id);
            }
        }

        frame
    }

    /// What a click at `(x, y)` would land on, given the current window size.
    ///
    /// Answered by drawing the frame and asking it, rather than by a parallel
    /// set of rectangles: one geometry, so a control that moves takes its
    /// clickable area with it.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.window_width, self.window_height)
            .hit_test(x, y)
    }

    fn render_header(&self, frame: &mut Frame, layout: &Layout) {
        let band = layout.header;
        let width = band.w;
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: band.y,
            width,
            height: band.h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        if let (Some(y), Some((x, w))) = (
            centre_line(band, FONT_SIZE_HEADING),
            span(band, PADDING, width - PADDING * 2.0),
        ) {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: String::from("Task Scheduler"),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_HEADING,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Task count in header.
        let count_text = format!(
            "{} tasks ({} enabled)",
            self.scheduler.task_count(),
            self.scheduler.enabled_count()
        );
        // Never left of the title, however narrow the window gets: at 200px the
        // two would otherwise swap places and the count would be drawn off the
        // left edge.
        if let (Some(y), Some((x, w))) = (
            centre_line(band, FONT_SIZE_SMALL),
            span(band, (width - 200.0).max(PADDING), 190.0),
        ) {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: count_text,
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_toolbar(&self, frame: &mut Frame, layout: &Layout) {
        let band = layout.toolbar;

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: band.y,
            width: band.w,
            height: band.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // The button shrinks to the strip rather than being centred in it at
        // its nominal height. A button is a fill and a hit box, and both are
        // taken exactly as asked whatever the clip: a 28-point button centred
        // in a 6-point toolbar paints over the tab bar below it *and* answers
        // clicks there.
        let btn_h = BUTTON_HEIGHT.min(band.h);
        let btn_y = band.y + (band.h - btn_h) / 2.0;
        let mut bx = PADDING;
        let mut button = |target: Option<Target>, label: &str, bg: Color| {
            // Width the same way: four 80-point buttons need 360 points of
            // toolbar, and the window is not obliged to provide them.
            let w = (band.right() - bx).clamp(0.0, BUTTON_WIDTH);
            let rect = Rect::new(bx, btn_y, w, btn_h);
            bx += BUTTON_WIDTH + 8.0;
            (target, label.to_string(), bg, rect)
        };

        // Three of the four buttons need a selected task to act on, and the
        // same condition decides both halves of "disabled": the grey fill and
        // the absent hit box. Passing `None` for the target is what makes a
        // greyed-out button genuinely unclickable rather than merely
        // grey-looking -- there is no second check in the click handler that
        // could disagree with what was drawn.
        let selected = self.selected_task_id;
        let enabled_bg = |on: Color| {
            if selected.is_some() {
                on
            } else {
                COLOR_SURFACE2
            }
        };
        let gate = |t: Target| selected.map(|_| t);

        let toggle_label = if selected
            .and_then(|id| self.scheduler.get_task(id))
            .is_some_and(|t| t.enabled)
        {
            "Disable"
        } else {
            "Enable"
        };

        let buttons = [
            button(Some(Target::Add), "Add", COLOR_GREEN),
            button(gate(Target::Edit), "Edit", enabled_bg(COLOR_BLUE)),
            button(gate(Target::Remove), "Remove", enabled_bg(COLOR_RED)),
            button(
                gate(Target::ToggleEnabled),
                toggle_label,
                enabled_bg(COLOR_PEACH),
            ),
        ];
        for (target, label, bg, rect) in buttons {
            self.render_button(frame, target, rect, &label, bg);
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, layout: &Layout) {
        let band = layout.tab_bar;

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: band.y,
            width: band.w,
            height: band.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line, along the bottom edge but never above the top one.
        let (sep_y, sep_h) = bottom_strip(band, 1.0);
        if sep_h > 0.0 {
            frame.push(RenderCommand::Line {
                x1: 0.0,
                y1: sep_y,
                x2: band.w,
                y2: sep_y,
                color: COLOR_SURFACE1,
                width: sep_h,
            });
        }

        for (tab, label, x, underline_w) in [
            (UiTab::Tasks, "Tasks", PADDING, 40.0),
            (UiTab::History, "History", PADDING + 80.0, 50.0),
        ] {
            let selected = self.tab == tab;
            if let (Some(y), Some((tx, tw))) =
                (centre_line(band, FONT_SIZE), span(band, x, TAB_HIT_WIDTH))
            {
                frame.push(RenderCommand::Text {
                    x: tx,
                    y,
                    text: label.to_string(),
                    color: if selected { COLOR_BLUE } else { COLOR_SUBTEXT },
                    font_size: FONT_SIZE,
                    font_weight: if selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(tw),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            if selected && let Some((ux, uw)) = span(band, x, underline_w) {
                let (uy, uh) = bottom_strip(band, 3.0);
                frame.push(RenderCommand::FillRect {
                    x: ux,
                    y: uy,
                    width: uw,
                    height: uh,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::all(1.5),
                });
            }

            // The whole height of the strip, not just the text's own line:
            // a tab that only answers to a click on its glyphs is a tab that
            // feels broken every time the pointer lands a pixel high.
            if let Some((hx, hw)) = span(band, x, TAB_HIT_WIDTH) {
                frame.hit(Target::Tab(tab), Rect::new(hx, band.y, hw, band.h));
            }
        }
    }

    fn render_task_list(&self, frame: &mut Frame, layout: &Layout) {
        let area = layout.content;
        let width = area.w;
        let top = area.y;
        let height = area.h;

        // Column headings. The strip is a band of the area, not a 32-point
        // rectangle drawn at the area's top corner: the content area is
        // whatever `take_top` had left, and it is routinely shorter than one
        // row in a small window.
        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT.min(area.h));
        frame.push(RenderCommand::FillRect {
            x: head.x,
            y: head.y,
            width: head.w,
            height: head.h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let col_enabled_x = PADDING;
        let col_name_x: f32 = 50.0;
        let col_command_x: f32 = 200.0;
        let col_freq_x: f32 = 420.0;
        let col_next_x: f32 = 560.0;
        let col_result_x: f32 = 700.0;

        for (label, x) in [
            ("On", col_enabled_x),
            ("Name", col_name_x),
            ("Command", col_command_x),
            ("Frequency", col_freq_x),
            ("Next Run", col_next_x),
            ("Last Result", col_result_x),
        ] {
            // A column that starts past the right edge is dropped rather than
            // drawn off it: the six columns want 815 points and the window is
            // free to be 400.
            run_in(
                frame,
                head,
                Run {
                    x,
                    w: 140.0,
                    size: FONT_SIZE_SMALL,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Bold,
                },
                label.to_string(),
            );
        }

        // Task rows. The old loop drew every task at a computed y with no
        // bound at all: the surrounding clip hid the overflow, and with no
        // offset to scroll by, a list longer than the window simply had rows
        // that could not be reached.
        let tasks = self.scheduler.list_tasks();
        let rows_top = top + ROW_HEIGHT;
        let window = scroll_window::visible(
            tasks.len(),
            ROW_HEIGHT,
            height - ROW_HEIGHT - LIST_MORE_HEIGHT,
            self.task_list_scroll,
        );
        for (drawn, task) in tasks
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            // Stripe by absolute position, not by position on screen, so the
            // banding does not invert as the list scrolls.
            let i = window.start.saturating_add(drawn);
            let row_y = rows_top + (drawn as f32) * ROW_HEIGHT;
            let is_selected = self.selected_task_id == Some(task.id);

            // The row is what the area and the nominal row height have in
            // common. `scroll_window::visible` already counts whole rows, so
            // this trims nothing today -- it is here so that the row a cell is
            // measured against can never be a row the area does not contain.
            let Some(row) = Rect::new(area.x, row_y, area.w, ROW_HEIGHT).intersect(area) else {
                continue;
            };

            // Row background.
            let row_bg = if is_selected {
                COLOR_SURFACE1
            } else if i % 2 == 0 {
                COLOR_BASE
            } else {
                COLOR_SURFACE0
            };
            frame.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });
            // By task id, not by row index: the list scrolls and can be
            // re-sorted, so an index recorded here would name a different
            // task by the time the click arrives.
            frame.hit(Target::TaskRow(task.id), row);

            // Enabled checkbox. Recorded after the row so it wins the click:
            // the box is inside the row's own rectangle, and hitting the box
            // must toggle rather than merely select.
            //
            // Padded to the full row height. A 16px square is a small target
            // for a pointer; the column is the checkbox's alone, so widening
            // it costs nothing and misses nothing.
            if let Some(hit) =
                Rect::new(row.x, row.y, col_enabled_x + CHECKBOX_SIZE + 6.0, row.h).intersect(row)
            {
                frame.hit(Target::TaskCheckbox(task.id), hit);
            }
            let cb_h = CHECKBOX_SIZE.min(row.h);
            if let Some(cb) = Rect::new(
                col_enabled_x,
                row.y + (row.h - cb_h) / 2.0,
                CHECKBOX_SIZE,
                cb_h,
            )
            .intersect(row)
            {
                frame.push(RenderCommand::StrokeRect {
                    x: cb.x,
                    y: cb.y,
                    width: cb.w,
                    height: cb.h,
                    color: COLOR_SUBTEXT,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(3.0),
                });
                if task.enabled
                    && let Some(tick) =
                        Rect::new(cb.x + 3.0, cb.y + 3.0, cb.w - 6.0, cb.h - 6.0).intersect(cb)
                {
                    frame.push(RenderCommand::FillRect {
                        x: tick.x,
                        y: tick.y,
                        width: tick.w,
                        height: tick.h,
                        color: COLOR_GREEN,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }
            }

            let name_color = if task.enabled {
                COLOR_TEXT
            } else {
                COLOR_SUBTEXT
            };
            let next_run_text = if task.next_run_timestamp == u64::MAX {
                String::from("--")
            } else {
                format_timestamp(task.next_run_timestamp)
            };
            let result_color = match &task.last_result {
                None => COLOR_SUBTEXT,
                Some(TaskResult::Ok) => COLOR_GREEN,
                Some(TaskResult::Error(_)) => COLOR_RED,
            };
            for (run, text) in [
                (
                    Run {
                        x: col_name_x,
                        w: 145.0,
                        size: FONT_SIZE,
                        color: name_color,
                        weight: FontWeightHint::Regular,
                    },
                    task.name.clone(),
                ),
                (
                    Run {
                        x: col_command_x,
                        w: 215.0,
                        size: FONT_SIZE,
                        color: COLOR_SUBTEXT,
                        weight: FontWeightHint::Regular,
                    },
                    task.command.clone(),
                ),
                (
                    Run {
                        x: col_freq_x,
                        w: 135.0,
                        size: FONT_SIZE_SMALL,
                        color: COLOR_MAUVE,
                        weight: FontWeightHint::Regular,
                    },
                    task.frequency.display_name(),
                ),
                (
                    Run {
                        x: col_next_x,
                        w: 135.0,
                        size: FONT_SIZE_SMALL,
                        color: COLOR_TEAL,
                        weight: FontWeightHint::Regular,
                    },
                    next_run_text,
                ),
                (
                    Run {
                        x: col_result_x,
                        w: 115.0,
                        size: FONT_SIZE_SMALL,
                        color: result_color,
                        weight: FontWeightHint::Regular,
                    },
                    task.result_display().to_string(),
                ),
            ] {
                run_in(frame, row, run, text);
            }
        }

        // A list hiding tasks says how many. The band is the line's own box,
        // so a strip with no room below the last row drops the note rather
        // than writing it over the status bar.
        let hidden = tasks.len().saturating_sub(window.count);
        if hidden > 0
            && let Some(band) = Rect::new(
                area.x,
                rows_top + (window.count as f32) * ROW_HEIGHT,
                area.w,
                FONT_SIZE_SMALL,
            )
            .intersect(area)
        {
            run_in(
                frame,
                band,
                Run {
                    x: PADDING,
                    w: width - PADDING * 2.0,
                    size: FONT_SIZE_SMALL,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Regular,
                },
                format!("{hidden} more"),
            );
        }

        // Empty state.
        if tasks.is_empty()
            && let Some(band) =
                Rect::new(area.x, top + ROW_HEIGHT + 40.0, area.w, FONT_SIZE).intersect(area)
        {
            run_in(
                frame,
                band,
                Run {
                    x: width / 2.0 - 80.0,
                    w: 200.0,
                    size: FONT_SIZE,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Regular,
                },
                String::from("No tasks scheduled"),
            );
        }
    }

    /// Draw the History tab.
    ///
    /// No hit boxes: a history entry is a record of something that already
    /// happened and there is nothing to do to one. Recording a target that no
    /// handler acts on would only make the rows look interactive.
    fn render_history(&self, frame: &mut Frame, layout: &Layout) {
        let area = layout.content;
        let width = area.w;
        let top = area.y;
        let height = area.h;

        // Column headings, as a band of the area rather than a fixed-height
        // rectangle at its top corner. See `render_task_list`.
        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT.min(area.h));
        frame.push(RenderCommand::FillRect {
            x: head.x,
            y: head.y,
            width: head.w,
            height: head.h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let col_time_x = PADDING;
        let col_name_x: f32 = 180.0;
        let col_status_x: f32 = 380.0;
        let col_duration_x: f32 = 500.0;
        let col_error_x: f32 = 620.0;

        for (label, x) in [
            ("Time", col_time_x),
            ("Task", col_name_x),
            ("Status", col_status_x),
            ("Duration", col_duration_x),
            ("Error", col_error_x),
        ] {
            run_in(
                frame,
                head,
                Run {
                    x,
                    w: 180.0,
                    size: FONT_SIZE_SMALL,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Bold,
                },
                label.to_string(),
            );
        }

        // History rows (newest first). The 100-entry cap was standing in for
        // a viewport and is not one: a hundred rows is 3200px, so it bounded
        // nothing and hid the rest behind the clip with no way to scroll to
        // them. The window bounds what is drawn; the cap now only bounds how
        // much history is offered.
        let entries = self.scheduler.history.recent(HISTORY_ROWS_OFFERED);
        let rows_top = top + ROW_HEIGHT;
        let window = scroll_window::visible(
            entries.len(),
            ROW_HEIGHT,
            height - ROW_HEIGHT - LIST_MORE_HEIGHT,
            self.history_scroll,
        );
        for (drawn, entry) in entries
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            // Stripe by absolute position so the banding does not invert as
            // the list scrolls.
            let i = window.start.saturating_add(drawn);
            let row_y = rows_top + (drawn as f32) * ROW_HEIGHT;
            let Some(row) = Rect::new(area.x, row_y, area.w, ROW_HEIGHT).intersect(area) else {
                continue;
            };
            let row_bg = if i % 2 == 0 {
                COLOR_BASE
            } else {
                COLOR_SURFACE0
            };
            frame.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let (status_text, status_color) = if entry.success {
                ("OK", COLOR_GREEN)
            } else {
                ("Failed", COLOR_RED)
            };
            for (run, text) in [
                (
                    Run {
                        x: col_time_x,
                        w: 165.0,
                        size: FONT_SIZE_SMALL,
                        color: COLOR_TEAL,
                        weight: FontWeightHint::Regular,
                    },
                    format_timestamp(entry.timestamp),
                ),
                (
                    Run {
                        x: col_name_x,
                        w: 195.0,
                        size: FONT_SIZE,
                        color: COLOR_TEXT,
                        weight: FontWeightHint::Regular,
                    },
                    entry.task_name.clone(),
                ),
                (
                    Run {
                        x: col_status_x,
                        w: 110.0,
                        size: FONT_SIZE,
                        color: status_color,
                        weight: FontWeightHint::Bold,
                    },
                    status_text.to_string(),
                ),
                (
                    Run {
                        x: col_duration_x,
                        w: 115.0,
                        size: FONT_SIZE_SMALL,
                        color: COLOR_SUBTEXT,
                        weight: FontWeightHint::Regular,
                    },
                    format_duration_ms(entry.duration_ms),
                ),
            ] {
                run_in(frame, row, run, text);
            }

            if let Some(err) = &entry.error {
                run_in(
                    frame,
                    row,
                    Run {
                        x: col_error_x,
                        w: 195.0,
                        size: FONT_SIZE_SMALL,
                        color: COLOR_RED,
                        weight: FontWeightHint::Regular,
                    },
                    err.clone(),
                );
            }
        }

        // A list hiding entries says how many.
        let hidden = entries.len().saturating_sub(window.count);
        if hidden > 0
            && let Some(band) = Rect::new(
                area.x,
                rows_top + (window.count as f32) * ROW_HEIGHT,
                area.w,
                FONT_SIZE_SMALL,
            )
            .intersect(area)
        {
            run_in(
                frame,
                band,
                Run {
                    x: PADDING,
                    w: width - PADDING * 2.0,
                    size: FONT_SIZE_SMALL,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Regular,
                },
                format!("{hidden} more"),
            );
        }

        // Empty state.
        if entries.is_empty()
            && let Some(band) =
                Rect::new(area.x, top + ROW_HEIGHT + 40.0, area.w, FONT_SIZE).intersect(area)
        {
            run_in(
                frame,
                band,
                Run {
                    x: width / 2.0 - 60.0,
                    w: 200.0,
                    size: FONT_SIZE,
                    color: COLOR_SUBTEXT,
                    weight: FontWeightHint::Regular,
                },
                String::from("No history yet"),
            );
        }
    }

    /// Move the task list `delta` rows, negative for up.
    ///
    /// Clamped at the top only: how many rows fit depends on the window
    /// height, which this method is not given. The render clamps against what
    /// it is actually drawing, so an offset past the end shows the last page.
    pub fn scroll_task_list_by(&mut self, delta: isize) {
        self.task_list_scroll = scroll_window::shift(self.task_list_scroll, delta);
    }

    /// Back to the first task.
    pub fn scroll_task_list_to_top(&mut self) {
        self.task_list_scroll = 0;
    }

    /// Move the history list `delta` rows, negative for up.
    pub fn scroll_history_by(&mut self, delta: isize) {
        self.history_scroll = scroll_window::shift(self.history_scroll, delta);
    }

    /// Back to the newest history entry.
    pub fn scroll_history_to_top(&mut self) {
        self.history_scroll = 0;
    }

    fn render_status_bar(&self, frame: &mut Frame, layout: &Layout, message: &str) {
        let band = layout.status;
        let width = band.w;
        let bar_h = band.h;
        let y = band.y;

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: bar_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });

        run_in(
            frame,
            band,
            Run {
                x: PADDING,
                w: width - PADDING * 2.0,
                size: FONT_SIZE_SMALL,
                color: COLOR_YELLOW,
                weight: FontWeightHint::Regular,
            },
            message.to_string(),
        );
    }

    fn render_add_edit_dialog(&self, frame: &mut Frame, layout: &Layout, title: &str) {
        let window = layout.window;

        // Semi-transparent overlay. The scrim's *hit* box was recorded by the
        // caller, before `discard_hits`; this only paints it.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: window.w,
            height: window.h,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_w: f32 = 440.0;
        let dialog_h: f32 = 380.0;
        // Never off the top or left edge. In a window smaller than the dialog
        // the centring subtraction goes negative, which would put the title
        // and the first fields out of reach rather than merely cramped.
        let dx = ((window.w - dialog_w) / 2.0).max(0.0);
        let dy = ((window.h - dialog_h) / 2.0).max(0.0);
        // ...and never off the bottom or right edge either. The clamp above
        // held the dialog's *origin* inside the window and left its size a
        // constant, so a window smaller than 440x380 got a dialog painted over
        // the desktop beyond it. Everything below is measured against this
        // rectangle rather than against the two constants, so a squeezed
        // dialog loses its lower rows instead of hanging them outside.
        let dialog = Rect::new(dx, dy, dialog_w.min(window.w), dialog_h.min(window.h));

        // Dialog background.
        frame.push(RenderCommand::FillRect {
            x: dialog.x,
            y: dialog.y,
            width: dialog.w,
            height: dialog.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        frame.push(RenderCommand::StrokeRect {
            x: dialog.x,
            y: dialog.y,
            width: dialog.w,
            height: dialog.h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title. The band is the line's own box, so the title is dropped
        // rather than drawn in a dialog with no room for it.
        if let Some(band) =
            Rect::new(dialog.x, dialog.y + PADDING, dialog.w, FONT_SIZE_HEADING).intersect(dialog)
        {
            run_in(
                frame,
                band,
                Run {
                    x: dialog.x + PADDING,
                    w: dialog.w - PADDING * 2.0,
                    size: FONT_SIZE_HEADING,
                    color: COLOR_TEXT,
                    weight: FontWeightHint::Bold,
                },
                title.to_string(),
            );
        }

        let mut field_y = dy + 44.0;
        let label_x = dx + PADDING;
        let value_x = dx + 130.0;
        let field_spacing = 36.0;

        // Every row is a label plus one control on the same baseline, so the
        // geometry is written once here rather than four times below. The
        // closure also advances `field_y`, which is what stops a row that is
        // conditionally drawn from leaving a gap where it would have been.
        //
        // The rows run down past the dialog's bottom edge in any window shorter
        // than 380, so both the label and the control that follows it are cut
        // to the dialog -- a field rectangle drawn uncut would record a hit box
        // outside the dialog it belongs to. `label` hands back the row's *full*
        // rectangle, because the checkbox row below measures itself from it and
        // a row cut down to nothing would have moved it to the window's corner;
        // `cut` is applied where the rectangle is drawn from. An empty
        // rectangle draws nothing -- see `render_text_field`.
        let cut = |r: Rect| r.intersect(dialog).unwrap_or(Rect::EMPTY);
        let mut label = |frame: &mut Frame, text: &str| {
            let y = field_y;
            if let Some(band) = Rect::new(dialog.x, y, dialog.w, FONT_SIZE).intersect(dialog) {
                run_in(
                    frame,
                    band,
                    Run {
                        x: label_x,
                        w: 100.0,
                        size: FONT_SIZE,
                        color: COLOR_SUBTEXT,
                        weight: FontWeightHint::Regular,
                    },
                    text.to_string(),
                );
            }
            field_y += field_spacing;
            Rect::new(value_x, y - 2.0, FIELD_WIDTH, FIELD_HEIGHT)
        };

        let rect = label(frame, "Name:");
        self.render_text_field(
            frame,
            Target::Field(FormField::Name),
            cut(rect),
            &self.form.name,
        );

        let rect = label(frame, "Command:");
        self.render_text_field(
            frame,
            Target::Field(FormField::Command),
            cut(rect),
            &self.form.command,
        );

        // The frequency is picked, not typed, so it takes no caret: the field
        // it is drawn as is a button that happens to look like one.
        let freq_label = FREQUENCY_LABELS
            .get(self.form.frequency_index)
            .unwrap_or(&"Unknown");
        let rect = label(frame, "Frequency:");
        self.render_picker(frame, Target::FrequencyCycle, cut(rect), freq_label);

        // Frequency-specific parameter. Which control this is -- picker, text
        // field or nothing at all -- is decided by `param_kind`, so the click
        // handler and the focus rules read the same answer as the drawing.
        match self.form.frequency_index {
            2 => {
                let day = DayOfWeek::from_u8(self.form.weekly_day)
                    .map(DayOfWeek::display_name)
                    .unwrap_or("Monday");
                let rect = label(frame, "Day of week:");
                self.render_picker(frame, Target::ParamCycle, cut(rect), day);
            }
            3 => {
                let day_text = self.form.monthly_day.to_string();
                let rect = label(frame, "Day of month:");
                self.render_text_field(
                    frame,
                    Target::Field(FormField::Param),
                    cut(rect),
                    &day_text,
                );
            }
            5 => {
                let min_text = self.form.interval_minutes.to_string();
                let rect = label(frame, "Minutes:");
                self.render_text_field(
                    frame,
                    Target::Field(FormField::Param),
                    cut(rect),
                    &min_text,
                );
            }
            6 => {
                let rect = label(frame, "Cron:");
                self.render_text_field(
                    frame,
                    Target::Field(FormField::Param),
                    cut(rect),
                    &self.form.cron_expr,
                );
            }
            _ => {}
        }

        // Enabled checkbox. Its row is the one `label` did not hand back as a
        // field rectangle, so it is cut to the dialog here instead.
        let cb_rect = label(frame, "Enabled:");
        let cb_y = cb_rect.y + 2.0;
        if let Some(cb) =
            Rect::new(value_x, cb_y - 1.0, CHECKBOX_SIZE, CHECKBOX_SIZE).intersect(dialog)
        {
            frame.push(RenderCommand::StrokeRect {
                x: cb.x,
                y: cb.y,
                width: cb.w,
                height: cb.h,
                color: COLOR_SUBTEXT,
                line_width: 1.0,
                corner_radii: CornerRadii::all(3.0),
            });
            if self.form.enabled
                && let Some(tick) = Rect::new(
                    value_x + 3.0,
                    cb_y + 2.0,
                    CHECKBOX_SIZE - 6.0,
                    CHECKBOX_SIZE - 6.0,
                )
                .intersect(cb)
            {
                frame.push(RenderCommand::FillRect {
                    x: tick.x,
                    y: tick.y,
                    width: tick.w,
                    height: tick.h,
                    color: COLOR_GREEN,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
        // The whole row, label included: on a form, "Enabled:" and its box are
        // one control, and a click on the word is a click on the box.
        if let Some(hit) = Rect::new(
            label_x,
            cb_rect.y,
            value_x - label_x + CHECKBOX_SIZE + 6.0,
            FIELD_HEIGHT,
        )
        .intersect(dialog)
        {
            frame.hit(Target::FormEnabled, hit);
        }

        // Dialog buttons, along the dialog's own bottom edge rather than at a
        // constant 380 points below its top.
        let btn_y = dialog.bottom() - BUTTON_HEIGHT - PADDING;
        let cancel_x = dialog.right() - PADDING - BUTTON_WIDTH;
        let save_x = cancel_x - 8.0 - BUTTON_WIDTH;

        // Save greys out when the form is not submittable but stays
        // *clickable*, which is the deliberate exception to how the toolbar
        // buttons work. A greyed toolbar button has a visible reason -- no row
        // is selected. A greyed Save does not: the missing field could be any
        // of them, and a button that cannot be pressed cannot say which. So it
        // is pressed, and `commit_*` answers in the status bar.
        let can_save = self.form_is_valid();
        self.render_button(
            frame,
            Some(Target::DialogSave),
            cut(Rect::new(save_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT)),
            "Save",
            if can_save {
                COLOR_GREEN
            } else {
                COLOR_SURFACE2
            },
        );
        self.render_button(
            frame,
            Some(Target::DialogCancel),
            cut(Rect::new(cancel_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT)),
            "Cancel",
            COLOR_SURFACE2,
        );
    }

    fn render_confirm_delete_dialog(&self, frame: &mut Frame, layout: &Layout, task_id: u64) {
        let window = layout.window;

        // Semi-transparent overlay; its hit box was recorded by the caller.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: window.w,
            height: window.h,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let dialog_w: f32 = 360.0;
        let dialog_h: f32 = 160.0;
        let dx = ((window.w - dialog_w) / 2.0).max(0.0);
        let dy = ((window.h - dialog_h) / 2.0).max(0.0);
        // Cut to the window, not merely nudged into its corner: see
        // `render_add_edit_dialog`.
        let dialog = Rect::new(dx, dy, dialog_w.min(window.w), dialog_h.min(window.h));
        let cut = |r: Rect| r.intersect(dialog).unwrap_or(Rect::EMPTY);

        frame.push(RenderCommand::FillRect {
            x: dialog.x,
            y: dialog.y,
            width: dialog.w,
            height: dialog.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        frame.push(RenderCommand::StrokeRect {
            x: dialog.x,
            y: dialog.y,
            width: dialog.w,
            height: dialog.h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let task_name = self
            .scheduler
            .get_task(task_id)
            .map(|t| t.name.as_str())
            .unwrap_or("this task");
        for (dy_off, size, color, weight, text) in [
            (
                PADDING,
                FONT_SIZE_HEADING,
                COLOR_TEXT,
                FontWeightHint::Bold,
                String::from("Confirm Delete"),
            ),
            (
                52.0,
                FONT_SIZE,
                COLOR_SUBTEXT,
                FontWeightHint::Regular,
                format!("Delete task '{task_name}'? This cannot be undone."),
            ),
        ] {
            let band = cut(Rect::new(dialog.x, dialog.y + dy_off, dialog.w, size));
            run_in(
                frame,
                band,
                Run {
                    x: dialog.x + PADDING,
                    w: dialog.w - PADDING * 2.0,
                    size,
                    color,
                    weight,
                },
                text,
            );
        }

        // Buttons, measured from the dialog's own bottom-right corner.
        let btn_y = dialog.bottom() - BUTTON_HEIGHT - PADDING;
        let cancel_x = dialog.right() - PADDING - BUTTON_WIDTH;
        let delete_x = cancel_x - 8.0 - BUTTON_WIDTH;

        self.render_button(
            frame,
            Some(Target::DeleteConfirm),
            cut(Rect::new(delete_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT)),
            "Delete",
            COLOR_RED,
        );
        self.render_button(
            frame,
            Some(Target::DialogCancel),
            cut(Rect::new(cancel_x, btn_y, BUTTON_WIDTH, BUTTON_HEIGHT)),
            "Cancel",
            COLOR_SURFACE2,
        );
    }

    /// Draw an editable field, highlighting it and drawing a caret when it has
    /// the focus.
    ///
    /// The caret is drawn at the end of the text because that is where the
    /// insertion point is: this form appends and backspaces rather than
    /// offering a movable cursor, so a caret anywhere else would promise an
    /// edit the field cannot make.
    fn render_text_field(&self, frame: &mut Frame, target: Target, rect: Rect, value: &str) {
        // A control with no area draws nothing and answers nothing. Returning
        // here rather than pushing zero-sized commands is the difference
        // between a field that has been squeezed out of the dialog and one
        // that is still recording a hit box at a position outside it.
        if rect.is_empty() {
            return;
        }
        let focused = matches!(target, Target::Field(f) if self.focus == Some(f));

        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if focused { COLOR_BLUE } else { COLOR_SURFACE2 },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });

        let text_y = centre_line(rect, FONT_SIZE);
        let text_span = span(rect, rect.x + 6.0, rect.w - 12.0);
        if let (Some(y), Some((x, w))) = (text_y, text_span) {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: value.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if focused && let (Some(y), Some((x, _))) = (text_y, text_span) {
            // Measured, not counted: a caret placed at `len * average_width`
            // drifts off the end of the text on anything but a monospace
            // font. Kept inside the field's own right edge so a long value
            // does not push it out over the dialog. Both of its dimensions are
            // measured against the field for the same reason: a fill is drawn
            // at exactly the size it is given, so a literal 1.5 x FONT_SIZE
            // caret is ink outside a field that has less than that to spare.
            let caret_x = (x + text::measure(value, FONT_SIZE, FontWeightHint::Regular))
                .min(rect.right() - 3.0)
                .max(rect.x);
            frame.push(RenderCommand::FillRect {
                x: caret_x,
                y,
                width: 1.5_f32.min(rect.right() - caret_x),
                height: FONT_SIZE.min(rect.h),
                color: COLOR_TEXT,
                corner_radii: CornerRadii::ZERO,
            });
        }

        frame.hit(target, rect);
    }

    /// Draw a value that is picked rather than typed: same box as a text
    /// field, but with a hint that clicking cycles it and never a caret.
    fn render_picker(&self, frame: &mut Frame, target: Target, rect: Rect, value: &str) {
        // See `render_text_field`: no area means no ink and no hit box.
        if rect.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: COLOR_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        let text_y = centre_line(rect, FONT_SIZE);

        // Room for the chevron, so a long value is elided before it reaches the
        // mark that says the value can be changed.
        if let (Some(y), Some((x, w))) = (text_y, span(rect, rect.x + 6.0, rect.w - 26.0)) {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: value.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if let (Some(y), Some((x, w))) = (text_y, span(rect, rect.right() - 16.0, 12.0)) {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: String::from("\u{25be}"),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
                overflow: TextOverflow::Clip,
            });
        }

        frame.hit(target, rect);
    }

    /// Draw a button, recording a hit box only when `target` is `Some`.
    ///
    /// The `Option` is the whole point: a disabled button is drawn but not
    /// recorded, so "greyed out" and "not clickable" are one decision made at
    /// one place rather than a colour here and a guard in the click handler
    /// that could come to disagree with it.
    fn render_button(
        &self,
        frame: &mut Frame,
        target: Option<Target>,
        rect: Rect,
        label: &str,
        bg: Color,
    ) {
        // See `render_text_field`: no area means no ink and no hit box. A
        // disabled button is drawn-but-not-recorded; a button squeezed to
        // nothing is neither.
        if rect.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Horizontal centring is no more a bound than vertical centring is:
        // `centre - measure / 2` is left of a button narrower than its own
        // label, and the `max_width` then carries the run off the right edge
        // as well. Clamping the start and measuring the width from it elides
        // the label instead, which is what a too-small button should do.
        let text_x = text::center_x(label, rect.centre().0, FONT_SIZE, FontWeightHint::Bold);
        if let (Some(y), Some((x, w))) = (centre_line(rect, FONT_SIZE), span(rect, text_x, rect.w))
        {
            frame.push(RenderCommand::Text {
                x,
                y,
                text: label.to_string(),
                color: COLOR_BASE,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        if let Some(target) = target {
            frame.hit(target, rect);
        }
    }
}

// -- event handling -----------------------------------------------------------

impl SchedulerUI {
    /// Remember the size the window is now, so the hit test can re-draw at it.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.window_width = sane(width);
        self.window_height = sane(height);
    }

    /// Read the host clock into [`SchedulerUI::now`], and run whatever became
    /// due since the last read.
    ///
    /// The one impure method on this type. Everything else takes the instant
    /// as an argument or reads the stored one, which is what lets a test drive
    /// a whole day of scheduling in a few microseconds without touching a
    /// clock it does not control.
    pub fn refresh_clock(&mut self) -> bool {
        let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            // A host clock set before 1970. Nothing here can be believed, and
            // treating it as second zero would make every task overdue.
            return false;
        };
        let now = since_epoch.as_secs();
        if now <= self.now {
            // Non-monotonic host clocks exist, and a scheduler that ran its
            // whole backlog again because the clock stepped backwards would be
            // worse than one that waited.
            return false;
        }
        self.now = now;
        let fired = self.scheduler.run_due_tasks(now);
        !fired.is_empty()
    }

    /// Act on a click at `(x, y)`, returning whether anything changed.
    pub fn handle_click(&mut self, x: f32, y: f32) -> bool {
        let Some(target) = self.target_at(x, y) else {
            // Bare background inside a dialog is impossible -- the scrim
            // covers the window -- so this is a click on the window's own
            // background with no dialog up, and there is nothing to do.
            return false;
        };
        match target {
            Target::Tab(tab) => {
                let changed = self.tab != tab;
                self.switch_to(tab);
                changed
            }
            Target::Add => {
                self.open_add_dialog();
                true
            }
            Target::Edit => {
                let Some(id) = self.selected_task_id else {
                    return false;
                };
                self.open_edit_dialog(id);
                true
            }
            Target::Remove => {
                let Some(id) = self.selected_task_id else {
                    return false;
                };
                self.open_delete_dialog(id);
                true
            }
            Target::ToggleEnabled => {
                self.toggle_selected_task();
                true
            }
            Target::TaskRow(id) => {
                let changed = self.selected_task_id != Some(id);
                self.select_task(id);
                changed
            }
            Target::TaskCheckbox(id) => {
                // Selects as well as toggles. Clicking a row's checkbox is
                // still a statement about which row you are looking at, and a
                // toolbar that went on offering to edit some other task would
                // be the surprising half of that.
                self.select_task(id);
                let enabled = self.scheduler.get_task(id).is_some_and(|t| t.enabled);
                if enabled {
                    self.scheduler.disable_task(id);
                } else {
                    self.scheduler.enable_task(id);
                }
                true
            }
            Target::Field(field) => {
                let changed = self.focus != Some(field);
                self.focus_field(field);
                changed
            }
            Target::FrequencyCycle => {
                self.cycle_frequency();
                true
            }
            Target::ParamCycle => {
                self.cycle_param();
                true
            }
            Target::FormEnabled => {
                self.toggle_form_enabled();
                true
            }
            Target::DialogSave => self.save_dialog(),
            Target::DialogCancel => {
                self.close_dialog();
                true
            }
            Target::DeleteConfirm => {
                let UiDialog::ConfirmDelete(id) = self.dialog else {
                    return false;
                };
                self.confirm_delete_task(id)
            }
            // A click on the scrim is a click at a dialog, not through it.
            // Swallowed rather than treated as Cancel: dismissing an
            // unsaved form by missing it is how work gets lost.
            Target::Scrim => false,
        }
    }

    /// Commit whichever form dialog is open.
    fn save_dialog(&mut self) -> bool {
        let now = self.now;
        match self.dialog {
            UiDialog::AddTask => self.commit_add_task(now).is_some(),
            UiDialog::EditTask(id) => self.commit_edit_task(id, now),
            // Save is not drawn on either of these, so reaching here would
            // mean the renderer and this handler disagree about what is on
            // screen. There is nothing safe to commit, so commit nothing.
            UiDialog::None | UiDialog::ConfirmDelete(_) => false,
        }
    }

    /// Act on a wheel notch, returning whether the view moved.
    ///
    /// `dy` is the raw delta, positive for a push away from the user.
    /// [`wheel::Accumulator::rows`] applies the sign flip, so its result is
    /// already a movement *down* the list.
    ///
    /// The accumulator is what makes a trackpad work: these lists are indexed
    /// in whole rows, so a stream of tenth-of-a-notch events would round to
    /// zero forever and scroll nothing at all. The fractions are banked
    /// instead, and delivered as a row once they add up to one.
    pub fn handle_scroll(&mut self, dy: f32) -> bool {
        // A dialog is modal: the list behind it does not scroll, and the
        // banked fraction is dropped so that a notch turned before the dialog
        // opened does not deliver a row after it closes.
        if !matches!(self.dialog, UiDialog::None) {
            self.wheel.reset();
            return false;
        }
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            return false;
        }
        let before = match self.tab {
            UiTab::Tasks => self.task_list_scroll,
            UiTab::History => self.history_scroll,
        };
        match self.tab {
            UiTab::Tasks => self.scroll_task_list_by(rows),
            UiTab::History => self.scroll_history_by(rows),
        }
        let after = match self.tab {
            UiTab::Tasks => self.task_list_scroll,
            UiTab::History => self.history_scroll,
        };
        before != after
    }

    /// Act on a key press, returning whether anything changed.
    ///
    /// Releases are ignored outright. Acting on both edges would run every
    /// shortcut twice -- and on a form, would type every character twice.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if !key.pressed {
            return false;
        }
        if matches!(self.dialog, UiDialog::None) {
            return self.handle_key_main(key);
        }
        self.handle_key_dialog(key)
    }

    /// Keys that act on the main window, with no dialog open.
    fn handle_key_main(&mut self, key: &KeyEvent) -> bool {
        match key.key {
            Key::Tab => {
                // The two tabs, not a focus ring: there is nothing else here
                // that takes the keyboard.
                self.switch_to(match self.tab {
                    UiTab::Tasks => UiTab::History,
                    UiTab::History => UiTab::Tasks,
                });
                true
            }
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            Key::Home => {
                let before = self.list_scroll();
                match self.tab {
                    UiTab::Tasks => self.scroll_task_list_to_top(),
                    UiTab::History => self.scroll_history_to_top(),
                }
                before != self.list_scroll()
            }
            Key::Delete => {
                let Some(id) = self.selected_task_id else {
                    return false;
                };
                // The confirmation, not the deletion. Delete is one keystroke
                // away from every arrow key, and a scheduled backup is not
                // something to lose to a mistyped Down.
                self.open_delete_dialog(id);
                true
            }
            Key::Enter => {
                let Some(id) = self.selected_task_id else {
                    return false;
                };
                self.open_edit_dialog(id);
                true
            }
            Key::N if key.modifiers.ctrl => {
                self.open_add_dialog();
                true
            }
            Key::Space => {
                if self.selected_task_id.is_none() {
                    return false;
                }
                self.toggle_selected_task();
                true
            }
            _ => false,
        }
    }

    /// Keys that act on an open dialog.
    fn handle_key_dialog(&mut self, key: &KeyEvent) -> bool {
        match key.key {
            Key::Escape => {
                self.close_dialog();
                true
            }
            Key::Tab => {
                self.focus_next();
                true
            }
            Key::Backspace => self.backspace(),
            Key::Enter => match self.dialog {
                UiDialog::ConfirmDelete(id) => self.confirm_delete_task(id),
                _ => self.save_dialog(),
            },
            _ => {
                // A keystroke with Ctrl or Alt held is a shortcut, not text:
                // Ctrl-N while filling in the Name field must not put an `n`
                // in it. Everything else goes through `KeyEvent::typed`, which
                // is the layout's own answer to what was typed -- Enter, Tab
                // and Escape all *produce* text on most layouts, and a field
                // that appended `key.text` raw would fill with control bytes.
                if key.modifiers.ctrl || key.modifiers.alt {
                    return false;
                }
                let mut typed = false;
                for ch in key.typed() {
                    typed |= self.type_char(ch);
                }
                typed
            }
        }
    }

    /// Where the showing list is scrolled to, in rows.
    fn list_scroll(&self) -> usize {
        match self.tab {
            UiTab::Tasks => self.task_list_scroll,
            UiTab::History => self.history_scroll,
        }
    }

    /// Move the selection `delta` rows through the task list.
    ///
    /// Only on the Tasks tab: the History tab has no selection to move, and
    /// silently moving the hidden one would leave the toolbar acting on a task
    /// the user cannot see.
    fn move_selection(&mut self, delta: isize) -> bool {
        if self.tab != UiTab::Tasks {
            return false;
        }
        let tasks = self.scheduler.list_tasks();
        if tasks.is_empty() {
            return false;
        }
        let last = tasks.len().saturating_sub(1);
        let current = self
            .selected_task_id
            .and_then(|id| tasks.iter().position(|t| t.id == id));
        let next = match (current, delta) {
            // Nothing selected: Down picks the first row and Up the last, so
            // either arrow gets a keyboard user onto the list.
            (None, d) if d >= 0 => 0,
            (None, _) => last,
            (Some(i), d) if d >= 0 => i.saturating_add(1).min(last),
            (Some(i), _) => i.saturating_sub(1),
        };
        let Some(task) = tasks.get(next) else {
            return false;
        };
        let id = task.id;
        let changed = self.selected_task_id != Some(id);
        self.select_task(id);
        // Keep the newly selected row on screen. Without this the selection
        // walks off the bottom of the viewport and the arrow keys appear to
        // stop working.
        self.reveal_row(next);
        changed
    }

    /// Scroll the task list so that row `index` is inside the viewport.
    fn reveal_row(&mut self, index: usize) {
        let content = Layout::new(
            self.window_width,
            self.window_height,
            self.status_message.is_some(),
        )
        .content;
        let capacity =
            scroll_window::capacity(ROW_HEIGHT, content.h - ROW_HEIGHT - LIST_MORE_HEIGHT);
        if capacity == 0 {
            return;
        }
        if index < self.task_list_scroll {
            self.task_list_scroll = index;
        } else if index >= self.task_list_scroll.saturating_add(capacity) {
            self.task_list_scroll = index.saturating_sub(capacity.saturating_sub(1));
        }
    }
}

/// Turn a window event into a change of state.
///
/// Free rather than a method so the whole event vocabulary can be read in one
/// place, and so the `App` impl below is the thin adapter it should be.
fn handle_event(ui: &mut SchedulerUI, event: &Event) -> EventResult {
    /// `true` means the state changed and the window needs repainting.
    fn result(changed: bool) -> EventResult {
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    match event {
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Press(MouseButton::Left) => result(ui.handle_click(m.x, m.y)),
            MouseEventKind::Scroll { dy, .. } => result(ui.handle_scroll(dy)),
            _ => EventResult::Ignored,
        },
        Event::Key(k) => result(ui.handle_key(k)),
        Event::Resize { width, height } => {
            // Consumed unconditionally: the size was recorded, which is a
            // change even when nothing visible moved.
            #[allow(clippy::cast_precision_loss)]
            ui.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // Only a clock that has actually advanced past a due time is worth a
        // repaint. Reporting `Ignored` otherwise is what lets an idle window
        // stop redrawing rather than repainting an unchanged picture once a
        // second forever.
        Event::Tick { .. } => result(ui.refresh_clock()),
        _ => EventResult::Ignored,
    }
}

impl App for SchedulerUI {
    fn title(&self) -> String {
        String::from("Task Scheduler")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A tick a second.
    ///
    /// This window's whole subject is *when* things run, so a "Next Run"
    /// column that only moved when the pointer did would be showing a time
    /// that had already passed. One second is also the resolution the
    /// scheduler itself works at, so a finer tick would find nothing new.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(1))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The renderer draws at the size it is given, but `target_at` has only
        // what it was last told. Recording it here means the two agree even if
        // the platform ever draws at a size it did not send a Resize for.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SchedulerUI {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

impl Default for SchedulerUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a unix timestamp into a human-readable UTC date/time string.
///
/// The two sentinels stay here rather than moving into the shared formatter:
/// `"Never"` means a task that has not run yet and `"--"` a next-run time
/// that does not exist, and neither is a rendering of an instant.
///
/// What replaced the rest is the point. This function decomposed the instant
/// **twice** — once through `decompose_timestamp` for the time of day and
/// again through a local `days_to_ymd(ts / 86400)` for the date — so the two
/// halves of one string were derived by two different routes, and nothing made
/// them agree. That is the shape a timezone would have broken first: applying
/// an offset to one half and not the other yields a clock reading from one day
/// stamped with another day's date. `guitk::datetime` decomposes once, and
/// [`decompose_timestamp`] now reads the same type, so the file holds one
/// calendar rather than two.
///
/// UTC, explicitly: there is no per-process zone plumbing yet (known-issues
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and a scheduler that shows the
/// wrong hour is a scheduler nobody can set. The matching half of that problem
/// — that a task scheduled for 03:00 *fires* at 03:00 UTC — is known-issues
/// `TD-CRON-MATCHES-UTC-FIELDS`.
fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::from("Never");
    }
    if ts == u64::MAX {
        return String::from("--");
    }
    guitk::datetime::stamp(
        i64::try_from(ts).unwrap_or(i64::MAX),
        &guitk::tzrules::Tz::utc(),
    )
}

/// Format milliseconds into a readable duration.
///
/// See automator's `format_duration_ms`, which was the other copy. This one
/// rounded through `f64`, so a task that ran 59 960 ms reported `60.0s` — a
/// full minute, printed in the shape reserved for spans shorter than one.
fn format_duration_ms(ms: u64) -> String {
    guitk::duration::units_ms(ms)
}

// ============================================================================
// Entry point
// ============================================================================

/// Open the window.
///
/// The clock is read once before the first frame rather than waiting for the
/// first tick: at second zero every task's next-run time is computed against
/// the epoch, so a window that opened before its first tick would spend that
/// second claiming every task was a lifetime overdue.
fn main() -> ExitCode {
    let mut ui = SchedulerUI::new();
    ui.refresh_clock();
    app::launch("taskscheduler", &mut ui)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::probe;

    // -- CronField tests ----------------------------------------------------

    #[test]
    fn test_cron_field_any_matches_everything() {
        let f = CronField::Any;
        for i in 0..60 {
            assert!(f.matches(i));
        }
    }

    #[test]
    fn test_cron_field_value_matches_exact() {
        let f = CronField::Value(5);
        assert!(f.matches(5));
        assert!(!f.matches(4));
        assert!(!f.matches(6));
    }

    #[test]
    fn test_cron_field_list_matches_members() {
        let f = CronField::List(vec![1, 3, 5, 7]);
        assert!(f.matches(1));
        assert!(f.matches(5));
        assert!(!f.matches(2));
        assert!(!f.matches(8));
    }

    #[test]
    fn test_cron_field_range_matches_inclusive() {
        let f = CronField::Range(3, 7);
        assert!(!f.matches(2));
        assert!(f.matches(3));
        assert!(f.matches(5));
        assert!(f.matches(7));
        assert!(!f.matches(8));
    }

    #[test]
    fn test_cron_field_step_matches_multiples() {
        let f = CronField::Step(0, 15);
        assert!(f.matches(0));
        assert!(f.matches(15));
        assert!(f.matches(30));
        assert!(f.matches(45));
        assert!(!f.matches(10));
        assert!(!f.matches(1));
    }

    #[test]
    fn test_cron_field_step_with_base() {
        let f = CronField::Step(5, 10);
        assert!(f.matches(5));
        assert!(f.matches(15));
        assert!(f.matches(25));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    #[test]
    fn test_cron_field_step_zero_step() {
        let f = CronField::Step(5, 0);
        assert!(f.matches(5));
        assert!(!f.matches(0));
        assert!(!f.matches(10));
    }

    // -- CronField parsing tests --------------------------------------------

    #[test]
    fn test_parse_cron_field_wildcard() {
        assert_eq!(CronField::parse("*"), Ok(CronField::Any));
    }

    #[test]
    fn test_parse_cron_field_single_value() {
        assert_eq!(CronField::parse("42"), Ok(CronField::Value(42)));
    }

    #[test]
    fn test_parse_cron_field_list() {
        assert_eq!(
            CronField::parse("1,3,5"),
            Ok(CronField::List(vec![1, 3, 5]))
        );
    }

    #[test]
    fn test_parse_cron_field_list_deduplicates() {
        assert_eq!(
            CronField::parse("5,3,5,1"),
            Ok(CronField::List(vec![1, 3, 5]))
        );
    }

    #[test]
    fn test_parse_cron_field_range() {
        assert_eq!(CronField::parse("1-5"), Ok(CronField::Range(1, 5)));
    }

    #[test]
    fn test_parse_cron_field_invalid_range() {
        assert_eq!(
            CronField::parse("5-1"),
            Err(CronParseError::InvalidRange(5, 1))
        );
    }

    #[test]
    fn test_parse_cron_field_step_from_zero() {
        assert_eq!(CronField::parse("*/15"), Ok(CronField::Step(0, 15)));
    }

    #[test]
    fn test_parse_cron_field_step_from_base() {
        assert_eq!(CronField::parse("5/10"), Ok(CronField::Step(5, 10)));
    }

    #[test]
    fn test_parse_cron_field_empty_is_error() {
        assert_eq!(CronField::parse(""), Err(CronParseError::EmptyField));
    }

    #[test]
    fn test_parse_cron_field_invalid_number() {
        assert!(matches!(
            CronField::parse("abc"),
            Err(CronParseError::InvalidNumber(_))
        ));
    }

    // -- CronExpr parsing tests ---------------------------------------------

    #[test]
    fn test_parse_cron_expr_all_wildcards() {
        let expr = CronExpr::parse("* * * * *").expect("should parse");
        assert_eq!(expr.minute, CronField::Any);
        assert_eq!(expr.hour, CronField::Any);
        assert_eq!(expr.day_of_month, CronField::Any);
        assert_eq!(expr.month, CronField::Any);
        assert_eq!(expr.day_of_week, CronField::Any);
    }

    #[test]
    fn test_parse_cron_expr_specific_time() {
        let expr = CronExpr::parse("30 2 * * *").expect("should parse");
        assert_eq!(expr.minute, CronField::Value(30));
        assert_eq!(expr.hour, CronField::Value(2));
    }

    #[test]
    fn test_parse_cron_expr_wrong_field_count() {
        assert_eq!(
            CronExpr::parse("* * *"),
            Err(CronParseError::WrongFieldCount(3))
        );
    }

    #[test]
    fn test_parse_cron_expr_too_many_fields() {
        assert_eq!(
            CronExpr::parse("* * * * * *"),
            Err(CronParseError::WrongFieldCount(6))
        );
    }

    #[test]
    fn test_cron_expr_matches() {
        let expr = CronExpr::parse("30 2 15 6 *").expect("should parse");
        assert!(expr.matches(30, 2, 15, 6, 3));
        assert!(!expr.matches(0, 2, 15, 6, 3));
        assert!(!expr.matches(30, 3, 15, 6, 3));
    }

    #[test]
    fn test_cron_expr_every_15_minutes() {
        let expr = CronExpr::parse("*/15 * * * *").expect("should parse");
        assert!(expr.matches(0, 10, 1, 1, 0));
        assert!(expr.matches(15, 10, 1, 1, 0));
        assert!(expr.matches(30, 10, 1, 1, 0));
        assert!(expr.matches(45, 10, 1, 1, 0));
        assert!(!expr.matches(10, 10, 1, 1, 0));
    }

    #[test]
    fn test_cron_expr_weekdays_only() {
        let expr = CronExpr::parse("0 9 * * 1-5").expect("should parse");
        assert!(expr.matches(0, 9, 1, 1, 1)); // Monday
        assert!(expr.matches(0, 9, 1, 1, 5)); // Friday
        assert!(!expr.matches(0, 9, 1, 1, 0)); // Sunday
        assert!(!expr.matches(0, 9, 1, 1, 6)); // Saturday
    }

    #[test]
    fn test_cron_expr_to_string_repr_roundtrip() {
        let original = "30 2 15 6 1,3,5";
        let expr = CronExpr::parse(original).expect("should parse");
        let repr = expr.to_string_repr();
        let reparsed = CronExpr::parse(&repr).expect("should reparse");
        assert_eq!(expr, reparsed);
    }

    #[test]
    fn test_cron_expr_validation_minute_out_of_range() {
        assert!(matches!(
            CronExpr::parse("60 * * * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_hour_out_of_range() {
        assert!(matches!(
            CronExpr::parse("0 24 * * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_day_zero() {
        // Day of month must be 1-31.
        assert!(matches!(
            CronExpr::parse("0 0 0 * *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    #[test]
    fn test_cron_expr_validation_month_zero() {
        // Month must be 1-12.
        assert!(matches!(
            CronExpr::parse("0 0 * 0 *"),
            Err(CronParseError::OutOfRange { .. })
        ));
    }

    // -- DayOfWeek tests ----------------------------------------------------

    #[test]
    fn test_day_of_week_from_u8() {
        assert_eq!(DayOfWeek::from_u8(0), Some(DayOfWeek::Sunday));
        assert_eq!(DayOfWeek::from_u8(6), Some(DayOfWeek::Saturday));
        assert_eq!(DayOfWeek::from_u8(7), None);
    }

    #[test]
    fn test_day_of_week_display_names() {
        assert_eq!(DayOfWeek::Monday.display_name(), "Monday");
        assert_eq!(DayOfWeek::Friday.short_name(), "Fri");
    }

    // -- ScheduleFrequency tests --------------------------------------------

    #[test]
    fn test_frequency_display_names() {
        assert_eq!(ScheduleFrequency::Once.display_name(), "Once");
        assert_eq!(ScheduleFrequency::Daily.display_name(), "Daily");
        assert_eq!(ScheduleFrequency::Hourly.display_name(), "Hourly");
        assert_eq!(
            ScheduleFrequency::Weekly(DayOfWeek::Monday).display_name(),
            "Weekly (Monday)"
        );
        assert_eq!(
            ScheduleFrequency::Monthly(15).display_name(),
            "Monthly (day 15)"
        );
        assert_eq!(
            ScheduleFrequency::EveryNMinutes(30).display_name(),
            "Every 30 min"
        );
    }

    // -- TaskResult tests ---------------------------------------------------

    #[test]
    fn test_task_result_ok() {
        let r = TaskResult::Ok;
        assert!(r.is_ok());
        assert_eq!(r.display_str(), "OK");
    }

    #[test]
    fn test_task_result_error() {
        let r = TaskResult::Error(String::from("timeout"));
        assert!(!r.is_ok());
        assert_eq!(r.display_str(), "timeout");
    }

    // -- ScheduledTask tests ------------------------------------------------

    #[test]
    fn test_scheduled_task_new() {
        let task = ScheduledTask::new(
            1,
            "backup",
            "/usr/bin/backup",
            ScheduleFrequency::Daily,
            1000,
        );
        assert_eq!(task.id, 1);
        assert_eq!(task.name, "backup");
        assert!(task.enabled);
        assert!(!task.has_run());
        assert_eq!(task.result_display(), "Never run");
    }

    #[test]
    fn test_scheduled_task_last_succeeded() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.last_succeeded());
        task.last_result = Some(TaskResult::Ok);
        assert!(task.last_succeeded());
    }

    #[test]
    fn test_scheduled_task_last_failed() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.last_failed());
        task.last_result = Some(TaskResult::Error(String::from("err")));
        assert!(task.last_failed());
    }

    #[test]
    fn test_scheduled_task_can_retry() {
        let mut task = ScheduledTask::new(1, "t", "cmd", ScheduleFrequency::Once, 0);
        assert!(!task.can_retry());
        task.retry_on_failure = true;
        task.max_retries = 3;
        assert!(task.can_retry());
        task.current_retries = 3;
        assert!(!task.can_retry());
    }

    // -- TaskHistory tests --------------------------------------------------

    #[test]
    fn test_history_initially_empty() {
        let h = TaskHistory::new();
        assert_eq!(h.count(), 0);
        assert_eq!(h.success_count(), 0);
        assert_eq!(h.failure_count(), 0);
    }

    #[test]
    fn test_history_record_success() {
        let mut h = TaskHistory::new();
        h.record_success(1, "task1", 1000, 50);
        assert_eq!(h.count(), 1);
        assert_eq!(h.success_count(), 1);
        assert_eq!(h.failure_count(), 0);
    }

    #[test]
    fn test_history_record_failure() {
        let mut h = TaskHistory::new();
        h.record_failure(1, "task1", 2000, 100, "timeout");
        assert_eq!(h.count(), 1);
        assert_eq!(h.success_count(), 0);
        assert_eq!(h.failure_count(), 1);
        let entry = &h.entries()[0];
        assert_eq!(entry.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_history_entries_for_task() {
        let mut h = TaskHistory::new();
        h.record_success(1, "task1", 1000, 50);
        h.record_success(2, "task2", 2000, 60);
        h.record_success(1, "task1", 3000, 70);

        let t1 = h.entries_for_task(1);
        assert_eq!(t1.len(), 2);
        let t2 = h.entries_for_task(2);
        assert_eq!(t2.len(), 1);
    }

    #[test]
    fn test_history_recent_ordering() {
        let mut h = TaskHistory::new();
        h.record_success(1, "a", 100, 10);
        h.record_success(2, "b", 200, 20);
        h.record_success(3, "c", 300, 30);

        let recent = h.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].timestamp, 300);
        assert_eq!(recent[1].timestamp, 200);
    }

    #[test]
    fn test_history_max_entries_trim() {
        let mut h = TaskHistory::new().with_max_entries(3);
        for i in 0..5 {
            h.record_success(i, &format!("t{i}"), i * 100, 10);
        }
        assert_eq!(h.count(), 3);
        // Oldest entries should have been trimmed.
        assert_eq!(h.entries()[0].task_id, 2);
    }

    #[test]
    fn test_history_clear() {
        let mut h = TaskHistory::new();
        h.record_success(1, "t", 100, 10);
        h.clear();
        assert_eq!(h.count(), 0);
    }

    // -- TaskScheduler CRUD tests -------------------------------------------

    #[test]
    fn test_scheduler_add_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("backup", "/bin/backup", ScheduleFrequency::Daily, 1000);
        assert_eq!(id, 1);
        assert_eq!(s.task_count(), 1);
        assert!(s.get_task(id).is_some());
    }

    #[test]
    fn test_scheduler_add_multiple_tasks() {
        let mut s = TaskScheduler::new();
        let id1 = s.add_task("a", "cmd_a", ScheduleFrequency::Daily, 100);
        let id2 = s.add_task("b", "cmd_b", ScheduleFrequency::Hourly, 100);
        assert_ne!(id1, id2);
        assert_eq!(s.task_count(), 2);
    }

    #[test]
    fn test_scheduler_remove_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Once, 0);
        assert!(s.remove_task(id));
        assert_eq!(s.task_count(), 0);
        assert!(!s.remove_task(id)); // Already removed.
    }

    #[test]
    fn test_scheduler_enable_disable() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 0);
        assert!(s.get_task(id).is_some_and(|t| t.enabled));

        s.disable_task(id);
        assert!(s.get_task(id).is_some_and(|t| !t.enabled));

        s.enable_task(id);
        assert!(s.get_task(id).is_some_and(|t| t.enabled));
    }

    #[test]
    fn test_scheduler_list_tasks_sorted_by_next_run() {
        let mut s = TaskScheduler::new();
        // EveryNMinutes(60) from now=1000 -> next_run = 1000 + 3600 = 4600
        s.add_task("later", "cmd", ScheduleFrequency::EveryNMinutes(60), 1000);
        // EveryNMinutes(5) from now=1000 -> next_run = 1000 + 300 = 1300
        s.add_task("sooner", "cmd", ScheduleFrequency::EveryNMinutes(5), 1000);

        let tasks = s.list_tasks();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "sooner");
        assert_eq!(tasks[1].name, "later");
    }

    #[test]
    fn test_scheduler_check_due() {
        let mut s = TaskScheduler::new();
        // This task is due at next_run = 100 + 300 = 400
        s.add_task("t1", "cmd1", ScheduleFrequency::EveryNMinutes(5), 100);
        // This task is due at next_run = 100 + 86400 = 86500
        s.add_task("t2", "cmd2", ScheduleFrequency::Daily, 100);

        let due = s.check_due(500);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "t1");

        let due_all = s.check_due(100_000);
        assert_eq!(due_all.len(), 2);
    }

    #[test]
    fn test_scheduler_check_due_excludes_disabled() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::EveryNMinutes(1), 100);
        s.disable_task(id);

        let due = s.check_due(10_000);
        assert!(due.is_empty());
    }

    #[test]
    fn test_scheduler_mark_completed() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.mark_completed(id, 2000, 50);

        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.last_run_timestamp, 2000);
        assert!(task.last_succeeded());
        assert!(task.next_run_timestamp > 2000);
        assert_eq!(s.history.count(), 1);
    }

    #[test]
    fn test_scheduler_mark_completed_once_disables() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Once, 1000);
        s.mark_completed(id, 2000, 50);

        let task = s.get_task(id).expect("task should exist");
        assert!(!task.enabled);
        assert_eq!(task.next_run_timestamp, u64::MAX);
    }

    #[test]
    fn test_scheduler_mark_failed_with_retry() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.set_retry_policy(id, true, 3);

        s.mark_failed(id, "connection refused", 2000, 100);

        let task = s.get_task(id).expect("task should exist");
        assert!(task.last_failed());
        assert_eq!(task.current_retries, 1);
        // Should be scheduled for retry at now + 60.
        assert_eq!(task.next_run_timestamp, 2060);
    }

    #[test]
    fn test_scheduler_mark_failed_exhausted_retries() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("t", "cmd", ScheduleFrequency::Daily, 1000);
        s.set_retry_policy(id, true, 1);

        s.mark_failed(id, "err", 2000, 10);
        assert_eq!(s.get_task(id).map(|t| t.current_retries), Some(1));

        // Second failure exhausts retries.
        s.mark_failed(id, "err2", 2060, 10);
        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.current_retries, 0);
        // Should schedule for next normal run, not retry.
        assert!(task.next_run_timestamp > 2060 + 60);
    }

    #[test]
    fn test_scheduler_update_task() {
        let mut s = TaskScheduler::new();
        let id = s.add_task("old_name", "old_cmd", ScheduleFrequency::Daily, 1000);
        let updated = s.update_task(id, "new_name", "new_cmd", ScheduleFrequency::Hourly, 2000);
        assert!(updated);

        let task = s.get_task(id).expect("task should exist");
        assert_eq!(task.name, "new_name");
        assert_eq!(task.command, "new_cmd");
        assert_eq!(task.frequency, ScheduleFrequency::Hourly);
    }

    #[test]
    fn test_scheduler_update_nonexistent_returns_false() {
        let mut s = TaskScheduler::new();
        assert!(!s.update_task(999, "n", "c", ScheduleFrequency::Once, 0));
    }

    // -- calculate_next_run tests -------------------------------------------

    #[test]
    fn test_calculate_next_run_once() {
        let next = calculate_next_run(&ScheduleFrequency::Once, 1000);
        assert_eq!(next, 1000);
    }

    #[test]
    fn test_calculate_next_run_daily() {
        let next = calculate_next_run(&ScheduleFrequency::Daily, 1000);
        assert_eq!(next, 1000 + 86400);
    }

    #[test]
    fn test_calculate_next_run_hourly() {
        let next = calculate_next_run(&ScheduleFrequency::Hourly, 1000);
        assert_eq!(next, 1000 + 3600);
    }

    #[test]
    fn test_calculate_next_run_every_n_minutes() {
        let next = calculate_next_run(&ScheduleFrequency::EveryNMinutes(15), 1000);
        assert_eq!(next, 1000 + 15 * 60);
    }

    #[test]
    fn test_calculate_next_run_weekly() {
        let next = calculate_next_run(&ScheduleFrequency::Weekly(DayOfWeek::Monday), 1000);
        assert_eq!(next, 1000 + 7 * 86400);
    }

    #[test]
    fn test_calculate_next_run_monthly() {
        let next = calculate_next_run(&ScheduleFrequency::Monthly(15), 1000);
        assert_eq!(next, 1000 + 30 * 86400);
    }

    #[test]
    fn test_calculate_next_run_cron_every_minute() {
        let expr = CronExpr::parse("* * * * *").expect("should parse");
        let now = 1_700_000_000u64; // Some reasonable timestamp.
        let next = calculate_next_run(&ScheduleFrequency::Cron(expr), now);
        // Should be next minute boundary.
        assert!(next > now);
        assert!(next <= now + 120);
    }

    // -- Config serialization tests -----------------------------------------

    #[test]
    fn test_config_serialize_deserialize_roundtrip() {
        let mut s = TaskScheduler::new();
        s.add_task("backup", "/bin/backup", ScheduleFrequency::Daily, 1000);
        s.add_task(
            "cleanup",
            "/bin/clean",
            ScheduleFrequency::Weekly(DayOfWeek::Monday),
            2000,
        );
        s.add_task(
            "report",
            "/bin/report",
            ScheduleFrequency::EveryNMinutes(30),
            3000,
        );

        let text = TaskSchedulerConfig::serialize(&s);
        let restored = TaskSchedulerConfig::deserialize(&text).expect("should deserialize");

        assert_eq!(restored.task_count(), 3);
        assert!(restored.get_task(1).is_some_and(|t| t.name == "backup"));
        assert!(restored.get_task(2).is_some_and(|t| t.name == "cleanup"));
    }

    #[test]
    fn test_config_serialize_cron_task() {
        let mut s = TaskScheduler::new();
        s.add_task(
            "cron_task",
            "/bin/job",
            ScheduleFrequency::Cron(CronExpr::parse("*/15 * * * *").expect("parse")),
            5000,
        );

        let text = TaskSchedulerConfig::serialize(&s);
        let restored = TaskSchedulerConfig::deserialize(&text).expect("should deserialize");
        let task = restored.get_task(1).expect("should exist");
        assert!(matches!(task.frequency, ScheduleFrequency::Cron(_)));
    }

    #[test]
    fn test_config_deserialize_empty() {
        let s = TaskSchedulerConfig::deserialize("").expect("should handle empty");
        assert_eq!(s.task_count(), 0);
    }

    #[test]
    fn test_config_deserialize_comments_and_blanks() {
        let text = "# comment\n\n# another comment\nVERSION|1\n";
        let s = TaskSchedulerConfig::deserialize(text).expect("should handle");
        assert_eq!(s.task_count(), 0);
    }

    // -- Frequency serialization tests --------------------------------------

    #[test]
    fn test_serialize_frequency_once() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Once), "once");
    }

    #[test]
    fn test_serialize_frequency_daily() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Daily), "daily");
    }

    #[test]
    fn test_serialize_frequency_hourly() {
        assert_eq!(serialize_frequency(&ScheduleFrequency::Hourly), "hourly");
    }

    #[test]
    fn test_deserialize_frequency_roundtrip() {
        let freqs = vec![
            ScheduleFrequency::Once,
            ScheduleFrequency::Daily,
            ScheduleFrequency::Hourly,
            ScheduleFrequency::Weekly(DayOfWeek::Wednesday),
            ScheduleFrequency::Monthly(15),
            ScheduleFrequency::EveryNMinutes(45),
        ];
        for freq in freqs {
            let s = serialize_frequency(&freq);
            let restored = deserialize_frequency(&s).expect("should roundtrip");
            assert_eq!(restored, freq);
        }
    }

    // -- decompose_timestamp tests ------------------------------------------

    #[test]
    fn test_decompose_epoch_zero() {
        let dt = decompose_timestamp(0);
        assert_eq!(dt.minute, 0);
        assert_eq!(dt.hour, 0);
        assert_eq!(dt.day, 1);
        assert_eq!(dt.month, 1);
        // 1970-01-01 is Thursday = weekday 4.
        assert_eq!(dt.weekday, 4);
    }

    /// The two dates the deleted `days_to_ymd` tests pinned, asserted through
    /// the only caller that ever wanted them.
    ///
    /// The epoch and the century boundary are exactly where a hand-rolled
    /// calendar goes wrong — 2000 is a leap year by the 400-rule that the
    /// 100-rule would otherwise deny — so a cron expression reading
    /// `0 0 29 2 *` depends on this being right.
    #[test]
    fn the_century_boundary_decomposes_the_way_a_calendar_says() {
        // 2000-01-01 is day 10957 since the epoch, and it was a Saturday.
        let dt = decompose_timestamp(10957 * 86_400);
        assert_eq!((dt.month, dt.day), (1, 1));
        assert_eq!(dt.weekday, 6);
        // 2000-02-29 exists; 1900-02-29 would not have.
        let leap = decompose_timestamp((10957 + 59) * 86_400);
        assert_eq!((leap.month, leap.day), (2, 29));
    }

    /// Every field a cron expression compares is in the range cron expects.
    ///
    /// The decomposition narrows five `u32`s to `u8`, and each narrowing has a
    /// fallback that is meant to be unreachable. This walks a year of days at
    /// an awkward offset from midnight and checks the fallbacks never fire —
    /// a `day` of 1 arriving from a failed conversion rather than from the
    /// calendar would silently fire every monthly task on the wrong date.
    #[test]
    fn no_field_ever_falls_back_out_of_cron_range() {
        // 2024 is a leap year, so this covers a 29 February.
        let start: u64 = 1_704_067_200; // 2024-01-01 00:00:00 UTC
        for day in 0..366u64 {
            let dt = decompose_timestamp(start + day * 86_400 + 13 * 3600 + 47 * 60);
            assert_eq!(dt.hour, 13, "day {day}");
            assert_eq!(dt.minute, 47, "day {day}");
            assert!(
                (1..=12).contains(&dt.month),
                "day {day}: month {}",
                dt.month
            );
            assert!((1..=31).contains(&dt.day), "day {day}: day {}", dt.day);
            assert!(dt.weekday <= 6, "day {day}: weekday {}", dt.weekday);
        }
    }

    // -- SchedulerUI tests --------------------------------------------------

    #[test]
    fn test_ui_initial_state() {
        let ui = SchedulerUI::new();
        assert_eq!(ui.tab, UiTab::Tasks);
        assert_eq!(ui.dialog, UiDialog::None);
        assert!(ui.selected_task_id.is_none());
    }

    #[test]
    fn test_ui_tab_switching() {
        let mut ui = SchedulerUI::new();
        ui.switch_to_history();
        assert_eq!(ui.tab, UiTab::History);
        ui.switch_to_tasks();
        assert_eq!(ui.tab, UiTab::Tasks);
    }

    #[test]
    fn test_ui_add_task_flow() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        assert_eq!(ui.dialog, UiDialog::AddTask);

        ui.form.name = String::from("test_task");
        ui.form.command = String::from("/bin/test");
        ui.form.frequency_index = 1; // Daily

        let id = ui.commit_add_task(1000);
        assert!(id.is_some());
        assert_eq!(ui.scheduler.task_count(), 1);
        assert_eq!(ui.dialog, UiDialog::None);
    }

    #[test]
    fn test_ui_add_task_requires_name() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        ui.form.command = String::from("/bin/test");
        // name is empty
        let id = ui.commit_add_task(1000);
        assert!(id.is_none());
    }

    #[test]
    fn test_ui_edit_task_flow() {
        let mut ui = SchedulerUI::new();
        let id = ui
            .scheduler
            .add_task("original", "cmd", ScheduleFrequency::Daily, 1000);

        ui.open_edit_dialog(id);
        assert!(matches!(ui.dialog, UiDialog::EditTask(_)));

        ui.form.name = String::from("updated");
        let ok = ui.commit_edit_task(id, 2000);
        assert!(ok);
        assert_eq!(
            ui.scheduler.get_task(id).map(|t| t.name.as_str()),
            Some("updated")
        );
    }

    #[test]
    fn test_ui_delete_task_flow() {
        let mut ui = SchedulerUI::new();
        let id = ui
            .scheduler
            .add_task("to_delete", "cmd", ScheduleFrequency::Once, 0);
        ui.select_task(id);

        ui.open_delete_dialog(id);
        assert!(matches!(ui.dialog, UiDialog::ConfirmDelete(_)));

        let removed = ui.confirm_delete_task(id);
        assert!(removed);
        assert_eq!(ui.scheduler.task_count(), 0);
        assert!(ui.selected_task_id.is_none());
    }

    #[test]
    fn test_ui_toggle_selected_task() {
        let mut ui = SchedulerUI::new();
        let id = ui
            .scheduler
            .add_task("t", "cmd", ScheduleFrequency::Daily, 0);
        ui.select_task(id);
        assert!(ui.scheduler.get_task(id).is_some_and(|t| t.enabled));

        ui.toggle_selected_task();
        assert!(ui.scheduler.get_task(id).is_some_and(|t| !t.enabled));

        ui.toggle_selected_task();
        assert!(ui.scheduler.get_task(id).is_some_and(|t| t.enabled));
    }

    // -- TaskFormState tests ------------------------------------------------

    #[test]
    fn test_form_state_from_task() {
        let task = ScheduledTask::new(
            1,
            "weekly_backup",
            "/bin/backup",
            ScheduleFrequency::Weekly(DayOfWeek::Friday),
            1000,
        );
        let form = TaskFormState::from_task(&task);
        assert_eq!(form.name, "weekly_backup");
        assert_eq!(form.frequency_index, 2);
        assert_eq!(form.weekly_day, 5); // Friday
    }

    #[test]
    fn test_form_state_build_frequency() {
        let mut form = TaskFormState::new();
        form.frequency_index = 1;
        assert_eq!(form.build_frequency(), Some(ScheduleFrequency::Daily));

        form.frequency_index = 4;
        assert_eq!(form.build_frequency(), Some(ScheduleFrequency::Hourly));

        form.frequency_index = 5;
        form.interval_minutes = 10;
        assert_eq!(
            form.build_frequency(),
            Some(ScheduleFrequency::EveryNMinutes(10))
        );
    }

    #[test]
    fn test_form_state_build_cron_frequency() {
        let mut form = TaskFormState::new();
        form.frequency_index = 6;
        form.cron_expr = String::from("*/5 * * * *");
        let freq = form.build_frequency();
        assert!(matches!(freq, Some(ScheduleFrequency::Cron(_))));
    }

    #[test]
    fn test_form_state_invalid_cron_returns_none() {
        let mut form = TaskFormState::new();
        form.frequency_index = 6;
        form.cron_expr = String::from("invalid");
        assert!(form.build_frequency().is_none());
    }

    // -- Render tests -------------------------------------------------------

    /// These six used to assert `!cmds.is_empty()`, which the window
    /// background alone satisfies -- they passed with every list, dialog and
    /// label removed. Each now names something it expects to see.
    #[test]
    fn the_tasks_tab_draws_its_column_headings() {
        let ui = SchedulerUI::new();
        let text = drawn_text(&ui);
        for heading in ["On", "Name", "Command", "Frequency", "Next Run"] {
            assert!(
                text.iter().any(|t| t == heading),
                "the Tasks tab drew no {heading:?} column"
            );
        }
        assert!(text.iter().any(|t| t == "No tasks scheduled"));
    }

    #[test]
    fn the_history_tab_draws_its_own_columns_and_not_the_task_list_s() {
        let mut ui = SchedulerUI::new();
        ui.switch_to_history();
        let text = drawn_text(&ui);
        for heading in ["Time", "Task", "Status", "Duration", "Error"] {
            assert!(
                text.iter().any(|t| t == heading),
                "the History tab drew no {heading:?} column"
            );
        }
        assert!(
            !text.iter().any(|t| t == "Next Run"),
            "the History tab drew the task list's columns"
        );
    }

    #[test]
    fn a_task_row_shows_its_name_command_and_schedule() {
        let mut ui = SchedulerUI::new();
        ui.scheduler
            .add_task("nightly", "backup.sh", ScheduleFrequency::Daily, 1000);
        let text = drawn_text(&ui);
        for expected in ["nightly", "backup.sh", "Daily"] {
            assert!(
                text.iter().any(|t| t == expected),
                "a task row did not show {expected:?}"
            );
        }
    }

    #[test]
    fn the_add_dialog_draws_a_field_for_every_part_of_a_task() {
        let mut ui = SchedulerUI::new();
        ui.open_add_dialog();
        let text = drawn_text(&ui);
        for label in ["Add Task", "Name:", "Command:", "Frequency:", "Enabled:"] {
            assert!(
                text.iter().any(|t| t == label),
                "the add dialog drew no {label:?}"
            );
        }
    }

    #[test]
    fn the_delete_dialog_names_the_task_it_would_delete() {
        let mut ui = SchedulerUI::new();
        let id = ui
            .scheduler
            .add_task("payroll", "run.sh", ScheduleFrequency::Once, 0);
        ui.open_delete_dialog(id);
        let text = drawn_text(&ui);
        assert!(
            text.iter().any(|t| t.contains("payroll")),
            "a delete confirmation that does not say what it is deleting is \
             not a confirmation"
        );
    }

    #[test]
    fn a_status_message_is_drawn_and_the_content_stops_above_it() {
        let mut ui = ui_with_tasks(100);
        let without = drawn_rows(&ui, 'T').len();
        ui.status_message = Some(String::from("Task added"));
        assert!(drawn_text(&ui).iter().any(|t| t == "Task added"));
        assert!(
            drawn_rows(&ui, 'T').len() < without,
            "the status bar is drawn over the list, so a row has to give way \
             to it"
        );
    }

    // -- Utility function tests ---------------------------------------------

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "Never");
    }

    #[test]
    fn test_format_timestamp_max() {
        assert_eq!(format_timestamp(u64::MAX), "--");
    }

    #[test]
    /// Asserted by value, not by prefix.
    ///
    /// `starts_with("2023-")` is satisfied by every one of the 365 days of
    /// that year, so it could not have noticed a calendar that was a
    /// fortnight out — which is precisely what `apps/undelete`'s copy of this
    /// same arithmetic was.
    fn test_format_timestamp_known() {
        // 2023-11-14 22:13:20 UTC.
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13");
        // The date and the time of day used to be decomposed by two separate
        // routes; this asserts they name one instant.
        assert_eq!(format_timestamp(1_787_070_645), "2026-08-18 16:30");
    }

    #[test]
    fn test_format_duration_ms_millis() {
        assert_eq!(format_duration_ms(500), "500ms");
    }

    #[test]
    fn test_format_duration_ms_seconds() {
        assert_eq!(format_duration_ms(2500), "2.5s");
    }

    #[test]
    fn test_format_duration_ms_minutes() {
        assert_eq!(format_duration_ms(125000), "2m 5s");
    }

    #[test]
    fn test_format_duration_ms_never_rounds_past_a_minute() {
        // Regression: the old body divided into an f64 and printed one decimal
        // place, so a task that ran 59.96 s reported "60.0s" — a duration its
        // own branch had excluded.
        assert_eq!(format_duration_ms(59_960), "59.9s");
    }

    #[test]
    fn test_format_duration_ms_has_an_hours_field() {
        // Regression: the old ladder stopped at minutes, so a 90-minute task
        // reported "90m 0s".
        assert_eq!(format_duration_ms(5_400_000), "1h 30m 0s");
    }

    // --- list scrolling -----------------------------------------------------

    /// A UI with `n` tasks, all sharing a next-run time so `list_tasks` keeps
    /// them in insertion order and T000 is genuinely first.
    fn ui_with_tasks(n: usize) -> SchedulerUI {
        let mut ui = SchedulerUI::new();
        for i in 0..n {
            ui.scheduler.add_task(
                &format!("T{i:03}"),
                "/bin/true",
                ScheduleFrequency::Hourly,
                0,
            );
        }
        ui
    }

    /// A UI on the History tab with `n` recorded runs. `recent` is newest
    /// first, so H{n-1} is the top row and H000 the last.
    fn ui_with_history(n: usize) -> SchedulerUI {
        let mut ui = SchedulerUI::new();
        for i in 0..n {
            ui.scheduler
                .history
                .record_success(1, &format!("H{i:03}"), i as u64, 5);
        }
        ui.tab = UiTab::History;
        ui
    }

    /// What the window draws at its default size.
    fn commands(ui: &SchedulerUI) -> Vec<RenderCommand> {
        ui.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree().commands
    }

    /// Every string the window drew, in draw order.
    fn drawn_text(ui: &SchedulerUI) -> Vec<String> {
        commands(ui)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Every `T000`/`H000`-shaped label the render drew, in draw order.
    fn drawn_rows(ui: &SchedulerUI, prefix: char) -> Vec<String> {
        commands(ui)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. }
                    if text.len() == 4
                        && text.starts_with(prefix)
                        && text
                            .get(1..)
                            .is_some_and(|d| d.chars().all(|c| c.is_ascii_digit())) =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect()
    }

    /// The bug: both lists drew every row at a computed y with no bound. The
    /// surrounding clip hid the overflow, and with no offset ever read, the
    /// hidden rows could not be scrolled to.
    #[test]
    fn the_task_list_stops_at_the_last_row_that_fits() {
        let ui = ui_with_tasks(100);
        let drawn = drawn_rows(&ui, 'T');
        assert!(!drawn.is_empty(), "the task list drew no rows at all");
        assert!(
            drawn.len() < 100,
            "the task list drew all 100 rows into a {WINDOW_HEIGHT}px window"
        );
        assert_eq!(drawn.first().map(String::as_str), Some("T000"));
    }

    #[test]
    fn the_history_list_stops_at_the_last_row_that_fits() {
        let ui = ui_with_history(60);
        let drawn = drawn_rows(&ui, 'H');
        assert!(!drawn.is_empty(), "the history list drew no rows at all");
        assert!(drawn.len() < 60, "the history list drew all 60 rows");
        assert_eq!(
            drawn.first().map(String::as_str),
            Some("H059"),
            "history is newest first"
        );
    }

    /// No row is drawn past the bottom of the content area -- and the content
    /// area stops above the status bar, which is drawn over it.
    #[test]
    fn no_list_row_is_drawn_past_the_bottom_of_the_content_area() {
        for status in [false, true] {
            for offset in [0, 9, 1_000] {
                let mut ui = ui_with_tasks(100);
                if status {
                    ui.status_message = Some(String::from("saved"));
                }
                ui.scroll_task_list_by(offset);
                let bottom = if status {
                    WINDOW_HEIGHT - STATUS_BAR_HEIGHT
                } else {
                    WINDOW_HEIGHT
                };
                for cmd in commands(&ui) {
                    if let RenderCommand::Text { y, text, .. } = cmd
                        && text.len() == 4
                        && text.starts_with('T')
                    {
                        assert!(
                            y + ROW_HEIGHT <= bottom,
                            "task row {text:?} at y={y} overruns the content bottom \
                             {bottom} (status={status}, offset={offset})"
                        );
                    }
                }
            }
        }
    }

    /// The rows past the fold are reachable, which is the fix.
    #[test]
    fn scrolling_reaches_the_task_rows_that_did_not_fit() {
        let mut ui = ui_with_tasks(100);
        assert!(!drawn_rows(&ui, 'T').contains(&String::from("T099")));
        ui.scroll_task_list_by(100);
        assert!(
            drawn_rows(&ui, 'T').contains(&String::from("T099")),
            "the last task is unreachable after scrolling to the end"
        );
    }

    #[test]
    fn scrolling_reaches_the_history_rows_that_did_not_fit() {
        let mut ui = ui_with_history(60);
        assert!(!drawn_rows(&ui, 'H').contains(&String::from("H000")));
        ui.scroll_history_by(60);
        assert!(
            drawn_rows(&ui, 'H').contains(&String::from("H000")),
            "the oldest history entry is unreachable after scrolling to the end"
        );
    }

    /// An offset past the end means the last page, not a blank list --
    /// deleting most of a long task list is exactly how that happens.
    #[test]
    fn a_task_list_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let mut ui = ui_with_tasks(100);
        ui.scroll_task_list_by(99);
        let doomed: Vec<u64> = ui
            .scheduler
            .list_tasks()
            .iter()
            .skip(4)
            .map(|t| t.id)
            .collect();
        for id in doomed {
            ui.scheduler.remove_task(id);
        }
        let drawn = drawn_rows(&ui, 'T');
        assert_eq!(drawn.len(), 4, "the task list must not go blank");
        assert_eq!(drawn.last().map(String::as_str), Some("T003"));
    }

    /// Scrolling up from the top stays at the top rather than wrapping.
    #[test]
    fn scrolling_a_list_up_from_the_top_stays_at_the_top() {
        let mut ui = ui_with_tasks(100);
        ui.scroll_task_list_by(-10);
        assert_eq!(ui.task_list_scroll, 0);
        ui.scroll_task_list_by(5);
        ui.scroll_task_list_to_top();
        assert_eq!(ui.task_list_scroll, 0);

        ui.scroll_history_by(-10);
        assert_eq!(ui.history_scroll, 0);
        ui.scroll_history_by(5);
        ui.scroll_history_to_top();
        assert_eq!(ui.history_scroll, 0);
    }

    /// A list hiding rows says how many.
    #[test]
    fn a_list_that_is_hiding_rows_says_so() {
        let ui = ui_with_tasks(100);
        let shown = drawn_rows(&ui, 'T').len();
        let labels = drawn_text(&ui);
        assert!(
            labels.contains(&format!("{} more", 100 - shown)),
            "expected a \"{} more\" line",
            100 - shown
        );

        // ...and a list with room for everything says nothing.
        let ui = ui_with_tasks(3);
        let labels = drawn_text(&ui);
        assert!(
            !labels.iter().any(|t| t.ends_with(" more")),
            "a complete list should not claim to be hiding rows"
        );
    }

    // --- clicking -----------------------------------------------------------

    #[test]
    fn clicking_a_tab_switches_to_it() {
        let mut ui = ui_with_tasks(3);
        assert_eq!(ui.tab, UiTab::Tasks);
        assert_eq!(
            probe::click(&mut ui, Target::Tab(UiTab::History)),
            EventResult::Consumed
        );
        assert_eq!(ui.tab, UiTab::History);
        // Clicking the tab already showing changes nothing, and says so rather
        // than reporting a redraw the window does not need.
        assert_eq!(
            probe::click(&mut ui, Target::Tab(UiTab::History)),
            EventResult::Ignored
        );
    }

    /// Two guarantees at once: a row scrolled off the top records no hit box
    /// (the content clip drops it), and the row that *is* under the cursor
    /// names the task the user can see rather than the one at index zero.
    #[test]
    fn a_scrolled_task_list_clicks_the_row_the_user_can_see() {
        let mut ui = ui_with_tasks(100);
        ui.scroll_task_list_by(10);
        let ids: Vec<u64> = ui.scheduler.list_tasks().iter().map(|t| t.id).collect();
        assert!(
            probe::rect_of(&ui, Target::TaskRow(ids[0])).is_none(),
            "a row scrolled off the top is still clickable"
        );
        assert_eq!(
            probe::click(&mut ui, Target::TaskRow(ids[10])),
            EventResult::Consumed
        );
        assert_eq!(ui.selected_task_id, Some(ids[10]));
    }

    /// The checkbox is recorded after the row it sits in, so it wins the click
    /// there -- and it selects the row as well as toggling it, because a
    /// toolbar still offering to edit some other task would be the surprising
    /// half of that.
    #[test]
    fn clicking_a_rows_checkbox_toggles_it_and_selects_the_row() {
        let mut ui = ui_with_tasks(3);
        let id = ui.scheduler.list_tasks()[0].id;
        let enabled = |ui: &SchedulerUI| ui.scheduler.get_task(id).is_some_and(|t| t.enabled);
        assert!(enabled(&ui));
        assert_eq!(
            probe::click(&mut ui, Target::TaskCheckbox(id)),
            EventResult::Consumed
        );
        assert!(!enabled(&ui), "the checkbox did not toggle its task");
        assert_eq!(ui.selected_task_id, Some(id));
        probe::click(&mut ui, Target::TaskCheckbox(id));
        assert!(enabled(&ui), "the checkbox does not toggle back");
    }

    /// A button that needs a selection records no hit box at all when there is
    /// none, so "greyed out" and "not clickable" are one decision in one place
    /// rather than two that can drift apart.
    #[test]
    fn the_toolbar_offers_no_target_for_a_button_that_needs_a_selection() {
        let mut ui = ui_with_tasks(3);
        let gated = [Target::Edit, Target::Remove, Target::ToggleEnabled];
        for target in gated {
            assert!(
                probe::rect_of(&ui, target).is_none(),
                "{target:?} is clickable with nothing selected"
            );
        }
        // Add never needs one.
        assert!(probe::rect_of(&ui, Target::Add).is_some());

        let id = ui.scheduler.list_tasks()[0].id;
        probe::click(&mut ui, Target::TaskRow(id));
        for target in gated {
            assert!(
                probe::rect_of(&ui, target).is_some(),
                "{target:?} stayed dead after a row was selected"
            );
        }
    }

    /// A modal that can be clicked past is not modal. Everything under a
    /// dialog stops being clickable -- including the button that occupied
    /// those exact coordinates a moment earlier.
    #[test]
    fn a_dialog_swallows_a_click_aimed_at_the_button_behind_it() {
        let mut ui = ui_with_tasks(3);
        let id = ui.scheduler.list_tasks()[0].id;
        probe::click(&mut ui, Target::TaskRow(id));
        let remove = probe::rect_of(&ui, Target::Remove).expect("Remove is drawn for a selection");

        probe::click(&mut ui, Target::Add);
        assert_eq!(ui.dialog, UiDialog::AddTask);
        assert!(
            probe::rect_of(&ui, Target::Remove).is_none(),
            "the toolbar is still live underneath a modal"
        );

        let (x, y) = remove.centre();
        assert_eq!(
            ui.click_at(x, y, MouseButton::Left, <SchedulerUI as Probe>::SIZE),
            EventResult::Ignored
        );
        assert_eq!(
            ui.dialog,
            UiDialog::AddTask,
            "the click reached through the scrim"
        );
    }

    // --- the add/edit form --------------------------------------------------

    #[test]
    fn saving_an_empty_form_leaves_the_dialog_open_and_says_what_is_missing() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        assert_eq!(
            probe::click(&mut ui, Target::DialogSave),
            EventResult::Ignored
        );
        assert_eq!(
            ui.dialog,
            UiDialog::AddTask,
            "an invalid form closed anyway"
        );
        assert!(
            ui.status_message.is_some(),
            "a refused Save has to say why -- a greyed button cannot name the \
             field that is missing"
        );
        assert!(ui.scheduler.list_tasks().is_empty());
    }

    /// The whole path a user actually takes, through the hit boxes rather than
    /// the methods behind them.
    #[test]
    fn a_task_can_be_added_entirely_by_clicking_and_typing() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        // The dialog opens with the caret in Name, so a window opened from the
        // keyboard can be filled in from the keyboard.
        assert_eq!(ui.focus, Some(FormField::Name));
        probe::type_str(&mut ui, "nightly");
        probe::click(&mut ui, Target::Field(FormField::Command));
        probe::type_str(&mut ui, "/bin/backup");
        // Once -> Daily.
        probe::click(&mut ui, Target::FrequencyCycle);
        assert_eq!(
            probe::click(&mut ui, Target::DialogSave),
            EventResult::Consumed
        );
        assert_eq!(ui.dialog, UiDialog::None);

        let tasks = ui.scheduler.list_tasks();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "nightly");
        assert_eq!(tasks[0].command, "/bin/backup");
        assert_eq!(tasks[0].frequency, ScheduleFrequency::Daily);
    }

    #[test]
    fn escape_closes_a_dialog_and_takes_the_caret_with_it() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Escape)),
            EventResult::Consumed
        );
        assert_eq!(ui.dialog, UiDialog::None);
        assert_eq!(
            ui.focus, None,
            "a caret survived into a window that has no fields"
        );
    }

    /// Tab visits exactly the fields that can take a keystroke: the picked
    /// day-of-week is a control, not a text field, and stopping on it would be
    /// a stop where typing does nothing.
    #[test]
    fn tab_visits_exactly_the_fields_that_can_take_a_keystroke() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        assert_eq!(ui.focus, Some(FormField::Name));

        // "Once" has no parameter at all: two stops in the cycle.
        for expected in [FormField::Command, FormField::Name, FormField::Command] {
            probe::key(&mut ui, &probe::press(Key::Tab));
            assert_eq!(ui.focus, Some(expected));
        }

        // "Every N Minutes" adds a typed parameter: three stops.
        ui.form.frequency_index = 5;
        for expected in [FormField::Param, FormField::Name, FormField::Command] {
            probe::key(&mut ui, &probe::press(Key::Tab));
            assert_eq!(ui.focus, Some(expected));
        }

        // "Weekly" has a parameter, but a picked one -- back to two stops.
        ui.form.frequency_index = 2;
        for expected in [FormField::Name, FormField::Command, FormField::Name] {
            probe::key(&mut ui, &probe::press(Key::Tab));
            assert_eq!(ui.focus, Some(expected));
        }
    }

    /// Changing the frequency out from under a caret parked on the parameter
    /// drops the caret, rather than leaving it on a field nobody can see.
    #[test]
    fn cycling_the_frequency_away_from_a_typed_parameter_drops_the_caret() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        ui.form.frequency_index = 5;
        probe::click(&mut ui, Target::Field(FormField::Param));
        assert_eq!(ui.focus, Some(FormField::Param));
        // 5 -> 6 (Cron) is still typed, so the caret stays.
        probe::click(&mut ui, Target::FrequencyCycle);
        assert_eq!(ui.focus, Some(FormField::Param));
        // 6 -> 0 (Once) has no parameter at all.
        probe::click(&mut ui, Target::FrequencyCycle);
        assert_eq!(ui.form.frequency_index, 0);
        assert_eq!(ui.focus, None);
    }

    #[test]
    fn a_numeric_field_refuses_a_digit_that_would_take_it_past_its_bound() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        // Monthly, whose parameter is a day of the month.
        ui.form.frequency_index = 3;
        probe::click(&mut ui, Target::Field(FormField::Param));
        // Clear the default before typing, the way a user retyping a field does.
        probe::key(&mut ui, &probe::press(Key::Backspace));
        assert_eq!(ui.form.monthly_day, 0);

        probe::type_str(&mut ui, "31");
        assert_eq!(ui.form.monthly_day, 31);
        // 311 is not a day. Dropped rather than wrapped: silently moving a
        // monthly job from the 31st to the 3rd is an edit nobody would notice
        // until it had already run.
        assert_eq!(
            probe::key(&mut ui, &probe::typing("1")),
            EventResult::Ignored
        );
        assert_eq!(ui.form.monthly_day, 31);
        // And a letter is not a digit.
        probe::key(&mut ui, &probe::typing("x"));
        assert_eq!(ui.form.monthly_day, 31);
    }

    /// A day of 0 is not a date, and it is reachable -- the field is typed one
    /// digit at a time and "0" is how somebody starts typing "5". The form
    /// refuses to build a frequency from it rather than clamping.
    #[test]
    fn a_half_typed_numeric_field_makes_the_form_invalid_rather_than_guessing() {
        let mut ui = SchedulerUI::new();
        probe::click(&mut ui, Target::Add);
        probe::type_str(&mut ui, "nightly");
        probe::click(&mut ui, Target::Field(FormField::Command));
        probe::type_str(&mut ui, "/bin/backup");
        ui.form.frequency_index = 3;
        ui.form.monthly_day = 0;
        assert!(!ui.form_is_valid());
        ui.form.monthly_day = 32;
        assert!(!ui.form_is_valid());
        ui.form.monthly_day = 15;
        assert!(ui.form_is_valid());
    }

    // --- keyboard on the main window ----------------------------------------

    #[test]
    fn the_keyboard_reaches_what_the_toolbar_does() {
        let mut ui = ui_with_tasks(3);

        // Ctrl-N opens Add. The modifier is what makes it a shortcut: a bare
        // `n` typed at the main window must not open anything.
        assert_eq!(
            probe::key(&mut ui, &probe::typing("n")),
            EventResult::Ignored
        );
        assert_eq!(ui.dialog, UiDialog::None);
        assert_eq!(
            probe::key(&mut ui, &probe::ctrl(Key::N)),
            EventResult::Consumed
        );
        assert_eq!(ui.dialog, UiDialog::AddTask);
        probe::key(&mut ui, &probe::press(Key::Escape));

        // Either arrow gets a keyboard user onto the list.
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Down)),
            EventResult::Consumed
        );
        let id = ui.selected_task_id.expect("Down selected nothing");
        assert_eq!(id, ui.scheduler.list_tasks()[0].id);

        let was = ui.scheduler.get_task(id).is_some_and(|t| t.enabled);
        probe::key(&mut ui, &probe::press(Key::Space));
        assert_ne!(
            ui.scheduler.get_task(id).is_some_and(|t| t.enabled),
            was,
            "Space did not toggle the selected task"
        );

        // Delete opens the confirmation rather than deleting: Delete is one
        // key away from every arrow, and a scheduled backup is not something
        // to lose to a mistyped Down.
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Delete)),
            EventResult::Consumed
        );
        assert_eq!(ui.dialog, UiDialog::ConfirmDelete(id));
        assert_eq!(ui.scheduler.list_tasks().len(), 3);
        probe::click(&mut ui, Target::DeleteConfirm);
        assert_eq!(ui.scheduler.list_tasks().len(), 2);
        assert_eq!(ui.selected_task_id, None);
    }

    /// A key release is not a second press. Acting on both edges would run
    /// every shortcut twice and type every character twice.
    #[test]
    fn a_key_release_does_nothing() {
        let mut ui = ui_with_tasks(3);
        let mut released = probe::ctrl(Key::N);
        released.pressed = false;
        assert_eq!(probe::key(&mut ui, &released), EventResult::Ignored);
        assert_eq!(ui.dialog, UiDialog::None);
    }

    // --- the wheel ----------------------------------------------------------

    /// A trackpad sends tenths of a notch. Rounding each one to zero rows
    /// would scroll nothing at all, forever, so the fractions are banked.
    #[test]
    fn the_wheel_banks_fractions_of_a_notch_until_they_make_a_row() {
        let mut ui = ui_with_tasks(100);
        for _ in 0..3 {
            assert!(!ui.handle_scroll(-0.1), "a tenth of a notch moved the list");
        }
        assert_eq!(ui.task_list_scroll, 0);
        assert!(
            ui.handle_scroll(-0.1),
            "four tenths of a notch banked to nothing"
        );
        assert_eq!(ui.task_list_scroll, 1);
    }

    #[test]
    fn a_dialog_stops_the_list_behind_it_scrolling() {
        let mut ui = ui_with_tasks(100);
        probe::click(&mut ui, Target::Add);
        assert!(!ui.handle_scroll(-10.0));
        assert_eq!(ui.task_list_scroll, 0);
    }

    // --- window size --------------------------------------------------------

    /// The layout is derived from the live window size on every frame and
    /// never remembered, so a taller window both draws more rows and hit-tests
    /// them. (`probe::rect_of` always draws at `Probe::SIZE`, so this asks the
    /// frame directly.)
    #[test]
    fn a_taller_window_hit_tests_the_extra_rows_it_draws() {
        let ui = ui_with_tasks(100);
        let rows = |h: f32| {
            let frame = ui.frame(WINDOW_WIDTH, h);
            frame
                .hits()
                .iter()
                .filter(|(t, _)| matches!(t, Target::TaskRow(_)))
                .count()
        };
        let short = rows(WINDOW_HEIGHT);
        let tall = rows(WINDOW_HEIGHT * 2.0);
        assert!(short > 0, "no row was clickable at the default size");
        assert!(
            tall > short,
            "a window twice as tall hit-tested {tall} rows, not more than {short}"
        );
    }

    // --- the executor seam --------------------------------------------------

    /// With no runner installed nothing runs -- and, crucially, nothing is
    /// rescheduled either. A scheduler that could not execute a command but
    /// advanced `next_run_timestamp` anyway would have quietly reported the
    /// task as having run.
    #[test]
    fn a_scheduler_with_no_runner_leaves_a_due_task_overdue() {
        let mut sched = TaskScheduler::new();
        let id = sched.add_task("nightly", "/bin/backup", ScheduleFrequency::Hourly, 0);
        assert!(!sched.can_run());
        let before = sched.get_task(id).map(|t| t.next_run_timestamp);
        assert!(sched.run_due_tasks(10_000).is_empty());
        assert_eq!(sched.get_task(id).map(|t| t.next_run_timestamp), before);
        assert_eq!(sched.history.count(), 0);
    }

    #[test]
    fn an_installed_runner_runs_what_is_due_and_records_the_outcome() {
        // The `Result` is not optional even though this one never fails: the
        // point of the test is that it is installable as a `RunFn`.
        #[allow(clippy::unnecessary_wraps)]
        fn succeeds(_command: &str) -> Result<u64, TaskRunError> {
            Ok(42)
        }
        fn fails(_command: &str) -> Result<u64, TaskRunError> {
            Err(TaskRunError {
                message: String::from("no such file"),
                duration_ms: 7,
            })
        }

        let mut sched = TaskScheduler::new();
        let id = sched.add_task("nightly", "/bin/backup", ScheduleFrequency::Hourly, 0);
        sched.set_runner(succeeds);
        assert!(sched.can_run());
        assert_eq!(sched.run_due_tasks(10_000), vec![id]);
        assert!(
            sched.get_task(id).map(|t| t.next_run_timestamp) > Some(10_000),
            "a task that ran was not rescheduled"
        );
        assert_eq!(sched.history.count(), 1);
        assert_eq!(sched.history.success_count(), 1);

        // Nothing is due again until the next hour, so a second sweep at the
        // same instant runs nothing.
        assert!(sched.run_due_tasks(10_000).is_empty());

        let mut sched = TaskScheduler::new();
        let id = sched.add_task("nightly", "/bin/backup", ScheduleFrequency::Hourly, 0);
        sched.set_runner(fails);
        assert_eq!(sched.run_due_tasks(10_000), vec![id]);
        assert_eq!(sched.history.count(), 1);
        assert_eq!(sched.history.success_count(), 0);
    }

    // --- containment --------------------------------------------------------

    /// The window widths every containment claim is checked at.
    ///
    /// A rule about geometry is a rule at *every* size, so a handful of sampled
    /// sizes tests a handful of points and nothing else. The sizes that break a
    /// rule are the ones nobody would think to sample: 5 is narrower than
    /// `PADDING`, so a run at a constant inset starts past the right edge; 40
    /// fits neither of the four toolbar buttons; 400 is where the last three
    /// list columns -- which run to 815 points -- fall off the edge.
    const GRID_W: [f32; 8] = [0.0, 5.0, 40.0, 120.0, 400.0, 820.0, 1200.0, 1600.0];

    /// The window heights every containment claim is checked at.
    ///
    /// The 6 is not a rounding of the 20 next to it. Between zero and one line
    /// of text there is a band of heights at which a strip exists but cannot
    /// show anything, and it is a band with its own bugs -- the toolbar's
    /// buttons are 30-point fills centred in the strip, so at six points tall
    /// they painted over the tab bar below and answered clicks there. Zero does
    /// not find that (`take_top` hands out an empty band and the fills come out
    /// empty too) and twenty does not either, because by then the button is
    /// merely cramped. The 130 is the same sample one level down: it leaves the
    /// content area six points tall, which is a list strip too short for its own
    /// column headings.
    const GRID_H: [f32; 8] = [0.0, 6.0, 20.0, 60.0, 130.0, 400.0, 600.0, 1000.0];

    /// Every window size the containment sweeps run at.
    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h)))
    }

    /// Is `inner` within `outer`, allowing for a pixel of rounding?
    ///
    /// A rectangle with no area is "inside" anything: it is what a pass draws
    /// when its band has been squeezed out of existence, and something that was
    /// left out cannot hang off an edge.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    /// The states the containment sweeps are run in.
    ///
    /// Every branch that draws something has to be represented, because a
    /// branch nobody enters is a branch nobody measures: the empty-state line,
    /// the "N more" note, the caret, the chevron, the tick in the checkbox and
    /// each dialog are all drawn by code the default state never reaches.
    fn states() -> Vec<(&'static str, SchedulerUI)> {
        let mut selected = ui_with_tasks(40);
        let first = selected.scheduler.list_tasks().first().map(|t| t.id);
        selected.selected_task_id = first;
        selected.status_message = Some(String::from(
            "A status message long enough to need eliding in a narrow window.",
        ));

        let mut long_names = ui_with_tasks(3);
        for task in long_names.scheduler.tasks.values_mut() {
            task.name = "a task name far wider than the column it goes in".to_string();
            task.command =
                "/usr/local/bin/a-command-with-a-very-long-path --and --flags".to_string();
            task.enabled = false;
        }

        let mut failed_history = SchedulerUI::new();
        failed_history.scheduler.history.record_failure(
            1,
            "nightly backup",
            1_000,
            120,
            "no such file or directory",
        );
        failed_history.tab = UiTab::History;

        let mut empty_history = SchedulerUI::new();
        empty_history.tab = UiTab::History;

        let mut add = SchedulerUI::new();
        add.open_add_dialog();
        add.focus = Some(FormField::Name);
        add.form.name = "a name far wider than the field it is typed into".to_string();

        let mut cron = SchedulerUI::new();
        cron.open_add_dialog();
        cron.form.frequency_index = 6;

        let mut weekly = SchedulerUI::new();
        weekly.open_add_dialog();
        weekly.form.frequency_index = 2;
        weekly.form.enabled = true;

        let mut edit = ui_with_tasks(3);
        if let Some(id) = edit.scheduler.list_tasks().first().map(|t| t.id) {
            edit.open_edit_dialog(id);
        }

        let mut delete = ui_with_tasks(3);
        if let Some(id) = delete.scheduler.list_tasks().first().map(|t| t.id) {
            delete.open_delete_dialog(id);
        }

        vec![
            ("an empty task list", SchedulerUI::new()),
            ("a selected task with a status message", selected),
            ("tasks with overlong text", long_names),
            ("a full history", ui_with_history(40)),
            ("a failed run in the history", failed_history),
            ("an empty history", empty_history),
            ("the add dialog with a focused field", add),
            ("the add dialog on a cron schedule", cron),
            ("the add dialog on a weekly schedule", weekly),
            ("the edit dialog", edit),
            ("the delete dialog", delete),
        ]
    }

    /// Every rectangle a pass filled or stroked.
    fn rects(f: &Frame) -> Vec<Rect> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                }
                | RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        // A rectangle drawn past the edge is a rectangle the compositor pays to
        // clip. More to the point it means the geometry believed in room the
        // window does not have -- which is how both dialogs came to paint a
        // 440x380 panel into a 120x60 window.
        for (name, ui) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for r in rects(&ui.frame(w, h)) {
                    assert!(
                        inside(window, r),
                        "{name}: a rect at {r:?} escapes a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn no_pass_paints_outside_the_region_it_owns() {
        // The window test above is the coarsest bound there is, and until this
        // test it was the only one. Every band is inside the window by
        // construction, so a pass that overruns its own band paints on top of a
        // *sibling* and the window test sees nothing at all: the header's title
        // was placed by `band.y + (band.h - size) / 2.0` with nothing bounding
        // it, and in a window too short for the header that put the title above
        // the header and over the toolbar.
        //
        // Text and hit boxes are checked as well as fills, because whole passes
        // here paint no fill of their own -- a list row's five cells are five
        // runs and nothing else. Note the asymmetry: `frame()` clips the content
        // area, and a clip hides an overrun from text and from hit boxes but not
        // from fills, which is why the top-level test above can only ask about
        // fills. Here each pass is handed an *unclipped* frame, so all three
        // witnesses are available.
        fn check(state: &str, pass: &str, region: Rect, f: &Frame) {
            for r in rects(f) {
                assert!(
                    inside(region, r),
                    "{state}: the {pass} pass, given {region:?}, filled {r:?}"
                );
            }
            for c in f.commands() {
                let RenderCommand::Text {
                    text,
                    x,
                    y,
                    max_width,
                    font_size,
                    font_weight,
                    ..
                } = c
                else {
                    continue;
                };
                // A run has no height in the command stream, so the height has
                // to be supplied before the question can be asked at all --
                // which is exactly why the centring bug went unseen.
                let bound =
                    max_width.unwrap_or_else(|| text::measure(text, *font_size, *font_weight));
                let run = Rect::new(*x, *y, bound, *font_size);
                assert!(
                    inside(region, run),
                    "{state}: the {pass} pass, given {region:?}, inked {text:?} at {run:?}"
                );
            }
            for (target, rect) in f.hits() {
                assert!(
                    inside(region, *rect),
                    "{state}: the {pass} pass, given {region:?}, hit-boxed {target:?} at {rect:?}"
                );
            }
        }

        /// `r`, and `r` squeezed to boxes the layout does not currently hand out.
        ///
        /// A sub-pass's contract is "stay inside the box you are given" for any
        /// box, not for the boxes today's `Layout::new` happens to produce. A
        /// text field is only ever handed a 280x24 rectangle from the dialog
        /// today; the guard being tested lives in the field, and the field takes
        /// its box as an argument, which means the test can simply hand it one.
        fn squeezes(r: Rect) -> Vec<Rect> {
            let mut out = vec![r];
            for h in [0.0, 1.0, 3.0, 6.0, 12.0] {
                if h < r.h {
                    out.push(Rect::new(r.x, r.y, r.w, h));
                }
            }
            for w in [0.0, 1.0, 5.0, 20.0] {
                if w < r.w {
                    out.push(Rect::new(r.x, r.y, w, r.h));
                }
            }
            out
        }

        for (name, ui) in states() {
            let task_id = ui.scheduler.list_tasks().first().map(|t| t.id).unwrap_or(0);
            for (w, h) in sizes() {
                let l = Layout::new(w, h, ui.status_message.is_some());
                let state = format!("{name} at {w}x{h}");

                // The passes whose box is a field of the layout. There is no
                // squeezing these: the box is whatever `Layout::new` decided and
                // the pass reads it back off the layout for itself.
                type Panel = Box<dyn Fn(&SchedulerUI, &mut Frame)>;
                let panels: Vec<(&'static str, Rect, Panel)> = vec![
                    (
                        "header",
                        l.header,
                        Box::new(move |u, f| u.render_header(f, &l)),
                    ),
                    (
                        "toolbar",
                        l.toolbar,
                        Box::new(move |u, f| u.render_toolbar(f, &l)),
                    ),
                    (
                        "tab bar",
                        l.tab_bar,
                        Box::new(move |u, f| u.render_tab_bar(f, &l)),
                    ),
                    (
                        "task list",
                        l.content,
                        Box::new(move |u, f| u.render_task_list(f, &l)),
                    ),
                    (
                        "history",
                        l.content,
                        Box::new(move |u, f| u.render_history(f, &l)),
                    ),
                    (
                        "status bar",
                        l.status,
                        Box::new(move |u, f| u.render_status_bar(f, &l, "a status message")),
                    ),
                    (
                        "add/edit dialog",
                        l.window,
                        Box::new(move |u, f| u.render_add_edit_dialog(f, &l, "Add Task")),
                    ),
                    (
                        "delete dialog",
                        l.window,
                        Box::new(move |u, f| u.render_confirm_delete_dialog(f, &l, task_id)),
                    ),
                ];
                for (pass, region, draw) in panels {
                    let mut f = Frame::new(w, h);
                    draw(&ui, &mut f);
                    check(&state, pass, region, &f);
                }
            }

            // The three controls that take their box as an argument. Their
            // sizes come from constants, so the layout never squeezes them --
            // but the dialog that hands out those boxes now cuts them to itself,
            // and a control handed a sliver has to cope with one.
            type Control = fn(&SchedulerUI, &mut Frame, Rect);
            let controls: [(&'static str, Control); 4] = [
                ("text field", |u, f, r| {
                    u.render_text_field(
                        f,
                        Target::Field(FormField::Name),
                        r,
                        "a value far wider than the field",
                    );
                }),
                ("empty text field", |u, f, r| {
                    u.render_text_field(f, Target::Field(FormField::Name), r, "");
                }),
                ("picker", |u, f, r| {
                    u.render_picker(f, Target::FrequencyCycle, r, "Every N minutes");
                }),
                ("button", |u, f, r| {
                    u.render_button(f, Some(Target::DialogSave), r, "Save", COLOR_GREEN);
                }),
            ];
            for (pass, draw) in controls {
                for region in squeezes(Rect::new(40.0, 60.0, FIELD_WIDTH, FIELD_HEIGHT)) {
                    let mut f = Frame::new(800.0, 600.0);
                    draw(&ui, &mut f, region);
                    check(name, pass, region, &f);
                }
            }
        }
    }
}
