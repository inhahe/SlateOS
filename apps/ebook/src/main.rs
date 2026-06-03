//! OurOS Ebook Reader
//!
//! A plain-text ebook reader with:
//! - Library view listing books with title, author, progress
//! - Paginated reading view with configurable font size and line spacing
//! - Page navigation (Left/Right, PageUp/PageDown, Home/End)
//! - Bookmarks (B to toggle, Ctrl+B to list, jump to bookmarked page)
//! - Reading progress with percentage and progress bar
//! - Table of contents from chapter separators (T key)
//! - Text search (/ to open, Enter to search, N/Shift+N for next/prev)
//! - Theme switching between dark (Catppuccin Mocha) and sepia (S key)
//! - Font size adjustment (+/- keys)
//! - Book metadata (word count, estimated reading time)
//!
//! Uses the guitk library for UI rendering.

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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;
#[allow(unused_imports)]
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

// ============================================================================
// Theme colors
// ============================================================================

/// A color theme for the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Sepia,
}

/// Resolved colors for a theme.
#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    pub background: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub highlight: Color,
    pub bookmark_color: Color,
    pub progress_bar: Color,
    pub progress_bg: Color,
    pub separator: Color,
    pub selected_bg: Color,
    pub error: Color,
}

impl ThemeColors {
    /// Catppuccin Mocha dark theme.
    pub const fn dark() -> Self {
        Self {
            background: Color::from_hex(0x1E1E2E),
            surface: Color::from_hex(0x313244),
            surface_alt: Color::from_hex(0x45475A),
            text: Color::from_hex(0xCDD6F4),
            text_dim: Color::from_hex(0xA6ADC8),
            accent: Color::from_hex(0x89B4FA),
            accent_dim: Color::from_hex(0x585B70),
            highlight: Color::rgba(249, 226, 175, 60),
            bookmark_color: Color::from_hex(0xF38BA8),
            progress_bar: Color::from_hex(0xA6E3A1),
            progress_bg: Color::from_hex(0x313244),
            separator: Color::from_hex(0x45475A),
            selected_bg: Color::from_hex(0x45475A),
            error: Color::from_hex(0xF38BA8),
        }
    }

    /// Warm sepia light theme.
    pub const fn sepia() -> Self {
        Self {
            background: Color::rgb(245, 235, 220),
            surface: Color::rgb(230, 218, 200),
            surface_alt: Color::rgb(215, 200, 180),
            text: Color::rgb(60, 50, 40),
            text_dim: Color::rgb(120, 105, 85),
            accent: Color::rgb(120, 80, 40),
            accent_dim: Color::rgb(180, 165, 145),
            highlight: Color::rgba(255, 200, 100, 80),
            bookmark_color: Color::rgb(180, 60, 60),
            progress_bar: Color::rgb(100, 140, 80),
            progress_bg: Color::rgb(215, 200, 180),
            separator: Color::rgb(200, 185, 165),
            selected_bg: Color::rgb(215, 200, 180),
            error: Color::rgb(180, 60, 60),
        }
    }

    pub const fn from_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Sepia => Self::sepia(),
        }
    }
}

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 700.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 32.0;
const PROGRESS_BAR_HEIGHT: f32 = 4.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const LIBRARY_ITEM_HEIGHT: f32 = 72.0;
const CORNER_RADIUS: f32 = 6.0;
const SMALL_RADIUS: f32 = 3.0;
const CONTENT_PADDING: f32 = 40.0;

/// Reading time estimate assumes 238 words per minute (average adult).
const READING_WPM: f32 = 238.0;

// ============================================================================
// Font size levels
// ============================================================================

/// Three font size levels for the reading view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSizeLevel {
    Small,
    Medium,
    Large,
}

impl FontSizeLevel {
    pub const fn font_size(self) -> f32 {
        match self {
            Self::Small => 14.0,
            Self::Medium => 18.0,
            Self::Large => 24.0,
        }
    }

    pub const fn line_height(self) -> f32 {
        match self {
            Self::Small => 22.0,
            Self::Medium => 28.0,
            Self::Large => 38.0,
        }
    }

    pub fn increase(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Large,
        }
    }

    pub fn decrease(self) -> Self {
        match self {
            Self::Small => Self::Small,
            Self::Medium => Self::Small,
            Self::Large => Self::Medium,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
}

// ============================================================================
// Chapter / TOC
// ============================================================================

/// A chapter entry in the table of contents.
#[derive(Clone, Debug)]
pub struct Chapter {
    /// Chapter title (first nonblank line after separator, or "Chapter N").
    pub title: String,
    /// Byte offset into the book text where this chapter starts.
    pub byte_offset: usize,
    /// Chapter index (0-based).
    pub index: usize,
}

/// Parse chapters from book text.
///
/// Chapters are delimited by `---` on a line by itself, or by double blank
/// lines. The first content before any separator is "Chapter 1" unless a
/// title line is found.
pub fn parse_chapters(text: &str) -> Vec<Chapter> {
    let mut chapters = Vec::new();
    let mut chapter_idx: usize = 0;

    // Always add the start of the text as chapter 0.
    let first_title = first_nonblank_line(text).unwrap_or("Chapter 1");
    chapters.push(Chapter {
        title: first_title.to_owned(),
        byte_offset: 0,
        index: chapter_idx,
    });
    chapter_idx = chapter_idx.saturating_add(1);

    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i: usize = 0;

    while i < len {
        // Check for `---` separator on its own line.
        if is_separator_line(text, i) {
            // Skip past the separator line.
            let sep_end = find_line_end(text, i);
            let title = first_nonblank_line(&text[sep_end..])
                .unwrap_or_else(|| default_chapter_title_leak(chapter_idx));
            chapters.push(Chapter {
                title: title.to_owned(),
                byte_offset: sep_end,
                index: chapter_idx,
            });
            chapter_idx = chapter_idx.saturating_add(1);
            i = sep_end;
            continue;
        }

        // Check for double blank line (two consecutive '\n\n' sequences).
        if is_double_blank(text, i) {
            // Skip the blank lines.
            let mut j = i;
            while j < len && (bytes.get(j).copied() == Some(b'\n') || bytes.get(j).copied() == Some(b'\r')) {
                j = j.saturating_add(1);
            }
            if j < len {
                let title = first_nonblank_line(&text[j..])
                    .unwrap_or_else(|| default_chapter_title_leak(chapter_idx));
                chapters.push(Chapter {
                    title: title.to_owned(),
                    byte_offset: j,
                    index: chapter_idx,
                });
                chapter_idx = chapter_idx.saturating_add(1);
            }
            i = j;
            continue;
        }

        i = find_line_end(text, i);
    }

    // Deduplicate chapters at the same offset (can happen if the first chapter
    // starts right at a separator).
    chapters.dedup_by_key(|c| c.byte_offset);
    chapters
}

/// Return a leaked &'static str for a default chapter title. This avoids
/// lifetime issues in the parser. The small number of chapters means
/// leaking is acceptable.
fn default_chapter_title_leak(idx: usize) -> &'static str {
    let s = format!("Chapter {}", idx.saturating_add(1));
    Box::leak(s.into_boxed_str())
}

/// Check if position `i` is the start of a `---` separator line.
fn is_separator_line(text: &str, pos: usize) -> bool {
    // Must be at start of text or right after a newline.
    if pos > 0 {
        let prev = text.as_bytes().get(pos.wrapping_sub(1)).copied();
        if prev != Some(b'\n') {
            return false;
        }
    }
    let rest = &text[pos..];
    if rest.starts_with("---") {
        // Remainder of line must be only dashes or whitespace until newline/EOF.
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..line_end];
        line.chars().all(|c| c == '-' || c.is_whitespace())
    } else {
        false
    }
}

/// Check if position `i` is at a double blank line (at least two consecutive
/// newlines beyond the current line).
fn is_double_blank(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    // Must be at a newline.
    if bytes.get(pos).copied() != Some(b'\n') {
        return false;
    }
    // Count consecutive newlines (allowing \r\n).
    let mut newline_count = 0u32;
    let mut j = pos;
    while j < bytes.len() {
        match bytes.get(j).copied() {
            Some(b'\n') => {
                newline_count = newline_count.saturating_add(1);
                j = j.saturating_add(1);
            }
            Some(b'\r') => {
                j = j.saturating_add(1);
            }
            _ => break,
        }
    }
    newline_count >= 3
}

/// Find the end of the line at `pos` (position after the trailing '\n').
fn find_line_end(text: &str, pos: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = pos;
    while i < bytes.len() && bytes.get(i).copied() != Some(b'\n') {
        i = i.saturating_add(1);
    }
    if i < bytes.len() {
        i.saturating_add(1)
    } else {
        i
    }
}

/// Get the first nonblank line from the given text slice.
fn first_nonblank_line(text: &str) -> Option<&str> {
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.chars().all(|c| c == '-') {
            return Some(trimmed);
        }
    }
    None
}

// ============================================================================
// Book metadata and model
// ============================================================================

/// Metadata for a book in the library.
#[derive(Clone, Debug)]
pub struct BookMeta {
    pub title: String,
    pub author: String,
}

/// A book with its full text content.
#[derive(Clone, Debug)]
pub struct Book {
    pub meta: BookMeta,
    pub text: String,
    pub chapters: Vec<Chapter>,
    pub word_count: usize,
}

impl Book {
    /// Create a new book, computing chapters and word count.
    pub fn new(title: &str, author: &str, text: &str) -> Self {
        let chapters = parse_chapters(text);
        let word_count = count_words(text);
        Self {
            meta: BookMeta {
                title: title.to_owned(),
                author: author.to_owned(),
            },
            text: text.to_owned(),
            chapters,
            word_count,
        }
    }

    /// Estimated reading time in minutes.
    pub fn reading_time_minutes(&self) -> f32 {
        self.word_count as f32 / READING_WPM
    }
}

/// Count words in text (split on whitespace).
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

// ============================================================================
// Pagination
// ============================================================================

/// Result of paginating a text.
#[derive(Clone, Debug)]
pub struct PaginatedBook {
    /// Each element is the byte range (start, end) into the book text for that page.
    pub pages: Vec<(usize, usize)>,
}

/// Paginate the book text given display parameters.
///
/// `chars_per_line` is the estimated number of characters fitting on one line,
/// and `lines_per_page` is the number of text lines fitting on one page.
pub fn paginate(text: &str, chars_per_line: usize, lines_per_page: usize) -> PaginatedBook {
    if text.is_empty() || chars_per_line == 0 || lines_per_page == 0 {
        return PaginatedBook { pages: vec![(0, 0)] };
    }

    let mut pages = Vec::new();
    let mut page_start: usize = 0;
    let text_len = text.len();

    while page_start < text_len {
        let mut line_count: usize = 0;
        let mut pos = page_start;

        while line_count < lines_per_page && pos < text_len {
            // Wrap one logical line.
            let line_end = next_line_break(text, pos);
            let line_text = &text[pos..line_end];
            let line_char_count = line_text.chars().count();

            if line_char_count == 0 {
                // Blank line.
                line_count = line_count.saturating_add(1);
                pos = line_end;
                continue;
            }

            // How many wrapped lines does this logical line produce? Use
            // `div_ceil` to get ceiling-division without the manual
            // (n + d - 1) / d dance (and to satisfy manual_checked_ops).
            let wrapped = if chars_per_line > 0 {
                line_char_count.div_ceil(chars_per_line).max(1)
            } else {
                1
            };

            if line_count.saturating_add(wrapped) > lines_per_page && line_count > 0 {
                // This line would exceed the page; break here.
                break;
            }

            line_count = line_count.saturating_add(wrapped);
            pos = line_end;
        }

        // If we didn't advance at all, force at least one character forward
        // to avoid infinite loops.
        if pos == page_start {
            pos = text.ceil_char_boundary(page_start.saturating_add(1)).min(text_len);
        }

        pages.push((page_start, pos));
        page_start = pos;
    }

    if pages.is_empty() {
        pages.push((0, 0));
    }

    PaginatedBook { pages }
}

/// Find the end of the current line (past the trailing newline if any).
fn next_line_break(text: &str, pos: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = pos;
    while i < bytes.len() {
        match bytes.get(i).copied() {
            Some(b'\n') => return i.saturating_add(1),
            _ => i = i.saturating_add(1),
        }
    }
    i
}

// ============================================================================
// Search
// ============================================================================

/// A search match in the book text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    /// Byte offset of the match start.
    pub byte_offset: usize,
    /// Length of the match in bytes.
    pub byte_len: usize,
}

/// Find all case-insensitive occurrences of `needle` in `haystack`.
pub fn find_all_matches(haystack: &str, needle: &str) -> Vec<SearchMatch> {
    if needle.is_empty() {
        return Vec::new();
    }

    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut matches = Vec::new();
    let mut start = 0usize;

    while let Some(pos) = lower_haystack[start..].find(&lower_needle) {
        let absolute = start.saturating_add(pos);
        matches.push(SearchMatch {
            byte_offset: absolute,
            byte_len: lower_needle.len(),
        });
        start = absolute.saturating_add(1);
    }

    matches
}

/// Determine which page a byte offset falls on.
pub fn page_for_offset(pages: &[(usize, usize)], byte_offset: usize) -> Option<usize> {
    for (i, &(start, end)) in pages.iter().enumerate() {
        if byte_offset >= start && byte_offset < end {
            return Some(i);
        }
    }
    // If offset is at the very end, it belongs to the last page.
    if !pages.is_empty() {
        let last = pages.len().saturating_sub(1);
        if byte_offset >= pages.get(last).map_or(0, |p| p.0) {
            return Some(last);
        }
    }
    None
}

// ============================================================================
// Sample books
// ============================================================================

/// Create the built-in sample library with 5 books of different genres.
pub fn sample_library() -> Vec<Book> {
    vec![
        Book::new(
            "The Clockwork Garden",
            "Eleanor Voss",
            "The Clockwork Garden\n\
             by Eleanor Voss\n\
             \n\
             Chapter 1: The Discovery\n\
             \n\
             Maren found the door behind the ivy on a Tuesday morning, just as the rain \
             began to thin. She had walked past the old stone wall a hundred times on her \
             way to the village market, but the creeping vines had always hidden whatever \
             lay beneath. Today, after a night of fierce wind, the ivy had peeled back \
             like a curtain being drawn.\n\
             \n\
             The door was made of dark iron, patterned with interlocking gears and tiny \
             engraved flowers. When she pressed her palm against it, the metal was warm, \
             as though something behind it were breathing. A low hum vibrated through \
             her fingers and up her wrist, a sound like a music box unwinding.\n\
             \n\
             She looked over her shoulder. The lane was empty, the cobblestones slick \
             with rain. No one would see if she opened it. No one would know.\n\
             \n\
             Maren turned the handle.\n\
             \n\
             ---\n\
             \n\
             Chapter 2: Beyond the Wall\n\
             \n\
             The garden on the other side was impossible. That was the only word for it. \
             Flowers bloomed in spirals of copper and glass, their petals ticking softly \
             as they opened and closed in rhythm. Paths of polished brass wound between \
             hedges of silver wire, and above it all a mechanical sun hung from chains, \
             casting warm amber light across the impossible landscape.\n\
             \n\
             Maren stepped forward carefully. The grass beneath her boots was real, soft \
             and green and damp, but everything else was crafted, engineered, built by \
             hands that understood both beauty and precision. A butterfly landed on her \
             sleeve. Its wings were painted porcelain, hinged with gold.\n\
             \n\
             In the center of the garden stood a clocktower, its face showing not hours \
             but seasons. The long hand pointed to spring. Maren walked toward it, drawn \
             by the ticking that seemed to match her heartbeat.\n\
             \n\
             At the base of the tower she found a journal, wrapped in oilcloth and tied \
             with string. The first page read: \"If you are reading this, the garden has \
             chosen you. Wind the key before midnight, or everything stops.\"\n\
             \n\
             ---\n\
             \n\
             Chapter 3: The Key\n\
             \n\
             Maren searched the garden for hours, turning over every brass leaf and \
             peering into every crystal bloom. The key was not under the fountain that \
             spouted liquid silver, nor inside the birdcage where mechanical larks sang \
             in three-part harmony. It was not buried in the soil, which hummed with \
             networks of copper wire.\n\
             \n\
             She found it, at last, growing on a vine like a fruit. A heavy iron key \
             shaped like a butterfly, its wings folded shut. When she plucked it, the \
             vine shivered and retracted into the wall.\n\
             \n\
             The clocktower door opened with a groan. Inside, gears the size of wagon \
             wheels turned in silent precision. At the center was a keyhole, and above \
             it, engraved in the iron: \"One winding lasts one year. Choose wisely.\"\n\
             \n\
             Maren inserted the key and turned it. The garden exhaled."
        ),
        Book::new(
            "Stars Over Quiet Water",
            "James Haruki",
            "Stars Over Quiet Water\n\
             by James Haruki\n\
             \n\
             Part One: The Lake\n\
             \n\
             The cabin had been his grandfather's, built with hands that knew wood \
             the way a musician knows silence: not as absence, but as the space where \
             meaning lives. Thomas arrived on the last day of September, carrying \
             nothing but a duffel bag and a heart full of questions he was not yet \
             brave enough to ask.\n\
             \n\
             Lake Heron stretched before him, flat as hammered pewter under the low \
             grey sky. The trees along the far shore were just beginning to turn, \
             gold edging into the green like a secret being told in stages. He sat \
             on the dock and let his feet hang over the water, the cold reaching up \
             through his shoes.\n\
             \n\
             His phone had no signal here. That was the point.\n\
             \n\
             For three days he did nothing. He chopped firewood because the nights \
             were cold. He boiled water from the well and drank black coffee on the \
             porch, watching the mist rise off the lake at dawn. He did not think \
             about the letter in the duffel bag, the one his grandfather had left \
             with the lawyers, the one addressed only to \"the one who comes back.\"\n\
             \n\
             On the fourth day, he opened it.\n\
             \n\
             ---\n\
             \n\
             Part Two: The Letter\n\
             \n\
             The handwriting was steady, the ink blue-black, the paper thick and \
             unlined. His grandfather had written the way he built: with care and \
             without waste.\n\
             \n\
             \"Thomas, if you're reading this, you've come to the lake. Good. The \
             city will try to keep you, but the water always calls you back, doesn't \
             it? That's not weakness. That's knowing where you belong.\n\
             \n\
             I have something to tell you that I could never say face to face. I \
             was afraid, and old men are allowed their fears, even if we pretend \
             otherwise. Under the third floorboard from the kitchen window, you'll \
             find a tin box. Inside it is the truth about your grandmother and me, \
             and about the year I spent away from this place.\n\
             \n\
             Read it by the water. The lake keeps secrets well.\"\n\
             \n\
             Thomas knelt on the kitchen floor, pried up the board, and found the \
             tin exactly where the letter promised. It was cold and heavy, as though \
             it held more than paper. He carried it to the dock and sat in his \
             grandfather's old chair, the one with the arm worn smooth from years \
             of resting hands.\n\
             \n\
             He opened it as the first stars appeared over the quiet water."
        ),
        Book::new(
            "A Brief History of Bread",
            "Sara Lindholm",
            "A Brief History of Bread\n\
             by Sara Lindholm\n\
             \n\
             Introduction: The First Loaf\n\
             \n\
             No one knows exactly when the first bread was baked, but the evidence \
             points to the Natufian people of the Levant, some 14,000 years ago, \
             millennia before the agricultural revolution that would transform human \
             civilization. They ground wild wheat and barley with stone tools, mixed \
             the coarse flour with water, and cooked flat cakes on heated rocks.\n\
             \n\
             This was not bread as we know it: no yeast, no rise, no crust brown \
             and crackled from an oven. It was dense and gritty and probably tasted \
             of stone dust and ash. But it was portable, it was caloric, and it \
             could be stored. In a world where every meal depended on the hunt or \
             the gather, bread was revolution.\n\
             \n\
             ---\n\
             \n\
             Chapter 1: Egypt and the Rise of Yeast\n\
             \n\
             The Egyptians discovered leavening by accident, around 3000 BCE. A \
             batch of dough left too long in the heat began to bubble, colonized \
             by wild yeast floating in the warm air. Some brave or desperate baker \
             decided to cook it anyway, and the result was transformative: a loaf \
             that was soft, airy, twice the size, and far easier to chew.\n\
             \n\
             Within a generation, leavened bread became the staple of Egyptian life. \
             Workers on the great pyramids were paid in bread and beer (which used \
             the same yeast), and bakeries became among the first specialized trades. \
             The Egyptians developed conical clay ovens that could reach temperatures \
             high enough to create a true crust, and they experimented with different \
             grains: emmer wheat, barley, and eventually the free-threshing bread \
             wheat that would become the foundation of Western baking.\n\
             \n\
             Bread was food, but it was also status. White bread, made from finely \
             sifted flour, was reserved for priests and nobles. The common people \
             ate coarse brown bread. This distinction would persist for thousands \
             of years, across cultures and continents, a quiet class marker baked \
             into every loaf.\n\
             \n\
             ---\n\
             \n\
             Chapter 2: Rome and Industrial Scale\n\
             \n\
             The Romans elevated bread making from craft to industry. By the first \
             century CE, Rome had over 300 commercial bakeries serving a population \
             of roughly one million. The state regulated grain supply, subsidized \
             flour distribution, and eventually provided free bread to citizens: \
             the famous \"bread and circuses\" that Juvenal would satirize.\n\
             \n\
             Roman bakers used water mills to grind grain at unprecedented scale \
             and developed the first standardized loaf shapes. The round, scored \
             loaf found preserved in the ruins of Pompeii looks remarkably like a \
             modern artisan boule. They added olive oil, honey, and even cheese to \
             their doughs, creating the ancestors of focaccia and pizza."
        ),
        Book::new(
            "The Probability Engine",
            "Marcus Chen",
            "The Probability Engine\n\
             by Marcus Chen\n\
             \n\
             1. Bootstrap\n\
             \n\
             The machine was ugly. That was the first thing Dr. Kira Nwosu noticed when \
             she entered Lab 7 on the morning her life changed. It looked like someone \
             had welded three server racks to a particle accelerator and wrapped the whole \
             thing in Christmas lights. Cables snaked across the floor. A faint smell of \
             ozone hung in the air.\n\
             \n\
             \"It works,\" said David, her postdoc, without looking up from his terminal. \
             He said it the way someone might say \"the sun came up\" or \"water is wet.\" \
             Flatly. As though the impossible were merely inevitable.\n\
             \n\
             Kira set down her coffee. \"Define 'works.'\"\n\
             \n\
             David finally looked at her. His eyes were red from a night without sleep, \
             but they were bright. \"I gave it a quantum random number generator. Truly \
             random, hardware-level entropy. Then I asked it to predict the next thousand \
             outputs.\" He paused. \"It got nine hundred and ninety-seven right.\"\n\
             \n\
             \"That's not possible.\"\n\
             \n\
             \"I know.\"\n\
             \n\
             ---\n\
             \n\
             2. Calibration\n\
             \n\
             They spent the next week running tests. The machine, which David had built \
             from a design he claimed came to him in a dream (a claim Kira chose not to \
             interrogate too deeply), could predict outcomes of genuinely random processes \
             with 99.7% accuracy. It could not predict the remaining 0.3%. This was, \
             David insisted, a feature, not a bug.\n\
             \n\
             \"True determinism would mean we're living in a simulation,\" he said over \
             lunch in the campus cafeteria. \"The fact that it misses sometimes means \
             reality has genuine randomness. The machine just sees further into the \
             probability wave than anything we've built before.\"\n\
             \n\
             Kira pushed her salad around her plate. \"You realize what this means for \
             cryptography? For financial markets? For every system built on the assumption \
             that randomness is actually random?\"\n\
             \n\
             \"Yes.\"\n\
             \n\
             \"And you built it in a university lab with grant money meant for quantum \
             computing research.\"\n\
             \n\
             \"Technically, this is quantum computing research.\"\n\
             \n\
             ---\n\
             \n\
             3. Containment\n\
             \n\
             The university's legal department got involved on day twelve. The dean called \
             Kira into her office, where two people in suits were already seated. They did \
             not introduce themselves. They asked questions for three hours. At the end, \
             one of them said, \"We appreciate your cooperation, Dr. Nwosu. You'll \
             understand that this technology has national security implications.\"\n\
             \n\
             \"I understand that you're going to classify my postdoc's invention and bury \
             it,\" Kira said.\n\
             \n\
             The suited woman almost smiled. \"We prefer 'secure.' But the practical \
             outcome is similar, yes.\"\n\
             \n\
             That night Kira sat in her office, staring at the wall. David had already \
             made a backup of the design on a flash drive that he kept in his shoe. She \
             knew this because he had told her, grinning, as though it were a joke. She \
             wasn't sure it was."
        ),
        Book::new(
            "Sonnets from the Edge",
            "Amara Okafor",
            "Sonnets from the Edge\n\
             by Amara Okafor\n\
             \n\
             I. Morning\n\
             \n\
             The light comes in oblique through curtain lace,\n\
             and traces patterns on the bedroom floor.\n\
             A geometry of sun and woven thread,\n\
             as perfect as the day that came before.\n\
             I lie awake and listen to the house:\n\
             the creak of wood expanding in the heat,\n\
             the distant hum of traffic on the bridge,\n\
             the slow percussion of my own heartbeat.\n\
             Another day assembles piece by piece,\n\
             familiar as a sentence I have read\n\
             a thousand times, yet cannot memorize.\n\
             The coffee cools. The cat sleeps on the bed.\n\
             I rise because the morning asks me to,\n\
             not knowing what the afternoon will do.\n\
             \n\
             ---\n\
             \n\
             II. The Commute\n\
             \n\
             We pack ourselves in carriages of steel,\n\
             a hundred strangers practicing the art\n\
             of looking nowhere, touching not at all,\n\
             each body an island, each face a chart\n\
             of private weather: storms behind the eyes,\n\
             calm seas along the set line of the jaw.\n\
             We sway together when the train curves left,\n\
             a brief choreography without a score.\n\
             A woman reads. A child presses her nose\n\
             against the glass and watches houses fly.\n\
             A man in grey stares at his phone and sighs.\n\
             The tunnel swallows us. We do not cry.\n\
             We surface into light and scatter wide,\n\
             each carrying a darkness tucked inside.\n\
             \n\
             ---\n\
             \n\
             III. Kitchen, After Midnight\n\
             \n\
             The kitchen holds the memory of meals:\n\
             garlic and butter layered in the walls,\n\
             the ghost of bread still warm upon the board,\n\
             a wineglass rinsed but not yet put away.\n\
             I stand barefoot on tile and drink cold water,\n\
             the house so quiet I can hear the clock\n\
             unwinding in the hallway, tick by tick,\n\
             each second certain, purposeful, exact.\n\
             What keeps me up is nothing I can name,\n\
             no crisis, no regret, no urgent thought,\n\
             just consciousness persisting past its cue,\n\
             a light left on in rooms where no one reads.\n\
             I rinse the glass. I turn the kitchen dark.\n\
             I carry silence with me to the bed.\n\
             \n\
             ---\n\
             \n\
             IV. To the Architect of Small Things\n\
             \n\
             Consider the hinge, the latch, the simple hook:\n\
             the engineering of the unremarked.\n\
             Someone designed the angle of this door,\n\
             the weight that lets it close but not slam shut.\n\
             Someone considered clearance, sweep, and arc,\n\
             and chose a metal that would last through years\n\
             of hands that never pause to wonder why\n\
             a door behaves exactly as it should.\n\
             This is the dignity of quiet craft:\n\
             not bridges or cathedrals, but the catch\n\
             that holds a window open on a breeze,\n\
             the hinge that bears the weight and does not creak.\n\
             Praise the anonymous and patient hand\n\
             that made the small things work as they were planned."
        ),
    ]
}

// ============================================================================
// Reading state
// ============================================================================

/// Per-book reading state stored in the library.
#[derive(Clone, Debug)]
pub struct ReadingState {
    pub current_page: usize,
    pub bookmarks: Vec<usize>,
    pub font_size: FontSizeLevel,
}

impl ReadingState {
    pub fn new() -> Self {
        Self {
            current_page: 0,
            bookmarks: Vec::new(),
            font_size: FontSizeLevel::Medium,
        }
    }

    /// Toggle bookmark on the given page.
    pub fn toggle_bookmark(&mut self, page: usize) {
        if let Some(idx) = self.bookmarks.iter().position(|&p| p == page) {
            self.bookmarks.remove(idx);
        } else {
            self.bookmarks.push(page);
            self.bookmarks.sort_unstable();
        }
    }

    /// Check whether a page is bookmarked.
    pub fn is_bookmarked(&self, page: usize) -> bool {
        self.bookmarks.contains(&page)
    }
}

impl Default for ReadingState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Application view state
// ============================================================================

/// Top-level view the application is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppView {
    Library,
    Reading,
    TableOfContents,
    BookmarkList,
    Search,
}

// ============================================================================
// Main application struct
// ============================================================================

/// The ebook reader application.
pub struct EbookApp {
    pub library: Vec<Book>,
    pub reading_states: Vec<ReadingState>,
    pub selected_book: usize,
    pub view: AppView,
    pub theme: ThemeKind,
    pub paginated: Option<PaginatedBook>,

    // Search state
    pub search_query: String,
    pub search_active: bool,
    pub search_matches: Vec<SearchMatch>,
    pub current_match_idx: Option<usize>,

    // TOC / bookmark list selection
    pub list_selection: usize,

    // Window dimensions for layout
    pub window_width: f32,
    pub window_height: f32,
}

impl EbookApp {
    /// Create a new ebook reader with the sample library.
    pub fn new() -> Self {
        let library = sample_library();
        let reading_states = library.iter().map(|_| ReadingState::new()).collect();
        Self {
            library,
            reading_states,
            selected_book: 0,
            view: AppView::Library,
            theme: ThemeKind::Dark,
            paginated: None,
            search_query: String::new(),
            search_active: false,
            search_matches: Vec::new(),
            current_match_idx: None,
            list_selection: 0,
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
        }
    }

    // --------------------------------------------------------------------
    // Pagination helpers
    // --------------------------------------------------------------------

    /// Compute how many characters fit per line and lines per page given
    /// current font size and window dimensions.
    pub fn layout_params(&self) -> (usize, usize) {
        let font_size = self.current_font_size().font_size();
        let line_h = self.current_font_size().line_height();
        let content_width = self.window_width - 2.0 * CONTENT_PADDING;
        let content_height = self.window_height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT - 2.0 * CONTENT_PADDING;

        // Estimate characters per line: typical character is roughly 0.5 * font_size wide.
        let char_width = font_size * 0.5;
        let chars_per_line = if char_width > 0.0 {
            (content_width / char_width) as usize
        } else {
            80
        };
        let lines_per_page = if line_h > 0.0 {
            (content_height / line_h) as usize
        } else {
            25
        };

        (chars_per_line.max(1), lines_per_page.max(1))
    }

    /// Re-paginate the current book.
    pub fn repaginate(&mut self) {
        if let Some(book) = self.library.get(self.selected_book) {
            let (cpl, lpp) = self.layout_params();
            self.paginated = Some(paginate(&book.text, cpl, lpp));
        }
    }

    /// Get the current book if one is selected.
    pub fn current_book(&self) -> Option<&Book> {
        self.library.get(self.selected_book)
    }

    /// Get the current reading state.
    pub fn current_reading_state(&self) -> Option<&ReadingState> {
        self.reading_states.get(self.selected_book)
    }

    /// Get mutable reading state.
    pub fn current_reading_state_mut(&mut self) -> Option<&mut ReadingState> {
        self.reading_states.get_mut(self.selected_book)
    }

    /// Get current font size level.
    pub fn current_font_size(&self) -> FontSizeLevel {
        self.current_reading_state()
            .map_or(FontSizeLevel::Medium, |s| s.font_size)
    }

    /// Total page count for the current book.
    pub fn total_pages(&self) -> usize {
        self.paginated.as_ref().map_or(1, |p| p.pages.len().max(1))
    }

    /// Current page number (0-based).
    pub fn current_page(&self) -> usize {
        self.current_reading_state().map_or(0, |s| s.current_page)
    }

    /// Reading progress as a fraction 0.0..=1.0.
    pub fn reading_progress(&self) -> f32 {
        let total = self.total_pages();
        if total <= 1 {
            return if self.current_page() == 0 { 0.0 } else { 1.0 };
        }
        self.current_page() as f32 / (total.saturating_sub(1)) as f32
    }

    /// Reading progress as a percentage string.
    pub fn reading_progress_pct(&self) -> String {
        format!("{:.0}%", self.reading_progress() * 100.0)
    }

    // --------------------------------------------------------------------
    // Navigation
    // --------------------------------------------------------------------

    /// Go to the next page.
    pub fn next_page(&mut self) {
        let total = self.total_pages();
        if let Some(state) = self.current_reading_state_mut()
            && state.current_page < total.saturating_sub(1) {
                state.current_page = state.current_page.saturating_add(1);
            }
    }

    /// Go to the previous page.
    pub fn prev_page(&mut self) {
        if let Some(state) = self.current_reading_state_mut() {
            state.current_page = state.current_page.saturating_sub(1);
        }
    }

    /// Jump to a specific page.
    pub fn go_to_page(&mut self, page: usize) {
        let total = self.total_pages();
        if let Some(state) = self.current_reading_state_mut() {
            state.current_page = page.min(total.saturating_sub(1));
        }
    }

    /// Go to the first page.
    pub fn go_to_first_page(&mut self) {
        self.go_to_page(0);
    }

    /// Go to the last page.
    pub fn go_to_last_page(&mut self) {
        let last = self.total_pages().saturating_sub(1);
        self.go_to_page(last);
    }

    // --------------------------------------------------------------------
    // Bookmarks
    // --------------------------------------------------------------------

    /// Toggle bookmark on the current page.
    pub fn toggle_bookmark(&mut self) {
        let page = self.current_page();
        if let Some(state) = self.current_reading_state_mut() {
            state.toggle_bookmark(page);
        }
    }

    /// Get bookmarks for the current book.
    pub fn bookmarks(&self) -> Vec<usize> {
        self.current_reading_state()
            .map_or_else(Vec::new, |s| s.bookmarks.clone())
    }

    // --------------------------------------------------------------------
    // Font size
    // --------------------------------------------------------------------

    /// Increase font size.
    pub fn increase_font_size(&mut self) {
        if let Some(state) = self.current_reading_state_mut() {
            state.font_size = state.font_size.increase();
        }
        self.repaginate();
    }

    /// Decrease font size.
    pub fn decrease_font_size(&mut self) {
        if let Some(state) = self.current_reading_state_mut() {
            state.font_size = state.font_size.decrease();
        }
        self.repaginate();
    }

    // --------------------------------------------------------------------
    // Theme
    // --------------------------------------------------------------------

    /// Toggle between dark and sepia themes.
    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            ThemeKind::Dark => ThemeKind::Sepia,
            ThemeKind::Sepia => ThemeKind::Dark,
        };
    }

    /// Get current theme colors.
    pub fn theme_colors(&self) -> ThemeColors {
        ThemeColors::from_kind(self.theme)
    }

    // --------------------------------------------------------------------
    // Search
    // --------------------------------------------------------------------

    /// Open the search bar.
    pub fn open_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.search_matches.clear();
        self.current_match_idx = None;
    }

    /// Close the search bar.
    pub fn close_search(&mut self) {
        self.search_active = false;
    }

    /// Execute a search with the current query.
    pub fn execute_search(&mut self) {
        if let Some(book) = self.library.get(self.selected_book) {
            self.search_matches = find_all_matches(&book.text, &self.search_query);
            if self.search_matches.is_empty() {
                self.current_match_idx = None;
            } else {
                self.current_match_idx = Some(0);
                // Jump to the page containing the first match.
                if let Some(paginated) = &self.paginated
                    && let Some(first) = self.search_matches.first()
                        && let Some(page) = page_for_offset(&paginated.pages, first.byte_offset) {
                            self.go_to_page(page);
                        }
            }
        }
    }

    /// Go to the next search match.
    pub fn next_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let next = match self.current_match_idx {
            Some(i) => (i.saturating_add(1)) % self.search_matches.len(),
            None => 0,
        };
        self.current_match_idx = Some(next);
        if let Some(paginated) = &self.paginated
            && let Some(m) = self.search_matches.get(next)
                && let Some(page) = page_for_offset(&paginated.pages, m.byte_offset) {
                    self.go_to_page(page);
                }
    }

    /// Go to the previous search match.
    pub fn prev_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let prev = match self.current_match_idx {
            Some(0) => self.search_matches.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.current_match_idx = Some(prev);
        if let Some(paginated) = &self.paginated
            && let Some(m) = self.search_matches.get(prev)
                && let Some(page) = page_for_offset(&paginated.pages, m.byte_offset) {
                    self.go_to_page(page);
                }
    }

    // --------------------------------------------------------------------
    // Book opening / TOC / library
    // --------------------------------------------------------------------

    /// Open a book for reading by its library index.
    pub fn open_book(&mut self, index: usize) {
        if index < self.library.len() {
            self.selected_book = index;
            self.view = AppView::Reading;
            self.repaginate();
            self.close_search();
        }
    }

    /// Return to the library view.
    pub fn return_to_library(&mut self) {
        self.view = AppView::Library;
        self.close_search();
    }

    /// Show the table of contents overlay.
    pub fn show_toc(&mut self) {
        self.list_selection = 0;
        self.view = AppView::TableOfContents;
    }

    /// Show the bookmark list overlay.
    pub fn show_bookmark_list(&mut self) {
        self.list_selection = 0;
        self.view = AppView::BookmarkList;
    }

    /// Jump to a chapter by its byte offset.
    pub fn jump_to_chapter(&mut self, chapter_idx: usize) {
        if let Some(book) = self.library.get(self.selected_book)
            && let Some(chapter) = book.chapters.get(chapter_idx)
                && let Some(paginated) = &self.paginated
                    && let Some(page) = page_for_offset(&paginated.pages, chapter.byte_offset) {
                        self.go_to_page(page);
                    }
        self.view = AppView::Reading;
    }

    /// Jump to a bookmarked page (from the bookmark list).
    pub fn jump_to_bookmark(&mut self, bookmark_idx: usize) {
        let bookmarks = self.bookmarks();
        if let Some(&page) = bookmarks.get(bookmark_idx) {
            self.go_to_page(page);
        }
        self.view = AppView::Reading;
    }

    /// Add a new book to the library.
    pub fn add_book(&mut self, title: &str, author: &str, text: &str) {
        self.library.push(Book::new(title, author, text));
        self.reading_states.push(ReadingState::new());
    }

    // --------------------------------------------------------------------
    // Get page text
    // --------------------------------------------------------------------

    /// Get the text content for the current page.
    pub fn current_page_text(&self) -> &str {
        if let Some(book) = self.library.get(self.selected_book)
            && let Some(paginated) = &self.paginated
                && let Some(&(start, end)) = paginated.pages.get(self.current_page()) {
                    let safe_start = start.min(book.text.len());
                    let safe_end = end.min(book.text.len());
                    return &book.text[safe_start..safe_end];
                }
        ""
    }

    // --------------------------------------------------------------------
    // Event handling
    // --------------------------------------------------------------------

    /// Handle a keyboard event. Returns true if the event was consumed.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }

        match self.view {
            AppView::Library => self.handle_library_key(event),
            AppView::Reading => {
                if self.search_active {
                    self.handle_search_key(event)
                } else {
                    self.handle_reading_key(event)
                }
            }
            AppView::TableOfContents => self.handle_toc_key(event),
            AppView::BookmarkList => self.handle_bookmark_list_key(event),
            AppView::Search => self.handle_search_key(event),
        }
    }

    fn handle_library_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Up => {
                if self.selected_book > 0 {
                    self.selected_book = self.selected_book.saturating_sub(1);
                }
                true
            }
            Key::Down => {
                if self.selected_book < self.library.len().saturating_sub(1) {
                    self.selected_book = self.selected_book.saturating_add(1);
                }
                true
            }
            Key::Enter => {
                self.open_book(self.selected_book);
                true
            }
            Key::Escape => {
                // No-op at library level.
                false
            }
            _ => false,
        }
    }

    fn handle_reading_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Right | Key::PageDown => {
                self.next_page();
                true
            }
            Key::Left | Key::PageUp => {
                self.prev_page();
                true
            }
            Key::Home => {
                self.go_to_first_page();
                true
            }
            Key::End => {
                self.go_to_last_page();
                true
            }
            Key::B => {
                if event.modifiers.ctrl {
                    self.show_bookmark_list();
                } else {
                    self.toggle_bookmark();
                }
                true
            }
            Key::T => {
                self.show_toc();
                true
            }
            Key::S => {
                self.toggle_theme();
                true
            }
            Key::Slash => {
                self.open_search();
                true
            }
            Key::N => {
                if event.modifiers.shift {
                    self.prev_match();
                } else {
                    self.next_match();
                }
                true
            }
            Key::Equals => {
                // '+' (Shift+Equals on US layout) or '=' for font increase
                self.increase_font_size();
                true
            }
            Key::Minus => {
                self.decrease_font_size();
                true
            }
            Key::Escape => {
                self.return_to_library();
                true
            }
            _ => false,
        }
    }

    fn handle_search_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Escape => {
                self.close_search();
                true
            }
            Key::Enter => {
                self.execute_search();
                true
            }
            Key::Backspace => {
                self.search_query.pop();
                true
            }
            _ => {
                // If the key produces a character, add it to the query.
                if let Some(ch) = event.text
                    && !ch.is_control() {
                        self.search_query.push(ch);
                        return true;
                    }
                false
            }
        }
    }

    fn handle_toc_key(&mut self, event: &KeyEvent) -> bool {
        let chapter_count = self.current_book().map_or(0, |b| b.chapters.len());
        match event.key {
            Key::Up => {
                if self.list_selection > 0 {
                    self.list_selection = self.list_selection.saturating_sub(1);
                }
                true
            }
            Key::Down => {
                if self.list_selection < chapter_count.saturating_sub(1) {
                    self.list_selection = self.list_selection.saturating_add(1);
                }
                true
            }
            Key::Enter => {
                self.jump_to_chapter(self.list_selection);
                true
            }
            Key::Escape => {
                self.view = AppView::Reading;
                true
            }
            _ => false,
        }
    }

    fn handle_bookmark_list_key(&mut self, event: &KeyEvent) -> bool {
        let bookmark_count = self.bookmarks().len();
        match event.key {
            Key::Up => {
                if self.list_selection > 0 {
                    self.list_selection = self.list_selection.saturating_sub(1);
                }
                true
            }
            Key::Down => {
                if self.list_selection < bookmark_count.saturating_sub(1) {
                    self.list_selection = self.list_selection.saturating_add(1);
                }
                true
            }
            Key::Enter => {
                self.jump_to_bookmark(self.list_selection);
                true
            }
            Key::Escape => {
                self.view = AppView::Reading;
                true
            }
            _ => false,
        }
    }

    /// Handle a mouse click. Returns true if consumed.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> bool {
        if let MouseEventKind::Press(MouseButton::Left) = &event.kind {
            match self.view {
                AppView::Library => {
                    // Check if a library item was clicked.
                    let y = event.y - TOOLBAR_HEIGHT;
                    if y >= 0.0 {
                        let idx = (y / LIBRARY_ITEM_HEIGHT) as usize;
                        if idx < self.library.len() {
                            self.selected_book = idx;
                            self.open_book(idx);
                            return true;
                        }
                    }
                }
                AppView::Reading => {
                    // Click on left/right half to navigate pages.
                    let mid = self.window_width / 2.0;
                    if event.y > TOOLBAR_HEIGHT && event.y < self.window_height - STATUS_BAR_HEIGHT {
                        if event.x < mid {
                            self.prev_page();
                        } else {
                            self.next_page();
                        }
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // --------------------------------------------------------------------
    // Rendering
    // --------------------------------------------------------------------

    /// Render the current view into a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let tc = self.theme_colors();
        let mut cmds = Vec::new();

        // Background fill.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: tc.background,
            corner_radii: CornerRadii::ZERO,
        });

        match self.view {
            AppView::Library => self.render_library(&tc, &mut cmds),
            AppView::Reading => self.render_reading(&tc, &mut cmds),
            AppView::TableOfContents => {
                self.render_reading(&tc, &mut cmds);
                self.render_toc_overlay(&tc, &mut cmds);
            }
            AppView::BookmarkList => {
                self.render_reading(&tc, &mut cmds);
                self.render_bookmark_list_overlay(&tc, &mut cmds);
            }
            AppView::Search => self.render_reading(&tc, &mut cmds),
        }

        cmds
    }

    fn render_library(&self, tc: &ThemeColors, cmds: &mut Vec<RenderCommand>) {
        // Toolbar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: tc.surface,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 12.0,
            text: "Library".to_owned(),
            color: tc.text,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: self.window_width - 200.0,
            y: 14.0,
            text: format!("{} books", self.library.len()),
            color: tc.text_dim,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: tc.separator,
            width: 1.0,
        });

        // Book list
        let list_top = TOOLBAR_HEIGHT;
        for (i, book) in self.library.iter().enumerate() {
            let y = list_top + (i as f32) * LIBRARY_ITEM_HEIGHT;
            let is_selected = i == self.selected_book;

            // Selection highlight
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y,
                    width: self.window_width,
                    height: LIBRARY_ITEM_HEIGHT,
                    color: tc.selected_bg,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Title
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: y + 12.0,
                text: book.meta.title.clone(),
                color: if is_selected { tc.accent } else { tc.text },
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.window_width - 200.0),
            });

            // Author
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: y + 32.0,
                text: book.meta.author.clone(),
                color: tc.text_dim,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.window_width - 200.0),
            });

            // Progress and word count
            let rs = self.reading_states.get(i);
            let progress = rs.map_or(0, |s| {
                if let Some(ref pag) = self.paginated {
                    if i == self.selected_book {
                        let total = pag.pages.len().max(1);
                        (s.current_page * 100) / total
                    } else {
                        0
                    }
                } else {
                    0
                }
            });
            let reading_min = book.reading_time_minutes();
            let info = format!(
                "{} words | {:.0} min | {}%",
                book.word_count, reading_min, progress
            );
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: y + 50.0,
                text: info,
                color: tc.accent_dim,
                font_size: 12.0,
                font_weight: FontWeightHint::Light,
                max_width: Some(self.window_width - 200.0),
            });

            // Separator between items
            cmds.push(RenderCommand::Line {
                x1: 16.0,
                y1: y + LIBRARY_ITEM_HEIGHT - 1.0,
                x2: self.window_width - 16.0,
                y2: y + LIBRARY_ITEM_HEIGHT - 1.0,
                color: tc.separator,
                width: 0.5,
            });
        }
    }

    fn render_reading(&self, tc: &ThemeColors, cmds: &mut Vec<RenderCommand>) {
        let book = match self.current_book() {
            Some(b) => b,
            None => return,
        };
        let font_lvl = self.current_font_size();
        let font_sz = font_lvl.font_size();
        let line_h = font_lvl.line_height();

        // -- Toolbar --
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: tc.surface,
            corner_radii: CornerRadii::ZERO,
        });

        // Back arrow
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "<".to_owned(),
            color: tc.accent,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Book title in toolbar
        cmds.push(RenderCommand::Text {
            x: 36.0,
            y: 12.0,
            text: book.meta.title.clone(),
            color: tc.text,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.window_width - 250.0),
        });

        // Font size indicator
        cmds.push(RenderCommand::Text {
            x: self.window_width - 180.0,
            y: 14.0,
            text: format!("Font: {}", font_lvl.label()),
            color: tc.text_dim,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bookmark indicator
        if self.current_reading_state().is_some_and(|s| s.is_bookmarked(self.current_page())) {
            cmds.push(RenderCommand::Text {
                x: self.window_width - 80.0,
                y: 12.0,
                text: "BM".to_owned(),
                color: tc.bookmark_color,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Theme indicator
        let theme_label = match self.theme {
            ThemeKind::Dark => "Dark",
            ThemeKind::Sepia => "Sepia",
        };
        cmds.push(RenderCommand::Text {
            x: self.window_width - 40.0,
            y: 14.0,
            text: theme_label.to_owned(),
            color: tc.text_dim,
            font_size: 11.0,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });

        // Toolbar separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: tc.separator,
            width: 1.0,
        });

        // -- Page content --
        let page_text = self.current_page_text();
        let content_x = CONTENT_PADDING;
        let content_y = TOOLBAR_HEIGHT + CONTENT_PADDING;
        let max_text_width = self.window_width - 2.0 * CONTENT_PADDING;

        let mut y_offset = content_y;
        for line in page_text.lines() {
            cmds.push(RenderCommand::Text {
                x: content_x,
                y: y_offset,
                text: line.to_owned(),
                color: tc.text,
                font_size: font_sz,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_text_width),
            });
            y_offset += line_h;
        }

        // -- Search highlight overlay --
        if !self.search_matches.is_empty()
            && let Some(paginated) = &self.paginated
                && let Some(&(page_start, page_end)) = paginated.pages.get(self.current_page()) {
                    // Highlight matches that fall on this page.
                    for (mi, m) in self.search_matches.iter().enumerate() {
                        if m.byte_offset >= page_start && m.byte_offset < page_end {
                            let is_current = self.current_match_idx == Some(mi);
                            let highlight_color = if is_current {
                                tc.accent
                            } else {
                                tc.highlight
                            };
                            // Approximate y position: count newlines from page_start to match.
                            let text_before = &book.text[page_start..m.byte_offset.min(book.text.len())];
                            let line_idx = text_before.chars().filter(|&c| c == '\n').count();
                            let hy = content_y + (line_idx as f32) * line_h;
                            // Approximate x from the last newline.
                            let last_nl = text_before.rfind('\n').map_or(0, |p| p.saturating_add(1));
                            let chars_before = text_before[last_nl..].chars().count();
                            let char_w = font_sz * 0.5;
                            let hx = content_x + (chars_before as f32) * char_w;
                            let hw = (m.byte_len as f32) * char_w;
                            cmds.push(RenderCommand::FillRect {
                                x: hx,
                                y: hy,
                                width: hw,
                                height: line_h,
                                color: highlight_color,
                                corner_radii: CornerRadii::all(2.0),
                            });
                        }
                    }
                }

        // -- Search bar --
        if self.search_active {
            let bar_y = self.window_height - STATUS_BAR_HEIGHT - 36.0;
            cmds.push(RenderCommand::FillRect {
                x: CONTENT_PADDING,
                y: bar_y,
                width: self.window_width - 2.0 * CONTENT_PADDING,
                height: 30.0,
                color: tc.surface,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: CONTENT_PADDING,
                y: bar_y,
                width: self.window_width - 2.0 * CONTENT_PADDING,
                height: 30.0,
                color: tc.accent,
                line_width: 1.0,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            let search_display = format!("/{}", self.search_query);
            cmds.push(RenderCommand::Text {
                x: CONTENT_PADDING + 8.0,
                y: bar_y + 8.0,
                text: search_display,
                color: tc.text,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.window_width - 2.0 * CONTENT_PADDING - 120.0),
            });

            // Match count
            if !self.search_matches.is_empty() {
                let match_info = match self.current_match_idx {
                    Some(idx) => format!("{}/{}", idx.saturating_add(1), self.search_matches.len()),
                    None => format!("{} matches", self.search_matches.len()),
                };
                cmds.push(RenderCommand::Text {
                    x: self.window_width - CONTENT_PADDING - 100.0,
                    y: bar_y + 8.0,
                    text: match_info,
                    color: tc.text_dim,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // -- Status bar with progress --
        self.render_status_bar(tc, cmds);
    }

    fn render_status_bar(&self, tc: &ThemeColors, cmds: &mut Vec<RenderCommand>) {
        let bar_y = self.window_height - STATUS_BAR_HEIGHT;

        // Status bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: tc.surface,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y,
            x2: self.window_width,
            y2: bar_y,
            color: tc.separator,
            width: 1.0,
        });

        // Page info
        let page_info = format!(
            "Page {} of {}",
            self.current_page().saturating_add(1),
            self.total_pages()
        );
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: bar_y + 9.0,
            text: page_info,
            color: tc.text_dim,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress percentage
        let pct = self.reading_progress_pct();
        cmds.push(RenderCommand::Text {
            x: self.window_width - 60.0,
            y: bar_y + 9.0,
            text: pct,
            color: tc.text_dim,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress bar
        let bar_width = self.window_width - 200.0;
        let bar_x = 120.0;
        let bar_inner_y = bar_y + (STATUS_BAR_HEIGHT - PROGRESS_BAR_HEIGHT) / 2.0;

        // Background track
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_inner_y,
            width: bar_width,
            height: PROGRESS_BAR_HEIGHT,
            color: tc.progress_bg,
            corner_radii: CornerRadii::all(2.0),
        });

        // Filled portion
        let filled_width = bar_width * self.reading_progress();
        if filled_width > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_inner_y,
                width: filled_width,
                height: PROGRESS_BAR_HEIGHT,
                color: tc.progress_bar,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    fn render_toc_overlay(&self, tc: &ThemeColors, cmds: &mut Vec<RenderCommand>) {
        let book = match self.current_book() {
            Some(b) => b,
            None => return,
        };

        let overlay_w = 400.0f32;
        let overlay_h = 500.0f32.min(self.window_height - 80.0);
        let overlay_x = (self.window_width - overlay_w) / 2.0;
        let overlay_y = (self.window_height - overlay_h) / 2.0;

        // Dim background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::ZERO,
        });

        // Overlay card
        cmds.push(RenderCommand::FillRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: tc.surface,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: tc.separator,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: overlay_x + 16.0,
            y: overlay_y + 14.0,
            text: "Table of Contents".to_owned(),
            color: tc.accent,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(overlay_w - 32.0),
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: overlay_x + 12.0,
            y1: overlay_y + 38.0,
            x2: overlay_x + overlay_w - 12.0,
            y2: overlay_y + 38.0,
            color: tc.separator,
            width: 1.0,
        });

        // Chapter list
        let item_h = 32.0f32;
        let list_top = overlay_y + 44.0;
        for (i, chapter) in book.chapters.iter().enumerate() {
            let iy = list_top + (i as f32) * item_h;
            if iy + item_h > overlay_y + overlay_h {
                break;
            }

            if i == self.list_selection {
                cmds.push(RenderCommand::FillRect {
                    x: overlay_x + 4.0,
                    y: iy,
                    width: overlay_w - 8.0,
                    height: item_h,
                    color: tc.selected_bg,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
            }

            cmds.push(RenderCommand::Text {
                x: overlay_x + 20.0,
                y: iy + 8.0,
                text: chapter.title.clone(),
                color: if i == self.list_selection { tc.accent } else { tc.text },
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(overlay_w - 40.0),
            });
        }
    }

    fn render_bookmark_list_overlay(&self, tc: &ThemeColors, cmds: &mut Vec<RenderCommand>) {
        let bookmarks = self.bookmarks();

        let overlay_w = 350.0f32;
        let item_count = bookmarks.len().max(1);
        let overlay_h = (60.0 + (item_count as f32) * 32.0).min(self.window_height - 80.0);
        let overlay_x = (self.window_width - overlay_w) / 2.0;
        let overlay_y = (self.window_height - overlay_h) / 2.0;

        // Dim background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::ZERO,
        });

        // Overlay card
        cmds.push(RenderCommand::FillRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: tc.surface,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: tc.separator,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: overlay_x + 16.0,
            y: overlay_y + 14.0,
            text: "Bookmarks".to_owned(),
            color: tc.bookmark_color,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(overlay_w - 32.0),
        });

        cmds.push(RenderCommand::Line {
            x1: overlay_x + 12.0,
            y1: overlay_y + 38.0,
            x2: overlay_x + overlay_w - 12.0,
            y2: overlay_y + 38.0,
            color: tc.separator,
            width: 1.0,
        });

        let list_top = overlay_y + 44.0;
        if bookmarks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: overlay_x + 20.0,
                y: list_top + 8.0,
                text: "No bookmarks yet. Press B to add one.".to_owned(),
                color: tc.text_dim,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(overlay_w - 40.0),
            });
        } else {
            let item_h = 32.0f32;
            for (i, &page) in bookmarks.iter().enumerate() {
                let iy = list_top + (i as f32) * item_h;
                if iy + item_h > overlay_y + overlay_h {
                    break;
                }

                if i == self.list_selection {
                    cmds.push(RenderCommand::FillRect {
                        x: overlay_x + 4.0,
                        y: iy,
                        width: overlay_w - 8.0,
                        height: item_h,
                        color: tc.selected_bg,
                        corner_radii: CornerRadii::all(SMALL_RADIUS),
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: overlay_x + 20.0,
                    y: iy + 8.0,
                    text: format!("Page {}", page.saturating_add(1)),
                    color: if i == self.list_selection { tc.accent } else { tc.text },
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(overlay_w - 40.0),
                });
            }
        }
    }
}

impl Default for EbookApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let _app = EbookApp::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use guitk::event::Modifiers;

    // -- Helpers --

    fn make_app() -> EbookApp {
        EbookApp::new()
    }

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        }
    }

    fn make_key_with_mod(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: None,
        }
    }

    fn make_key_with_text(key: Key, ch: char) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::default(),
            text: Some(ch),
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    // ================================================================
    // Pagination tests
    // ================================================================

    #[test]
    fn test_paginate_empty_text() {
        let result = paginate("", 80, 25);
        assert_eq!(result.pages.len(), 1);
        assert_eq!(result.pages[0], (0, 0));
    }

    #[test]
    fn test_paginate_single_line() {
        let text = "Hello, world!";
        let result = paginate(text, 80, 25);
        assert_eq!(result.pages.len(), 1);
        assert_eq!(&text[result.pages[0].0..result.pages[0].1], text);
    }

    #[test]
    fn test_paginate_multi_page() {
        // Create text with many lines that should span multiple pages.
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("Line number {} of the text.\n", i));
        }
        let result = paginate(&text, 80, 10);
        assert!(result.pages.len() > 1, "Expected multiple pages");
        // First page covers lines 0..10.
        // All pages should cover the full text.
        let last = result.pages.last().unwrap();
        assert_eq!(last.1, text.len());
    }

    #[test]
    fn test_paginate_wrapping_long_lines() {
        // A line with 200 chars at 80 chars per line should wrap to 3 lines.
        let text = "A".repeat(200) + "\n";
        let result = paginate(&text, 80, 25);
        assert_eq!(result.pages.len(), 1);
    }

    #[test]
    fn test_paginate_page_boundaries_are_contiguous() {
        let mut text = String::new();
        for i in 0..50 {
            text.push_str(&format!("Line {}.\n", i));
        }
        let result = paginate(&text, 80, 5);
        // Each page end should be the next page start.
        for window in result.pages.windows(2) {
            assert_eq!(window[0].1, window[1].0);
        }
    }

    #[test]
    fn test_paginate_zero_params() {
        let result = paginate("text", 0, 0);
        assert_eq!(result.pages.len(), 1);
    }

    #[test]
    fn test_paginate_one_line_per_page() {
        let text = "A\nB\nC\nD\n";
        let result = paginate(text, 80, 1);
        assert_eq!(result.pages.len(), 4);
    }

    // ================================================================
    // Page navigation tests
    // ================================================================

    #[test]
    fn test_next_page() {
        let mut app = make_app();
        app.open_book(0);
        let initial = app.current_page();
        app.next_page();
        // Only advances if there are multiple pages.
        if app.total_pages() > 1 {
            assert_eq!(app.current_page(), initial + 1);
        }
    }

    #[test]
    fn test_prev_page_at_start() {
        let mut app = make_app();
        app.open_book(0);
        app.prev_page();
        assert_eq!(app.current_page(), 0);
    }

    #[test]
    fn test_go_to_first_page() {
        let mut app = make_app();
        app.open_book(0);
        app.next_page();
        app.go_to_first_page();
        assert_eq!(app.current_page(), 0);
    }

    #[test]
    fn test_go_to_last_page() {
        let mut app = make_app();
        app.open_book(0);
        app.go_to_last_page();
        assert_eq!(app.current_page(), app.total_pages().saturating_sub(1));
    }

    #[test]
    fn test_go_to_page_clamped() {
        let mut app = make_app();
        app.open_book(0);
        app.go_to_page(99999);
        assert_eq!(app.current_page(), app.total_pages().saturating_sub(1));
    }

    #[test]
    fn test_next_page_does_not_exceed_total() {
        let mut app = make_app();
        app.open_book(0);
        for _ in 0..1000 {
            app.next_page();
        }
        assert!(app.current_page() < app.total_pages());
    }

    #[test]
    fn test_keyboard_navigation_right() {
        let mut app = make_app();
        app.open_book(0);
        let before = app.current_page();
        app.handle_key_event(&make_key(Key::Right));
        if app.total_pages() > 1 {
            assert_eq!(app.current_page(), before + 1);
        }
    }

    #[test]
    fn test_keyboard_navigation_left() {
        let mut app = make_app();
        app.open_book(0);
        app.next_page();
        let before = app.current_page();
        app.handle_key_event(&make_key(Key::Left));
        assert_eq!(app.current_page(), before.saturating_sub(1));
    }

    #[test]
    fn test_keyboard_home() {
        let mut app = make_app();
        app.open_book(0);
        app.next_page();
        app.handle_key_event(&make_key(Key::Home));
        assert_eq!(app.current_page(), 0);
    }

    #[test]
    fn test_keyboard_end() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::End));
        assert_eq!(app.current_page(), app.total_pages().saturating_sub(1));
    }

    #[test]
    fn test_keyboard_page_down() {
        let mut app = make_app();
        app.open_book(0);
        let before = app.current_page();
        app.handle_key_event(&make_key(Key::PageDown));
        if app.total_pages() > 1 {
            assert_eq!(app.current_page(), before + 1);
        }
    }

    #[test]
    fn test_keyboard_page_up() {
        let mut app = make_app();
        app.open_book(0);
        app.next_page();
        let before = app.current_page();
        app.handle_key_event(&make_key(Key::PageUp));
        assert_eq!(app.current_page(), before.saturating_sub(1));
    }

    // ================================================================
    // Bookmark tests
    // ================================================================

    #[test]
    fn test_toggle_bookmark() {
        let mut app = make_app();
        app.open_book(0);
        assert!(!app.current_reading_state().unwrap().is_bookmarked(0));
        app.toggle_bookmark();
        assert!(app.current_reading_state().unwrap().is_bookmarked(0));
        app.toggle_bookmark();
        assert!(!app.current_reading_state().unwrap().is_bookmarked(0));
    }

    #[test]
    fn test_bookmark_multiple_pages() {
        let mut app = make_app();
        app.open_book(0);
        app.toggle_bookmark(); // page 0
        app.next_page();
        app.toggle_bookmark(); // page 1
        let bm = app.bookmarks();
        assert_eq!(bm.len(), 2);
        assert!(bm.contains(&0));
        assert!(bm.contains(&1));
    }

    #[test]
    fn test_bookmark_sorted() {
        let mut state = ReadingState::new();
        state.toggle_bookmark(5);
        state.toggle_bookmark(2);
        state.toggle_bookmark(8);
        assert_eq!(state.bookmarks, vec![2, 5, 8]);
    }

    #[test]
    fn test_bookmark_key_b() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::B));
        assert!(app.current_reading_state().unwrap().is_bookmarked(0));
    }

    #[test]
    fn test_bookmark_list_ctrl_b() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key_with_mod(Key::B, ctrl()));
        assert_eq!(app.view, AppView::BookmarkList);
    }

    #[test]
    fn test_jump_to_bookmark() {
        let mut app = make_app();
        app.open_book(0);
        if app.total_pages() > 2 {
            app.go_to_page(2);
            app.toggle_bookmark();
            app.go_to_page(0);
            app.show_bookmark_list();
            app.jump_to_bookmark(0);
            assert_eq!(app.current_page(), 2);
            assert_eq!(app.view, AppView::Reading);
        }
    }

    #[test]
    fn test_bookmark_persistence_across_pages() {
        let mut app = make_app();
        app.open_book(0);
        app.toggle_bookmark();
        app.next_page();
        // Page 0 should still be bookmarked.
        assert!(app.current_reading_state().unwrap().is_bookmarked(0));
        assert!(!app.current_reading_state().unwrap().is_bookmarked(app.current_page()));
    }

    // ================================================================
    // Search tests
    // ================================================================

    #[test]
    fn test_find_all_matches_basic() {
        let matches = find_all_matches("hello world hello", "hello");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].byte_offset, 0);
        assert_eq!(matches[1].byte_offset, 12);
    }

    #[test]
    fn test_find_all_matches_case_insensitive() {
        let matches = find_all_matches("Hello HELLO hello", "hello");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_find_all_matches_empty_needle() {
        let matches = find_all_matches("some text", "");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_all_matches_no_match() {
        let matches = find_all_matches("some text", "xyz");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_search_workflow() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        assert!(app.search_active);

        // Type search query.
        app.search_query = "the".to_owned();
        app.execute_search();
        assert!(!app.search_matches.is_empty());
        assert_eq!(app.current_match_idx, Some(0));
    }

    #[test]
    fn test_search_next_prev_match() {
        let mut app = make_app();
        app.open_book(0);
        app.search_query = "the".to_owned();
        app.execute_search();

        if app.search_matches.len() > 1 {
            app.next_match();
            assert_eq!(app.current_match_idx, Some(1));
            app.prev_match();
            assert_eq!(app.current_match_idx, Some(0));
        }
    }

    #[test]
    fn test_search_wraps_around() {
        let mut app = make_app();
        app.open_book(0);
        app.search_query = "the".to_owned();
        app.execute_search();

        let count = app.search_matches.len();
        if count > 0 {
            // Go to last match.
            for _ in 0..count {
                app.next_match();
            }
            // Should wrap to 0.
            assert_eq!(app.current_match_idx, Some(0));
        }
    }

    #[test]
    fn test_search_prev_wraps_to_end() {
        let mut app = make_app();
        app.open_book(0);
        app.search_query = "the".to_owned();
        app.execute_search();

        if !app.search_matches.is_empty() {
            app.prev_match();
            assert_eq!(
                app.current_match_idx,
                Some(app.search_matches.len().saturating_sub(1))
            );
        }
    }

    #[test]
    fn test_search_close() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        assert!(app.search_active);
        app.close_search();
        assert!(!app.search_active);
    }

    #[test]
    fn test_search_key_slash() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::Slash));
        assert!(app.search_active);
    }

    #[test]
    fn test_search_escape_closes() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        app.handle_key_event(&make_key(Key::Escape));
        assert!(!app.search_active);
    }

    #[test]
    fn test_search_typing() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        app.handle_key_event(&make_key_with_text(Key::H, 'h'));
        app.handle_key_event(&make_key_with_text(Key::I, 'i'));
        assert_eq!(app.search_query, "hi");
    }

    #[test]
    fn test_search_backspace() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        app.search_query = "hello".to_owned();
        app.handle_key_event(&make_key(Key::Backspace));
        assert_eq!(app.search_query, "hell");
    }

    #[test]
    fn test_page_for_offset() {
        let pages = vec![(0, 100), (100, 200), (200, 300)];
        assert_eq!(page_for_offset(&pages, 0), Some(0));
        assert_eq!(page_for_offset(&pages, 50), Some(0));
        assert_eq!(page_for_offset(&pages, 100), Some(1));
        assert_eq!(page_for_offset(&pages, 250), Some(2));
    }

    #[test]
    fn test_page_for_offset_at_end() {
        let pages = vec![(0, 100), (100, 200)];
        assert_eq!(page_for_offset(&pages, 200), Some(1));
    }

    // ================================================================
    // TOC / Chapter tests
    // ================================================================

    #[test]
    fn test_parse_chapters_separator() {
        let text = "Chapter 1\nSome text.\n---\nChapter 2\nMore text.";
        let chapters = parse_chapters(text);
        assert!(chapters.len() >= 2, "Expected at least 2 chapters, got {}", chapters.len());
        assert_eq!(chapters[0].title, "Chapter 1");
    }

    #[test]
    fn test_parse_chapters_double_blank() {
        let text = "Chapter 1\nText here.\n\n\n\nChapter 2\nMore text.";
        let chapters = parse_chapters(text);
        assert!(chapters.len() >= 2, "Expected at least 2 chapters, got {}", chapters.len());
    }

    #[test]
    fn test_parse_chapters_no_separators() {
        let text = "Just one long chapter of text with no breaks.";
        let chapters = parse_chapters(text);
        assert_eq!(chapters.len(), 1);
    }

    #[test]
    fn test_parse_chapters_offsets_are_sorted() {
        let text = "Ch 1\n---\nCh 2\n---\nCh 3";
        let chapters = parse_chapters(text);
        for window in chapters.windows(2) {
            assert!(window[0].byte_offset <= window[1].byte_offset);
        }
    }

    #[test]
    fn test_toc_navigation() {
        let mut app = make_app();
        app.open_book(0);
        app.show_toc();
        assert_eq!(app.view, AppView::TableOfContents);
        app.handle_key_event(&make_key(Key::Down));
        assert_eq!(app.list_selection, 1);
        app.handle_key_event(&make_key(Key::Up));
        assert_eq!(app.list_selection, 0);
    }

    #[test]
    fn test_toc_enter_jumps_to_chapter() {
        let mut app = make_app();
        app.open_book(0);
        app.show_toc();
        app.handle_key_event(&make_key(Key::Enter));
        assert_eq!(app.view, AppView::Reading);
    }

    #[test]
    fn test_toc_escape_returns_to_reading() {
        let mut app = make_app();
        app.open_book(0);
        app.show_toc();
        app.handle_key_event(&make_key(Key::Escape));
        assert_eq!(app.view, AppView::Reading);
    }

    #[test]
    fn test_toc_key_t() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::T));
        assert_eq!(app.view, AppView::TableOfContents);
    }

    #[test]
    fn test_sample_books_have_chapters() {
        let lib = sample_library();
        for book in &lib {
            assert!(
                !book.chapters.is_empty(),
                "Book '{}' should have at least one chapter",
                book.meta.title
            );
        }
    }

    // ================================================================
    // Font size tests
    // ================================================================

    #[test]
    fn test_font_size_increase() {
        let mut app = make_app();
        app.open_book(0);
        assert_eq!(app.current_font_size(), FontSizeLevel::Medium);
        app.increase_font_size();
        assert_eq!(app.current_font_size(), FontSizeLevel::Large);
    }

    #[test]
    fn test_font_size_decrease() {
        let mut app = make_app();
        app.open_book(0);
        assert_eq!(app.current_font_size(), FontSizeLevel::Medium);
        app.decrease_font_size();
        assert_eq!(app.current_font_size(), FontSizeLevel::Small);
    }

    #[test]
    fn test_font_size_clamp_max() {
        let mut app = make_app();
        app.open_book(0);
        app.increase_font_size();
        app.increase_font_size();
        app.increase_font_size();
        assert_eq!(app.current_font_size(), FontSizeLevel::Large);
    }

    #[test]
    fn test_font_size_clamp_min() {
        let mut app = make_app();
        app.open_book(0);
        app.decrease_font_size();
        app.decrease_font_size();
        app.decrease_font_size();
        assert_eq!(app.current_font_size(), FontSizeLevel::Small);
    }

    #[test]
    fn test_font_size_key_plus() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::Equals));
        assert_eq!(app.current_font_size(), FontSizeLevel::Large);
    }

    #[test]
    fn test_font_size_key_minus() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::Minus));
        assert_eq!(app.current_font_size(), FontSizeLevel::Small);
    }

    #[test]
    fn test_font_size_affects_pagination() {
        let mut app = make_app();
        app.open_book(0);
        let pages_medium = app.total_pages();
        app.increase_font_size(); // Large
        let pages_large = app.total_pages();
        // Larger font should produce more pages (or at least not fewer).
        assert!(
            pages_large >= pages_medium,
            "Large font ({} pages) should produce >= pages than medium ({} pages)",
            pages_large,
            pages_medium
        );
    }

    #[test]
    fn test_font_size_labels() {
        assert_eq!(FontSizeLevel::Small.label(), "Small");
        assert_eq!(FontSizeLevel::Medium.label(), "Medium");
        assert_eq!(FontSizeLevel::Large.label(), "Large");
    }

    #[test]
    fn test_font_size_values() {
        assert!(FontSizeLevel::Small.font_size() < FontSizeLevel::Medium.font_size());
        assert!(FontSizeLevel::Medium.font_size() < FontSizeLevel::Large.font_size());
    }

    // ================================================================
    // Theme switching tests
    // ================================================================

    #[test]
    fn test_toggle_theme() {
        let mut app = make_app();
        assert_eq!(app.theme, ThemeKind::Dark);
        app.toggle_theme();
        assert_eq!(app.theme, ThemeKind::Sepia);
        app.toggle_theme();
        assert_eq!(app.theme, ThemeKind::Dark);
    }

    #[test]
    fn test_theme_key_s() {
        let mut app = make_app();
        app.open_book(0);
        assert_eq!(app.theme, ThemeKind::Dark);
        app.handle_key_event(&make_key(Key::S));
        assert_eq!(app.theme, ThemeKind::Sepia);
    }

    #[test]
    fn test_dark_theme_colors() {
        let tc = ThemeColors::dark();
        // Dark theme should have dark background.
        assert!(tc.background.r < 100);
        assert!(tc.text.r > 150);
    }

    #[test]
    fn test_sepia_theme_colors() {
        let tc = ThemeColors::sepia();
        // Sepia theme should have light background.
        assert!(tc.background.r > 200);
        assert!(tc.text.r < 100);
    }

    // ================================================================
    // Reading progress tests
    // ================================================================

    #[test]
    fn test_progress_at_start() {
        let mut app = make_app();
        app.open_book(0);
        assert!((app.reading_progress() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_progress_at_end() {
        let mut app = make_app();
        app.open_book(0);
        app.go_to_last_page();
        assert!((app.reading_progress() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_pct_format() {
        let mut app = make_app();
        app.open_book(0);
        let pct = app.reading_progress_pct();
        assert!(pct.ends_with('%'));
    }

    #[test]
    fn test_progress_increases_with_pages() {
        let mut app = make_app();
        app.open_book(0);
        let p0 = app.reading_progress();
        app.next_page();
        let p1 = app.reading_progress();
        if app.total_pages() > 1 {
            assert!(p1 > p0);
        }
    }

    // ================================================================
    // Word count tests
    // ================================================================

    #[test]
    fn test_count_words_empty() {
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn test_count_words_simple() {
        assert_eq!(count_words("hello world"), 2);
    }

    #[test]
    fn test_count_words_multiline() {
        assert_eq!(count_words("hello\nworld\nfoo"), 3);
    }

    #[test]
    fn test_count_words_extra_whitespace() {
        assert_eq!(count_words("  hello   world  "), 2);
    }

    #[test]
    fn test_book_word_count() {
        let lib = sample_library();
        for book in &lib {
            assert!(
                book.word_count > 0,
                "Book '{}' should have a positive word count",
                book.meta.title
            );
        }
    }

    #[test]
    fn test_reading_time() {
        let book = Book::new("Test", "Author", "word ".repeat(238).trim());
        let minutes = book.reading_time_minutes();
        // 238 words at 238 WPM should be about 1 minute.
        assert!((minutes - 1.0).abs() < 0.1);
    }

    // ================================================================
    // Library management tests
    // ================================================================

    #[test]
    fn test_sample_library_count() {
        let lib = sample_library();
        assert_eq!(lib.len(), 5);
    }

    #[test]
    fn test_sample_library_has_metadata() {
        let lib = sample_library();
        for book in &lib {
            assert!(!book.meta.title.is_empty());
            assert!(!book.meta.author.is_empty());
        }
    }

    #[test]
    fn test_sample_library_genres() {
        let lib = sample_library();
        let titles: Vec<&str> = lib.iter().map(|b| b.meta.title.as_str()).collect();
        assert!(titles.contains(&"The Clockwork Garden")); // Fantasy
        assert!(titles.contains(&"Stars Over Quiet Water")); // Literary fiction
        assert!(titles.contains(&"A Brief History of Bread")); // Non-fiction
        assert!(titles.contains(&"The Probability Engine")); // Sci-fi
        assert!(titles.contains(&"Sonnets from the Edge")); // Poetry
    }

    #[test]
    fn test_open_book() {
        let mut app = make_app();
        app.open_book(2);
        assert_eq!(app.selected_book, 2);
        assert_eq!(app.view, AppView::Reading);
        assert!(app.paginated.is_some());
    }

    #[test]
    fn test_return_to_library() {
        let mut app = make_app();
        app.open_book(0);
        app.return_to_library();
        assert_eq!(app.view, AppView::Library);
    }

    #[test]
    fn test_library_navigation() {
        let mut app = make_app();
        assert_eq!(app.selected_book, 0);
        app.handle_key_event(&make_key(Key::Down));
        assert_eq!(app.selected_book, 1);
        app.handle_key_event(&make_key(Key::Down));
        assert_eq!(app.selected_book, 2);
        app.handle_key_event(&make_key(Key::Up));
        assert_eq!(app.selected_book, 1);
    }

    #[test]
    fn test_library_open_with_enter() {
        let mut app = make_app();
        app.handle_key_event(&make_key(Key::Enter));
        assert_eq!(app.view, AppView::Reading);
    }

    #[test]
    fn test_library_down_clamp() {
        let mut app = make_app();
        for _ in 0..100 {
            app.handle_key_event(&make_key(Key::Down));
        }
        assert_eq!(app.selected_book, app.library.len() - 1);
    }

    #[test]
    fn test_library_up_clamp() {
        let mut app = make_app();
        app.handle_key_event(&make_key(Key::Up));
        assert_eq!(app.selected_book, 0);
    }

    #[test]
    fn test_add_book() {
        let mut app = make_app();
        let orig_len = app.library.len();
        app.add_book("New Book", "New Author", "New text content.");
        assert_eq!(app.library.len(), orig_len + 1);
        assert_eq!(app.reading_states.len(), orig_len + 1);
    }

    #[test]
    fn test_escape_from_reading_returns_to_library() {
        let mut app = make_app();
        app.open_book(0);
        app.handle_key_event(&make_key(Key::Escape));
        assert_eq!(app.view, AppView::Library);
    }

    // ================================================================
    // Rendering tests
    // ================================================================

    #[test]
    fn test_render_library_produces_commands() {
        let app = make_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_reading_produces_commands() {
        let mut app = make_app();
        app.open_book(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
        // Should contain at least background, toolbar, text, status bar.
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_toc_overlay() {
        let mut app = make_app();
        app.open_book(0);
        app.show_toc();
        let cmds = app.render();
        // TOC overlay should produce extra commands on top of reading view.
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_bookmark_list_overlay() {
        let mut app = make_app();
        app.open_book(0);
        app.toggle_bookmark();
        app.show_bookmark_list();
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_with_search_active() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        app.search_query = "the".to_owned();
        app.execute_search();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_search_bar_visible() {
        let mut app = make_app();
        app.open_book(0);
        app.open_search();
        let cmds = app.render();
        // Should contain a search bar rectangle.
        let has_stroke = cmds.iter().any(|cmd| matches!(cmd, RenderCommand::StrokeRect { .. }));
        assert!(has_stroke, "Search bar should have a stroke rect");
    }

    #[test]
    fn test_render_progress_bar_exists() {
        let mut app = make_app();
        app.open_book(0);
        let cmds = app.render();
        // The progress bar should be a filled rect with the progress_bar color.
        let tc = app.theme_colors();
        let has_progress = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::FillRect { color, height, .. }
                     if *color == tc.progress_bg && (*height - PROGRESS_BAR_HEIGHT).abs() < 0.1)
        });
        assert!(has_progress, "Should render progress bar track");
    }

    #[test]
    fn test_render_different_themes() {
        let mut app = make_app();
        app.open_book(0);
        let cmds_dark = app.render();
        app.toggle_theme();
        let cmds_sepia = app.render();
        // Both should produce commands, and the background colors should differ.
        assert!(!cmds_dark.is_empty());
        assert!(!cmds_sepia.is_empty());
        // The first command (background fill) should have different colors.
        let bg_dark = match &cmds_dark[0] {
            RenderCommand::FillRect { color, .. } => *color,
            _ => panic!("First command should be FillRect"),
        };
        let bg_sepia = match &cmds_sepia[0] {
            RenderCommand::FillRect { color, .. } => *color,
            _ => panic!("First command should be FillRect"),
        };
        assert_ne!(bg_dark, bg_sepia);
    }

    #[test]
    fn test_render_bookmark_indicator() {
        let mut app = make_app();
        app.open_book(0);
        app.toggle_bookmark();
        let cmds = app.render();
        let has_bm_text = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "BM")
        });
        assert!(has_bm_text, "Should show bookmark indicator when page is bookmarked");
    }

    // ================================================================
    // Mouse event tests
    // ================================================================

    #[test]
    fn test_mouse_click_library() {
        let mut app = make_app();
        let event = MouseEvent {
            x: 100.0,
            y: TOOLBAR_HEIGHT + 10.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        let consumed = app.handle_mouse_event(&event);
        assert!(consumed);
        assert_eq!(app.view, AppView::Reading);
    }

    #[test]
    fn test_mouse_click_reading_next_page() {
        let mut app = make_app();
        app.open_book(0);
        let before = app.current_page();
        let event = MouseEvent {
            x: app.window_width - 10.0, // right side
            y: TOOLBAR_HEIGHT + 50.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse_event(&event);
        if app.total_pages() > 1 {
            assert_eq!(app.current_page(), before + 1);
        }
    }

    #[test]
    fn test_mouse_click_reading_prev_page() {
        let mut app = make_app();
        app.open_book(0);
        app.next_page();
        let before = app.current_page();
        let event = MouseEvent {
            x: 10.0, // left side
            y: TOOLBAR_HEIGHT + 50.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse_event(&event);
        assert_eq!(app.current_page(), before.saturating_sub(1));
    }

    // ================================================================
    // Misc / edge case tests
    // ================================================================

    #[test]
    fn test_current_page_text_when_no_book() {
        let app = make_app();
        // No book opened yet (paginated is None).
        let text = app.current_page_text();
        assert!(text.is_empty());
    }

    #[test]
    fn test_current_page_text_after_open() {
        let mut app = make_app();
        app.open_book(0);
        let text = app.current_page_text();
        assert!(!text.is_empty());
    }

    #[test]
    fn test_default_view_is_library() {
        let app = make_app();
        assert_eq!(app.view, AppView::Library);
    }

    #[test]
    fn test_default_theme_is_dark() {
        let app = make_app();
        assert_eq!(app.theme, ThemeKind::Dark);
    }

    #[test]
    fn test_reading_state_default() {
        let state = ReadingState::new();
        assert_eq!(state.current_page, 0);
        assert!(state.bookmarks.is_empty());
        assert_eq!(state.font_size, FontSizeLevel::Medium);
    }

    #[test]
    fn test_key_not_pressed_is_ignored() {
        let mut app = make_app();
        app.open_book(0);
        let event = KeyEvent {
            key: Key::Right,
            pressed: false, // key released, not pressed
            modifiers: Modifiers::default(),
            text: None,
        };
        let consumed = app.handle_key_event(&event);
        assert!(!consumed);
    }

    #[test]
    fn test_layout_params_reasonable() {
        let app = make_app();
        let (cpl, lpp) = app.layout_params();
        assert!(cpl > 0);
        assert!(lpp > 0);
        assert!(cpl < 1000);
        assert!(lpp < 200);
    }

    #[test]
    fn test_search_n_key_next_match() {
        let mut app = make_app();
        app.open_book(0);
        app.search_query = "the".to_owned();
        app.execute_search();
        if app.search_matches.len() > 1 {
            app.handle_key_event(&make_key(Key::N));
            assert_eq!(app.current_match_idx, Some(1));
        }
    }

    #[test]
    fn test_search_shift_n_prev_match() {
        let mut app = make_app();
        app.open_book(0);
        app.search_query = "the".to_owned();
        app.execute_search();
        if !app.search_matches.is_empty() {
            let last = app.search_matches.len() - 1;
            app.handle_key_event(&make_key_with_mod(Key::N, shift()));
            assert_eq!(app.current_match_idx, Some(last));
        }
    }

    #[test]
    fn test_chapter_indices_are_sequential() {
        let lib = sample_library();
        for book in &lib {
            for (i, chapter) in book.chapters.iter().enumerate() {
                assert_eq!(chapter.index, i);
            }
        }
    }

    #[test]
    fn test_find_line_end_basic() {
        assert_eq!(find_line_end("hello\nworld", 0), 6);
        assert_eq!(find_line_end("hello\nworld", 6), 11);
    }

    #[test]
    fn test_first_nonblank_line() {
        assert_eq!(first_nonblank_line("  \n  hello  \n"), Some("hello"));
        assert_eq!(first_nonblank_line("  \n  \n  "), None);
    }

    #[test]
    fn test_is_separator_line() {
        assert!(is_separator_line("---\nstuff", 0));
        assert!(is_separator_line("hello\n---\nstuff", 6));
        assert!(!is_separator_line("hello---", 5));
    }

    #[test]
    fn test_open_book_out_of_range() {
        let mut app = make_app();
        app.open_book(999);
        // Should not crash; view stays as library since index is invalid.
        assert_eq!(app.view, AppView::Library);
    }

    #[test]
    fn test_jump_to_chapter_out_of_range() {
        let mut app = make_app();
        app.open_book(0);
        let page_before = app.current_page();
        app.jump_to_chapter(999);
        // Should not crash; page should not change.
        assert_eq!(app.current_page(), page_before);
    }
}
