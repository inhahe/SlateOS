//! Slate OS Typing Tutor
//!
//! A typing practice application with multiple lesson types, WPM tracking,
//! accuracy statistics, and progressive difficulty levels.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

/// The size the window opens at, and the size the tests measure against.
const WINDOW_WIDTH: f32 = 620.0;
const WINDOW_HEIGHT: f32 = 560.0;

/// Everything in this program a click can land on.
///
/// Naming the controls is what lets a test say "click Start" rather than
/// "click at (247, 133)" — and it is also the whole of the app's mouse
/// support, which before this did not exist: `handle_event` matched `Key` and
/// `Tick` and nothing else, so every button drawn on all four views was a
/// picture of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A row of the lesson list, by its index into `lessons` — not into the
    /// filtered view, because a filter that changes under a stored index is
    /// exactly the bug this app already had.
    Lesson(usize),
    /// The category filter chip: cycles to the next category.
    Filter,
    /// Opens the statistics view.
    Stats,
    /// Leaves the current view for the lesson list.
    Back,
    /// Starts the selected lesson again from the beginning.
    Retry,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_CRUST: u32 = 0x11111B;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
const COL_SURFACE2: u32 = 0x585B70;
const COL_TEXT: u32 = 0xCDD6F4;
const COL_SUBTEXT0: u32 = 0xA6ADC8;
const COL_SUBTEXT1: u32 = 0xBAC2DE;
const COL_BLUE: u32 = 0x89B4FA;
const COL_GREEN: u32 = 0xA6E3A1;
const COL_RED: u32 = 0xF38BA8;
const COL_YELLOW: u32 = 0xF9E2AF;
const COL_PEACH: u32 = 0xFAB387;
const COL_LAVENDER: u32 = 0xB4BEFE;
const COL_OVERLAY0: u32 = 0x6C7086;
const COL_TEAL: u32 = 0x94E2D5;
const COL_MAUVE: u32 = 0xCBA6F7;

// ---------------------------------------------------------------------------
// Lesson content
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LessonCategory {
    HomeRow,
    TopRow,
    BottomRow,
    Numbers,
    Punctuation,
    CommonWords,
    Sentences,
    Paragraphs,
}

impl LessonCategory {
    fn name(self) -> &'static str {
        match self {
            Self::HomeRow => "Home Row",
            Self::TopRow => "Top Row",
            Self::BottomRow => "Bottom Row",
            Self::Numbers => "Numbers",
            Self::Punctuation => "Punctuation",
            Self::CommonWords => "Common Words",
            Self::Sentences => "Sentences",
            Self::Paragraphs => "Paragraphs",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::HomeRow => Color::from_hex(COL_GREEN),
            Self::TopRow => Color::from_hex(COL_BLUE),
            Self::BottomRow => Color::from_hex(COL_PEACH),
            Self::Numbers => Color::from_hex(COL_YELLOW),
            Self::Punctuation => Color::from_hex(COL_MAUVE),
            Self::CommonWords => Color::from_hex(COL_TEAL),
            Self::Sentences => Color::from_hex(COL_LAVENDER),
            Self::Paragraphs => Color::from_hex(COL_RED),
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::HomeRow,
            Self::TopRow,
            Self::BottomRow,
            Self::Numbers,
            Self::Punctuation,
            Self::CommonWords,
            Self::Sentences,
            Self::Paragraphs,
        ]
    }
}

#[derive(Debug, Clone)]
struct Lesson {
    category: LessonCategory,
    title: String,
    text: String,
}

fn default_lessons() -> Vec<Lesson> {
    vec![
        Lesson {
            category: LessonCategory::HomeRow,
            title: String::from("Home Row Basics"),
            text: String::from("asdf jkl; asdf jkl; asdf jkl; fall lads flask salad"),
        },
        Lesson {
            category: LessonCategory::HomeRow,
            title: String::from("Home Row Extended"),
            text: String::from("add glad flag salad flask half jag lad gaff all sad dad fad"),
        },
        Lesson {
            category: LessonCategory::TopRow,
            title: String::from("Top Row Basics"),
            text: String::from("qwert yuiop qwert yuiop type write quiet route power"),
        },
        Lesson {
            category: LessonCategory::TopRow,
            title: String::from("Top Row Words"),
            text: String::from("quip wire rope type your tower query equity wrote trip top pet"),
        },
        Lesson {
            category: LessonCategory::BottomRow,
            title: String::from("Bottom Row Basics"),
            text: String::from("zxcvb nm zxcvb nm mix van cab box zinc move beg cave van"),
        },
        Lesson {
            category: LessonCategory::BottomRow,
            title: String::from("Bottom Row Words"),
            text: String::from("zinc boxing climb venom bank numb vex cab comb zone maze"),
        },
        Lesson {
            category: LessonCategory::Numbers,
            title: String::from("Number Practice"),
            text: String::from("123 456 789 101 202 303 2024 1984 42 100 3000 7890"),
        },
        Lesson {
            category: LessonCategory::Punctuation,
            title: String::from("Basic Punctuation"),
            text: String::from("Hello, world! How are you? I'm fine. Yes: no; maybe."),
        },
        Lesson {
            category: LessonCategory::CommonWords,
            title: String::from("Most Common Words"),
            text: String::from(
                "the quick brown fox jumps over the lazy dog and then runs back again to find more food",
            ),
        },
        Lesson {
            category: LessonCategory::CommonWords,
            title: String::from("Frequent Words"),
            text: String::from(
                "about their would other which water people could these first after where those because right",
            ),
        },
        Lesson {
            category: LessonCategory::Sentences,
            title: String::from("Simple Sentences"),
            text: String::from(
                "The cat sat on the mat. A dog ran through the park. She wrote a letter to her friend.",
            ),
        },
        Lesson {
            category: LessonCategory::Sentences,
            title: String::from("Complex Sentences"),
            text: String::from(
                "Although the weather was cold, they decided to go hiking in the mountains near the river.",
            ),
        },
        Lesson {
            category: LessonCategory::Paragraphs,
            title: String::from("Short Paragraph"),
            text: String::from(
                "Programming is the art of telling a computer what to do. It requires patience, logic, and creativity. Good programmers write code that humans can understand.",
            ),
        },
        Lesson {
            category: LessonCategory::Paragraphs,
            title: String::from("Medium Paragraph"),
            text: String::from(
                "The operating system is the most important software on a computer. It manages memory, processes, and devices. Without it, the computer would be unable to function.",
            ),
        },
    ]
}

// ---------------------------------------------------------------------------
// Typing session — tracks progress through one lesson
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharStatus {
    Pending,
    Correct,
    Incorrect,
}

#[derive(Debug, Clone)]
struct TypingSession {
    text: Vec<char>,
    statuses: Vec<CharStatus>,
    cursor: usize,
    total_keystrokes: u32,
    correct_keystrokes: u32,
    incorrect_keystrokes: u32,
    start_time_ms: u64,
    end_time_ms: Option<u64>,
    finished: bool,
}

impl TypingSession {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        Self {
            text: chars,
            statuses: vec![CharStatus::Pending; len],
            cursor: 0,
            total_keystrokes: 0,
            correct_keystrokes: 0,
            incorrect_keystrokes: 0,
            start_time_ms: 0,
            end_time_ms: None,
            finished: false,
        }
    }

    fn type_char(&mut self, ch: char, time_ms: u64) {
        if self.finished {
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }

        // Start timer on first keystroke
        if self.total_keystrokes == 0 {
            self.start_time_ms = time_ms;
        }

        self.total_keystrokes = self.total_keystrokes.saturating_add(1);

        let Some(&expected) = self.text.get(self.cursor) else {
            return;
        };
        let status = if ch == expected {
            self.correct_keystrokes = self.correct_keystrokes.saturating_add(1);
            CharStatus::Correct
        } else {
            self.incorrect_keystrokes = self.incorrect_keystrokes.saturating_add(1);
            CharStatus::Incorrect
        };
        if let Some(slot) = self.statuses.get_mut(self.cursor) {
            *slot = status;
        }
        self.cursor = self.cursor.saturating_add(1);

        // Check completion
        if self.cursor >= self.text.len() {
            self.finished = true;
            self.end_time_ms = Some(time_ms);
        }
    }

    fn backspace(&mut self) {
        if self.finished || self.cursor == 0 {
            return;
        }
        self.cursor = self.cursor.saturating_sub(1);
        if let Some(slot) = self.statuses.get_mut(self.cursor) {
            *slot = CharStatus::Pending;
        }
    }

    fn accuracy(&self) -> f64 {
        if self.total_keystrokes == 0 {
            return 100.0;
        }
        (self.correct_keystrokes as f64 / self.total_keystrokes as f64) * 100.0
    }

    fn elapsed_ms(&self, current_time_ms: u64) -> u64 {
        if self.total_keystrokes == 0 {
            return 0;
        }
        let end = self.end_time_ms.unwrap_or(current_time_ms);
        end.saturating_sub(self.start_time_ms)
    }

    /// Words per minute: (correct chars / 5) / minutes
    fn wpm(&self, current_time_ms: u64) -> f64 {
        let elapsed = self.elapsed_ms(current_time_ms);
        if elapsed == 0 {
            return 0.0;
        }
        let minutes = elapsed as f64 / 60000.0;
        let words = self.correct_keystrokes as f64 / 5.0;
        words / minutes
    }

    fn progress_percent(&self) -> f64 {
        if self.text.is_empty() {
            return 100.0;
        }
        (self.cursor as f64 / self.text.len() as f64) * 100.0
    }

    fn chars_remaining(&self) -> usize {
        self.text.len().saturating_sub(self.cursor)
    }

    fn correct_count(&self) -> usize {
        self.statuses
            .iter()
            .filter(|s| **s == CharStatus::Correct)
            .count()
    }

    fn incorrect_count(&self) -> usize {
        self.statuses
            .iter()
            .filter(|s| **s == CharStatus::Incorrect)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Session history (stats)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SessionResult {
    lesson_title: String,
    category: LessonCategory,
    wpm: f64,
    accuracy: f64,
    duration_ms: u64,
    text_length: usize,
}

// ---------------------------------------------------------------------------
// App views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    LessonSelect,
    Typing,
    Results,
    Statistics,
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// What the window is worth in pixels, solved once per frame.
///
/// Every coordinate in this program used to be a literal: the three views that
/// took a `_width` threw it away, none of them was given the height at all, and
/// the lesson list stepped 50 px per row from y=120 whatever the window did.
/// A window shorter than the list drew rows below its own bottom edge, and
/// there was no scrolling to reach them — `scroll_offset` was declared, reset
/// in two places and read nowhere.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    window: Rect,
    /// The view's name, and on the lesson list the category filter.
    header: Rect,
    /// One line of key reminders under the header.
    subhead: Rect,
    /// The list, the typing panel, the cards -- whatever the view is about.
    body: Rect,
    /// The bottom line of key reminders.
    footer: Rect,
    /// The height of one lesson row, and of one row of the results table.
    row: f32,
    /// How many cards fit across `body`, and how big one is.
    cards_across: usize,
    card: (f32, f32),
    /// The gap between cards, and the general padding.
    pad: f32,
    big: f32,
    font: f32,
    small: f32,
    tiny: f32,
}

impl Layout {
    /// Solve the layout for a window of the given size.
    #[must_use]
    fn solve(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);

        // Type sizes come from the height, because that is what runs out
        // first: the views are lists and stacks of bands, not columns.
        let font = (h / 34.0).clamp(9.0, 17.0);
        let big = (font * 2.1).clamp(15.0, 36.0);
        let small = (font - 3.0).max(7.0);
        let tiny = (font - 5.0).max(6.0);
        let pad = (w.min(h) * 0.025).clamp(3.0, 15.0);

        // The three bands of chrome, in the order they are given up when the
        // window cannot pay for them. The subhead goes first: it is a
        // reminder of keys that still work when it is not on screen. The
        // header goes last, because a view with no title is a view the user
        // cannot name.
        let mut wants = [
            (h * 0.14).clamp(24.0, 74.0), // header
            (h * 0.06).clamp(14.0, 30.0), // subhead
            (h * 0.06).clamp(14.0, 30.0), // footer
        ];
        // What is left once the body has its guaranteed share. Charging the
        // padding to the chrome rather than the body is what keeps a squeezed
        // window's list showing rows rather than showing only its own title.
        let budget = (h - h * 0.45 - pad * 2.0).max(0.0);
        for &i in &[1usize, 2, 0] {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, sub_h, ftr_h] = wants;

        // A dropped band is a full-width strip nought pixels tall rather than
        // `Rect::EMPTY`: `is_empty` already answers "no" to the only question
        // the drawing code asks, and the strip form puts the body's edges
        // where they belong for free (lesson 51, recorded for sokoban).
        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let subhead = Rect::new(0.0, hdr_h, w, sub_h);
        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);
        let body = Rect::new(
            pad,
            subhead.bottom() + pad,
            (w - pad * 2.0).max(0.0),
            (footer.y - subhead.bottom() - pad * 2.0).max(0.0),
        );

        // A row holds a title over a subtitle, so it is sized from the two
        // type sizes it must fit rather than from a share of the body: a body
        // twice as tall should show twice as many lessons, not two lessons
        // twice the size.
        let row = (font + small + pad * 1.6).max(1.0);

        // Cards wrap to as many columns as fit, down to one. The old code
        // wrote three across at a fixed 180 px pitch, which is 570 px of
        // content poured into whatever width the window happened to have.
        let card_w_min = (font * 9.0).max(1.0);
        let across = ((body.w + pad) / (card_w_min + pad)).floor();
        let cards_across = if across.is_finite() && across >= 1.0 {
            (across as usize).min(3)
        } else {
            1
        };
        let card_w = ((body.w - pad * (cards_across as f32 - 1.0)) / cards_across as f32).max(0.0);
        let card_h = (font * 2.0 + small + pad * 2.0).max(0.0);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            subhead,
            body,
            footer,
            row,
            cards_across,
            card: (card_w, card_h),
            pad,
            big,
            font,
            small,
            tiny,
        }
    }

    /// How many list rows the body can show at once.
    ///
    /// Zero when the body is too short for even one — the caller must not
    /// treat that as "show one anyway", because a row drawn in a body that
    /// cannot hold it is a row drawn over the footer.
    #[must_use]
    fn rows_visible(&self) -> usize {
        if self.row <= 0.0 || self.body.h < self.row {
            return 0;
        }
        (self.body.h / self.row).floor() as usize
    }

    /// The box of the `i`th visible list row.
    #[must_use]
    fn row_rect(&self, i: usize) -> Rect {
        Rect::new(
            self.body.x,
            self.body.y + i as f32 * self.row,
            self.body.w,
            (self.row - self.pad * 0.4).max(0.0),
        )
    }

    /// The box of the `i`th card, wrapping at [`Self::cards_across`].
    #[must_use]
    fn card_rect(&self, i: usize) -> Rect {
        let (cw, ch) = self.card;
        // `max(1)` twice over: `cards_across` is never zero, and a divisor that
        // is only *believed* non-zero is a division that panics the day the
        // belief stops holding.
        let across = self.cards_across.max(1);
        let col = i.checked_rem(across).unwrap_or(0);
        let rowi = i.checked_div(across).unwrap_or(0);
        Rect::new(
            self.body.x + col as f32 * (cw + self.pad),
            self.body.y + rowi as f32 * (ch + self.pad),
            cw,
            ch,
        )
    }
}

// ---------------------------------------------------------------------------
// Main app
// ---------------------------------------------------------------------------

struct TypingTutorApp {
    lessons: Vec<Lesson>,
    /// Which lesson the cursor is on, as an index into `lessons`.
    ///
    /// Into `lessons`, not into the filtered view of it. The two used to be
    /// the same field holding whichever the last writer meant: the arrow keys
    /// bounds-checked it against `filtered_lessons().len()` while
    /// `start_lesson` stored the unfiltered index into it. Finish a lesson
    /// with a category filter on and the highlight landed on some other row —
    /// and if the unfiltered index was past the end of the filtered list,
    /// Down was refused and Up walked back from a row that was not there.
    selected_lesson: usize,
    view: AppView,
    session: Option<TypingSession>,
    current_time_ms: u64,
    results: Vec<SessionResult>,
    category_filter: Option<LessonCategory>,
    /// The first lesson row on screen. Read by the drawing pass, which is new:
    /// this field existed, was reset in two places, and was read nowhere, so
    /// the list could not scroll and every lesson past the window's bottom
    /// edge was unreachable by mouse or key.
    scroll_offset: usize,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    width: f32,
    height: f32,
}

impl TypingTutorApp {
    fn new() -> Self {
        Self {
            lessons: default_lessons(),
            selected_lesson: 0,
            view: AppView::LessonSelect,
            session: None,
            current_time_ms: 0,
            results: Vec::new(),
            category_filter: None,
            scroll_offset: 0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Remember the size the window is now, so the next click is read against
    /// the picture the user actually clicked on.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// The layout of the window as it stands.
    fn layout(&self) -> Layout {
        Layout::solve(self.width, self.height)
    }

    /// Where the cursor sits in the filtered list, if it is in it at all.
    ///
    /// `None` when the selected lesson is filtered out — which is a state the
    /// user can reach by pressing C, and which the arrow keys have to handle
    /// rather than index past.
    fn cursor_position(&self) -> Option<usize> {
        self.filtered_lessons()
            .iter()
            .position(|&i| i == self.selected_lesson)
    }

    fn filtered_lessons(&self) -> Vec<usize> {
        self.lessons
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                self.category_filter.is_none() || Some(l.category) == self.category_filter
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn start_lesson(&mut self, lesson_idx: usize) {
        if let Some(lesson) = self.lessons.get(lesson_idx) {
            self.session = Some(TypingSession::new(&lesson.text));
            self.selected_lesson = lesson_idx;
            self.view = AppView::Typing;
        }
    }

    fn finish_lesson(&mut self) {
        if let Some(ref session) = self.session
            && session.finished
            && let Some(lesson) = self.lessons.get(self.selected_lesson)
        {
            let wpm = session.wpm(self.current_time_ms);
            let acc = session.accuracy();
            let dur = session.elapsed_ms(self.current_time_ms);
            let title = lesson.title.clone();
            let cat = lesson.category;
            let tlen = session.text.len();
            self.results.push(SessionResult {
                lesson_title: title,
                category: cat,
                wpm,
                accuracy: acc,
                duration_ms: dur,
                text_length: tlen,
            });
            self.view = AppView::Results;
        }
    }

    fn cycle_category_filter(&mut self) {
        let cats = LessonCategory::all();
        match self.category_filter {
            None => self.category_filter = cats.first().copied(),
            Some(current) => {
                let idx = cats.iter().position(|c| *c == current).unwrap_or(0);
                // `None` off the end of the list is the wrap back to "all
                // categories", so a `get` that misses is the answer and not a
                // failure to look.
                self.category_filter = cats.get(idx.saturating_add(1)).copied();
            }
        }
        // Put the cursor on the first lesson the new filter admits, rather
        // than on lesson zero. `selected_lesson` indexes `lessons`, and
        // lesson zero is in the list only when the filter happens to admit
        // its category; setting it blind is what used to leave the cursor on
        // a row that was not drawn.
        self.selected_lesson = self.filtered_lessons().first().copied().unwrap_or(0);
        self.scroll_offset = 0;
    }

    fn average_wpm(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.results.iter().map(|r| r.wpm).sum();
        sum / self.results.len() as f64
    }

    fn average_accuracy(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.results.iter().map(|r| r.accuracy).sum();
        sum / self.results.len() as f64
    }

    fn best_wpm(&self) -> f64 {
        self.results.iter().map(|r| r.wpm).fold(0.0_f64, f64::max)
    }

    fn total_chars_typed(&self) -> usize {
        self.results.iter().map(|r| r.text_length).sum()
    }

    /// A key release is not a key press: handling both would type every
    /// character twice and count twice the keystrokes the typist made, which
    /// is an accuracy figure they could not explain.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        match self.view {
            AppView::LessonSelect => self.handle_lesson_select(event),
            AppView::Typing => self.handle_typing(event),
            AppView::Results => self.handle_results(event),
            AppView::Statistics => self.handle_statistics(event),
        }
    }

    fn handle_lesson_select(&mut self, event: &KeyEvent) -> EventResult {
        let filtered = self.filtered_lessons();
        // Where the cursor is *in the list being drawn*. Every arrow decision
        // below is made here and then translated back into an index into
        // `lessons`, which is the one place the two numberings meet.
        let here = self.cursor_position();
        match event.key {
            Key::Up => {
                let next = match here {
                    Some(0) | None => return EventResult::Ignored,
                    Some(i) => i.saturating_sub(1),
                };
                let Some(&idx) = filtered.get(next) else {
                    return EventResult::Ignored;
                };
                self.selected_lesson = idx;
            }
            Key::Down => {
                let next = match here {
                    // A cursor that the filter has excluded is put back on the
                    // list rather than left off it: Down from nowhere lands on
                    // the first row, which is the only answer that leaves the
                    // user able to see where they are.
                    None => 0,
                    Some(i) => i.saturating_add(1),
                };
                let Some(&idx) = filtered.get(next) else {
                    return EventResult::Ignored;
                };
                self.selected_lesson = idx;
            }
            Key::Home => {
                let Some(&idx) = filtered.first() else {
                    return EventResult::Ignored;
                };
                self.selected_lesson = idx;
            }
            Key::End => {
                let Some(&idx) = filtered.last() else {
                    return EventResult::Ignored;
                };
                self.selected_lesson = idx;
            }
            Key::Enter => {
                if here.is_none() {
                    return EventResult::Ignored;
                }
                self.start_lesson(self.selected_lesson);
            }
            Key::C => self.cycle_category_filter(),
            Key::S => self.view = AppView::Statistics,
            // Escape is not this view's to take. Swallowing it here told the
            // window manager the key had been dealt with when nothing had
            // happened at all.
            _ => return EventResult::Ignored,
        }
        self.scroll_cursor_into_view();
        EventResult::Consumed
    }

    /// Move the window of drawn rows the least distance that puts the cursor
    /// back inside it.
    ///
    /// The least distance, in both directions: a cursor that walked one row
    /// off the bottom brings the list up by one and leaves the page the user
    /// has just read on screen, where anchoring the cursor at the top would
    /// throw that page away (known-issues.md lesson 70, recorded for
    /// snippets).
    fn scroll_cursor_into_view(&mut self) {
        let capacity = self.layout().rows_visible();
        if capacity == 0 {
            return;
        }
        let Some(here) = self.cursor_position() else {
            return;
        };
        if here < self.scroll_offset {
            self.scroll_offset = here;
        } else if here >= self.scroll_offset.saturating_add(capacity) {
            self.scroll_offset = here.saturating_sub(capacity.saturating_sub(1));
        }
        // A list that has shrunk under a scrolled window leaves blank rows
        // where lessons used to be, so the offset is pulled back to the last
        // page rather than left where the old, longer list put it.
        let max_offset = self.filtered_lessons().len().saturating_sub(capacity);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    fn handle_typing(&mut self, event: &KeyEvent) -> EventResult {
        if event.key == Key::Escape {
            self.view = AppView::LessonSelect;
            self.session = None;
            return EventResult::Consumed;
        }

        if event.key == Key::Backspace {
            if let Some(ref mut session) = self.session {
                session.backspace();
            }
            return EventResult::Consumed;
        }

        // Type the character.
        //
        // `typed`, not `text`: Enter and Tab produce `\r` and `\t` on most
        // layouts, and a lesson that scored those would count a carriage
        // return the user never saw as a mistyped letter — and then report an
        // accuracy the typist has no way to explain.
        let mut typed_any = false;
        if let Some(ref mut session) = self.session {
            for ch in event.typed() {
                session.type_char(ch, self.current_time_ms);
                typed_any = true;
            }
            if session.finished {
                self.finish_lesson();
            }
        }
        // A key that produced no character -- a function key, a bare modifier
        // -- was not this view's to take. Reporting it consumed asks for a
        // repaint of a picture that has not changed, on every such key.
        if typed_any {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn handle_results(&mut self, event: &KeyEvent) -> EventResult {
        match event.key {
            Key::Enter | Key::Space | Key::Escape => {
                self.view = AppView::LessonSelect;
                self.session = None;
            }
            Key::R => {
                // Retry the same lesson. `selected_lesson` indexes `lessons`,
                // which is why this is right whether or not a filter is on.
                self.start_lesson(self.selected_lesson);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn handle_statistics(&mut self, event: &KeyEvent) -> EventResult {
        if event.key == Key::Escape || event.key == Key::Enter {
            self.view = AppView::LessonSelect;
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// Act on a click that has already been resolved to a named control.
    ///
    /// Named, not measured: the drawing pass records the boxes, so a control
    /// that moves because the window changed size cannot drift away from the
    /// clicks meant for it.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Lesson(idx) => {
                // First click selects, second starts -- the same two-step the
                // arrow keys and Enter give, so the mouse cannot start a
                // lesson the user has not seen highlighted.
                if self.selected_lesson == idx && self.view == AppView::LessonSelect {
                    self.start_lesson(idx);
                } else {
                    self.selected_lesson = idx;
                    self.scroll_cursor_into_view();
                }
            }
            Target::Filter => self.cycle_category_filter(),
            Target::Stats => self.view = AppView::Statistics,
            Target::Back => {
                self.view = AppView::LessonSelect;
                self.session = None;
            }
            Target::Retry => self.start_lesson(self.selected_lesson),
        }
        EventResult::Consumed
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            // The mouse arm that did not exist. Every button this app drew on
            // all four views was a picture of a button: `handle_event` matched
            // `Key` and `Tick`, so a click reached nothing anywhere in the
            // program.
            Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }) => {
                let frame = self.frame(self.width, self.height);
                match frame.hit_test(*x, *y) {
                    Some(target) => self.activate(target),
                    None => EventResult::Ignored,
                }
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Scroll { dy, .. },
                ..
            }) if self.view == AppView::LessonSelect => self.scroll_by(*dy),
            // Without this the clock stood at zero, so every WPM figure the
            // app showed was zero and every duration read 0:00 -- on the
            // live screen, on the results screen, and in the history it
            // saved.  `advance_time` and everything above it were correct
            // and tested; nothing called them.  known-issues.md lesson 45.
            Event::Tick { elapsed_ms } => {
                self.advance_time(*elapsed_ms);
                // Only the typing view shows a clock. Asking for a repaint on
                // every tick of the lesson list would redraw a still picture
                // sixty times a second.
                if self.view == AppView::Typing {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Scroll the lesson list by a wheel notch.
    ///
    /// Positive `dy` is away from the user, which walks the list *up* towards
    /// its first row -- the direction the wheel moves the page, not the
    /// direction it moves the index.
    fn scroll_by(&mut self, dy: f32) -> EventResult {
        let capacity = self.layout().rows_visible();
        let len = self.filtered_lessons().len();
        if capacity == 0 || len <= capacity {
            return EventResult::Ignored;
        }
        let max_offset = len.saturating_sub(capacity);
        let step = 3usize;
        let next = if dy > 0.0 {
            self.scroll_offset.saturating_sub(step)
        } else if dy < 0.0 {
            self.scroll_offset.saturating_add(step).min(max_offset)
        } else {
            return EventResult::Ignored;
        };
        if next == self.scroll_offset {
            // A wheel notch at the end of the list changed nothing, and
            // saying otherwise asks for a repaint of an identical picture.
            return EventResult::Ignored;
        }
        self.scroll_offset = next;
        EventResult::Consumed
    }

    /// Move the app's clock forward by `delta_ms`.
    ///
    /// An interval rather than a timestamp, because that is what
    /// [`Event::Tick`] carries.  The origin does not matter: `start_time_ms`
    /// and `end_time_ms` are both stamped from this same counter, and every
    /// figure derived from them is a difference.
    fn advance_time(&mut self, delta_ms: u64) {
        self.current_time_ms = self.current_time_ms.saturating_add(delta_ms);
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    /// Draw the whole window, recording a hit box for everything clickable.
    ///
    /// One frame, not a `Vec<RenderCommand>`: the boxes and the paint come out
    /// of the same pass, so a control cannot be drawn in one place and clicked
    /// in another.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::solve(width, height);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, hex(COL_BASE), CornerRadii::ZERO);
        match self.view {
            AppView::LessonSelect => self.draw_lesson_select(&mut f, &l),
            AppView::Typing => self.draw_typing(&mut f, &l),
            AppView::Results => self.draw_results(&mut f, &l),
            AppView::Statistics => self.draw_statistics(&mut f, &l),
        }
        f
    }

    fn draw_lesson_select(&self, f: &mut Frame<Target>, l: &Layout) {
        // Right to left, so that what is measured from the right edge is taken
        // out of the row before the title is asked what it can have.
        let mut bar = inset_x(l.header, l.pad);
        chip(f, l, &mut bar, "Stats", Target::Stats);
        let filter_text = match self.category_filter {
            None => String::from("All Categories"),
            Some(cat) => format!("Category: {}", cat.name()),
        };
        chip(f, l, &mut bar, &filter_text, Target::Filter);
        label_left(
            f,
            &Label {
                text: "Typing Tutor",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: hex(COL_BLUE),
            },
            bar,
        );

        label_left(
            f,
            &Label {
                text: "Up/Down: Select  |  Enter: Start  |  C: Category  |  S: Stats",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_OVERLAY0),
            },
            inset_x(l.subhead, l.pad),
        );

        let filtered = self.filtered_lessons();
        let capacity = l.rows_visible();
        f.clip(l.body);
        for (i, &lesson_idx) in filtered
            .iter()
            .skip(self.scroll_offset)
            .take(capacity)
            .enumerate()
        {
            let Some(lesson) = self.lessons.get(lesson_idx) else {
                continue;
            };
            let r = l.row_rect(i);
            let selected = lesson_idx == self.selected_lesson;
            // An unselected row is drawn a shade off the background rather than
            // in it. The program this replaces filled it with `COL_BASE` --
            // the background -- so the whole list was a single flat field and
            // the only visible row boundary was the one the cursor was on.
            fill(
                f,
                r,
                hex(if selected { COL_SURFACE0 } else { COL_MANTLE }),
                CornerRadii::all(l.pad * 0.4),
            );
            let stripe = Rect::new(
                r.x + l.pad * 0.5,
                r.y + l.pad * 0.3,
                (l.pad * 0.3).max(2.0),
                (r.h - l.pad * 0.6).max(0.0),
            );
            fill(f, stripe, lesson.category.color(), CornerRadii::all(1.0));

            let text_x = stripe.right() + l.pad * 0.6;
            let text_w = (r.right() - l.pad * 0.5 - text_x).max(0.0);
            push_text(
                f,
                &Label {
                    text: &lesson.title,
                    size: l.font,
                    weight: if selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    color: hex(if selected { COL_TEXT } else { COL_SUBTEXT1 }),
                },
                text_x,
                r.y + l.pad * 0.3,
                text_w,
            );
            // `chars().count()`, not `len()`. The subtitle says "chars" and the
            // program counted bytes, so any lesson holding a character outside
            // ASCII advertises more characters than it has. Every lesson
            // shipped here happens to be ASCII, where the two agree -- which is
            // why the test for this adds a lesson where they do not, rather
            // than trusting the list to keep disagreeing on its own.
            let sub = format!(
                "{} - {} chars",
                lesson.category.name(),
                lesson.text.chars().count()
            );
            push_text(
                f,
                &Label {
                    text: &sub,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: hex(COL_OVERLAY0),
                },
                text_x,
                r.y + l.pad * 0.3 + l.font + l.pad * 0.2,
                text_w,
            );
            f.hit(Target::Lesson(lesson_idx), r);
        }
        f.unclip();

        // The footer says what is off screen. A list that scrolls silently is a
        // list the user believes they have seen all of.
        let shown = filtered
            .len()
            .min(self.scroll_offset.saturating_add(capacity));
        let hidden = filtered.len().saturating_sub(shown);
        let mut ftr = inset_x(l.footer, l.pad);
        if hidden > 0 {
            let more = format!("{hidden} more below");
            let w = text::measure(&more, l.small, FontWeightHint::Regular).min(ftr.w);
            let r = take_right(&mut ftr, w, l.pad);
            label_left(
                f,
                &Label {
                    text: &more,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: hex(COL_YELLOW),
                },
                r,
            );
        }
        let count = format!("{} lessons", filtered.len());
        label_left(
            f,
            &Label {
                text: &count,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_OVERLAY0),
            },
            ftr,
        );
    }

    fn draw_typing(&self, f: &mut Frame<Target>, l: &Layout) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(lesson) = self.lessons.get(self.selected_lesson) else {
            return;
        };

        let mut bar = inset_x(l.header, l.pad);
        chip(f, l, &mut bar, "Esc", Target::Back);
        label_left(
            f,
            &Label {
                text: &lesson.title,
                size: l.big,
                weight: FontWeightHint::Bold,
                color: hex(COL_BLUE),
            },
            bar,
        );

        let secs = session.elapsed_ms(self.current_time_ms) / 1000;
        // `chars_remaining` is on the bar because it is the one number a typist
        // in the middle of a lesson actually wants: how much is left. It
        // existed, was tested, and was drawn nowhere.
        let stats = format!(
            "WPM: {:.0}  |  Accuracy: {:.1}%  |  Time: {}:{:02}  |  {} left",
            session.wpm(self.current_time_ms),
            session.accuracy(),
            secs / 60,
            secs % 60,
            session.chars_remaining()
        );
        label_left(
            f,
            &Label {
                text: &stats,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_SUBTEXT0),
            },
            inset_x(l.subhead, l.pad),
        );

        let mut area = l.body;
        let bar_h = (l.pad * 0.5).max(3.0);
        let track = take_top(&mut area, bar_h, l.pad * 0.6);
        fill(f, track, hex(COL_SURFACE0), CornerRadii::all(bar_h / 2.0));
        let progress = (session.progress_percent() / 100.0).clamp(0.0, 1.0) as f32;
        fill(
            f,
            Rect::new(track.x, track.y, track.w * progress, track.h),
            hex(COL_GREEN),
            CornerRadii::all(bar_h / 2.0),
        );

        fill(f, area, hex(COL_MANTLE), CornerRadii::all(l.pad * 0.5));
        draw_lesson_text(f, l, session, shrink(area, l.pad));

        // The next keystroke, named. It used to be drawn at a fixed y = 320,
        // which is inside the typing panel on a short window and in the middle
        // of nothing on a tall one.
        let hint = match session.text.get(session.cursor) {
            Some(&' ') => Some(String::from("Space")),
            Some(&ch) => Some(format!("Type: '{ch}'")),
            None => None,
        };
        if let Some(hint) = hint {
            label_left(
                f,
                &Label {
                    text: &hint,
                    size: l.font,
                    weight: FontWeightHint::Bold,
                    color: hex(COL_YELLOW),
                },
                inset_x(l.footer, l.pad),
            );
        }
    }

    fn draw_results(&self, f: &mut Frame<Target>, l: &Layout) {
        let Some(session) = self.session.as_ref() else {
            return;
        };

        let mut bar = inset_x(l.header, l.pad);
        chip(f, l, &mut bar, "Retry", Target::Retry);
        chip(f, l, &mut bar, "Lessons", Target::Back);
        label_left(
            f,
            &Label {
                text: "Lesson Complete!",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: hex(COL_GREEN),
            },
            bar,
        );

        let title = self
            .lessons
            .get(self.selected_lesson)
            .map_or("", |x| x.title.as_str());
        label_left(
            f,
            &Label {
                text: title,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: hex(COL_TEXT),
            },
            inset_x(l.subhead, l.pad),
        );

        let wpm = session.wpm(self.current_time_ms);
        let secs = session.elapsed_ms(self.current_time_ms) / 1000;
        let cards = [
            ("WPM", format!("{wpm:.0}"), COL_BLUE),
            ("Accuracy", format!("{:.1}%", session.accuracy()), COL_GREEN),
            ("Time", format!("{}:{:02}", secs / 60, secs % 60), COL_PEACH),
            (
                "Keystrokes",
                session.total_keystrokes.to_string(),
                COL_YELLOW,
            ),
            ("Correct", session.correct_keystrokes.to_string(), COL_TEAL),
            ("Errors", session.incorrect_keystrokes.to_string(), COL_RED),
        ];
        f.clip(l.body);
        let bottom = draw_cards(f, l, &cards);
        let rating = format!("Rating: {}", wpm_rating(wpm));
        push_text(
            f,
            &Label {
                text: &rating,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: hex(COL_MAUVE),
            },
            l.body.x,
            bottom + l.pad,
            l.body.w,
        );

        // Characters, not keystrokes. The cards above count every key pressed;
        // this counts how the text ended up, so a typist who made a mistake and
        // corrected it can see that the two are not the same number.
        // `correct_count` and `incorrect_count` computed exactly this, were
        // tested, and were shown nowhere.
        let settled = format!(
            "{} characters right, {} left wrong",
            session.correct_count(),
            session.incorrect_count()
        );
        push_text(
            f,
            &Label {
                text: &settled,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_SUBTEXT0),
            },
            l.body.x,
            bottom + l.pad + l.font * 1.4,
            l.body.w,
        );
        f.unclip();

        label_left(
            f,
            &Label {
                text: "Enter: Lesson List  |  R: Retry",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_OVERLAY0),
            },
            inset_x(l.footer, l.pad),
        );
    }

    fn draw_statistics(&self, f: &mut Frame<Target>, l: &Layout) {
        let mut bar = inset_x(l.header, l.pad);
        chip(f, l, &mut bar, "Lessons", Target::Back);
        label_left(
            f,
            &Label {
                text: "Statistics",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: hex(COL_LAVENDER),
            },
            bar,
        );

        if self.results.is_empty() {
            label_left(
                f,
                &Label {
                    text: "No lessons completed yet. Start typing!",
                    size: l.font,
                    weight: FontWeightHint::Regular,
                    color: hex(COL_SUBTEXT0),
                },
                inset_x(l.subhead, l.pad),
            );
        } else {
            let cards = [
                ("Lessons", self.results.len().to_string(), COL_BLUE),
                ("Avg WPM", format!("{:.0}", self.average_wpm()), COL_GREEN),
                ("Best WPM", format!("{:.0}", self.best_wpm()), COL_YELLOW),
                (
                    "Avg Accuracy",
                    format!("{:.1}%", self.average_accuracy()),
                    COL_TEAL,
                ),
                (
                    "Total Chars",
                    self.total_chars_typed().to_string(),
                    COL_PEACH,
                ),
            ];
            f.clip(l.body);
            let bottom = draw_cards(f, l, &cards);
            let table = Rect::new(
                l.body.x,
                bottom + l.pad,
                l.body.w,
                (l.body.bottom() - bottom - l.pad).max(0.0),
            );
            self.draw_recent_table(f, l, table);
            f.unclip();
        }

        label_left(
            f,
            &Label {
                text: "Esc/Enter: Back to lessons",
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_OVERLAY0),
            },
            inset_x(l.footer, l.pad),
        );
    }

    /// The last few finished lessons, newest first, in as many rows as `area`
    /// can hold.
    fn draw_recent_table(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        if area.is_empty() {
            return;
        }
        let row_h = (text::line_height(l.small, FontWeightHint::Regular) + l.pad * 0.4).max(1.0);
        let head_h = row_h * 2.0;
        if area.h < head_h + row_h {
            // No room for the title, the column heads and one row. Drawing the
            // heads alone would be a table promising rows it has nowhere to
            // put.
            return;
        }
        push_text(
            f,
            &Label {
                text: "Recent Results",
                size: l.font,
                weight: FontWeightHint::Bold,
                color: hex(COL_TEXT),
            },
            area.x,
            area.y,
            area.w,
        );

        // Columns as shares of the width they are given, so a narrower window
        // narrows the lesson-name column rather than pushing the time column
        // off the right-hand edge -- which is what four constants at x = 30,
        // 250, 340 and 440 did in any window under 480 px wide.
        const SHARES: [f32; 4] = [0.0, 0.52, 0.70, 0.86];
        let headers = ["Lesson", "WPM", "Accuracy", "Time"];
        let col_x = |i: usize| area.x + area.w * SHARES.get(i).copied().unwrap_or(0.0);
        let col_w = |i: usize| {
            let here = SHARES.get(i).copied().unwrap_or(0.0);
            let next = SHARES.get(i.saturating_add(1)).copied().unwrap_or(1.0);
            (area.w * (next - here) - l.pad * 0.5).max(0.0)
        };
        // A band behind the column heads, so the heads read as a heading rather
        // than as a first row that happens to be in bold.
        fill(
            f,
            Rect::new(area.x, area.y + row_h, area.w, row_h),
            hex(COL_CRUST),
            CornerRadii::all(l.pad * 0.3),
        );
        for (i, head) in headers.iter().enumerate() {
            push_text(
                f,
                &Label {
                    text: head,
                    size: l.tiny,
                    weight: FontWeightHint::Bold,
                    color: hex(COL_SUBTEXT0),
                },
                col_x(i),
                area.y + row_h,
                col_w(i),
            );
        }

        let capacity = ((area.h - head_h) / row_h).floor().max(0.0) as usize;
        for (n, result) in self.results.iter().rev().take(capacity.min(8)).enumerate() {
            let y = area.y + head_h + n as f32 * row_h;
            let secs = result.duration_ms / 1000;
            let cells = [
                // The lesson's name in its category's colour, which is what the
                // list it came from used. `SessionResult::category` was stored
                // on every finished lesson and read by nothing at all, so the
                // history could not say what kind of practice any row was.
                (result.lesson_title.clone(), result.category.color()),
                (format!("{:.0}", result.wpm), hex(COL_GREEN)),
                (format!("{:.1}%", result.accuracy), hex(COL_TEAL)),
                (format!("{}:{:02}", secs / 60, secs % 60), hex(COL_PEACH)),
            ];
            for (i, (body, color)) in cells.iter().enumerate() {
                push_text(
                    f,
                    &Label {
                        text: body,
                        size: l.small,
                        weight: FontWeightHint::Regular,
                        color: *color,
                    },
                    col_x(i),
                    y,
                    col_w(i),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/// The palette is a wall of `u32` literals, and every use of one needs this.
fn hex(c: u32) -> Color {
    Color::from_hex(c)
}

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed through as `max_width`, so a caller that worked out a
/// width limit gets one the renderer will actually stop at. The program this
/// replaces wrote `max_width: Some(400.0)` and `Some(200.0)` beside strings in
/// a layout that was itself made of constants.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: f32) {
    if l.text.is_empty() || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Against the left edge of `r`, centred down it.
fn label_left(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x, r.y + (r.h - lh) / 2.0, r.w);
}

/// Centred in `r` -- across from the measured width, down from the line height
/// -- and limited to `r`, so the width that decides the centre is the width the
/// renderer is told to stop at and the two cannot disagree.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, w);
}

/// Take `h` off the top of `area`, leaving `gap` between what was taken and
/// what is left. Returns [`Rect::EMPTY`] and takes nothing if there is no room.
fn take_top(area: &mut Rect, h: f32, gap: f32) -> Rect {
    if h <= 0.0 || area.h < h {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.x, area.y, area.w, h);
    area.y += h + gap;
    area.h = (area.h - h - gap).max(0.0);
    taken
}

/// Take `w` off the right-hand end of `area`. See [`take_top`].
fn take_right(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.right() - w, area.y, w, area.h);
    area.w = (area.w - w - gap).max(0.0);
    taken
}

/// `r` with `dx` taken off each of its left and right edges.
fn inset_x(r: Rect, dx: f32) -> Rect {
    Rect::new(r.x + dx, r.y, (r.w - dx * 2.0).max(0.0), r.h)
}

/// `r` with `dy` taken off each of its top and bottom edges.
fn inset_y(r: Rect, dy: f32) -> Rect {
    Rect::new(r.x, r.y + dy, r.w, (r.h - dy * 2.0).max(0.0))
}

/// `r` with `d` taken off all four edges.
fn shrink(r: Rect, d: f32) -> Rect {
    inset_y(inset_x(r, d), d)
}

/// A labelled button against the right end of `bar`, with a hit box on it.
///
/// The box is measured from its own label rather than given a width, because a
/// chip narrower than its text is a button whose name the user cannot read. If
/// the row has no room the chip is dropped entirely -- no paint and no hit box,
/// so a test asking for its rectangle is told `None` rather than being handed
/// an empty one it could mistake for a control.
fn chip(f: &mut Frame<Target>, l: &Layout, bar: &mut Rect, body: &str, t: Target) {
    let w = text::measure(body, l.small, FontWeightHint::Bold) + l.pad * 2.0;
    let r = take_right(bar, w, l.pad);
    if r.is_empty() {
        return;
    }
    let inner_h = text::line_height(l.small, FontWeightHint::Bold) + l.pad;
    let box_r = inset_y(r, ((r.h - inner_h) / 2.0).max(0.0));
    fill(f, box_r, hex(COL_SURFACE0), CornerRadii::all(l.pad * 0.4));
    label_centred(
        f,
        &Label {
            text: body,
            size: l.small,
            weight: FontWeightHint::Bold,
            color: hex(COL_TEXT),
        },
        box_r,
    );
    f.hit(t, box_r);
}

/// A grid of `(name, value, colour)` cards across the body, wrapping at
/// [`Layout::cards_across`].
///
/// Returns the y the grid ended at, so whatever comes after it starts below it
/// rather than at a constant chosen by eye -- which is what `table_y = 240.0`
/// was, and it overlapped the cards on any window where they wrapped to a
/// third row.
fn draw_cards(f: &mut Frame<Target>, l: &Layout, cards: &[(&str, String, u32)]) -> f32 {
    let mut bottom = l.body.y;
    for (i, (name, value, col)) in cards.iter().enumerate() {
        let r = l.card_rect(i);
        if r.bottom() > l.body.bottom() {
            // Out of body. Wrapping on rather than drawing over the footer.
            break;
        }
        fill(f, r, hex(COL_SURFACE0), CornerRadii::all(l.pad * 0.4));
        let inner = shrink(r, l.pad * 0.5);
        push_text(
            f,
            &Label {
                text: name,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: hex(COL_SUBTEXT0),
            },
            inner.x,
            inner.y,
            inner.w,
        );
        push_text(
            f,
            &Label {
                text: value,
                size: l.font * 1.4,
                weight: FontWeightHint::Bold,
                color: hex(*col),
            },
            inner.x,
            inner.y + l.small + l.pad * 0.3,
            inner.w,
        );
        bottom = bottom.max(r.bottom());
    }
    bottom
}

/// What to call a typing speed.
///
/// A free function so the boundaries can be tested without drawing anything;
/// they used to be an `if` chain buried in the middle of the results view.
fn wpm_rating(wpm: f64) -> &'static str {
    if wpm >= 80.0 {
        "Expert!"
    } else if wpm >= 60.0 {
        "Advanced"
    } else if wpm >= 40.0 {
        "Intermediate"
    } else if wpm >= 20.0 {
        "Beginner"
    } else {
        "Keep Practicing!"
    }
}

/// The lesson's text, coloured a character at a time, scrolled so the cursor is
/// always on screen.
///
/// Each character is a separate `Text` command because each carries its own
/// colour, which means this function, and not the renderer, owns the pen. It
/// used to advance the pen by a hardcoded 13.2 px -- "approximate monospace
/// character width" -- while drawing in the *proportional* UI face, so nothing
/// lined up: an `i` left a gap two thirds of a cell wide, an `M` ran into its
/// neighbour, and the cursor's highlight box, also 13.2 px, sat over the wrong
/// part of the glyph it marked. Every advance is measured in the family it is
/// drawn in now.
fn draw_lesson_text(f: &mut Frame<Target>, l: &Layout, session: &TypingSession, area: Rect) {
    if area.is_empty() || session.text.is_empty() {
        return;
    }
    let size = (l.font * 1.25).max(8.0);
    let weight = FontWeightHint::Regular;
    let line_h = (text::line_height(size, weight) * 1.25).max(1.0);

    // Pass one: where every character goes, in lines from the top of the text
    // rather than pixels from the top of the panel -- because the panel may not
    // be showing the top of the text.
    let mut places: Vec<(usize, f32, f32)> = Vec::with_capacity(session.text.len());
    let mut x = 0.0f32;
    let mut line = 0usize;
    for &ch in &session.text {
        let mut buf = [0u8; 4];
        let glyph: &str = ch.encode_utf8(&mut buf);
        let advance = text::measure_in(glyph, size, weight, FontFamily::Mono);
        // Break before a character that would cross the right edge, not after a
        // count of them. The `x > 0.0` guard keeps a character wider than the
        // whole panel from looping forever.
        if x > 0.0 && x + advance > area.w {
            x = 0.0;
            line = line.saturating_add(1);
        }
        places.push((line, x, advance));
        x += advance;
    }

    let lines_visible = (area.h / line_h).floor().max(1.0) as usize;
    let cursor_line = places
        .get(session.cursor.min(places.len().saturating_sub(1)))
        .map_or(0, |p| p.0);
    // The panel follows the typist rather than the text. Without this it showed
    // the first few lines of every lesson and the cursor walked off the bottom
    // of it -- which is what the old fixed 200 px panel at y = 100 did.
    let first_line = cursor_line.saturating_sub(lines_visible.saturating_sub(1));

    f.clip(area);
    f.push(RenderCommand::PushFont {
        family: FontFamily::Mono,
    });
    for (i, &(line, cx, advance)) in places.iter().enumerate() {
        if line < first_line || line >= first_line.saturating_add(lines_visible) {
            continue;
        }
        let Some(&ch) = session.text.get(i) else {
            continue;
        };
        let px = area.x + cx;
        let py = area.y + line.saturating_sub(first_line) as f32 * line_h;
        let color = if i == session.cursor {
            fill(
                f,
                Rect::new(px - 1.0, py - 1.0, advance + 2.0, line_h),
                hex(COL_SURFACE1),
                CornerRadii::all(2.0),
            );
            hex(COL_TEXT)
        } else {
            match session.statuses.get(i) {
                Some(CharStatus::Correct) => hex(COL_GREEN),
                Some(CharStatus::Incorrect) => hex(COL_RED),
                _ => hex(COL_SURFACE2),
            }
        };
        f.push(RenderCommand::Text {
            x: px,
            y: py,
            text: String::from(ch),
            color,
            font_size: size,
            font_weight: weight,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
    f.push(RenderCommand::PopFont);
    f.unclip();
}

impl App for TypingTutorApp {
    fn title(&self) -> String {
        "Typing Tutor".to_string()
    }

    fn app_id(&self) -> String {
        "typingtutor".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A typing tutor is a stopwatch with a lesson attached, so it asks for the
    /// tick that drives one. Without an interval here the loop never sends
    /// `Event::Tick`, `advance_time` is never called, and every WPM and every
    /// duration the app can show reads zero -- which is what this program did.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(100))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for TypingTutorApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<Self::Outcome> {
        self.resize(size.0, size.1);
        Some(self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })))
    }
}

fn main() -> ExitCode {
    let mut app = TypingTutorApp::new();
    app::launch("typingtutor", &mut app)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a test that panics on bad data is a test that failed, which is the point"
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    fn make_key(key: Key, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.map_or_else(String::new, |c| c.to_string()),
        }
    }

    // --- LessonCategory ---

    #[test]
    fn lesson_category_names() {
        assert_eq!(LessonCategory::HomeRow.name(), "Home Row");
        assert_eq!(LessonCategory::Paragraphs.name(), "Paragraphs");
    }

    #[test]
    fn lesson_category_colors() {
        for cat in LessonCategory::all() {
            let _ = cat.color(); // No panic
        }
    }

    #[test]
    fn lesson_category_all_count() {
        assert_eq!(LessonCategory::all().len(), 8);
    }

    // --- Default lessons ---

    #[test]
    fn default_lessons_non_empty() {
        let lessons = default_lessons();
        assert!(!lessons.is_empty());
        for l in &lessons {
            assert!(!l.title.is_empty());
            assert!(!l.text.is_empty());
        }
    }

    #[test]
    fn default_lessons_has_all_categories() {
        let lessons = default_lessons();
        for cat in LessonCategory::all() {
            assert!(
                lessons.iter().any(|l| l.category == *cat),
                "Missing category: {:?}",
                cat
            );
        }
    }

    // --- TypingSession ---

    #[test]
    fn new_session() {
        let s = TypingSession::new("hello");
        assert_eq!(s.text.len(), 5);
        assert_eq!(s.cursor, 0);
        assert_eq!(s.total_keystrokes, 0);
        assert!(!s.finished);
    }

    #[test]
    fn type_correct_char() {
        let mut s = TypingSession::new("ab");
        s.type_char('a', 1000);
        assert_eq!(s.cursor, 1);
        assert_eq!(s.statuses[0], CharStatus::Correct);
        assert_eq!(s.correct_keystrokes, 1);
    }

    #[test]
    fn type_incorrect_char() {
        let mut s = TypingSession::new("ab");
        s.type_char('x', 1000);
        assert_eq!(s.cursor, 1);
        assert_eq!(s.statuses[0], CharStatus::Incorrect);
        assert_eq!(s.incorrect_keystrokes, 1);
    }

    #[test]
    fn type_completion() {
        let mut s = TypingSession::new("hi");
        s.type_char('h', 1000);
        s.type_char('i', 2000);
        assert!(s.finished);
        assert_eq!(s.end_time_ms, Some(2000));
    }

    #[test]
    fn type_after_finished_ignored() {
        let mut s = TypingSession::new("a");
        s.type_char('a', 1000);
        assert!(s.finished);
        s.type_char('b', 2000); // Should be ignored
        assert_eq!(s.total_keystrokes, 1);
    }

    #[test]
    fn backspace() {
        let mut s = TypingSession::new("abc");
        s.type_char('a', 1000);
        s.type_char('x', 2000); // wrong
        assert_eq!(s.cursor, 2);
        s.backspace();
        assert_eq!(s.cursor, 1);
        assert_eq!(s.statuses[1], CharStatus::Pending);
    }

    #[test]
    fn backspace_at_start_ignored() {
        let mut s = TypingSession::new("abc");
        s.backspace();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn backspace_after_finished_ignored() {
        let mut s = TypingSession::new("a");
        s.type_char('a', 1000);
        assert!(s.finished);
        s.backspace();
        assert_eq!(s.cursor, 1); // Still finished, cursor didn't move back
    }

    #[test]
    fn accuracy_all_correct() {
        let mut s = TypingSession::new("abc");
        s.type_char('a', 100);
        s.type_char('b', 200);
        s.type_char('c', 300);
        assert!((s.accuracy() - 100.0).abs() < 0.01);
    }

    #[test]
    fn accuracy_half_correct() {
        let mut s = TypingSession::new("abcd");
        s.type_char('a', 100); // correct
        s.type_char('x', 200); // wrong
        s.type_char('c', 300); // correct
        s.type_char('x', 400); // wrong
        assert!((s.accuracy() - 50.0).abs() < 0.01);
    }

    #[test]
    fn accuracy_no_keystrokes() {
        let s = TypingSession::new("abc");
        assert!((s.accuracy() - 100.0).abs() < 0.01);
    }

    #[test]
    fn wpm_calculation() {
        let mut s = TypingSession::new("hello world test");
        // Type 15 correct chars in 60 seconds
        for (i, ch) in "hello world tes".chars().enumerate() {
            s.type_char(ch, (i as u64 + 1) * 4000); // spread over 60 sec
        }
        // 15 correct chars in 60s = 3 words/min (15/5)
        let wpm = s.wpm(60000);
        assert!(wpm > 0.0);
    }

    #[test]
    fn wpm_zero_elapsed() {
        let s = TypingSession::new("abc");
        assert_eq!(s.wpm(0), 0.0);
    }

    #[test]
    fn elapsed_ms_not_started() {
        let s = TypingSession::new("abc");
        assert_eq!(s.elapsed_ms(5000), 0);
    }

    #[test]
    fn elapsed_ms_in_progress() {
        let mut s = TypingSession::new("abc");
        s.type_char('a', 1000);
        assert_eq!(s.elapsed_ms(3000), 2000);
    }

    #[test]
    fn elapsed_ms_finished() {
        let mut s = TypingSession::new("ab");
        s.type_char('a', 1000);
        s.type_char('b', 3000);
        // After finish, elapsed is fixed at finish time
        assert_eq!(s.elapsed_ms(99999), 2000);
    }

    #[test]
    fn progress_percent() {
        let mut s = TypingSession::new("abcd");
        assert!((s.progress_percent() - 0.0).abs() < 0.01);
        s.type_char('a', 100);
        assert!((s.progress_percent() - 25.0).abs() < 0.01);
        s.type_char('b', 200);
        assert!((s.progress_percent() - 50.0).abs() < 0.01);
    }

    #[test]
    fn progress_empty_text() {
        let s = TypingSession::new("");
        assert!((s.progress_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn chars_remaining() {
        let mut s = TypingSession::new("hello");
        assert_eq!(s.chars_remaining(), 5);
        s.type_char('h', 100);
        assert_eq!(s.chars_remaining(), 4);
    }

    #[test]
    fn correct_and_incorrect_counts() {
        let mut s = TypingSession::new("abc");
        s.type_char('a', 100); // correct
        s.type_char('x', 200); // wrong
        s.type_char('c', 300); // correct
        assert_eq!(s.correct_count(), 2);
        assert_eq!(s.incorrect_count(), 1);
    }

    // --- App creation ---

    #[test]
    fn new_app() {
        let app = TypingTutorApp::new();
        assert_eq!(app.view, AppView::LessonSelect);
        assert!(!app.lessons.is_empty());
        assert!(app.session.is_none());
        assert!(app.results.is_empty());
    }

    // --- Lesson selection ---

    #[test]
    fn navigate_down() {
        let mut app = TypingTutorApp::new();
        assert_eq!(app.selected_lesson, 0);
        app.handle_key(&make_key(Key::Down, None));
        assert_eq!(app.selected_lesson, 1);
    }

    #[test]
    fn navigate_up() {
        let mut app = TypingTutorApp::new();
        app.selected_lesson = 2;
        app.handle_key(&make_key(Key::Up, None));
        assert_eq!(app.selected_lesson, 1);
    }

    #[test]
    fn navigate_up_at_top() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&make_key(Key::Up, None));
        assert_eq!(app.selected_lesson, 0);
    }

    #[test]
    fn start_lesson_with_enter() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&make_key(Key::Enter, None));
        assert_eq!(app.view, AppView::Typing);
        assert!(app.session.is_some());
    }

    #[test]
    fn start_specific_lesson() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(3);
        assert_eq!(app.selected_lesson, 3);
        assert!(app.session.is_some());
        assert_eq!(app.view, AppView::Typing);
    }

    #[test]
    fn start_invalid_lesson() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(999);
        assert!(app.session.is_none());
    }

    // --- Category filter ---

    #[test]
    fn cycle_category() {
        let mut app = TypingTutorApp::new();
        assert!(app.category_filter.is_none());
        app.cycle_category_filter();
        assert_eq!(app.category_filter, Some(LessonCategory::HomeRow));
        app.cycle_category_filter();
        assert_eq!(app.category_filter, Some(LessonCategory::TopRow));
    }

    #[test]
    fn cycle_category_wraps() {
        let mut app = TypingTutorApp::new();
        // Cycle through all + 1 to wrap to None
        for _ in 0..=LessonCategory::all().len() {
            app.cycle_category_filter();
        }
        assert!(app.category_filter.is_none());
    }

    #[test]
    fn filter_lessons() {
        let mut app = TypingTutorApp::new();
        let all = app.filtered_lessons();
        app.category_filter = Some(LessonCategory::HomeRow);
        let filtered = app.filtered_lessons();
        assert!(filtered.len() < all.len());
        for &idx in &filtered {
            assert_eq!(app.lessons[idx].category, LessonCategory::HomeRow);
        }
    }

    #[test]
    fn key_c_cycles_category() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&make_key(Key::C, Some('c')));
        assert!(app.category_filter.is_some());
    }

    // --- Typing view ---

    #[test]
    fn escape_returns_to_select() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        app.handle_key(&make_key(Key::Escape, None));
        assert_eq!(app.view, AppView::LessonSelect);
        assert!(app.session.is_none());
    }

    #[test]
    fn typing_correct_char() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let first_char = app.lessons[0].text.chars().next().unwrap_or('a');
        app.current_time_ms = 1000;
        app.handle_key(&KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: first_char.to_string(),
        });
        let session = app.session.as_ref().unwrap();
        assert_eq!(session.cursor, 1);
        assert_eq!(session.statuses[0], CharStatus::Correct);
    }

    #[test]
    fn typing_backspace() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let first_char = app.lessons[0].text.chars().next().unwrap_or('a');
        app.handle_key(&KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: first_char.to_string(),
        });
        app.handle_key(&make_key(Key::Backspace, None));
        let session = app.session.as_ref().unwrap();
        assert_eq!(session.cursor, 0);
    }

    #[test]
    fn completing_lesson_goes_to_results() {
        let mut app = TypingTutorApp::new();
        // Create a tiny lesson for fast completion
        app.lessons.push(Lesson {
            category: LessonCategory::HomeRow,
            title: String::from("Tiny"),
            text: String::from("ab"),
        });
        let idx = app.lessons.len() - 1;
        app.start_lesson(idx);
        app.current_time_ms = 1000;
        app.handle_key(&KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "a".to_string(),
        });
        app.current_time_ms = 2000;
        app.handle_key(&KeyEvent {
            key: Key::B,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "b".to_string(),
        });
        assert_eq!(app.view, AppView::Results);
        assert_eq!(app.results.len(), 1);
    }

    // --- Results view ---

    #[test]
    fn results_enter_returns_to_select() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Results;
        app.session = Some(TypingSession::new("test"));
        app.handle_key(&make_key(Key::Enter, None));
        assert_eq!(app.view, AppView::LessonSelect);
    }

    #[test]
    fn results_retry() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Results;
        app.session = Some(TypingSession::new("test"));
        app.selected_lesson = 0;
        app.handle_key(&make_key(Key::R, Some('r')));
        assert_eq!(app.view, AppView::Typing);
        assert!(app.session.is_some());
    }

    // --- Statistics view ---

    #[test]
    fn open_statistics() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&make_key(Key::S, Some('s')));
        assert_eq!(app.view, AppView::Statistics);
    }

    #[test]
    fn close_statistics() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Statistics;
        app.handle_key(&make_key(Key::Escape, None));
        assert_eq!(app.view, AppView::LessonSelect);
    }

    // --- Stats calculations ---

    #[test]
    fn average_wpm_empty() {
        let app = TypingTutorApp::new();
        assert_eq!(app.average_wpm(), 0.0);
    }

    #[test]
    fn average_wpm_with_results() {
        let mut app = TypingTutorApp::new();
        app.results.push(SessionResult {
            lesson_title: String::from("A"),
            category: LessonCategory::HomeRow,
            wpm: 40.0,
            accuracy: 95.0,
            duration_ms: 10000,
            text_length: 50,
        });
        app.results.push(SessionResult {
            lesson_title: String::from("B"),
            category: LessonCategory::TopRow,
            wpm: 60.0,
            accuracy: 90.0,
            duration_ms: 15000,
            text_length: 75,
        });
        assert!((app.average_wpm() - 50.0).abs() < 0.01);
    }

    #[test]
    fn best_wpm_tracking() {
        let mut app = TypingTutorApp::new();
        app.results.push(SessionResult {
            lesson_title: String::from("A"),
            category: LessonCategory::HomeRow,
            wpm: 30.0,
            accuracy: 95.0,
            duration_ms: 10000,
            text_length: 50,
        });
        app.results.push(SessionResult {
            lesson_title: String::from("B"),
            category: LessonCategory::TopRow,
            wpm: 55.0,
            accuracy: 90.0,
            duration_ms: 8000,
            text_length: 40,
        });
        assert!((app.best_wpm() - 55.0).abs() < 0.01);
    }

    #[test]
    fn average_accuracy_with_results() {
        let mut app = TypingTutorApp::new();
        app.results.push(SessionResult {
            lesson_title: String::from("A"),
            category: LessonCategory::HomeRow,
            wpm: 40.0,
            accuracy: 90.0,
            duration_ms: 10000,
            text_length: 50,
        });
        app.results.push(SessionResult {
            lesson_title: String::from("B"),
            category: LessonCategory::TopRow,
            wpm: 50.0,
            accuracy: 100.0,
            duration_ms: 8000,
            text_length: 40,
        });
        assert!((app.average_accuracy() - 95.0).abs() < 0.01);
    }

    #[test]
    fn total_chars_typed() {
        let mut app = TypingTutorApp::new();
        app.results.push(SessionResult {
            lesson_title: String::from("A"),
            category: LessonCategory::HomeRow,
            wpm: 40.0,
            accuracy: 95.0,
            duration_ms: 10000,
            text_length: 50,
        });
        app.results.push(SessionResult {
            lesson_title: String::from("B"),
            category: LessonCategory::TopRow,
            wpm: 50.0,
            accuracy: 90.0,
            duration_ms: 8000,
            text_length: 30,
        });
        assert_eq!(app.total_chars_typed(), 80);
    }

    // --- Rendering ---

    /// Every `Text` the frame draws at this size, in order.
    fn texts(app: &TypingTutorApp, w: f32, h: f32) -> Vec<String> {
        app.frame(w, h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn render_lesson_select() {
        let app = TypingTutorApp::new();
        let drawn = texts(&app, 600.0, 800.0);
        assert!(
            drawn.iter().any(|t| t == "Typing Tutor"),
            "the lesson list has no title: {drawn:?}"
        );
    }

    #[test]
    fn render_typing_view() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let title = app.lessons[0].title.clone();
        let drawn = texts(&app, 600.0, 800.0);
        assert!(
            drawn.contains(&title),
            "the typing view does not name its lesson: {drawn:?}"
        );
    }

    /// The lesson text is laid out on the advances the font actually reports,
    /// so consecutive characters sit exactly one advance apart and nothing runs
    /// past the right edge of the panel. The hardcoded 13.2 px it used to
    /// advance by satisfied neither: it was not the advance of the face being
    /// drawn in, so glyphs collided or left gaps, and the wrap it implied let a
    /// line of wide characters overrun the panel.
    #[test]
    fn typed_characters_are_placed_on_measured_advances() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let frame = app.frame(600.0, 800.0);

        // The lesson body is drawn one character per command, inside the
        // monospace scope; the surrounding chrome is not.
        let mut in_mono = false;
        let mut glyphs: Vec<(f32, f32, String, f32)> = Vec::new();
        for cmd in frame.commands() {
            match cmd {
                RenderCommand::PushFont { .. } => in_mono = true,
                RenderCommand::PopFont => in_mono = false,
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    ..
                } if in_mono => {
                    glyphs.push((*x, *y, text.clone(), *font_size));
                }
                _ => {}
            }
        }
        assert!(glyphs.len() > 5, "the lesson body is drawn: {glyphs:?}");

        // Counted for the same reason: if every glyph landed on its own line
        // the comparison below would be skipped for all of them (lesson 89).
        let mut compared = 0usize;
        for pair in glyphs.windows(2) {
            let Some(((x0, y0, glyph, size), (x1, y1, _, _))) = pair.first().zip(pair.get(1))
            else {
                continue;
            };
            // A line break resets the pen; only compare within a line.
            if (y0 - y1).abs() > f32::EPSILON {
                continue;
            }
            let advance = text::measure_in(glyph, *size, FontWeightHint::Regular, FontFamily::Mono);
            compared = compared.saturating_add(1);
            assert!(
                (x1 - x0 - advance).abs() < 0.01,
                "{glyph:?} at {x0} is followed by {x1}, which is not one \
                 measured advance ({advance}) along"
            );
        }
        assert!(
            compared > 3,
            "only {compared} pair(s) of glyphs shared a line, so the advance \
             between them was barely tested"
        );
    }

    #[test]
    fn render_results_view() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Results;
        app.session = Some(TypingSession::new("test"));
        let drawn = texts(&app, 600.0, 800.0);
        assert!(drawn.iter().any(|t| t == "Lesson Complete!"), "{drawn:?}");
    }

    #[test]
    fn render_statistics_empty() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Statistics;
        let drawn = texts(&app, 600.0, 800.0);
        assert!(drawn.iter().any(|t| t == "Statistics"), "{drawn:?}");
        assert!(
            drawn.iter().any(|t| t.contains("No lessons")),
            "an empty history says so: {drawn:?}"
        );
    }

    #[test]
    fn render_statistics_with_data() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Statistics;
        app.results.push(SessionResult {
            lesson_title: String::from("Test"),
            category: LessonCategory::HomeRow,
            wpm: 45.0,
            accuracy: 92.0,
            duration_ms: 30000,
            text_length: 100,
        });
        let drawn = texts(&app, 600.0, 800.0);
        assert!(drawn.iter().any(|t| t == "Recent Results"), "{drawn:?}");
    }

    #[test]
    fn render_has_background() {
        let app = TypingTutorApp::new();
        let frame = app.frame(600.0, 800.0);
        assert!(
            frame.commands().iter().any(|c| matches!(c,
                RenderCommand::FillRect { x, y, width, height, .. }
                    if *x == 0.0 && *y == 0.0 && *width == 600.0 && *height == 800.0)),
            "the window is not painted to its own edges"
        );
    }

    /// The progress bar's fill is as wide a share of its track as the typist
    /// has covered of the lesson.
    ///
    /// The old test looked for `height == 6.0` at `y == 80.0` -- the two
    /// constants the bar was written with, which is a test of the constants and
    /// not of the bar. It would have passed just as happily against a fill of
    /// zero width, which is exactly what an untyped lesson draws.
    #[test]
    fn the_progress_bar_fills_by_as_much_as_was_typed() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let total = app.session.as_ref().expect("a session").text.len();
        for _ in 0..total / 4 {
            let ch = app.session.as_ref().expect("a session").text
                [app.session.as_ref().expect("a session").cursor];
            app.handle_key(&make_key(Key::A, Some(ch)));
        }
        let typed = app.session.as_ref().expect("a session").cursor;
        assert!(typed > 0, "the test typed nothing");

        let l = Layout::solve(600.0, 800.0);
        let frame = app.frame(600.0, 800.0);
        let bars: Vec<f32> = frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { y, width, .. }
                    if (*y - l.body.y).abs() < 0.01 && *width > 0.0 =>
                {
                    Some(*width)
                }
                _ => None,
            })
            .collect();
        let (track, fill) = match bars.as_slice() {
            [track, fill] => (*track, *fill),
            other => panic!("want a track and a fill at the top of the body, got {other:?}"),
        };
        let want = track * (typed as f32 / total as f32);
        assert!(
            (fill - want).abs() < 0.5,
            "{typed} of {total} characters typed should fill {want} of the \
             {track}-wide track, not {fill}"
        );
    }

    // =======================================================================
    // Wiring: the window, the layout, the mouse and the scroll
    // =======================================================================

    /// The window every test that does not say otherwise is read against.
    const W: (f32, f32) = TypingTutorApp::SIZE;

    /// The sizes a claim about the layout has to hold at.
    ///
    /// A narrow one, a short one, a tall one and a very large one. Two sizes
    /// cannot separate a formula that scales from one that is merely
    /// proportional to the one number both happen to share (lesson 86).
    const SIZES: [(f32, f32); 6] = [
        (320.0, 240.0),
        (400.0, 900.0),
        (620.0, 560.0),
        (900.0, 400.0),
        (1280.0, 800.0),
        (1920.0, 1080.0),
    ];

    /// True when `body` is painted with its origin inside `r`.
    ///
    /// A hit box says a click lands somewhere; this says the user can see what
    /// they are aiming at (`known-issues.md` lesson 81).
    fn text_inside(frame: &Frame<Target>, body: &str, r: Rect) -> bool {
        frame.commands().iter().any(|c| {
            matches!(c, RenderCommand::Text { text, x, y, .. }
                if text.as_str() == body && r.contains(*x, *y))
        })
    }

    /// An app on the typing view, `typed` characters into lesson `idx`, all of
    /// them correct.
    fn typed_into(idx: usize, typed: usize) -> TypingTutorApp {
        let mut app = TypingTutorApp::new();
        app.start_lesson(idx);
        for _ in 0..typed {
            let session = app.session.as_ref().expect("a session");
            let Some(&ch) = session.text.get(session.cursor) else {
                break;
            };
            app.handle_key(&make_key(Key::A, Some(ch)));
        }
        app
    }

    // --- The window ---------------------------------------------------------

    /// The frame is painted to the edges of whatever window it is given, not to
    /// the edges of the one the program was written for.
    #[test]
    fn the_background_covers_the_window_at_every_size() {
        for size in SIZES {
            let app = TypingTutorApp::new();
            let frame = app.frame(size.0, size.1);
            assert!(
                frame.commands().iter().any(|c| matches!(c,
                    RenderCommand::FillRect { x, y, width, height, .. }
                        if *x == 0.0 && *y == 0.0
                            && (*width - size.0).abs() < 0.01
                            && (*height - size.1).abs() < 0.01)),
                "no background covering {size:?}"
            );
        }
    }

    /// Nothing the app draws is placed outside the window it was given.
    ///
    /// Unclipped, deliberately: `Frame::hit` drops a box that is empty after
    /// clipping, so testing hit boxes here would be asking a question whose
    /// answer the clip already guaranteed (lesson 80). This walks the paint.
    #[test]
    fn nothing_is_painted_outside_the_window() {
        for size in SIZES {
            for view in [
                AppView::LessonSelect,
                AppView::Typing,
                AppView::Results,
                AppView::Statistics,
            ] {
                let mut app = typed_into(0, 12);
                app.results.push(SessionResult {
                    lesson_title: String::from("Practice"),
                    category: LessonCategory::HomeRow,
                    wpm: 42.0,
                    accuracy: 97.5,
                    duration_ms: 65_000,
                    text_length: 120,
                });
                app.view = view;
                for cmd in app.frame(size.0, size.1).commands() {
                    let (x, y) = match cmd {
                        RenderCommand::FillRect { x, y, .. } | RenderCommand::Text { x, y, .. } => {
                            (*x, *y)
                        }
                        _ => continue,
                    };
                    assert!(
                        x >= -1.5 && y >= -1.5 && x <= size.0 && y <= size.1,
                        "{view:?} draws at ({x}, {y}), outside a {size:?} window"
                    );
                }
            }
        }
    }

    // --- The layout ---------------------------------------------------------

    /// A taller window shows more lessons, and the count is the body's height
    /// divided by a row -- not a constant, and not a share of the window.
    #[test]
    fn a_taller_window_lists_more_lessons() {
        let short = Layout::solve(620.0, 300.0);
        let tall = Layout::solve(620.0, 1000.0);
        assert!(
            tall.rows_visible() > short.rows_visible(),
            "300 px shows {} rows and 1000 px shows {} -- the list does not \
             grow with the window",
            short.rows_visible(),
            tall.rows_visible()
        );
        // And the rows are the same height in both, so what grew is the number
        // of lessons and not the size of each.
        assert!(
            tall.row < short.row * 2.0,
            "the rows grew with the window instead of the list growing"
        );
    }

    /// Every visible row is inside the body, and no two of them overlap.
    #[test]
    fn rows_tile_the_body_without_overlapping() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            let n = l.rows_visible();
            for i in 0..n {
                let r = l.row_rect(i);
                assert!(
                    r.y >= l.body.y - 0.01 && r.bottom() <= l.body.bottom() + 0.01,
                    "row {i} of {n} at {r:?} leaves the body {:?} at {size:?}",
                    l.body
                );
                if i > 0 {
                    let above = l.row_rect(i - 1);
                    assert!(
                        r.y >= above.bottom() - 0.01,
                        "row {i} at {r:?} overlaps row {} at {above:?}",
                        i - 1
                    );
                }
            }
        }
    }

    /// A window too short for a row says zero rather than one.
    ///
    /// One is the tempting answer and the wrong one: a row drawn in a body that
    /// cannot hold it is a row drawn over the footer.
    #[test]
    fn a_body_too_short_for_a_row_shows_none() {
        // 20 px, not 40. At 40 the chrome is given up entirely and the body
        // gets 34 px, which is more than a row -- so the `if` never held and
        // this test asserted nothing whatever the layout did. An assertion
        // behind a condition is an assertion that can retire without failing,
        // so the precondition is asserted rather than checked.
        let l = Layout::solve(620.0, 20.0);
        assert!(
            l.body.h < l.row,
            "this test needs a body too short for a row: body {:?}, row {}",
            l.body,
            l.row
        );
        assert_eq!(
            l.rows_visible(),
            0,
            "{:?} fits no row but claims one",
            l.body
        );
    }

    /// The card grid wraps to as many columns as fit and no more, and the cards
    /// in a row do not overlap.
    #[test]
    fn cards_tile_their_row_without_overlapping() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            assert!(
                (1..=3).contains(&l.cards_across),
                "{} cards across at {size:?}",
                l.cards_across
            );
            for i in 1..l.cards_across {
                let left = l.card_rect(i - 1);
                let right = l.card_rect(i);
                assert!(
                    right.x >= left.right() - 0.01,
                    "card {i} at {right:?} overlaps card {} at {left:?} at {size:?}",
                    i - 1
                );
            }
            let last = l.card_rect(l.cards_across - 1);
            assert!(
                last.right() <= l.body.right() + 0.01,
                "the last card in a row at {last:?} leaves the body {:?} at {size:?}",
                l.body
            );
            // The next one wraps rather than continuing off the edge.
            let wrapped = l.card_rect(l.cards_across);
            assert!(
                wrapped.y > l.body.y,
                "card {} did not wrap to a second row at {size:?}",
                l.cards_across
            );
        }
    }

    // --- The mouse ----------------------------------------------------------

    /// Every lesson row on screen can be clicked, and the click selects the
    /// lesson whose title is drawn in that row.
    ///
    /// The row's own title, not "some lesson": the index the hit box carries is
    /// into `lessons` and the drawing walks the *filtered* list, and confusing
    /// the two is the bug this app already had.
    #[test]
    fn clicking_a_row_selects_the_lesson_drawn_in_it() {
        let app = TypingTutorApp::new();
        let frame = app.draw(W);
        // Counted, because the `continue` below is a silent exit: a list drawn
        // with no rows at all would skip every iteration and pass (lesson 89).
        let mut checked = 0usize;
        for idx in 0..app.lessons.len() {
            let Some(r) = probe::rect_of(&app, Target::Lesson(idx)) else {
                continue;
            };
            checked = checked.saturating_add(1);
            assert!(
                text_inside(&frame, &app.lessons[idx].title, r),
                "row {idx} is clickable but lesson {:?} is not drawn in it",
                app.lessons[idx].title
            );
            let mut app = TypingTutorApp::new();
            assert_eq!(
                probe::click(&mut app, Target::Lesson(idx)),
                EventResult::Consumed
            );
            assert_eq!(
                app.selected_lesson, idx,
                "clicking row {idx} selected lesson {}",
                app.selected_lesson
            );
        }
        assert!(checked > 1, "only {checked} row(s) were on screen to check");
    }

    /// A second click on the row already selected starts it.
    ///
    /// The same two-step the arrow keys and Enter give, so the mouse cannot
    /// start a lesson the user has not seen highlighted.
    #[test]
    fn a_second_click_on_the_selected_row_starts_the_lesson() {
        let mut app = TypingTutorApp::new();
        probe::click(&mut app, Target::Lesson(2));
        assert_eq!(
            app.view,
            AppView::LessonSelect,
            "one click started a lesson"
        );
        assert!(app.session.is_none());
        probe::click(&mut app, Target::Lesson(2));
        assert_eq!(
            app.view,
            AppView::Typing,
            "the second click did not start it"
        );
        assert_eq!(app.selected_lesson, 2);
    }

    /// The filter chip is a button, it says what it filters by, and clicking it
    /// walks the categories.
    #[test]
    fn the_filter_chip_says_what_it_does_and_does_it() {
        let mut app = TypingTutorApp::new();
        for expected in LessonCategory::all() {
            let before = app.category_filter;
            let label = match before {
                None => String::from("All Categories"),
                Some(cat) => format!("Category: {}", cat.name()),
            };
            let r = probe::rect_of(&app, Target::Filter).expect("the filter chip");
            assert!(
                text_inside(&app.draw(W), &label, r),
                "the chip is clickable but does not say {label:?}"
            );
            assert_eq!(
                probe::click(&mut app, Target::Filter),
                EventResult::Consumed
            );
            assert_eq!(
                app.category_filter,
                Some(*expected),
                "the chip did not step to {expected:?}"
            );
        }
        probe::click(&mut app, Target::Filter);
        assert_eq!(app.category_filter, None, "the filter did not wrap to all");
    }

    /// Filtering leaves the cursor on a lesson the filter admits.
    ///
    /// It used to set `selected_lesson = 0`, which is a lesson the new filter
    /// shows only when it happens to admit lesson zero's category -- otherwise
    /// the highlight was on a row that was not drawn.
    #[test]
    fn filtering_moves_the_cursor_to_a_lesson_the_filter_admits() {
        let mut app = TypingTutorApp::new();
        for _ in 0..LessonCategory::all().len() {
            probe::click(&mut app, Target::Filter);
            let shown = app.filtered_lessons();
            assert!(
                shown.contains(&app.selected_lesson),
                "filter {:?} shows {shown:?} and the cursor is on {}",
                app.category_filter,
                app.selected_lesson
            );
        }
    }

    /// The Stats chip is a button that opens the statistics view.
    #[test]
    fn the_stats_chip_opens_the_statistics_view() {
        let mut app = TypingTutorApp::new();
        let r = probe::rect_of(&app, Target::Stats).expect("the stats chip");
        assert!(text_inside(&app.draw(W), "Stats", r));
        assert_eq!(probe::click(&mut app, Target::Stats), EventResult::Consumed);
        assert_eq!(app.view, AppView::Statistics);
    }

    /// Every view the user can be taken away from offers a way back, and the
    /// way back is a button that is drawn.
    #[test]
    fn every_view_away_from_the_list_has_a_labelled_way_back() {
        for (view, label) in [
            (AppView::Typing, "Esc"),
            (AppView::Results, "Lessons"),
            (AppView::Statistics, "Lessons"),
        ] {
            let mut app = typed_into(0, 4);
            app.view = view;
            let r = probe::rect_of(&app, Target::Back)
                .unwrap_or_else(|| panic!("{view:?} offers no way back"));
            assert!(
                text_inside(&app.draw(W), label, r),
                "{view:?}'s Back button is clickable but unlabelled"
            );
            assert_eq!(probe::click(&mut app, Target::Back), EventResult::Consumed);
            assert_eq!(app.view, AppView::LessonSelect, "{view:?} did not go back");
            assert!(
                app.session.is_none(),
                "{view:?} went back and left the abandoned lesson in place, so \
                 the next Enter resumes a session the user walked out of"
            );
        }
    }

    /// Retry is on the results screen, is labelled, and starts the same lesson
    /// over from the beginning.
    #[test]
    fn retry_restarts_the_lesson_that_was_just_finished() {
        let mut app = typed_into(3, 6);
        app.view = AppView::Results;
        let r = probe::rect_of(&app, Target::Retry).expect("the retry button");
        assert!(text_inside(&app.draw(W), "Retry", r));
        assert_eq!(probe::click(&mut app, Target::Retry), EventResult::Consumed);
        assert_eq!(app.view, AppView::Typing);
        assert_eq!(app.selected_lesson, 3, "retry changed the lesson");
        assert_eq!(
            app.session.as_ref().expect("a session").cursor,
            0,
            "retry did not start from the beginning"
        );
    }

    /// A click on nothing changes nothing and says so.
    ///
    /// `Ignored`, not `Consumed`: a click the app did not use must not ask for
    /// a repaint of a picture that has not changed.
    #[test]
    fn a_click_on_empty_space_is_ignored() {
        let mut app = TypingTutorApp::new();
        let before = (app.view, app.selected_lesson, app.category_filter);
        let (x, y) = probe::bare_point(&app, W).expect(
            "no point in the window is free of a hit box, so a click that \
             reaches nothing cannot be tested here",
        );
        assert_eq!(
            app.click_at(x, y, MouseButton::Left, W),
            EventResult::Ignored
        );
        assert_eq!((app.view, app.selected_lesson, app.category_filter), before);
    }

    /// A control is clickable wherever the window puts it.
    ///
    /// The point is read from the frame drawn at that size, which is the whole
    /// reason the drawn size is stored: a window the user resized used to be
    /// clicked against a picture drawn at 620x560.
    #[test]
    fn the_controls_move_with_the_window_and_are_still_clickable() {
        // The chip is dropped rather than drawn illegibly narrow when the
        // header cannot pay for it, so `continue` is right -- but it must not be
        // able to take every size with it (lesson 89).
        let mut checked = 0usize;
        for size in SIZES {
            let mut app = TypingTutorApp::new();
            let Some(r) = probe::rect_of_sized(&app, Target::Stats, size) else {
                continue;
            };
            checked = checked.saturating_add(1);
            assert_eq!(
                app.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0, MouseButton::Left, size),
                EventResult::Consumed,
                "the Stats chip at {r:?} does not answer a click at {size:?}"
            );
            assert_eq!(app.view, AppView::Statistics, "at {size:?}");
        }
        assert!(
            checked >= SIZES.len() - 1,
            "the Stats chip was drawn at only {checked} of {} sizes",
            SIZES.len()
        );
    }

    // --- Scrolling ----------------------------------------------------------

    /// A list longer than the window scrolls, and scrolling brings rows that
    /// were off the bottom onto the screen.
    #[test]
    fn the_wheel_reveals_lessons_that_were_off_the_bottom() {
        // Short enough that the twelve default lessons cannot all fit.
        let size = (620.0, 260.0);
        let mut app = TypingTutorApp::new();
        let capacity = Layout::solve(size.0, size.1).rows_visible();
        assert!(
            capacity > 0 && capacity < app.lessons.len(),
            "this test needs a window that shows some but not all lessons"
        );
        let hidden = app.lessons.len() - capacity;
        let last = app.lessons.len() - 1;
        assert!(
            probe::rect_of_sized(&app, Target::Lesson(last), size).is_none(),
            "the last lesson is already on screen"
        );
        for _ in 0..=hidden {
            app.scroll_at(size.0 / 2.0, size.1 / 2.0, -1.0, size);
        }
        assert!(
            probe::rect_of_sized(&app, Target::Lesson(last), size).is_some(),
            "scrolling to the end never brought the last lesson into view"
        );
    }

    /// The wheel at the end of the list is ignored rather than consumed.
    #[test]
    fn a_wheel_notch_that_moves_nothing_is_ignored() {
        let size = (620.0, 260.0);
        let mut app = TypingTutorApp::new();
        assert_eq!(
            app.scroll_at(size.0 / 2.0, size.1 / 2.0, 1.0, size),
            Some(EventResult::Ignored),
            "scrolling up from the top asked for a repaint"
        );
        for _ in 0..40 {
            app.scroll_at(size.0 / 2.0, size.1 / 2.0, -1.0, size);
        }
        assert_eq!(
            app.scroll_at(size.0 / 2.0, size.1 / 2.0, -1.0, size),
            Some(EventResult::Ignored),
            "scrolling down from the end asked for a repaint"
        );
    }

    /// A list that fits does not scroll at all.
    #[test]
    fn a_list_that_fits_does_not_scroll() {
        let size = (620.0, 1400.0);
        let mut app = TypingTutorApp::new();
        assert!(Layout::solve(size.0, size.1).rows_visible() >= app.lessons.len());
        app.scroll_at(size.0 / 2.0, size.1 / 2.0, -1.0, size);
        assert_eq!(app.scroll_offset, 0, "a list that fits scrolled anyway");
    }

    /// Walking off the bottom with the arrow keys scrolls the list rather than
    /// moving the cursor to a row that is not drawn.
    #[test]
    fn the_cursor_key_scrolls_the_list_to_follow_the_cursor() {
        let size = (620.0, 260.0);
        let mut app = TypingTutorApp::new();
        let capacity = Layout::solve(size.0, size.1).rows_visible();
        assert!(capacity > 0 && capacity < app.lessons.len());
        for _ in 0..app.lessons.len() {
            app.key_at(&make_key(Key::Down, None), size);
            let here = app.selected_lesson;
            assert!(
                probe::rect_of_sized(&app, Target::Lesson(here), size).is_some(),
                "the cursor is on lesson {here} and that row is not on screen"
            );
            // Stepping one row off the bottom brings the list up by one, so the
            // page just read stays on screen. Anchoring the cursor at the top
            // instead would also keep it visible -- and would throw that page
            // away every time (known-issues.md lesson 70).
            if here > 0 {
                assert!(
                    probe::rect_of_sized(&app, Target::Lesson(here - 1), size).is_some(),
                    "reaching lesson {here} scrolled the row above it off the top"
                );
            }
        }
    }

    // --- The typing panel ---------------------------------------------------

    /// The panel scrolls to keep the character being typed on screen.
    ///
    /// The old fixed panel showed the first few lines of every lesson and the
    /// cursor walked off the bottom of it. The cursor's highlight is the box
    /// under the current character, so this asks whether that box was drawn at
    /// all -- which is the same question as whether the cursor is in view.
    ///
    /// It types a lesson of its own rather than the longest shipped one,
    /// because the longest shipped one is 51 characters and a 300x300 panel
    /// holds around five lines of forty: the text fit whole, the panel never
    /// had to scroll, and pinning it to the top of the text was therefore
    /// *correct* on the only fixture the test built. The precondition below
    /// is what stops that happening again -- it asserts the panel cannot show
    /// the whole text, which is the regime the rule being tested exists for.
    #[test]
    fn the_typing_panel_follows_the_cursor() {
        let size = (300.0, 300.0);
        let mut app = TypingTutorApp::new();
        app.lessons.push(Lesson {
            category: LessonCategory::Sentences,
            title: String::from("Long enough to overflow the panel"),
            text: "the quick brown fox jumps over the lazy dog ".repeat(12),
        });
        let longest = app.lessons.len().saturating_sub(1);
        app.start_lesson(longest);
        let total = app.session.as_ref().expect("a session").text.len();

        // Precondition: the panel is smaller than the text. Counted from the
        // glyphs actually drawn, not from arithmetic over the layout, so it
        // stays true to what the reader sees.
        let mut in_mono = false;
        let mut drawn = 0usize;
        for cmd in app.frame(size.0, size.1).commands() {
            match cmd {
                RenderCommand::PushFont { .. } => in_mono = true,
                RenderCommand::PopFont => in_mono = false,
                RenderCommand::Text { .. } if in_mono => {
                    drawn = drawn.saturating_add(1);
                }
                _ => {}
            }
        }
        assert!(
            drawn < total,
            "the panel shows all {total} characters at once, so it never has \
             to scroll and this test cannot tell whether it would"
        );

        for n in 0..total {
            let session = app.session.as_ref().expect("a session");
            let Some(&ch) = session.text.get(session.cursor) else {
                break;
            };
            app.key_at(&make_key(Key::A, Some(ch)), size);
            if app.view != AppView::Typing {
                break;
            }
            let frame = app.frame(size.0, size.1);
            let highlight = frame.commands().iter().any(|c| {
                matches!(c, RenderCommand::FillRect { color, .. }
                    if *color == hex(COL_SURFACE1))
            });
            assert!(
                highlight,
                "after {n} characters the cursor's highlight is not drawn -- \
                 the panel is not following the typist"
            );
        }
    }

    /// The results text is placed below the cards, not at a constant.
    ///
    /// `table_y = 240.0` was one of the faults the wiring found, and this is
    /// the same shape in the view next door: a hardcoded start for the text
    /// under the summary cards. It is wrong exactly when the card grid wraps
    /// far enough to reach past the constant, and the damage is text printed
    /// *over* the cards -- which leaves nothing outside the window, so the
    /// geometric catch-all only ever noticed it at the sizes where 240 itself
    /// fell off the bottom. This asks the rule instead of a side effect of it.
    #[test]
    fn the_results_text_sits_below_the_cards_it_summarises() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        app.view = AppView::Results;
        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);

        // The cards are the only fills inside the body on this view.
        let mut rows: Vec<f32> = Vec::new();
        let mut card_bottom = f32::NEG_INFINITY;
        for cmd in frame.commands() {
            if let RenderCommand::FillRect { y, height, .. } = cmd {
                if *y >= l.body.y {
                    if !rows.iter().any(|r| (r - y).abs() < 0.5) {
                        rows.push(*y);
                    }
                    card_bottom = card_bottom.max(y + height);
                }
            }
        }
        assert!(
            rows.len() > 1,
            "the cards fit on a single row at {W:?}, and a constant above a \
             single row of cards is indistinguishable from the rule -- this \
             test needs a window the grid wraps in"
        );

        let rating = frame
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { y, text, .. } if text.starts_with("Rating:") => Some(*y),
                _ => None,
            })
            .expect("the results view names a rating");
        assert!(
            rating >= card_bottom,
            "the rating is drawn at y {rating}, above the bottom of the cards \
             at {card_bottom} -- it is printed over them"
        );
    }

    /// The lesson text is drawn in the monospace family it is measured in.
    ///
    /// The two have to agree: the pen is advanced by `measure_in(.., Mono)`,
    /// and if the renderer is left in the proportional face then every advance
    /// is a measurement of a font that is not being used.
    #[test]
    fn the_lesson_text_is_drawn_in_the_family_it_is_measured_in() {
        let app = typed_into(0, 3);
        let cmds = app.frame(W.0, W.1).commands().to_vec();
        let pushed = cmds.iter().position(|c| {
            matches!(
                c,
                RenderCommand::PushFont {
                    family: FontFamily::Mono
                }
            )
        });
        let popped = cmds
            .iter()
            .position(|c| matches!(c, RenderCommand::PopFont));
        let (pushed, popped) = (pushed.expect("no mono scope"), popped.expect("no pop"));
        assert!(
            pushed < popped,
            "the font scope is closed before it is opened"
        );
        let glyphs = cmds
            .get(pushed..popped)
            .expect("the scope")
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .count();
        assert!(glyphs > 3, "only {glyphs} characters inside the mono scope");
    }

    /// A character that has been typed correctly is drawn in the correct
    /// colour, a mistyped one in the error colour, and an untouched one in
    /// neither.
    #[test]
    fn each_character_is_coloured_by_how_it_was_typed() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        // One right, then one deliberately wrong.
        let first = app.session.as_ref().expect("a session").text[0];
        app.handle_key(&make_key(Key::A, Some(first)));
        let second = app.session.as_ref().expect("a session").text[1];
        let wrong = if second == 'z' { 'q' } else { 'z' };
        app.handle_key(&make_key(Key::A, Some(wrong)));

        let frame = app.frame(W.0, W.1);
        let mut in_mono = false;
        let mut colours = Vec::new();
        for cmd in frame.commands() {
            match cmd {
                RenderCommand::PushFont { .. } => in_mono = true,
                RenderCommand::PopFont => in_mono = false,
                RenderCommand::Text { color, .. } if in_mono => colours.push(*color),
                _ => {}
            }
        }
        assert_eq!(colours.first().copied(), Some(hex(COL_GREEN)));
        assert_eq!(colours.get(1).copied(), Some(hex(COL_RED)));
        assert_eq!(
            colours.get(3).copied(),
            Some(hex(COL_SURFACE2)),
            "an untouched character is not drawn as pending"
        );
    }

    // --- The clock, end to end ----------------------------------------------

    /// The window loop is asked for a tick, and the tick is what moves the
    /// clock. Neither half is any use without the other: `advance_time` was
    /// correct and tested throughout the period when nothing called it.
    #[test]
    fn the_app_asks_the_window_loop_for_a_clock() {
        let app = TypingTutorApp::new();
        let interval = App::tick_interval(&app).expect("a typing tutor needs a clock");
        assert!(
            interval <= Duration::from_millis(500),
            "a WPM figure updated every {interval:?} is a stopwatch that stutters"
        );
    }

    /// A tick during a lesson asks for a repaint; a tick on a still view does
    /// not.
    #[test]
    fn only_the_view_with_a_clock_repaints_on_a_tick() {
        let mut app = typed_into(0, 2);
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 100 }),
            EventResult::Consumed
        );
        app.view = AppView::LessonSelect;
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 100 }),
            EventResult::Ignored,
            "the lesson list asked to be redrawn sixty times a second"
        );
    }

    /// Closing the window exits rather than being handled as an ordinary event.
    #[test]
    fn a_close_request_exits() {
        let mut app = TypingTutorApp::new();
        assert!(matches!(
            App::on_event(&mut app, &Event::CloseRequested),
            Response::Exit
        ));
    }

    /// A consumed event asks for a repaint and an ignored one does not.
    #[test]
    fn the_window_repaints_for_what_the_app_used_and_not_for_what_it_did_not() {
        let mut app = TypingTutorApp::new();
        assert!(matches!(
            App::on_event(&mut app, &Event::Key(make_key(Key::Down, None))),
            Response::Redraw
        ));
        assert!(matches!(
            App::on_event(&mut app, &Event::FocusIn),
            Response::Idle
        ));
    }

    /// `render` draws at the size it is given and remembers it, so the next
    /// click is read against the picture the user actually clicked on.
    #[test]
    fn rendering_remembers_the_size_the_frame_was_drawn_at() {
        let mut app = TypingTutorApp::new();
        let tree = App::render(&mut app, 1024.0, 768.0);
        assert!(!tree.commands.is_empty());
        assert!((app.width - 1024.0).abs() < f32::EPSILON);
        assert!((app.height - 768.0).abs() < f32::EPSILON);
    }

    // --- Rating and the results screen --------------------------------------

    /// Each rating band is reported at its own boundary and one below it.
    #[test]
    fn the_rating_bands_are_where_they_say_they_are() {
        assert_eq!(wpm_rating(80.0), "Expert!");
        assert_eq!(wpm_rating(79.9), "Advanced");
        assert_eq!(wpm_rating(60.0), "Advanced");
        assert_eq!(wpm_rating(59.9), "Intermediate");
        assert_eq!(wpm_rating(40.0), "Intermediate");
        assert_eq!(wpm_rating(39.9), "Beginner");
        assert_eq!(wpm_rating(20.0), "Beginner");
        assert_eq!(wpm_rating(19.9), "Keep Practicing!");
        assert_eq!(wpm_rating(0.0), "Keep Practicing!");
    }

    /// The results screen shows the six figures it claims to, each under its
    /// own name and inside its own card.
    #[test]
    fn every_results_card_shows_its_own_figure() {
        let mut app = typed_into(0, 10);
        app.view = AppView::Results;
        let session = app.session.as_ref().expect("a session");
        let secs = session.elapsed_ms(app.current_time_ms) / 1000;
        let want = [
            ("WPM", format!("{:.0}", session.wpm(app.current_time_ms))),
            ("Accuracy", format!("{:.1}%", session.accuracy())),
            ("Time", format!("{}:{:02}", secs / 60, secs % 60)),
            ("Keystrokes", session.total_keystrokes.to_string()),
            ("Correct", session.correct_keystrokes.to_string()),
            ("Errors", session.incorrect_keystrokes.to_string()),
        ];
        let l = Layout::solve(1280.0, 800.0);
        let frame = app.frame(1280.0, 800.0);
        for (i, (name, value)) in want.iter().enumerate() {
            let card = l.card_rect(i);
            assert!(
                text_inside(&frame, name, card),
                "card {i} does not name itself {name:?}"
            );
            assert!(
                text_inside(&frame, value, card),
                "card {i} ({name}) does not show {value:?}"
            );
        }
    }

    /// The statistics table shows the newest result first.
    #[test]
    fn the_history_table_is_newest_first() {
        let mut app = TypingTutorApp::new();
        for n in 0..3u32 {
            app.results.push(SessionResult {
                lesson_title: format!("Lesson {n}"),
                category: LessonCategory::HomeRow,
                wpm: f64::from(n) * 10.0,
                accuracy: 90.0,
                duration_ms: 10_000,
                text_length: 40,
            });
        }
        app.view = AppView::Statistics;
        let names: Vec<String> = app
            .frame(1280.0, 800.0)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } if text.starts_with("Lesson ") => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            vec![
                String::from("Lesson 2"),
                String::from("Lesson 1"),
                String::from("Lesson 0")
            ]
        );
    }

    /// A history row is drawn in its lesson's category colour, which is the
    /// only thing that says what kind of practice it was.
    #[test]
    fn a_history_row_carries_its_category_colour() {
        let mut app = TypingTutorApp::new();
        app.results.push(SessionResult {
            lesson_title: String::from("Numbers drill"),
            category: LessonCategory::Numbers,
            wpm: 30.0,
            accuracy: 88.0,
            duration_ms: 20_000,
            text_length: 60,
        });
        app.view = AppView::Statistics;
        let drawn = app
            .frame(1280.0, 800.0)
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == "Numbers drill" => Some(*color),
                _ => None,
            })
            .expect("the row is drawn");
        assert_eq!(
            drawn,
            LessonCategory::Numbers.color(),
            "the row is not in its category's colour"
        );
    }

    // --- What the sweep found missing ---------------------------------------

    /// The four bands stack down the window in order and do not overlap.
    ///
    /// The body is measured from the bottom of the subhead rather than from the
    /// top of the window, and nothing else here would notice a body that starts
    /// at the top: it would still be inside the window, still tile its rows,
    /// still answer clicks. It would simply be drawn over its own title.
    #[test]
    fn the_bands_stack_without_overlapping() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            assert!(
                l.header.y.abs() < 0.01,
                "the header starts at {} rather than the top at {size:?}",
                l.header.y
            );
            assert!(
                l.subhead.y >= l.header.bottom() - 0.01,
                "the subhead {:?} overlaps the header {:?} at {size:?}",
                l.subhead,
                l.header
            );
            assert!(
                l.body.y >= l.subhead.bottom() - 0.01,
                "the body {:?} is drawn over the chrome above it at {size:?}",
                l.body
            );
            assert!(
                l.body.bottom() <= l.footer.y + 0.01,
                "the body {:?} runs into the footer {:?} at {size:?}",
                l.body,
                l.footer
            );
            assert!(
                (l.footer.bottom() - size.1).abs() < 0.01,
                "the footer ends at {} rather than the bottom at {size:?}",
                l.footer.bottom()
            );
        }
    }

    /// A window too small to pay for its chrome gives the chrome up, not the
    /// list -- and gives up the reminder lines before the title.
    ///
    /// A view with no lessons in it is a view with nothing in it; a view with no
    /// title is one the user cannot name, which is why the title goes last.
    #[test]
    fn a_squeezed_window_gives_up_its_chrome_before_its_body() {
        let l = Layout::solve(620.0, 60.0);
        assert!(
            l.rows_visible() >= 1,
            "a 60 px window shows no lessons at all -- body {:?}, row {}",
            l.body,
            l.row
        );
        assert!(
            l.subhead.h == 0.0 || l.footer.h == 0.0,
            "a window this short kept every band and still found room for a row"
        );
        assert!(
            l.header.h > 0.0,
            "the title was given up before the reminder lines were"
        );
    }

    /// A row clicked while a filter is on selects the lesson that row draws.
    ///
    /// The hit box carries an index into `lessons` and the drawing walks the
    /// *filtered* list. Confusing the two selects a lesson from a different part
    /// of the list than the one pointed at, and it cannot show up until a filter
    /// makes the two numberings disagree -- which is why the unfiltered test
    /// above it passes either way.
    #[test]
    fn a_row_clicked_under_a_filter_selects_the_lesson_it_shows() {
        for steps in 1..=LessonCategory::all().len() {
            let mut app = TypingTutorApp::new();
            for _ in 0..steps {
                probe::click(&mut app, Target::Filter);
            }
            let frame = app.draw(W);
            let shown = app.filtered_lessons();
            assert!(
                !shown.is_empty(),
                "filter {:?} admits no lesson, so this round checks nothing",
                app.category_filter
            );
            for &idx in &shown {
                let r = probe::rect_of(&app, Target::Lesson(idx)).unwrap_or_else(|| {
                    panic!(
                        "filter {:?} shows lesson {idx} and no row can be clicked for it",
                        app.category_filter
                    )
                });
                assert!(
                    text_inside(&frame, &app.lessons[idx].title, r),
                    "filter {:?} draws a row for lesson {idx} without its title",
                    app.category_filter
                );
                let mut clicked = TypingTutorApp::new();
                for _ in 0..steps {
                    probe::click(&mut clicked, Target::Filter);
                }
                probe::click(&mut clicked, Target::Lesson(idx));
                assert_eq!(
                    clicked.selected_lesson, idx,
                    "under filter {:?}, the row drawing {:?} selected lesson {}",
                    app.category_filter, app.lessons[idx].title, clicked.selected_lesson
                );
            }
        }
    }

    /// A window that grows shows the rows the scroll had hidden.
    ///
    /// Scrolling to the end of a short window and then enlarging it leaves an
    /// offset the now-shorter remainder no longer needs. Left alone it holds the
    /// top of the list off screen and pads the bottom with blank rows where
    /// lessons used to be.
    #[test]
    fn a_window_that_grows_shows_the_rows_the_scroll_had_hidden() {
        let small = (620.0, 260.0);
        let big = (620.0, 1400.0);
        let mut app = TypingTutorApp::new();
        for _ in 0..40 {
            app.scroll_at(small.0 / 2.0, small.1 / 2.0, -1.0, small);
        }
        assert!(
            app.scroll_offset > 0,
            "the short window did not scroll, so this proves nothing"
        );
        assert!(
            Layout::solve(big.0, big.1).rows_visible() >= app.lessons.len(),
            "this test needs a window the whole list fits into"
        );
        app.key_at(&make_key(Key::Down, None), big);
        assert!(
            probe::rect_of_sized(&app, Target::Lesson(0), big).is_some(),
            "the window grew to fit every lesson and the first is still \
             scrolled off the top -- offset {}",
            app.scroll_offset
        );
    }

    /// A key during a lesson that produces no character asks for no repaint.
    ///
    /// A function key or a bare modifier changes nothing on screen, and a view
    /// that reports one consumed redraws the whole window for each.
    #[test]
    fn a_key_that_types_nothing_does_not_ask_for_a_repaint() {
        let mut app = typed_into(0, 3);
        assert_eq!(app.view, AppView::Typing, "this test needs a live lesson");
        assert_eq!(
            app.handle_key(&make_key(Key::Down, None)),
            EventResult::Ignored
        );
    }

    /// An unselected row is told apart from the background it sits on.
    ///
    /// The program this replaces filled it with `COL_BASE` -- the background --
    /// so the list was one flat field and the only visible boundary in it was
    /// the one the cursor was on.
    #[test]
    fn an_unselected_row_is_told_apart_from_the_background() {
        let app = TypingTutorApp::new();
        assert_ne!(app.selected_lesson, 1, "row one is meant to be unselected");
        let r = probe::rect_of(&app, Target::Lesson(1)).expect("a second row");
        let painted = app.draw(W).commands().iter().any(|c| {
            matches!(c, RenderCommand::FillRect { x, y, color, .. }
                if (*x - r.x).abs() < 0.01
                    && (*y - r.y).abs() < 0.01
                    && *color != hex(COL_BASE))
        });
        assert!(
            painted,
            "an unselected row is painted in the background colour, so the list \
             is a single flat field"
        );
    }

    /// A lesson's subtitle counts characters, not bytes.
    ///
    /// Every lesson shipped here is ASCII, where the two agree, so this adds one
    /// where they do not rather than trusting the list to keep disagreeing --
    /// which is the only way this can be a test of the count rather than a test
    /// of what happens to be in the list today.
    #[test]
    fn a_lesson_subtitle_counts_characters_not_bytes() {
        let mut app = TypingTutorApp::new();
        app.lessons.push(Lesson {
            category: LessonCategory::Sentences,
            title: String::from("Accents"),
            text: String::from("café naïve résumé"),
        });
        let idx = app.lessons.len() - 1;
        let text = &app.lessons[idx].text;
        assert_ne!(
            text.len(),
            text.chars().count(),
            "this test needs a lesson whose bytes and characters differ"
        );
        let want = format!("Sentences - {} chars", text.chars().count());
        app.selected_lesson = idx;
        app.scroll_cursor_into_view();
        let drawn = texts(&app, W.0, W.1);
        assert!(
            drawn.contains(&want),
            "the subtitle does not say {want:?}: {drawn:?}"
        );
    }

    /// The window is named, identified, and opens at the size its own tests
    /// measure against.
    ///
    /// The last of the three is the one that matters here: every geometric test
    /// in this file is written against [`Probe::SIZE`], so a window that opens
    /// at some other size is a window none of them has ever seen.
    #[test]
    fn the_window_is_named_and_identified() {
        let app = TypingTutorApp::new();
        assert_eq!(App::title(&app), "Typing Tutor");
        assert_eq!(App::app_id(&app), "typingtutor");
        let (w, h) = App::initial_size(&app);
        let opened = (
            f32::from(u16::try_from(w).expect("a sane width")),
            f32::from(u16::try_from(h).expect("a sane height")),
        );
        assert!(
            (opened.0 - W.0).abs() < 1.0 && (opened.1 - W.1).abs() < 1.0,
            "the window opens at {opened:?}, which is not the {W:?} its tests \
             measure it at"
        );
    }

    // --- Key released ignored ---

    #[test]
    fn key_released_ignored() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.selected_lesson, 0);
    }

    // --- Event handling ---

    #[test]
    fn handle_event_key() {
        let mut app = TypingTutorApp::new();
        app.handle_event(&Event::Key(make_key(Key::Down, None)));
        assert_eq!(app.selected_lesson, 1);
    }

    // --- The clock ---

    /// A real `Event::Tick` moves the clock, and so the WPM figure.
    ///
    /// Through `handle_event` on purpose.  `wpm` and `elapsed_ms` were
    /// correct and directly tested throughout the period when the clock
    /// never moved, because those tests passed `current_time_ms` in by hand;
    /// only a test that goes through the event can tell the difference.
    /// Falsified by deleting the `Event::Tick` arm: this test fails and
    /// nothing else does.
    #[test]
    fn a_tick_event_moves_the_clock_and_the_wpm() {
        let mut app = TypingTutorApp::new();
        app.handle_event(&Event::Tick { elapsed_ms: 5000 });
        assert_eq!(
            app.current_time_ms, 5000,
            "Event::Tick did not reach the clock"
        );
    }

    /// Intervals accumulate; the clock is not merely the last one.
    #[test]
    fn tick_intervals_accumulate() {
        let mut app = TypingTutorApp::new();
        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        app.handle_event(&Event::Tick { elapsed_ms: 1500 });
        assert_eq!(app.current_time_ms, 2500);
    }

    /// The symptom the user would have seen: WPM stuck at zero.
    ///
    /// `wpm` divides by the elapsed time and returns 0.0 when that is 0, so
    /// a clock that never moves does not produce a wrong number or a panic --
    /// it produces a plausible-looking zero on every screen that shows a
    /// speed.  That is why nobody noticed.
    #[test]
    fn typing_then_ticking_produces_a_real_speed() {
        let mut app = TypingTutorApp::new();
        let mut session = TypingSession::new("hello");
        session.type_char('h', app.current_time_ms);
        session.type_char('e', app.current_time_ms);
        app.session = Some(session);

        app.handle_event(&Event::Tick { elapsed_ms: 60_000 });

        let wpm = app
            .session
            .as_ref()
            .map_or(0.0, |s| s.wpm(app.current_time_ms));
        // Two correct characters in one minute = 2/5 of a word per minute.
        assert!(wpm > 0.0, "one minute of typing still reported {wpm} WPM");
    }

    // --- CharStatus enum ---

    #[test]
    fn char_status_eq() {
        assert_eq!(CharStatus::Pending, CharStatus::Pending);
        assert_ne!(CharStatus::Correct, CharStatus::Incorrect);
    }

    // --- AppView enum ---

    #[test]
    fn app_view_eq() {
        assert_eq!(AppView::LessonSelect, AppView::LessonSelect);
        assert_ne!(AppView::Typing, AppView::Results);
    }

    #[test]
    fn navigate_down_at_bottom_clamped() {
        let mut app = TypingTutorApp::new();
        let max = app.filtered_lessons().len().saturating_sub(1);
        app.selected_lesson = max;
        app.handle_key(&make_key(Key::Down, None));
        assert_eq!(app.selected_lesson, max);
    }
}
