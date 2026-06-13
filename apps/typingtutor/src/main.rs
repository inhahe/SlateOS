//! Slate OS Typing Tutor
//!
//! A typing practice application with multiple lesson types, WPM tracking,
//! accuracy statistics, and progressive difficulty levels.

#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

        let expected = self.text[self.cursor];
        if ch == expected {
            self.statuses[self.cursor] = CharStatus::Correct;
            self.correct_keystrokes = self.correct_keystrokes.saturating_add(1);
            self.cursor += 1;
        } else {
            self.statuses[self.cursor] = CharStatus::Incorrect;
            self.incorrect_keystrokes = self.incorrect_keystrokes.saturating_add(1);
            self.cursor += 1;
        }

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
        self.cursor -= 1;
        self.statuses[self.cursor] = CharStatus::Pending;
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
// Main app
// ---------------------------------------------------------------------------

struct TypingTutorApp {
    lessons: Vec<Lesson>,
    selected_lesson: usize,
    view: AppView,
    session: Option<TypingSession>,
    current_time_ms: u64,
    results: Vec<SessionResult>,
    category_filter: Option<LessonCategory>,
    scroll_offset: usize,
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
        }
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
        if lesson_idx < self.lessons.len() {
            self.session = Some(TypingSession::new(&self.lessons[lesson_idx].text));
            self.selected_lesson = lesson_idx;
            self.view = AppView::Typing;
        }
    }

    fn finish_lesson(&mut self) {
        if let Some(ref session) = self.session
            && session.finished {
                let wpm = session.wpm(self.current_time_ms);
                let acc = session.accuracy();
                let dur = session.elapsed_ms(self.current_time_ms);
                let title = self.lessons[self.selected_lesson].title.clone();
                let cat = self.lessons[self.selected_lesson].category;
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
            None => self.category_filter = Some(cats[0]),
            Some(current) => {
                let idx = cats.iter().position(|c| *c == current).unwrap_or(0);
                if idx + 1 < cats.len() {
                    self.category_filter = Some(cats[idx + 1]);
                } else {
                    self.category_filter = None;
                }
            }
        }
        self.selected_lesson = 0;
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

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match self.view {
            AppView::LessonSelect => self.handle_lesson_select(event),
            AppView::Typing => self.handle_typing(event),
            AppView::Results => self.handle_results(event),
            AppView::Statistics => self.handle_statistics(event),
        }
    }

    fn handle_lesson_select(&mut self, event: &KeyEvent) {
        let filtered = self.filtered_lessons();
        match event.key {
            Key::Up
                if self.selected_lesson > 0 => {
                    self.selected_lesson -= 1;
                }
            Key::Down
                if self.selected_lesson + 1 < filtered.len() => {
                    self.selected_lesson += 1;
                }
            Key::Enter => {
                if let Some(&idx) = filtered.get(self.selected_lesson) {
                    self.start_lesson(idx);
                }
            }
            Key::C => {
                self.cycle_category_filter();
            }
            Key::S => {
                self.view = AppView::Statistics;
            }
            Key::Escape => {
                // no-op at top level
            }
            _ => {}
        }
    }

    fn handle_typing(&mut self, event: &KeyEvent) {
        if event.key == Key::Escape {
            self.view = AppView::LessonSelect;
            self.session = None;
            return;
        }

        if event.key == Key::Backspace {
            if let Some(ref mut session) = self.session {
                session.backspace();
            }
            return;
        }

        // Type the character
        if let Some(ch) = event.text
            && let Some(ref mut session) = self.session {
                session.type_char(ch, self.current_time_ms);
                if session.finished {
                    self.finish_lesson();
                }
            }
    }

    fn handle_results(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Enter | Key::Space => {
                self.view = AppView::LessonSelect;
                self.session = None;
            }
            Key::R => {
                // Retry same lesson
                self.start_lesson(self.selected_lesson);
            }
            _ => {}
        }
    }

    fn handle_statistics(&mut self, event: &KeyEvent) {
        if event.key == Key::Escape || event.key == Key::Enter {
            self.view = AppView::LessonSelect;
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event {
            self.handle_key(ke);
        }
    }

    fn set_time(&mut self, time_ms: u64) {
        self.current_time_ms = time_ms;
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::from_hex(COL_BASE),
            corner_radii: CornerRadii::ZERO,
        });

        match self.view {
            AppView::LessonSelect => self.render_lesson_select(&mut cmds, width),
            AppView::Typing => self.render_typing(&mut cmds, width),
            AppView::Results => self.render_results(&mut cmds, width),
            AppView::Statistics => self.render_statistics(&mut cmds, width),
        }

        cmds
    }

    fn render_lesson_select(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        // Header
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Typing Tutor"),
            color: Color::from_hex(COL_BLUE),
            font_size: 36.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Category filter
        let filter_text = match self.category_filter {
            None => String::from("All Categories"),
            Some(cat) => format!("Category: {}", cat.name()),
        };
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 65.0,
            text: filter_text,
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 88.0,
            text: String::from("↑/↓: Select  |  Enter: Start  |  C: Category  |  S: Stats"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Lesson list
        let filtered = self.filtered_lessons();
        let list_y = 120.0;
        for (display_idx, &lesson_idx) in filtered.iter().enumerate() {
            let y = list_y + display_idx as f32 * 50.0;
            let lesson = &self.lessons[lesson_idx];
            let is_selected = display_idx == self.selected_lesson;

            // Row background
            let bg_color = if is_selected {
                Color::from_hex(COL_SURFACE0)
            } else {
                Color::from_hex(COL_BASE)
            };
            cmds.push(RenderCommand::FillRect {
                x: 20.0,
                y,
                width: 560.0,
                height: 44.0,
                color: bg_color,
                corner_radii: CornerRadii::all(6.0),
            });

            // Category indicator
            cmds.push(RenderCommand::FillRect {
                x: 26.0,
                y: y + 10.0,
                width: 4.0,
                height: 24.0,
                color: lesson.category.color(),
                corner_radii: CornerRadii::all(2.0),
            });

            // Title
            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: y + 6.0,
                text: lesson.title.clone(),
                color: if is_selected {
                    Color::from_hex(COL_TEXT)
                } else {
                    Color::from_hex(COL_SUBTEXT1)
                },
                font_size: 17.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(400.0),
            });

            // Category label
            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: y + 26.0,
                text: format!("{} · {} chars", lesson.category.name(), lesson.text.len()),
                color: Color::from_hex(COL_OVERLAY0),
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_typing(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        let session = match &self.session {
            Some(s) => s,
            None => return,
        };
        let lesson = &self.lessons[self.selected_lesson];

        // Header
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: lesson.title.clone(),
            color: Color::from_hex(COL_BLUE),
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Stats bar
        let wpm = session.wpm(self.current_time_ms);
        let acc = session.accuracy();
        let elapsed = session.elapsed_ms(self.current_time_ms);
        let secs = elapsed / 1000;
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 55.0,
            text: format!(
                "WPM: {:.0}  |  Accuracy: {:.1}%  |  Time: {}:{:02}  |  Esc: quit",
                wpm,
                acc,
                secs / 60,
                secs % 60
            ),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress bar
        let progress = session.progress_percent() as f32 / 100.0;
        let bar_w = width - 60.0;
        cmds.push(RenderCommand::FillRect {
            x: 30.0,
            y: 80.0,
            width: bar_w,
            height: 6.0,
            color: Color::from_hex(COL_SURFACE0),
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: 30.0,
            y: 80.0,
            width: bar_w * progress,
            height: 6.0,
            color: Color::from_hex(COL_GREEN),
            corner_radii: CornerRadii::all(3.0),
        });

        // Typing area background
        cmds.push(RenderCommand::FillRect {
            x: 20.0,
            y: 100.0,
            width: width - 40.0,
            height: 200.0,
            color: Color::from_hex(COL_MANTLE),
            corner_radii: CornerRadii::all(8.0),
        });

        // Render the text with color-coded characters
        let font_size = 22.0;
        let char_width = 13.2; // approximate monospace character width
        let max_chars_per_line = ((width - 80.0) / char_width) as usize;
        let mut x = 35.0;
        let mut y = 120.0;
        let line_height = 32.0;
        let mut chars_on_line = 0;

        for (i, &ch) in session.text.iter().enumerate() {
            if chars_on_line >= max_chars_per_line {
                x = 35.0;
                y += line_height;
                chars_on_line = 0;
            }

            let color = if i == session.cursor {
                // Current cursor position — highlight background
                cmds.push(RenderCommand::FillRect {
                    x: x - 1.0,
                    y: y - 2.0,
                    width: char_width + 2.0,
                    height: font_size + 4.0,
                    color: Color::from_hex(COL_SURFACE1),
                    corner_radii: CornerRadii::all(2.0),
                });
                Color::from_hex(COL_TEXT)
            } else {
                match session.statuses[i] {
                    CharStatus::Pending => Color::from_hex(COL_SURFACE2),
                    CharStatus::Correct => Color::from_hex(COL_GREEN),
                    CharStatus::Incorrect => Color::from_hex(COL_RED),
                }
            };

            cmds.push(RenderCommand::Text {
                x,
                y,
                text: String::from(ch),
                color,
                font_size,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            x += char_width;
            chars_on_line += 1;
        }

        // Keyboard hint for current character
        if session.cursor < session.text.len() {
            let next_char = session.text[session.cursor];
            let hint = if next_char == ' ' {
                String::from("Space")
            } else {
                format!("Type: '{next_char}'")
            };
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: 320.0,
                text: hint,
                color: Color::from_hex(COL_YELLOW),
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_results(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        let session = match &self.session {
            Some(s) => s,
            None => return,
        };
        let lesson = &self.lessons[self.selected_lesson];

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 30.0,
            text: String::from("Lesson Complete!"),
            color: Color::from_hex(COL_GREEN),
            font_size: 36.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 80.0,
            text: lesson.title.clone(),
            color: Color::from_hex(COL_TEXT),
            font_size: 20.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Stats cards
        let wpm = session.wpm(self.current_time_ms);
        let acc = session.accuracy();
        let elapsed = session.elapsed_ms(self.current_time_ms);
        let secs = elapsed / 1000;

        let stats = [
            ("WPM", format!("{wpm:.0}"), COL_BLUE),
            ("Accuracy", format!("{acc:.1}%"), COL_GREEN),
            ("Time", format!("{}:{:02}", secs / 60, secs % 60), COL_PEACH),
            (
                "Keystrokes",
                session.total_keystrokes.to_string(),
                COL_YELLOW,
            ),
            ("Correct", session.correct_keystrokes.to_string(), COL_TEAL),
            ("Errors", session.incorrect_keystrokes.to_string(), COL_RED),
        ];

        for (i, (label, value, col)) in stats.iter().enumerate() {
            let col_idx = i % 3;
            let row_idx = i / 3;
            let sx = 30.0 + col_idx as f32 * 180.0;
            let sy = 120.0 + row_idx as f32 * 90.0;

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: 160.0,
                height: 75.0,
                color: Color::from_hex(COL_SURFACE0),
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: sx + 15.0,
                y: sy + 10.0,
                text: String::from(*label),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: sx + 15.0,
                y: sy + 32.0,
                text: value.clone(),
                color: Color::from_hex(*col),
                font_size: 28.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // WPM rating
        let rating = if wpm >= 80.0 {
            "Expert!"
        } else if wpm >= 60.0 {
            "Advanced"
        } else if wpm >= 40.0 {
            "Intermediate"
        } else if wpm >= 20.0 {
            "Beginner"
        } else {
            "Keep Practicing!"
        };

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 320.0,
            text: format!("Rating: {rating}"),
            color: Color::from_hex(COL_MAUVE),
            font_size: 22.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 360.0,
            text: String::from("Enter: Lesson List  |  R: Retry"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_statistics(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 20.0,
            text: String::from("Statistics"),
            color: Color::from_hex(COL_LAVENDER),
            font_size: 32.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if self.results.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: 80.0,
                text: String::from("No lessons completed yet. Start typing!"),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 18.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            // Summary cards
            let summary = [
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

            for (i, (label, value, col)) in summary.iter().enumerate() {
                let sx = 30.0 + (i % 3) as f32 * 180.0;
                let sy = 70.0 + (i / 3) as f32 * 80.0;

                cmds.push(RenderCommand::FillRect {
                    x: sx,
                    y: sy,
                    width: 160.0,
                    height: 65.0,
                    color: Color::from_hex(COL_SURFACE0),
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: sx + 10.0,
                    y: sy + 8.0,
                    text: String::from(*label),
                    color: Color::from_hex(COL_SUBTEXT0),
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: sx + 10.0,
                    y: sy + 28.0,
                    text: value.clone(),
                    color: Color::from_hex(*col),
                    font_size: 24.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Recent results table
            let table_y = 240.0;
            cmds.push(RenderCommand::Text {
                x: 30.0,
                y: table_y,
                text: String::from("Recent Results"),
                color: Color::from_hex(COL_TEXT),
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            let headers = ["Lesson", "WPM", "Accuracy", "Time"];
            let col_xs = [30.0, 250.0, 340.0, 440.0];
            for (i, header) in headers.iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: col_xs[i],
                    y: table_y + 30.0,
                    text: String::from(*header),
                    color: Color::from_hex(COL_SUBTEXT0),
                    font_size: 13.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Show last 8 results (newest first)
            let start = if self.results.len() > 8 {
                self.results.len() - 8
            } else {
                0
            };
            for (row_idx, result) in self.results[start..].iter().rev().enumerate() {
                let ry = table_y + 52.0 + row_idx as f32 * 26.0;
                let secs = result.duration_ms / 1000;

                cmds.push(RenderCommand::Text {
                    x: col_xs[0],
                    y: ry,
                    text: result.lesson_title.clone(),
                    color: Color::from_hex(COL_TEXT),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
                cmds.push(RenderCommand::Text {
                    x: col_xs[1],
                    y: ry,
                    text: format!("{:.0}", result.wpm),
                    color: Color::from_hex(COL_GREEN),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: col_xs[2],
                    y: ry,
                    text: format!("{:.1}%", result.accuracy),
                    color: Color::from_hex(COL_TEAL),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: col_xs[3],
                    y: ry,
                    text: format!("{}:{:02}", secs / 60, secs % 60),
                    color: Color::from_hex(COL_PEACH),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        cmds.push(RenderCommand::Text {
            x: 30.0,
            y: 500.0,
            text: String::from("Esc/Enter: Back to lessons"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn main() {
    let _app = TypingTutorApp::new();
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(key: Key, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text,
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
            text: Some(first_char),
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
            text: Some(first_char),
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
            text: Some('a'),
        });
        app.current_time_ms = 2000;
        app.handle_key(&KeyEvent {
            key: Key::B,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('b'),
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

    #[test]
    fn render_lesson_select() {
        let app = TypingTutorApp::new();
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Typing Tutor"));
        assert!(has_title);
    }

    #[test]
    fn render_typing_view() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
        // Should have the lesson title
        let title = &app.lessons[0].title;
        let has_lesson = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == title));
        assert!(has_lesson);
    }

    #[test]
    fn render_results_view() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Results;
        app.session = Some(TypingSession::new("test"));
        let cmds = app.render(600.0, 800.0);
        let has_complete = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Lesson Complete!"));
        assert!(has_complete);
    }

    #[test]
    fn render_statistics_empty() {
        let mut app = TypingTutorApp::new();
        app.view = AppView::Statistics;
        let cmds = app.render(600.0, 800.0);
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Statistics"));
        assert!(has_title);
        let has_empty_msg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("No lessons")));
        assert!(has_empty_msg);
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
        let cmds = app.render(600.0, 800.0);
        let has_recent = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Recent Results"));
        assert!(has_recent);
    }

    #[test]
    fn render_has_background() {
        let app = TypingTutorApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_typing_has_progress_bar() {
        let mut app = TypingTutorApp::new();
        app.start_lesson(0);
        let cmds = app.render(600.0, 800.0);
        // Should have the progress bar (two thin fill rects around y=80)
        let thin_rects = cmds.iter().filter(|c| matches!(c, RenderCommand::FillRect { height, y, .. } if *height == 6.0 && *y == 80.0)).count();
        assert!(thin_rects >= 2);
    }

    // --- Key released ignored ---

    #[test]
    fn key_released_ignored() {
        let mut app = TypingTutorApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
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

    // --- Set time ---

    #[test]
    fn set_time() {
        let mut app = TypingTutorApp::new();
        app.set_time(5000);
        assert_eq!(app.current_time_ms, 5000);
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
