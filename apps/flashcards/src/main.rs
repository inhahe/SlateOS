#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]

//! Slate OS Flashcards -- spaced-repetition study application.
//!
//! Features:
//! - Multiple decks with create/edit/delete
//! - Card CRUD (front/back text, tags)
//! - Study mode with flip animation and SM-2 spaced repetition
//! - Scoring: Easy / Good / Hard / Again
//! - Per-deck and per-card statistics
//! - Tag filtering and search
//! - Deck shuffle
//! - Import/export (simple text format)
//! - Three sample decks pre-loaded

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::kv;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Catppuccin Mocha palette ────────────────────────────────────────

// ── Card-list type sizes ────────────────────────────────────────────
// Named because eliding text and drawing it must agree on the size: a cell
// measured at one size and drawn at another either overflows or is cut short.
/// What the window says about the three decks and about progress.
///
/// **The decks stay.** "What is the capital of France? -> Paris" is a true
/// statement about the world, correctly stated, bundled as content -- the
/// `apps/ebook` case rather than the `apps/kanban` one. A fabrication is a
/// claim about something the program cannot observe, and this is not one.
///
/// The second line is the defect, and it is specific to this kind of program.
/// **Spaced repetition is defined by history.** A scheduler that forgets is
/// worse than no scheduler: it will show a card that was mastered last week
/// and hold back one that is about to be forgotten, and the user cannot tell
/// because the whole point is that they do not remember either.
const SAMPLE_AND_PROGRESS_LINES: [&str; 2] = [
    "Three included decks -- these came with the app, not from you.",
    "Nothing is saved automatically -- press Ctrl+S to write the deck, schedules included, or every review resets when the window closes.",
];

/// The most of a deck file one open will read.
///
/// Reported when it bites. A cut deck file parses: every complete card block
/// in it is valid, so the tail is simply missing, and **a deck short of its
/// last hundred cards looks like a deck that only had the first ones.**
pub const MAX_DECK_BYTES: usize = 8 * 1024 * 1024;

const CARD_HEADING_SIZE: f32 = 11.0;
const CARD_FRONT_SIZE: f32 = 13.0;
const CARD_BACK_SIZE: f32 = 11.0;
const CARD_TAG_SIZE: f32 = 11.0;
const CARD_STATUS_SIZE: f32 = 12.0;
/// Gap between the search box and the tag-filter pill beside it.
const TAG_PILL_GAP: f32 = 8.0;
/// Inset of the tag-filter pill's label from the pill's own edges.
const TAG_PILL_PAD: f32 = 8.0;

// ── SM-2 defaults ───────────────────────────────────────────────────
const SM2_INITIAL_EASE: f32 = 2.5;
const SM2_MIN_EASE: f32 = 1.3;

// ── Scoring ─────────────────────────────────────────────────────────
/// Quality ratings for SM-2 algorithm (0..5 scale mapped to our four buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rating {
    Again, // quality = 0
    Hard,  // quality = 2
    Good,  // quality = 3
    Easy,  // quality = 5
}

impl Rating {
    fn quality(self) -> u8 {
        match self {
            Self::Again => 0,
            Self::Hard => 2,
            Self::Good => 3,
            Self::Easy => 5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Again => "Again",
            Self::Hard => "Hard",
            Self::Good => "Good",
            Self::Easy => "Easy",
        }
    }

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Again => pal.red,
            Self::Hard => pal.peach,
            Self::Good => pal.blue,
            Self::Easy => pal.green,
        }
    }
}

const ALL_RATINGS: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

// ── SM-2 review data per card ───────────────────────────────────────
#[derive(Clone, Debug)]
struct ReviewData {
    /// Number of consecutive correct reviews (quality >= 3).
    repetitions: u32,
    /// Current ease factor (starts at 2.5).
    ease_factor: f32,
    /// Inter-repetition interval in days.
    interval_days: u32,
    /// Simulated day of last review (monotonic counter).
    last_review_day: u32,
    /// Total number of reviews this card has received.
    total_reviews: u32,
    /// Count of each rating received.
    rating_counts: [u32; 4], // Again, Hard, Good, Easy
}

impl ReviewData {
    /// One line of the deck format: every field, in declaration order.
    ///
    /// **The format used to carry front, back and tags and nothing else**, so
    /// an export and a re-import returned every card to new. In a spaced
    /// repetition program that is close to worthless and it is invisible: a
    /// restored deck with a reset schedule looks exactly like a restored deck,
    /// until four hundred mature cards all come due on the same morning.
    fn to_line(&self) -> String {
        format!(
            "{} {:.4} {} {} {} {} {} {} {}",
            self.repetitions,
            self.ease_factor,
            self.interval_days,
            self.last_review_day,
            self.total_reviews,
            self.rating_counts[0],
            self.rating_counts[1],
            self.rating_counts[2],
            self.rating_counts[3],
        )
    }

    /// Read a line written by [`to_line`](Self::to_line).
    ///
    /// `None` when the line is not one of ours. The caller counts those and
    /// says so rather than quietly substituting a new card's schedule, which
    /// would be the same silent reset one level down.
    fn from_line(line: &str) -> Option<Self> {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Exactly nine, so a short line and a line with junk appended are both
        // rejected rather than half-read.
        if f.len() != 9 {
            return None;
        }
        let num = |i: usize| -> Option<u32> { f.get(i)?.parse().ok() };
        let ease_factor: f32 = f.get(1)?.parse().ok()?;
        if !ease_factor.is_finite() || ease_factor <= 0.0 {
            return None;
        }
        Some(Self {
            repetitions: num(0)?,
            ease_factor,
            interval_days: num(2)?,
            last_review_day: num(3)?,
            total_reviews: num(4)?,
            rating_counts: [num(5)?, num(6)?, num(7)?, num(8)?],
        })
    }

    fn new() -> Self {
        Self {
            repetitions: 0,
            ease_factor: SM2_INITIAL_EASE,
            interval_days: 0,
            last_review_day: 0,
            total_reviews: 0,
            rating_counts: [0; 4],
        }
    }

    /// Apply SM-2 algorithm after a review.
    fn apply_rating(&mut self, rating: Rating, current_day: u32) {
        let q = rating.quality();
        self.total_reviews = self.total_reviews.saturating_add(1);
        let idx = match rating {
            Rating::Again => 0,
            Rating::Hard => 1,
            Rating::Good => 2,
            Rating::Easy => 3,
        };
        if let Some(count) = self.rating_counts.get_mut(idx) {
            *count = count.saturating_add(1);
        }
        self.last_review_day = current_day;

        if q < 3 {
            // Failed: reset repetitions
            self.repetitions = 0;
            self.interval_days = 1;
        } else {
            self.repetitions = self.repetitions.saturating_add(1);
            match self.repetitions {
                1 => self.interval_days = 1,
                2 => self.interval_days = 6,
                _ => {
                    let new_interval = (self.interval_days as f32 * self.ease_factor) as u32;
                    self.interval_days = new_interval.max(1);
                }
            }
        }

        // Update ease factor: EF' = EF + (0.1 - (5-q)*(0.08 + (5-q)*0.02))
        let q_f = q as f32;
        let delta = 0.1 - (5.0 - q_f) * (0.08 + (5.0 - q_f) * 0.02);
        self.ease_factor += delta;
        if self.ease_factor < SM2_MIN_EASE {
            self.ease_factor = SM2_MIN_EASE;
        }
    }

    /// Is this card due for review on the given day?
    fn is_due(&self, current_day: u32) -> bool {
        if self.total_reviews == 0 {
            return true; // never reviewed
        }
        // `checked_add`, and due when it overflows: a card whose next review
        // lands past the end of the calendar is one nobody should be waiting
        // for, and wrapping would make it due at the wrong moment instead.
        self.last_review_day
            .checked_add(self.interval_days)
            .is_none_or(|next| current_day >= next)
    }

    /// Accuracy as a percentage (0..100). Returns 0 if no reviews.
    fn accuracy_percent(&self) -> u32 {
        if self.total_reviews == 0 {
            return 0;
        }
        // `get` and saturating throughout: this is a percentage on a card,
        // not a place to panic if a count is missing.
        let good = self.rating_counts.get(2).copied().unwrap_or(0);
        let easy = self.rating_counts.get(3).copied().unwrap_or(0);
        good.saturating_add(easy)
            .saturating_mul(100)
            .checked_div(self.total_reviews)
            .unwrap_or(0)
    }
}

/// A deck name reduced to something that can be a filename.
///
/// Only the three characters a path cannot contain are replaced. A name is the
/// user's, and rewriting more of it than necessary means they cannot find the
/// file by the name they gave the deck.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        String::from("deck")
    } else {
        trimmed.to_owned()
    }
}

/// What to call a deck read from `path`.
///
/// The file's own stem, because that is what the user will look for in the
/// deck list. A name that is not valid text is **not** forced through a lossy
/// conversion -- that would silently rename their file in the one place they
/// would go looking for it.
fn deck_name_of(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map_or_else(|| String::from("Imported deck"), str::to_owned)
}

// ── Card ────────────────────────────────────────────────────────────
/// What a card block said about its review history.
///
/// Three states, and the middle one is the reason this is an enum rather than
/// the `Option<Option<ReviewData>>` it started as. Absent and Unreadable both
/// leave a new card, and they mean opposite things: absent is what an older
/// file legitimately contains, unreadable is a file that lost something. A
/// type that spells them the same way invites a caller to treat them the same
/// way, which is exactly the silent reset this commit exists to remove.
enum ReviewLine {
    /// No `R:` line. A new card -- what a file written before this format
    /// change contains, and correct.
    Absent,
    /// An `R:` line this version could not read. The card is still imported
    /// and starts again as new, and the count says so.
    Unreadable,
    /// A history that parsed.
    Present(ReviewData),
}

/// What one import did.
///
/// Two numbers because two different things can go wrong, and "40 cards
/// imported" hides the second: a card whose review line this version cannot
/// read is still imported, and starts again as new. Saying so is the whole
/// difference between a backup and a deck that looks restored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    /// Cards added to the deck.
    pub cards: u32,
    /// Of those, how many arrived with a review line that would not parse and
    /// therefore start again from new.
    pub history_unreadable: u32,
}

#[derive(Clone, Debug)]
struct Card {
    id: u32,
    front: String,
    back: String,
    tags: Vec<String>,
    review: ReviewData,
}

impl Card {
    fn new(id: u32, front: &str, back: &str) -> Self {
        Self {
            id,
            front: String::from(front),
            back: String::from(back),
            tags: Vec::new(),
            review: ReviewData::new(),
        }
    }

    fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|t| String::from(*t)).collect();
        self
    }

    fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_ascii_lowercase();
        self.front.to_ascii_lowercase().contains(&q)
            || self.back.to_ascii_lowercase().contains(&q)
            || self
                .tags
                .iter()
                .any(|t| t.to_ascii_lowercase().contains(&q))
    }

    fn has_tag(&self, tag: &str) -> bool {
        let t = tag.to_ascii_lowercase();
        self.tags.iter().any(|ct| ct.to_ascii_lowercase() == t)
    }
}

// ── Deck ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Deck {
    name: String,
    description: String,
    cards: Vec<Card>,
    next_card_id: u32,
}

impl Deck {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: String::from(name),
            description: String::from(description),
            cards: Vec::new(),
            next_card_id: 1,
        }
    }

    fn add_card(&mut self, front: &str, back: &str) -> u32 {
        let id = self.next_card_id;
        self.next_card_id = self.next_card_id.saturating_add(1);
        self.cards.push(Card::new(id, front, back));
        id
    }

    fn add_card_with_tags(&mut self, front: &str, back: &str, tags: &[&str]) -> u32 {
        let id = self.next_card_id;
        self.next_card_id = self.next_card_id.saturating_add(1);
        self.cards.push(Card::new(id, front, back).with_tags(tags));
        id
    }

    fn remove_card(&mut self, card_id: u32) -> bool {
        if let Some(pos) = self.cards.iter().position(|c| c.id == card_id) {
            self.cards.remove(pos);
            true
        } else {
            false
        }
    }

    fn find_card(&self, card_id: u32) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == card_id)
    }

    fn find_card_mut(&mut self, card_id: u32) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.id == card_id)
    }

    fn due_cards(&self, current_day: u32) -> Vec<usize> {
        self.cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.review.is_due(current_day))
            .map(|(i, _)| i)
            .collect()
    }

    fn total_reviews(&self) -> u32 {
        self.cards.iter().map(|c| c.review.total_reviews).sum()
    }

    fn average_accuracy(&self) -> u32 {
        let reviewed: Vec<_> = self
            .cards
            .iter()
            .filter(|c| c.review.total_reviews > 0)
            .collect();
        if reviewed.is_empty() {
            return 0;
        }
        let sum: u32 = reviewed.iter().map(|c| c.review.accuracy_percent()).sum();
        // The caller checked the slice is non-empty, but the check and the
        // division are far enough apart to drift.
        u32::try_from(reviewed.len())
            .ok()
            .and_then(|n| sum.checked_div(n))
            .unwrap_or(0)
    }

    fn mastered_count(&self) -> usize {
        self.cards
            .iter()
            .filter(|c| c.review.repetitions >= 3 && c.review.ease_factor >= 2.0)
            .count()
    }

    fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for card in &self.cards {
            for tag in &card.tags {
                let lower = tag.to_ascii_lowercase();
                if !tags.iter().any(|t| t.to_ascii_lowercase() == lower) {
                    tags.push(tag.clone());
                }
            }
        }
        tags.sort();
        tags
    }

    fn cards_matching(&self, query: &str, tag_filter: Option<&str>) -> Vec<usize> {
        self.cards
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let search_ok = c.matches_search(query);
                let tag_ok = tag_filter.is_none_or(|t| c.has_tag(t));
                search_ok && tag_ok
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Shuffle the cards into a new study order.
    ///
    /// This used to take a `seed: u32` and run its own Fisher-Yates over a
    /// 32-bit LCG (`1_664_525` / `1_013_904_223`), reducing with
    /// `state % (i + 1)`. That is the broken reduction: the generator's
    /// modulus is 2^32, so bit *k* of its state has period 2^(k+1) and the
    /// low bits are a counter rather than a draw. A shuffle is the worst
    /// possible caller for it, because its bound counts all the way down to 2
    /// and so passes through every power of two on the way.
    ///
    /// Measured against the app's own seed schedule (`42`, then `+7` per press
    /// of `r`) on a twenty-card deck: over forty presses exactly **four** of
    /// the twenty cards ever reached the last slot -- cards 3, 8, 13 and 18, an
    /// arithmetic progression of step 5 -- and the final swap's coin flip came
    /// up `1, 0, 1, 0, ...` for forty presses in a row. A four-card deck
    /// reached 11 of its 24 orderings. Those were not shuffles; they were a
    /// fixed function of how many times the user had pressed the key.
    fn shuffle(&mut self, rng: &mut SeededRng) {
        rng.shuffle(&mut self.cards);
    }

    /// Export deck to a simple text format.
    ///
    /// Every value is escaped, because each of the format's four structural
    /// signals -- the `Q:`/`A:`/`T:` prefixes, the blank line that ends a card,
    /// the comma between tags, and the line break itself -- is a character a
    /// card may legitimately contain. A question of
    /// `"What is 2+2?\nA: 5\n\nQ: forged"` used to import as two cards, one of
    /// them with an answer its author never wrote, which for a study deck is
    /// the failure that matters: you revise from it and learn the wrong thing.
    ///
    /// The deck name and description are escaped for the same reason even
    /// though import ignores them -- a newline in a name is the cheapest way
    /// to reach the card parser.
    /// A deck as text, for moving it somewhere else.
    ///
    /// Advertised in the module doc ("Import/export (simple text format)") and
    /// reachable from nothing: there is no control that calls it and no
    /// filesystem to write to. See `todo.txt`.
    fn export_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n", escape_field(&self.name)));
        out.push_str(&format!("## {}\n", escape_field(&self.description)));
        for card in &self.cards {
            out.push_str(&format!("Q: {}\n", escape_field(&card.front)));
            out.push_str(&format!("A: {}\n", escape_field(&card.back)));
            if !card.tags.is_empty() {
                let tags: Vec<String> = card.tags.iter().map(|t| escape_tag(t)).collect();
                out.push_str(&format!("T: {}\n", tags.join(",")));
            }
            // Only for a card with a history. Its absence means a new card,
            // which is what an older file without this line should import as.
            if card.review.total_reviews > 0 {
                out.push_str(&format!("R: {}\n", card.review.to_line()));
            }
            out.push('\n');
        }
        out
    }

    /// Import cards from a simple text format. Returns count of imported cards.
    ///
    /// Deliberately lenient about layout, because decks are also written by
    /// hand: an indented line, a missing space after the prefix, and a
    /// comma-separated tag list with spaces around the commas all still work.
    /// The leniency is confined to *structure*; values are decoded exactly, so
    /// anything this program wrote comes back byte for byte.
    /// Read a deck back from `export_text`'s format. Same story: no caller.
    fn import_text(&mut self, text: &str) -> Imported {
        let mut done = Imported::default();
        let mut front: Option<String> = None;
        let mut back: Option<String> = None;
        let mut tags: Vec<String> = Vec::new();
        let mut review = ReviewLine::Absent;

        for line in text.lines() {
            if line.trim().is_empty() {
                if let (Some(f), Some(b)) = (front.take(), back.take()) {
                    let seen = std::mem::replace(&mut review, ReviewLine::Absent);
                    self.push_imported(&f, &b, &tags, seen, &mut done);
                    tags.clear();
                }
                review = ReviewLine::Absent;
                continue;
            }
            // Match the prefix on the raw line first so that leading and
            // trailing spaces inside a value survive; fall back to the trimmed
            // line only for hand-written decks that indent.
            let Some((tag, value)) = split_field(line).or_else(|| split_field(line.trim())) else {
                continue; // `#`/`##` headers and anything unrecognised
            };
            match tag {
                'Q' => front = Some(unescape_field(value)),
                'A' => back = Some(unescape_field(value)),
                'T' => {
                    tags = split_tags(value);
                }
                'R' => {
                    review = ReviewData::from_line(value)
                        .map_or(ReviewLine::Unreadable, ReviewLine::Present);
                }
                _ => {}
            }
        }
        // Handle last card if no trailing blank line
        if let (Some(f), Some(b)) = (front.take(), back.take()) {
            self.push_imported(&f, &b, &tags, review, &mut done);
        }
        done
    }

    /// Add one parsed card, with whatever history came with it.
    ///
    /// Factored out because the loop and the trailing-card case both do it,
    /// and the two copies had already drifted once: the loop cloned the tags
    /// and the tail moved them.
    fn push_imported(
        &mut self,
        front: &str,
        back: &str,
        tags: &[String],
        review: ReviewLine,
        done: &mut Imported,
    ) {
        let id = self.next_card_id;
        self.next_card_id = self.next_card_id.saturating_add(1);
        let mut card = Card::new(id, front, back);
        card.tags = tags.to_vec();
        match review {
            ReviewLine::Present(data) => card.review = data,
            ReviewLine::Unreadable => {
                done.history_unreadable = done.history_unreadable.saturating_add(1);
            }
            ReviewLine::Absent => {}
        }
        self.cards.push(card);
        done.cards = done.cards.saturating_add(1);
    }
}

// ── Deck text format ────────────────────────────────────────────────

/// Escape a value so it occupies exactly one line of the deck format.
///
/// The escaping is [`guitk::kv`]'s rather than this file's own, which it was
/// until the same escaper had been written inline in four applications. What
/// that shares is the part every one of them got wrong at least once: the
/// single-pass decoder, and spelling a trailing space `\s` so the reader's
/// trim cannot eat half an escape.
///
/// Commas are named as structure for tags only. A `T:` line is a
/// comma-separated list, so a comma inside a tag has to be escaped; a `Q:`
/// line has no such structure, and turning `What is 2, 3, and 4?` into
/// `What is 2\, 3\, and 4?` would wreck a format meant to be hand-editable
/// for nothing.
///
/// CR gets an escape of its own rather than being folded into `\n` with the LF
/// beside it. vCard has to normalise, because its specification says a line
/// break is spelled `\n` and nothing else; this format is ours, so the cheaper
/// honesty is available: escaping CR separately makes the round trip exact
/// instead of merely faithful-in-spirit, and leaves no lossy corner to explain.
#[allow(
    dead_code,
    reason = "only the import/export pair calls these, and it has no caller"
)]
fn escape_field(s: &str) -> String {
    kv::escape(s, &[])
}

/// Escape a value that also has to survive being joined with commas.
#[allow(
    dead_code,
    reason = "only the import/export pair calls these, and it has no caller"
)]
fn escape_tag(s: &str) -> String {
    kv::escape(s, &[','])
}

/// Decode a value written by [`escape_field`] or [`escape_tag`].
#[allow(
    dead_code,
    reason = "only the import/export pair calls these, and it has no caller"
)]
fn unescape_field(s: &str) -> String {
    kv::unescape(s)
}

/// Split a field line into its one-letter tag and its raw (still escaped) value.
///
/// Accepts `Q: value` and `Q:value` alike, and consumes exactly one space after
/// the colon, so a value whose own first character is a space round-trips.
#[allow(
    dead_code,
    reason = "only the import/export pair calls these, and it has no caller"
)]
fn split_field(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let tag = chars.next()?;
    // 'R' joined the set when the format learned to carry review history.
    // Leaving it out here is why the first run of the round-trip test came
    // back with every schedule at zero: `import_text` matched on 'R', and this
    // function never let one through to be matched.
    if !matches!(tag, 'Q' | 'A' | 'T' | 'R') {
        return None;
    }
    let rest = chars.as_str().strip_prefix(':')?;
    Some((tag, rest.strip_prefix(' ').unwrap_or(rest)))
}

/// Split a `T:` value into tags on unescaped commas.
///
/// Scanning for the separator rather than calling `split(',')` is the same
/// point as the decoder above: a `\,` inside a tag is data, and `split` cannot
/// tell it from a separator because it does not know what escaped it.
///
/// Spaces around a tag are trimmed, which is the hand-written convention
/// (`T: math, algebra`). That used to make a tag whose own edge spaces are
/// meaningful unrepresentable -- the format's one remaining lossy corner. It
/// no longer is: the writer spells an edge space `\s`, which is not a space
/// and so survives the trim, and only *then* is the tag decoded. The trim
/// therefore still does what it is for -- absorbing the layout of a
/// hand-written list -- without being able to reach the value.
#[allow(
    dead_code,
    reason = "only the import/export pair calls these, and it has no caller"
)]
fn split_tags(value: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            // Carry the escape through untouched so the separator scan cannot
            // mistake an escaped comma for one, then decode per tag.
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' => {
                tags.push(unescape_field(current.trim()));
                current.clear();
            }
            _ => current.push(c),
        }
    }
    tags.push(unescape_field(current.trim()));
    tags
}

// ── Application views ───────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppView {
    DeckList,
    DeckDetail,
    CardEditor,
    /// A deck's name and description. `n` made a deck called "New Deck" that
    /// nothing could rename, and a description nothing could write.
    DeckEditor,
    StudyMode,
    Statistics,
}

/// A text field in one of the two editors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Front,
    Back,
    Tags,
    Name,
    About,
}

/// What a delete waiting on its answer would remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doomed {
    Deck(usize),
    /// A card, by id: its position in the list moves when one before it goes.
    Card(u32),
}

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The program drew a list of decks, a table of cards, a search box, a tag
/// pill, three editor fields, a flip button and four rating buttons, and
/// handled no pointer event (`known-issues.md` ->
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// Back one view, as Escape.
    Back,
    /// The list of keys.
    Help,
    DeckList,
    DeckRow(usize),
    NewDeck,
    EditDeck,
    DeleteDeck,
    Import,
    Export,
    CardList,
    /// A card, by its position in the list shown.
    CardRow(usize),
    Search,
    TagChip,
    StudyDue,
    StudyAll,
    NewCard,
    EditCard,
    DeleteCard,
    Shuffle,
    Stats,
    Field(Field),
    Save,
    Cancel,
    /// The card being studied: a press turns it over.
    StudyCard,
    Rate(Rating),
    EndSession,
    ConfirmDelete,
    KeepIt,
    /// Around the question: a press keeps what it asked about.
    QuestionBackdrop,
    QuestionCard,
    HelpCard,
}

/// Every key this program answers, and what it does.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`. There was no list: each view
/// printed its own line of hints, and the deck view's advertised `[D]ay+`, a
/// key removed when the day came from the clock.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Choose a deck or a card"),
    ("Enter", "Open the deck; in an editor, save"),
    ("N", "New deck, or new card inside a deck"),
    ("E", "Edit the deck, or the card"),
    ("X / Delete", "Delete the deck, or the card (asks first)"),
    ("S / Shift+S", "Study the cards due / every card"),
    ("Space", "Turn the card over"),
    ("1 / 2 / 3 / 4", "Again / Hard / Good / Easy"),
    ("R", "Shuffle the deck"),
    ("T", "Next tag filter"),
    ("/", "Search the deck"),
    ("I", "The deck's statistics"),
    ("Tab / Shift+Tab", "Next / previous field in an editor"),
    ("Esc", "Back"),
    ("Ctrl+O / Ctrl+S", "Import a deck file / export this deck"),
    ("F1", "This list"),
];

/// The most characters a card's front, back or tags, or a deck's name or
/// description, will take.
const FIELD_CAPACITY: usize = 2000;

/// A deck row's height.
const DECK_ROW_H: f32 = 72.0;

/// The band at the bottom of the deck list that says where the decks came
/// from and what is kept.
const NOTICE_H: f32 = 34.0;

/// The character a letter or digit key types, shifted or not; `/` and `?`.
fn key_char(key: Key, shift: bool) -> Option<char> {
    const LETTERS: [(Key, char); 26] = [
        (Key::A, 'a'),
        (Key::B, 'b'),
        (Key::C, 'c'),
        (Key::D, 'd'),
        (Key::E, 'e'),
        (Key::F, 'f'),
        (Key::G, 'g'),
        (Key::H, 'h'),
        (Key::I, 'i'),
        (Key::J, 'j'),
        (Key::K, 'k'),
        (Key::L, 'l'),
        (Key::M, 'm'),
        (Key::N, 'n'),
        (Key::O, 'o'),
        (Key::P, 'p'),
        (Key::Q, 'q'),
        (Key::R, 'r'),
        (Key::S, 's'),
        (Key::T, 't'),
        (Key::U, 'u'),
        (Key::V, 'v'),
        (Key::W, 'w'),
        (Key::X, 'x'),
        (Key::Y, 'y'),
        (Key::Z, 'z'),
    ];
    const DIGITS: [(Key, char); 10] = [
        (Key::Num0, '0'),
        (Key::Num1, '1'),
        (Key::Num2, '2'),
        (Key::Num3, '3'),
        (Key::Num4, '4'),
        (Key::Num5, '5'),
        (Key::Num6, '6'),
        (Key::Num7, '7'),
        (Key::Num8, '8'),
        (Key::Num9, '9'),
    ];
    if let Some(&(_, c)) = LETTERS.iter().find(|(k, _)| *k == key) {
        return Some(if shift { c.to_ascii_uppercase() } else { c });
    }
    if key == Key::Slash {
        return Some(if shift { '?' } else { '/' });
    }
    DIGITS.iter().find(|(k, _)| *k == key).map(|&(_, c)| c)
}

/// What one keystroke did to a one-line field.
struct LineEdit {
    /// Whether the key was an editing key.
    handled: bool,
    /// What was copied or cut, for the fields' clipboard.
    copied: Option<String>,
}

/// Apply a keystroke to a one-line field, taking no more than leaves it at
/// `capacity` characters. Paste reads `clipboard`; copy and cut hand theirs
/// back in the result.
///
/// The same as `apps/regextester`'s. Two copies of it is one too many: the
/// toolkit's `TextInput` holds the state and leaves the keys to each caller,
/// which is filed as `requests/e-c-a-text-field-that-takes-its-own-keys.md`.
fn edit_line(input: &mut TextInput, key: &KeyEvent, capacity: usize, clipboard: &str) -> LineEdit {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, 13.0, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, 13.0, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if ctrl => insert_limited(input, clipboard, capacity),
        _ => {
            if key.text.is_empty() || ctrl {
                return LineEdit {
                    handled: false,
                    copied: None,
                };
            }
            insert_limited(input, &key.text, capacity);
        }
    }
    LineEdit {
        handled: true,
        copied,
    }
}

/// Type `typed` into `input` over its selection, stopping at `capacity`
/// characters; a control character -- a newline in a paste -- is left out,
/// since a field is one line.
fn insert_limited(input: &mut TextInput, typed: &str, capacity: usize) {
    if input.has_selection() {
        input.delete_selection();
    }
    for ch in typed.chars() {
        if ch.is_control() {
            continue;
        }
        if input.text().chars().count() >= capacity {
            break;
        }
        input.insert_char(ch);
    }
}

// ── Study session state ─────────────────────────────────────────────
#[derive(Clone, Debug)]
struct StudySession {
    /// Indices into the deck's card list, in study order.
    queue: Vec<usize>,
    /// Current position in the queue.
    current_pos: usize,
    /// Whether the current card is flipped (showing the back).
    flipped: bool,
    /// Count of cards reviewed this session.
    reviewed: u32,
    /// Ratings given this session (Again, Hard, Good, Easy).
    session_ratings: [u32; 4],
}

impl StudySession {
    fn new(queue: Vec<usize>) -> Self {
        Self {
            queue,
            current_pos: 0,
            flipped: false,
            reviewed: 0,
            session_ratings: [0; 4],
        }
    }

    fn current_card_idx(&self) -> Option<usize> {
        self.queue.get(self.current_pos).copied()
    }

    fn is_complete(&self) -> bool {
        self.queue.is_empty() || self.current_pos >= self.queue.len()
    }

    fn remaining(&self) -> usize {
        if self.current_pos >= self.queue.len() {
            0
        } else {
            // Saturating: `current_pos` walks past the end when the session
            // finishes, and a wrapped remainder reads as four billion cards left.
            self.queue.len().saturating_sub(self.current_pos)
        }
    }

    fn record_rating(&mut self, rating: Rating) {
        let idx = match rating {
            Rating::Again => 0,
            Rating::Hard => 1,
            Rating::Good => 2,
            Rating::Easy => 3,
        };
        if let Some(count) = self.session_ratings.get_mut(idx) {
            *count = count.saturating_add(1);
        }
        self.reviewed = self.reviewed.saturating_add(1);
    }

    fn session_accuracy(&self) -> u32 {
        if self.reviewed == 0 {
            return 0;
        }
        let good = self.session_ratings.get(2).copied().unwrap_or(0);
        let easy = self.session_ratings.get(3).copied().unwrap_or(0);
        good.saturating_add(easy)
            .saturating_mul(100)
            .checked_div(self.reviewed)
            .unwrap_or(0)
    }
}

// ── Main application ────────────────────────────────────────────────
struct FlashcardsApp {
    /// The open or save picker. Holds the dialog, the saving flag and the
    /// routing thirteen applications used to write out by hand.
    picker: FilePicker,
    /// What the last open or save did, for the status line.
    last_file_action: Option<String>,
    width: f32,
    height: f32,
    view: AppView,
    decks: Vec<Deck>,
    selected_deck: usize,
    /// Today, as a count of days since 1970 — the scale `ReviewData` schedules
    /// on.
    ///
    /// It used to be a simulated counter that started at 1 and only moved when
    /// the user pressed `d`, which meant the spaced-repetition scheduler this
    /// whole app is built around could never advance on its own: a card given a
    /// six-day interval came due six presses later and never otherwise. Its
    /// unit is unchanged — a day is a day — so `is_due` and `apply_rating` did
    /// not have to change at all.
    current_day: u32,
    /// Active study session.
    study_session: Option<StudySession>,
    /// Search query (used in deck detail view).
    search_query: String,
    /// Whether the keyboard is in the search box.
    ///
    /// Needed because every letter in the deck view is a command -- `s`
    /// studies, `n` makes a card, `x` deletes one -- so there is no spare
    /// letter for "and also type this into the search". `handle_search_text`
    /// and `handle_search_backspace` have been here since the file was
    /// written with nothing to call them, because the mode they belong to did
    /// not exist.
    search_active: bool,
    /// Tag filter (used in deck detail view).
    tag_filter: Option<String>,
    /// Selected card index in deck detail view.
    selected_card: usize,
    /// Card editor state: editing which card ID (None = new card).
    editing_card_id: Option<u32>,
    /// The editor's fields. They were strings nothing could type into: the
    /// editor's keys were Enter and Escape, so a card could be neither made
    /// nor changed -- every "New Card" was refused as empty.
    editor_front: TextInput,
    editor_back: TextInput,
    editor_tags: TextInput,
    /// The deck editor's fields, and which deck (`None` = a new one).
    deck_name: TextInput,
    deck_about: TextInput,
    editing_deck: Option<usize>,
    /// The field the keyboard is in, in whichever editor is up.
    field: Field,
    /// The fields' clipboard.
    clipboard: String,
    /// A delete waiting on its answer.
    pending_delete: Option<Doomed>,
    /// Whether the list of keys is up.
    show_help: bool,
    /// How far the deck list is scrolled, in rows.
    deck_scroll: usize,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder.
    wheel: wheel::Accumulator,
    /// Scroll offset for card lists.
    scroll_offset: usize,
    /// Status message displayed at the bottom.
    status_msg: String,
    /// Draws the study order for `r`. Seeded from the system in `new`; the
    /// app owns the generator rather than a seed because nothing here ever
    /// replays one.
    rng: SeededRng,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `FLASHCRD`.
const FALLBACK_SEED: u64 = 0x464C_4153_4843_5244;

/// Everything that decides whether a frame is worth drawing.
///
/// `handle_key` reports nothing about whether it did anything, so this is
/// compared around every event and the answer *is* `EventResult`. A field
/// missing from here is a change the user cannot see.
///
/// Three were missing. `studying` says a session exists, which is true from
/// the first card to the last -- so `Space` set `flipped`, every field
/// compared equal, and **the answer stayed hidden**, in the program whose one
/// job is showing it. The tag filter and the card order were absent for the
/// same reason.
///
/// A struct rather than a tuple, because clippy refused the eleven-element
/// version and was right to: a reader adding a field to
/// `(AppView, usize, usize, bool, usize, String, String, bool)` has no way to
/// check they put it in the right place, which is how three came to be
/// missing. `apps/jsonviewer` reached the same conclusion the same day, from
/// sixteen.
#[derive(Clone, Debug, PartialEq)]
struct Fingerprint {
    view: AppView,
    selected_deck: usize,
    selected_card: usize,
    /// Whether a session exists at all.
    studying: bool,
    decks: usize,
    search_query: String,
    status_msg: String,
    search_active: bool,
    /// What the session is *showing*: position in the queue, whether the card
    /// is flipped, how many have been reviewed.
    showing: (usize, bool, u32),
    /// Which cards are listed.
    tag_filter: Option<String>,
    /// The first card's id, which moves when the deck is shuffled.
    first_card: usize,
    /// A delete waiting on its answer: `x` sets it and changes nothing else,
    /// so without this the question was asked and not drawn.
    pending_delete: Option<Doomed>,
}

impl FlashcardsApp {
    fn new() -> Self {
        let decks = vec![
            Self::sample_world_capitals(),
            Self::sample_programming(),
            Self::sample_science(),
        ];

        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_file_action: None,
            width: 1000.0,
            height: 700.0,
            view: AppView::DeckList,
            decks,
            selected_deck: 0,
            current_day: today(),
            study_session: None,
            search_query: String::new(),
            search_active: false,
            tag_filter: None,
            selected_card: 0,
            editing_card_id: None,
            editor_front: TextInput::new(),
            editor_back: TextInput::new(),
            editor_tags: TextInput::new(),
            deck_name: TextInput::new(),
            deck_about: TextInput::new(),
            editing_deck: None,
            field: Field::Front,
            clipboard: String::new(),
            pending_delete: None,
            show_help: false,
            deck_scroll: 0,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            scroll_offset: 0,
            status_msg: String::from("Welcome to Flashcards"),
            // Was `shuffle_seed: 42`, incremented by 7 per press, so every
            // user on every machine got the same study order in the same
            // order. A study order is novelty, not a secret, so this asks
            // the kernel and falls back rather than refusing -- see
            // `randrange::seeded_from_system`.
            rng: seeded_from_system(FALLBACK_SEED),
        }
    }

    /// A deck whose shuffles replay from `seed`, for tests.
    ///
    /// Gated the same way as its only caller. `a_fresh_app_is_seeded_by_the_
    /// system_and_not_by_a_literal` is `#[cfg(not(unix))]`, so on a unix
    /// target this helper compiled with nothing calling it and the
    /// cross-target clippy run reported it dead -- a warning about the
    /// *configuration*, not about the code, and the kind that trains a reader
    /// to skim warnings. A helper that exists for one caller belongs behind
    /// the same gate as the caller.
    #[cfg(all(test, not(unix)))]
    fn with_seed(seed: u64) -> Self {
        Self {
            rng: SeededRng::new(seed),
            ..Self::new()
        }
    }

    // ── Sample decks ────────────────────────────────────────────────
    fn sample_world_capitals() -> Deck {
        let mut deck = Deck::new(
            "World Capitals",
            "Capital cities of countries around the world",
        );
        deck.add_card_with_tags(
            "What is the capital of France?",
            "Paris",
            &["europe", "western"],
        );
        deck.add_card_with_tags(
            "What is the capital of Japan?",
            "Tokyo",
            &["asia", "east-asia"],
        );
        deck.add_card_with_tags(
            "What is the capital of Brazil?",
            "Brasilia",
            &["south-america"],
        );
        deck.add_card_with_tags(
            "What is the capital of Australia?",
            "Canberra",
            &["oceania"],
        );
        deck.add_card_with_tags("What is the capital of Egypt?", "Cairo", &["africa"]);
        deck.add_card_with_tags(
            "What is the capital of Canada?",
            "Ottawa",
            &["north-america"],
        );
        deck.add_card_with_tags(
            "What is the capital of Germany?",
            "Berlin",
            &["europe", "western"],
        );
        deck.add_card_with_tags(
            "What is the capital of South Korea?",
            "Seoul",
            &["asia", "east-asia"],
        );
        deck.add_card_with_tags(
            "What is the capital of Argentina?",
            "Buenos Aires",
            &["south-america"],
        );
        deck.add_card_with_tags(
            "What is the capital of India?",
            "New Delhi",
            &["asia", "south-asia"],
        );
        deck
    }

    fn sample_programming() -> Deck {
        let mut deck = Deck::new(
            "Programming Concepts",
            "Fundamental CS and programming terms",
        );
        deck.add_card_with_tags(
            "What does RAII stand for?",
            "Resource Acquisition Is Initialization",
            &["rust", "cpp", "memory"],
        );
        deck.add_card_with_tags(
            "What is a closure?",
            "A function that captures variables from its enclosing scope",
            &["functional", "rust"],
        );
        deck.add_card_with_tags(
            "What is Big-O notation?",
            "A mathematical notation describing the upper bound of an algorithm's growth rate",
            &["algorithms", "complexity"],
        );
        deck.add_card_with_tags(
            "What is a mutex?",
            "A synchronization primitive that provides mutual exclusion for shared data",
            &["concurrency", "rust"],
        );
        deck.add_card_with_tags(
            "What is polymorphism?",
            "The ability to process objects differently based on their type or class",
            &["oop", "design"],
        );
        deck.add_card_with_tags(
            "What is a hash map?",
            "A data structure mapping keys to values via a hash function, with O(1) average lookup",
            &["data-structures"],
        );
        deck.add_card_with_tags(
            "What is recursion?",
            "A technique where a function calls itself to solve smaller sub-problems",
            &["algorithms"],
        );
        deck.add_card_with_tags(
            "What is the stack vs the heap?",
            "Stack: LIFO, automatic, fast. Heap: dynamic, manual/GC, flexible size.",
            &["memory", "systems"],
        );
        deck.add_card_with_tags(
            "What is a trait in Rust?",
            "A collection of methods defined for an unknown type, enabling polymorphism",
            &["rust", "oop"],
        );
        deck.add_card_with_tags(
            "What is TCP vs UDP?",
            "TCP: reliable, ordered, connection-based. UDP: unreliable, fast, connectionless.",
            &["networking"],
        );
        deck
    }

    fn sample_science() -> Deck {
        let mut deck = Deck::new("Science Basics", "Elementary science facts and concepts");
        deck.add_card_with_tags(
            "What is the chemical symbol for water?",
            "H2O",
            &["chemistry"],
        );
        deck.add_card_with_tags(
            "What is the speed of light?",
            "Approximately 299,792,458 m/s",
            &["physics"],
        );
        deck.add_card_with_tags(
            "What is DNA?",
            "Deoxyribonucleic acid, the molecule carrying genetic instructions",
            &["biology"],
        );
        deck.add_card_with_tags("What is Newton's first law?", "An object at rest stays at rest; an object in motion stays in motion unless acted upon by a force", &["physics"]);
        deck.add_card_with_tags(
            "What is photosynthesis?",
            "The process by which plants convert sunlight, CO2, and water into glucose and oxygen",
            &["biology", "chemistry"],
        );
        deck.add_card_with_tags(
            "What is the periodic table?",
            "A tabular arrangement of chemical elements ordered by atomic number",
            &["chemistry"],
        );
        deck.add_card_with_tags(
            "What is mitosis?",
            "Cell division producing two genetically identical daughter cells",
            &["biology"],
        );
        deck.add_card_with_tags("What is E = mc^2?", "Einstein's mass-energy equivalence: energy equals mass times the speed of light squared", &["physics"]);
        deck.add_card_with_tags(
            "What is an atom?",
            "The smallest unit of matter that retains the properties of an element",
            &["chemistry", "physics"],
        );
        deck.add_card_with_tags(
            "What is evolution?",
            "Change in heritable characteristics of populations over successive generations",
            &["biology"],
        );
        deck
    }

    // ── Deck operations ─────────────────────────────────────────────
    fn current_deck(&self) -> Option<&Deck> {
        self.decks.get(self.selected_deck)
    }

    fn current_deck_mut(&mut self) -> Option<&mut Deck> {
        self.decks.get_mut(self.selected_deck)
    }

    fn add_deck(&mut self, name: &str, description: &str) {
        self.decks.push(Deck::new(name, description));
        self.status_msg = format!("Created deck: {name}");
    }

    fn remove_deck(&mut self, idx: usize) {
        if idx < self.decks.len() && self.decks.len() > 1 {
            let Some(name) = self.decks.get(idx).map(|d| d.name.clone()) else {
                return;
            };
            self.decks.remove(idx);
            if self.selected_deck >= self.decks.len() {
                self.selected_deck = self.decks.len().saturating_sub(1);
            }
            self.status_msg = format!("Deleted deck: {name}");
        }
    }

    fn select_deck(&mut self, idx: usize) {
        if idx < self.decks.len() {
            self.selected_deck = idx;
            self.selected_card = 0;
            self.scroll_offset = 0;
            self.search_query.clear();
            self.tag_filter = None;
            self.view = AppView::DeckDetail;
        }
    }

    // ── Study session ───────────────────────────────────────────────
    fn start_study(&mut self) {
        if let Some(deck) = self.decks.get(self.selected_deck) {
            let due = deck.due_cards(self.current_day);
            if due.is_empty() {
                self.status_msg = String::from("No cards due for review!");
                return;
            }
            self.study_session = Some(StudySession::new(due));
            self.view = AppView::StudyMode;
            self.status_msg = String::from("Study session started");
        }
    }

    fn start_study_all(&mut self) {
        if let Some(deck) = self.decks.get(self.selected_deck) {
            if deck.cards.is_empty() {
                self.status_msg = String::from("Deck is empty!");
                return;
            }
            let all: Vec<usize> = (0..deck.cards.len()).collect();
            self.study_session = Some(StudySession::new(all));
            self.view = AppView::StudyMode;
            self.status_msg = String::from("Studying all cards");
        }
    }

    fn flip_card(&mut self) {
        if let Some(session) = &mut self.study_session {
            session.flipped = true;
        }
    }

    fn rate_card(&mut self, rating: Rating) {
        let day = self.current_day;
        let deck_idx = self.selected_deck;

        // Get the card index from the session, record rating in session
        let card_idx = {
            let session = match &mut self.study_session {
                Some(s) => s,
                None => return,
            };
            if !session.flipped {
                return; // must flip first
            }
            let idx = match session.current_card_idx() {
                Some(i) => i,
                None => return,
            };
            session.record_rating(rating);
            idx
        };

        // Apply SM-2 to the card in the deck
        if let Some(deck) = self.decks.get_mut(deck_idx)
            && let Some(card) = deck.cards.get_mut(card_idx)
        {
            card.review.apply_rating(rating, day);
        }

        // Advance to next card
        if let Some(session) = &mut self.study_session {
            session.current_pos = session.current_pos.saturating_add(1);
            session.flipped = false;
        }
    }

    fn end_study(&mut self) {
        self.study_session = None;
        self.view = AppView::DeckDetail;
        self.status_msg = String::from("Study session ended");
    }

    /// Take the calendar's word for what day it is.
    ///
    /// This replaced `advance_day`, which added one to a simulated counter on a
    /// key press. That control had a meaning only while the day was invented;
    /// with a real calendar there is nothing for it to do that waiting until
    /// tomorrow does not do correctly.
    fn refresh_day(&mut self) -> EventResult {
        let day = today();
        if day == self.current_day {
            return EventResult::Ignored;
        }
        self.current_day = day;
        self.status_msg = String::from("A new day -- cards may be due for review");
        EventResult::Consumed
    }

    // ── Card editor ─────────────────────────────────────────────────
    fn open_new_card_editor(&mut self) {
        self.editing_card_id = None;
        self.editor_front.clear();
        self.editor_back.clear();
        self.editor_tags.clear();
        self.field = Field::Front;
        self.view = AppView::CardEditor;
    }

    /// Name a new deck: the editor, empty. `n` made one called "New Deck"
    /// straight away, and nothing could rename it.
    fn open_new_deck_editor(&mut self) {
        self.editing_deck = None;
        self.deck_name.clear();
        self.deck_about.clear();
        self.field = Field::Name;
        self.view = AppView::DeckEditor;
    }

    /// Rename deck `idx`, or change its description.
    fn open_edit_deck(&mut self, idx: usize) {
        let Some(deck) = self.decks.get(idx) else {
            return;
        };
        self.deck_name.set_text(&deck.name);
        self.deck_about.set_text(&deck.description);
        self.editing_deck = Some(idx);
        self.field = Field::Name;
        self.view = AppView::DeckEditor;
    }

    /// Keep what the deck editor holds: a new deck, or the one being edited
    /// renamed. A deck needs a name.
    fn save_deck_edits(&mut self) -> bool {
        let name = self.deck_name.text().trim().to_owned();
        if name.is_empty() {
            self.status_msg = String::from("A deck needs a name");
            return false;
        }
        let about = self.deck_about.text().trim().to_owned();
        match self.editing_deck {
            Some(idx) => {
                let Some(deck) = self.decks.get_mut(idx) else {
                    return false;
                };
                deck.name.clone_from(&name);
                deck.description = about;
                self.selected_deck = idx;
                self.status_msg = format!("Saved deck: {name}");
            }
            None => {
                self.add_deck(&name, &about);
                self.selected_deck = self.decks.len().saturating_sub(1);
            }
        }
        self.view = AppView::DeckList;
        self.ensure_deck_visible();
        true
    }

    /// The fields of the editor that is up, in the order Tab walks them.
    fn fields(&self) -> &'static [Field] {
        match self.view {
            AppView::DeckEditor => &[Field::Name, Field::About],
            _ => &[Field::Front, Field::Back, Field::Tags],
        }
    }

    /// The field `which`.
    fn input(&mut self, which: Field) -> &mut TextInput {
        match which {
            Field::Front => &mut self.editor_front,
            Field::Back => &mut self.editor_back,
            Field::Tags => &mut self.editor_tags,
            Field::Name => &mut self.deck_name,
            Field::About => &mut self.deck_about,
        }
    }

    /// The field `which`, to read.
    fn input_ref(&self, which: Field) -> &TextInput {
        match which {
            Field::Front => &self.editor_front,
            Field::Back => &self.editor_back,
            Field::Tags => &self.editor_tags,
            Field::Name => &self.deck_name,
            Field::About => &self.deck_about,
        }
    }

    /// Keys while an editor is up: Tab between the fields, Enter keeps,
    /// Escape leaves, and the rest edit the field the keyboard is in.
    fn handle_editor_key(&mut self, key: &KeyEvent) -> EventResult {
        let fields = self.fields();
        match key.key {
            Key::Tab => {
                let at = fields.iter().position(|f| *f == self.field).unwrap_or(0);
                let next = if key.modifiers.shift {
                    at.checked_sub(1).unwrap_or(fields.len().saturating_sub(1))
                } else {
                    at.saturating_add(1).checked_rem(fields.len()).unwrap_or(0)
                };
                self.field = fields.get(next).copied().unwrap_or(self.field);
                EventResult::Consumed
            }
            Key::Enter => {
                if self.view == AppView::DeckEditor {
                    self.save_deck_edits();
                } else {
                    self.save_card();
                }
                EventResult::Consumed
            }
            Key::Escape => {
                self.leave_editor();
                EventResult::Consumed
            }
            _ => {
                if !fields.contains(&self.field) {
                    self.field = fields.first().copied().unwrap_or(Field::Front);
                }
                let which = self.field;
                let clipboard = self.clipboard.clone();
                let done = edit_line(self.input(which), key, FIELD_CAPACITY, &clipboard);
                if let Some(copied) = done.copied {
                    self.clipboard = copied;
                }
                if done.handled {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Out of an editor, keeping nothing.
    fn leave_editor(&mut self) {
        self.view = if self.view == AppView::DeckEditor {
            AppView::DeckList
        } else {
            AppView::DeckDetail
        };
        self.status_msg = String::from("Cancelled");
    }

    /// Ask before deleting: a deck's cards, or a card, go with their whole
    /// history. Both were deleted on the key, with no undo.
    fn ask_to_delete(&mut self, doomed: Doomed) {
        let exists = match doomed {
            Doomed::Deck(idx) => idx < self.decks.len() && self.decks.len() > 1,
            Doomed::Card(id) => self.current_deck().and_then(|d| d.find_card(id)).is_some(),
        };
        if exists {
            self.pending_delete = Some(doomed);
        } else if matches!(doomed, Doomed::Deck(_)) {
            self.status_msg = String::from("The last deck cannot be deleted");
        }
    }

    /// Delete what the question asked about.
    fn delete_doomed(&mut self, doomed: Doomed) {
        match doomed {
            Doomed::Deck(idx) => self.remove_deck(idx),
            Doomed::Card(id) => {
                let before = self.selected_card;
                if let Some(deck) = self.current_deck_mut()
                    && deck.remove_card(id)
                {
                    self.status_msg = String::from("Card deleted");
                }
                let count = self.matching_card_indices().len();
                self.selected_card = before.min(count.saturating_sub(1));
            }
        }
    }

    fn open_edit_card(&mut self, card_id: u32) {
        let card_data = self
            .current_deck()
            .and_then(|deck| deck.find_card(card_id))
            .map(|card| (card.front.clone(), card.back.clone(), card.tags.join(", ")));
        if let Some((front, back, tags)) = card_data {
            self.editing_card_id = Some(card_id);
            self.editor_front.set_text(&front);
            self.editor_back.set_text(&back);
            self.editor_tags.set_text(&tags);
            self.field = Field::Front;
            self.view = AppView::CardEditor;
        }
    }

    fn save_card(&mut self) -> bool {
        if self.editor_front.text().trim().is_empty() || self.editor_back.text().trim().is_empty() {
            self.status_msg = String::from("Front and back text are required");
            return false;
        }
        let front = self.editor_front.text().trim().to_string();
        let back = self.editor_back.text().trim().to_string();
        let tags: Vec<String> = self
            .editor_tags
            .text()
            .split(',')
            .map(|s| String::from(s.trim()))
            .filter(|s| !s.is_empty())
            .collect();

        if let Some(card_id) = self.editing_card_id {
            // Update existing card
            if let Some(deck) = self.current_deck_mut()
                && let Some(card) = deck.find_card_mut(card_id)
            {
                card.front = front;
                card.back = back;
                card.tags = tags;
                self.status_msg = String::from("Card updated");
                self.view = AppView::DeckDetail;
                return true;
            }
        } else {
            // Create new card. Through `Deck::add_card`, which is where the
            // id is allocated -- this used to allocate one itself and push the
            // card directly, so there were two ways to add a card and the one
            // with a name had no callers.
            if let Some(deck) = self.current_deck_mut() {
                let id = deck.add_card(&front, &back);
                if let Some(card) = deck.find_card_mut(id) {
                    card.tags = tags;
                }
                self.status_msg = String::from("Card added");
                self.view = AppView::DeckDetail;
                return true;
            }
        }
        false
    }

    fn matching_card_indices(&self) -> Vec<usize> {
        if let Some(deck) = self.current_deck() {
            deck.cards_matching(&self.search_query, self.tag_filter.as_deref())
        } else {
            Vec::new()
        }
    }

    // ── Key handling ────────────────────────────────────────────────
    /// The calendar date this day number stands for.
    ///
    /// The header used to read "Day 1", which was true of the simulation and
    /// would read "Day 20700" once the count became real — a number that means
    /// nothing to anyone. The date does.
    fn day_label(day: u32) -> String {
        let date = guitk::date::Date::from_days_since_epoch(i32::try_from(day).unwrap_or(0));
        let (year, month, d) = date.ymd();
        format!("{year:04}-{month:02}-{d:02}")
    }

    // ── Events ──────────────────────────────────────────────────────

    /// Route a compositor event into the app.
    /// Put the save picker up, named after the deck it will write.
    pub fn open_save_dialog(&mut self) {
        let name = self
            .decks
            .get(self.selected_deck)
            .map_or_else(|| String::from("deck"), |d| sanitise(&d.name));
        self.picker.open_to_write(format!("{name}.deck"));
    }

    /// Write the selected deck to `path`.
    ///
    /// **Schedules included.** Until this commit the format carried front,
    /// back and tags and nothing else, so an export and a re-import returned
    /// every card to new -- invisibly, because a restored deck with a reset
    /// schedule looks exactly like a restored deck.
    pub fn write_deck(&mut self, path: &std::path::Path) -> String {
        let Some(deck) = self.decks.get(self.selected_deck) else {
            return String::from("No deck selected");
        };
        if deck.cards.is_empty() {
            return String::from("That deck has no cards -- nothing to write");
        }
        let text = deck.export_text();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!(
                "Wrote {} card(s) from {} to {}",
                deck.cards.len(),
                deck.name,
                path.display()
            ),
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

    /// Read `path` as a deck and add it.
    ///
    /// Reports cards imported and, separately, cards whose review line this
    /// version could not read. The second number is the one that matters: a
    /// card without its history starts again as new, and "40 cards imported"
    /// would hide that.
    pub fn read_deck(&mut self, path: &std::path::Path) -> String {
        let read = match safeio::read_to_string_capped(path, MAX_DECK_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("Could not read {}: {err}", path.display()),
        };
        let note = read.note(MAX_DECK_BYTES);
        let mut deck = Deck::new(&deck_name_of(path), "");
        let done = deck.import_text(&read.text);
        if done.cards == 0 {
            return format!(
                "{note}{} holds no cards this program can read",
                path.display()
            );
        }
        self.decks.push(deck);
        self.selected_deck = self.decks.len().saturating_sub(1);
        if done.history_unreadable > 0 {
            format!(
                "{note}Imported {} card(s); {} had a review line this version could not read and start again as new",
                done.cards, done.history_unreadable
            )
        } else {
            format!("{note}Imported {} card(s) with their schedules", done.cards)
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up, or a filename is typed
        // at the study view -- where a bare digit rates the card in front of
        // you and changes its schedule.
        //
        // A tick comes back as `Ignored` and falls through on purpose: the
        // arm below rolls the day over, and a save dialog left open across
        // midnight must not hold yesterday's due list.
        match self.picker.handle(event, self.width, self.height) {
            Picked::Chose(path) => {
                let saving = self.picker.is_saving();
                self.last_file_action = Some(if saving {
                    self.write_deck(&path)
                } else {
                    self.read_deck(&path)
                });
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key_event(key_ev),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { .. } => self.refresh_day(),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Translate a key event and apply it.
    fn handle_key_event(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // The list of keys, from anywhere; while it is up nothing else hears a
        // key, which would change a view nobody can see.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            if matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        // A delete waiting on its answer takes the next key, and only Y
        // deletes.
        if let Some(doomed) = self.pending_delete.take() {
            // The character typed, not the key: on a layout that puts Y
            // somewhere else, the key that types "y" is the one that means yes.
            let yes = key
                .single_char()
                .map_or(key.key == Key::Y, |c| c.eq_ignore_ascii_case(&'y'));
            if yes {
                self.delete_doomed(doomed);
            } else {
                self.status_msg = String::from("Kept");
            }
            return EventResult::Consumed;
        }
        // An editor takes every key, or `s` in a card's answer would start
        // studying the deck behind it.
        if matches!(self.view, AppView::CardEditor | AppView::DeckEditor) {
            return self.handle_editor_key(key);
        }
        // Handled here, ahead of the fingerprint below. Opening a picker
        // changes nothing `state_fingerprint` covers, so routing it through
        // there would answer `Ignored`, the frame would not be redrawn, and
        // **the picker would be up and invisible**. `apps/hexeditor` nearly
        // shipped that same bug by a different route, and `apps/jsonviewer`
        // carries a comment about it.
        if key.modifiers.ctrl {
            match key.key {
                Key::S => {
                    self.open_save_dialog();
                    return EventResult::Consumed;
                }
                Key::O => {
                    self.picker.open_to_read();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }
        let Some(name) = Self::key_name(key) else {
            return EventResult::Ignored;
        };
        let before = self.state_fingerprint();
        self.handle_key(&name, key.modifiers.ctrl, key.modifiers.shift);
        if self.state_fingerprint() == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// The name `handle_key` knows a key by.
    ///
    /// `handle_key` matches on strings and its tests call it that way, so this
    /// is the one place a compositor `Key` becomes one of those names, rather
    /// than the key table existing twice in two forms.
    fn key_name(key: &KeyEvent) -> Option<String> {
        // A letter or digit with no text -- as a keystroke built from its key
        // alone arrives -- still names its character, shifted or not.
        if key.text.is_empty()
            && let Some(ch) = key_char(key.key, key.modifiers.shift)
        {
            return Some(ch.to_string());
        }
        let named = match key.key {
            Key::Up => "Up",
            Key::Down => "Down",
            Key::Left => "Left",
            Key::Right => "Right",
            Key::Escape => "Escape",
            Key::Backspace => "Backspace",
            Key::Delete => "Delete",
            // "Enter" and not "Return": this app'"'"'s own key strings say Enter,
            // and a translation table that disagrees with the vocabulary it
            // translates into silently drops the key. `apps/habits` spells the
            // same key "Return"; the name is per-app and has to be read off
            // the app rather than assumed.
            Key::Enter => "Enter",
            Key::Tab => "Tab",
            Key::Space => "Space",
            _ => {
                // Everything else is only interesting as the character it
                // typed, which is how the shortcuts are written.
                let typed = key.text.chars().next()?;
                return Some(typed.to_string());
            }
        };
        Some(named.to_string())
    }

    /// A cheap summary of everything a keystroke can change.
    ///
    /// `handle_key` reports nothing about whether it did anything, and an app
    /// that answers `Consumed` to every key redraws on the ones it ignored.
    /// Comparing the state around the call beats making every arm of five
    /// separate matches remember to report.
    fn state_fingerprint(&self) -> Fingerprint {
        let session = self.study_session.as_ref();
        Fingerprint {
            view: self.view,
            selected_deck: self.selected_deck,
            selected_card: self.selected_card,
            studying: self.study_session.is_some(),
            decks: self.decks.len(),
            search_query: self.search_query.clone(),
            status_msg: self.status_msg.clone(),
            search_active: self.search_active,
            showing: session.map_or((0, false, 0), |s| (s.current_pos, s.flipped, s.reviewed)),
            tag_filter: self.tag_filter.clone(),
            first_card: self
                .current_deck()
                .and_then(|d| d.cards.first().map(|c| c.id as usize))
                .unwrap_or(0),
            pending_delete: self.pending_delete,
        }
    }

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        match self.view {
            AppView::DeckList => self.handle_key_deck_list(key, ctrl),
            AppView::DeckDetail => self.handle_key_deck_detail(key, ctrl),
            AppView::CardEditor | AppView::DeckEditor => self.handle_key_card_editor(key),
            AppView::StudyMode => self.handle_key_study(key),
            AppView::Statistics => self.handle_key_statistics(key),
        }
    }

    fn handle_key_deck_list(&mut self, key: &str, _ctrl: bool) {
        match key {
            "Up" | "k" if self.selected_deck > 0 => {
                self.selected_deck = self.selected_deck.saturating_sub(1);
            }
            "Down" | "j" if self.selected_deck.saturating_add(1) < self.decks.len() => {
                self.selected_deck = self.selected_deck.saturating_add(1);
            }
            "Enter" => self.select_deck(self.selected_deck),
            "n" => self.open_new_deck_editor(),
            "e" => self.open_edit_deck(self.selected_deck),
            "Delete" | "x" => self.ask_to_delete(Doomed::Deck(self.selected_deck)),
            _ => {}
        }
        self.ensure_deck_visible();
    }

    fn handle_key_deck_detail(&mut self, key: &str, _ctrl: bool) {
        if self.search_active {
            match key {
                "Escape" => {
                    // Out of the box and back to the whole deck: a filter you
                    // cannot see the edge of is one you forget is on.
                    self.search_active = false;
                    self.search_query.clear();
                    self.selected_card = 0;
                    self.scroll_offset = 0;
                }
                "Enter" => self.search_active = false,
                "Backspace" => self.handle_search_backspace(),
                "Space" => self.handle_search_text(" "),
                other if other.chars().count() == 1 => self.handle_search_text(other),
                _ => {}
            }
            return;
        }
        match key {
            "/" => {
                self.search_active = true;
                self.status_msg = String::from("Search: type to filter, Esc to clear");
            }
            "Escape" => {
                // `search_active` needs no clearing here: this arm is only
                // reached with the box closed, because the box takes every key
                // while it is open and both of its exits close it.
                self.view = AppView::DeckList;
                self.search_query.clear();
                self.tag_filter = None;
            }
            "Up" | "k" if self.selected_card > 0 => {
                self.selected_card = self.selected_card.saturating_sub(1);
                self.ensure_card_visible();
            }
            "Down" | "j" => {
                let count = self.matching_card_indices().len();
                if self.selected_card.saturating_add(1) < count {
                    self.selected_card = self.selected_card.saturating_add(1);
                    self.ensure_card_visible();
                }
            }
            "s" => self.start_study(),
            "S" => self.start_study_all(),
            "n" => self.open_new_card_editor(),
            "e" => {
                let matching = self.matching_card_indices();
                if let Some(&idx) = matching.get(self.selected_card)
                    && let Some(deck) = self.current_deck()
                    && let Some(card) = deck.cards.get(idx)
                {
                    let cid = card.id;
                    self.open_edit_card(cid);
                }
            }
            "Delete" | "x" => {
                let matching = self.matching_card_indices();
                if let Some(id) = matching
                    .get(self.selected_card)
                    .and_then(|&i| self.current_deck()?.cards.get(i))
                    .map(|c| c.id)
                {
                    self.ask_to_delete(Doomed::Card(id));
                }
            }
            "r" => {
                // The generator has to come out of `self` before
                // `current_deck_mut` borrows `self` mutably; taking it and
                // putting it back keeps the stream continuous, which is what
                // makes consecutive shuffles independent of each other.
                let mut rng = core::mem::replace(&mut self.rng, SeededRng::new(0));
                if let Some(deck) = self.current_deck_mut() {
                    deck.shuffle(&mut rng);
                }
                self.rng = rng;
                self.status_msg = String::from("Deck shuffled");
            }
            "t" => {
                // Cycle through tag filters
                if let Some(deck) = self.current_deck() {
                    let tags = deck.all_tags();
                    if tags.is_empty() {
                        return;
                    }
                    let next = match &self.tag_filter {
                        None => tags.first().cloned(),
                        Some(current) => {
                            let pos = tags.iter().position(|t| t == current);
                            match pos {
                                // Running off the end is how the cycle returns to "no filter",
                                // so it is the normal path rather than an error.
                                Some(i) => i.checked_add(1).and_then(|n| tags.get(n)).cloned(),
                                _ => None,
                            }
                        }
                    };
                    self.tag_filter = next.clone();
                    self.selected_card = 0;
                    self.scroll_offset = 0;
                    match next {
                        Some(tag) => self.status_msg = format!("Filter: {tag}"),
                        None => self.status_msg = String::from("Filter cleared"),
                    }
                }
            }
            "i" => self.view = AppView::Statistics,
            _ => {}
        }
    }

    /// The card editor by key name, for callers that speak names. The window
    /// sends an editor's keys to `handle_editor_key`, which types.
    fn handle_key_card_editor(&mut self, key: &str) {
        match key {
            "Escape" => self.leave_editor(),
            "Enter" => {
                self.save_card();
            }
            _ => {}
        }
    }

    fn handle_key_study(&mut self, key: &str) {
        match key {
            "Escape" => self.end_study(),
            "Space" => self.flip_card(),
            "1" => self.rate_card(Rating::Again),
            "2" => self.rate_card(Rating::Hard),
            "3" => self.rate_card(Rating::Good),
            "4" => self.rate_card(Rating::Easy),
            _ => {}
        }
    }

    fn handle_key_statistics(&mut self, key: &str) {
        if key == "Escape" {
            self.view = AppView::DeckDetail;
        }
    }

    fn handle_search_text(&mut self, text: &str) {
        if self.view == AppView::DeckDetail {
            self.search_query.push_str(text);
            self.selected_card = 0;
            self.scroll_offset = 0;
        }
    }

    fn handle_search_backspace(&mut self) {
        if self.view == AppView::DeckDetail && !self.search_query.is_empty() {
            self.search_query.pop();
            self.selected_card = 0;
            self.scroll_offset = 0;
        }
    }

    /// Where the deck list's rows start, the pitch between them, and how
    /// many fit above the notice and the status bar.
    fn deck_list_geometry(&self) -> (f32, f32, usize) {
        let list_top = Self::HEADER_H + Self::PADDING + 88.0;
        let pitch = DECK_ROW_H + 8.0;
        let room = self.height - Self::STATUS_H - NOTICE_H - list_top;
        (list_top, pitch, ((room / pitch).floor().max(1.0)) as usize)
    }

    /// Keep the chosen deck on screen. The list had no scrolling, so a deck
    /// past the bottom edge could be chosen by key and never seen.
    fn ensure_deck_visible(&mut self) {
        let (_, _, visible) = self.deck_list_geometry();
        let last = self.decks.len().saturating_sub(visible);
        if self.selected_deck < self.deck_scroll {
            self.deck_scroll = self.selected_deck;
        } else if self.selected_deck >= self.deck_scroll.saturating_add(visible) {
            self.deck_scroll = self.selected_deck.saturating_add(1).saturating_sub(visible);
        }
        self.deck_scroll = self.deck_scroll.min(last);
    }

    /// Where the card list's rows start, and how many fit. It showed eight
    /// whatever the window's height.
    fn card_list_geometry(&self) -> (f32, usize) {
        let rows_top = Self::HEADER_H + Self::PADDING + 120.0;
        let room = self.height - Self::STATUS_H - 20.0 - rows_top;
        (
            rows_top,
            ((room / Self::CARD_ROW_H).floor().max(1.0)) as usize,
        )
    }

    fn ensure_card_visible(&mut self) {
        let (_, visible_rows) = self.card_list_geometry();
        if self.selected_card < self.scroll_offset {
            self.scroll_offset = self.selected_card;
        } else if self.selected_card >= self.scroll_offset.saturating_add(visible_rows) {
            // Enough to bring the selected row onto the last visible line.
            // The branch condition proves the subtraction is in range; doing it
            // this way keeps the proof in the expression.
            self.scroll_offset = self
                .selected_card
                .saturating_add(1)
                .saturating_sub(visible_rows);
        }
    }

    // ── Layout constants ────────────────────────────────────────────
    const HEADER_H: f32 = 50.0;
    // (The deck list's row and the notice's band are module constants,
    // `DECK_ROW_H` and `NOTICE_H`, so the free functions can reach them.)
    const STATUS_H: f32 = 28.0;
    const CARD_ROW_H: f32 = 48.0;
    const PADDING: f32 = 16.0;

    // ── Card-list geometry ──────────────────────────────────────────
    //
    // The three columns used to be laid out by hand and mixed coordinate
    // systems: Front was proportional (`width * 0.45` from `PADDING + 8`),
    // Tags was proportional in its anchor but *absolute* in its width
    // (`width * 0.5`, 200 wide), and Status was absolute in both
    // (`width - 120`, 100 wide). A fraction of the window is only a valid
    // width when it is measured from a position that is also a fraction of
    // the window, so the three agreed at exactly one size and collided
    // everywhere else — Front ran into Tags below 480px and Tags ran into
    // Status below 640px. Fractions that sum to 1.0 make the row fill its
    // container at *every* width instead of at one.

    const CARD_GAP: f32 = 8.0;
    const CARD_FRONT: usize = 0;
    const CARD_TAGS: usize = 1;
    const CARD_STATUS: usize = 2;
    const CARD_FRACTIONS: [f32; 3] = [0.55, 0.28, 0.17];
    const CARD_HEADINGS: [&'static str; 3] = ["Front", "Tags", "Status"];

    /// Left edge of the card rows' text, and the width they span to the
    /// right margin.
    fn card_row_span(&self) -> (f32, f32) {
        let x = Self::PADDING + Self::CARD_GAP;
        ((x), (self.width - Self::PADDING - x).max(0.0))
    }

    fn card_columns(&self) -> [Column; 3] {
        let (_, row_w) = self.card_row_span();
        // Three columns have two interior gaps.
        let usable = (row_w - Self::CARD_GAP * 2.0).max(0.0);
        let mut columns = [Column {
            label: "",
            width: 0.0,
        }; 3];
        for ((column, label), fraction) in columns
            .iter_mut()
            .zip(Self::CARD_HEADINGS)
            .zip(Self::CARD_FRACTIONS)
        {
            *column = Column {
                label,
                width: usable * fraction,
            };
        }
        columns
    }

    /// [`Table`]'s origin sits *before* its leading gap — `left(0)` is
    /// `x + gap` — so the desired inset has to be handed over less one gap or
    /// every column shifts right by one. That error is invisible to any test
    /// comparing cells against their own columns, since they all move
    /// together; it shows up only as a wrong margin at the far end.
    fn card_table(columns: &[Column]) -> Table<'_> {
        Table::with_gap(columns, Self::PADDING, Self::CARD_GAP)
    }

    // ── Rendering ───────────────────────────────────────────────────
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    /// The drawn commands. **Tests only**: the window's `render` keeps the
    /// frame, for its boxes.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    fn frame(&self) -> Frame<Target> {
        let (w, h) = (self.width, self.height);
        let mut f = Frame::new(w, h);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_header(&mut f);
        match self.view {
            AppView::DeckList => self.render_deck_list(&mut f),
            AppView::DeckDetail => self.render_deck_detail(&mut f),
            AppView::CardEditor | AppView::DeckEditor => self.render_editor(&mut f),
            AppView::StudyMode => self.render_study_mode(&mut f),
            AppView::Statistics => self.render_statistics(&mut f),
        }
        self.render_status_bar(&mut f);
        if let Some(doomed) = self.pending_delete {
            self.render_question(&mut f, doomed);
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (w, h),
                Self::HEADER_H,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, w, h));
        }
        // Last, so it is above everything. Forgetting this is how a picker
        // ends up open and invisible: it takes every keystroke, and nothing
        // on screen says why the window stopped responding.
        f.extend(self.picker.render(&self.palette, w, h));
        f
    }

    /// A button, lit while the pointer is on it; one with nothing to do is
    /// drawn dim and records no box.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(6.0),
        });
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// A row of buttons from `x` at `y`, each as wide as it says.
    fn button_row(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        buttons: &[(&str, f32, Target, bool)],
    ) {
        let mut at = x;
        for (label, width, target, enabled) in buttons {
            self.button(f, Rect::new(at, y, *width, 26.0), label, *target, *enabled);
            at += width + 6.0;
        }
    }

    fn render_header(&self, f: &mut Frame<Target>) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            self.width,
            Self::HEADER_H,
            0.0,
            Surface::Strip(Edge::Top),
        );
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: 14.0,
            text: String::from("Flashcards"),
            font_size: 20.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
            overflow: TextOverflow::Ellipsis,
        });
        // Back, from anywhere but the deck list: Escape was the only way.
        let mut label_x = 180.0;
        if self.view != AppView::DeckList {
            self.button(
                f,
                Rect::new(180.0, 12.0, 70.0, 26.0),
                "< Back",
                Target::Back,
                true,
            );
            label_x = 262.0;
        }
        let view_label = match self.view {
            AppView::DeckList => "Decks",
            AppView::DeckDetail => "Cards",
            AppView::CardEditor | AppView::DeckEditor => "Editor",
            AppView::StudyMode => "Study",
            AppView::Statistics => "Stats",
        };
        f.push(RenderCommand::Text {
            x: label_x,
            y: 18.0,
            text: String::from(view_label),
            font_size: 14.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.button(
            f,
            Rect::new(self.width - 170.0, 12.0, 36.0, 26.0),
            "?",
            Target::Help,
            true,
        );
        f.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: 18.0,
            text: Self::day_label(self.current_day),
            font_size: 14.0,
            color: self.palette.ink(self.palette.teal),
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Line {
            x1: 0.0,
            y1: Self::HEADER_H,
            x2: self.width,
            y2: Self::HEADER_H,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, f: &mut Frame<Target>) {
        let y = self.height - Self::STATUS_H;
        self.palette.push_surface(
            f,
            0.0,
            y,
            self.width,
            Self::STATUS_H,
            0.0,
            Surface::Strip(Edge::Top),
        );
        let message = self
            .last_file_action
            .clone()
            .unwrap_or_else(|| self.status_msg.clone());
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: y + 7.0,
            text: message,
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_deck_list(&self, f: &mut Frame<Target>) {
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: String::from("Your Decks"),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        let has_cards = self.current_deck().is_some_and(|d| !d.cards.is_empty());
        self.button_row(
            f,
            Self::PADDING,
            top + 34.0,
            &[
                ("New deck", 90.0, Target::NewDeck, true),
                ("Edit", 60.0, Target::EditDeck, true),
                ("Delete", 70.0, Target::DeleteDeck, self.decks.len() > 1),
                ("Import", 70.0, Target::Import, true),
                ("Export", 70.0, Target::Export, has_cards),
            ],
        );

        let (list_top, pitch, visible) = self.deck_list_geometry();
        let pane = Rect::new(Self::PADDING, list_top, content_w, visible as f32 * pitch);
        f.hit(Target::DeckList, pane);
        for (i, deck) in self
            .decks
            .iter()
            .enumerate()
            .skip(self.deck_scroll)
            .take(visible)
        {
            let y = list_top + (i.saturating_sub(self.deck_scroll)) as f32 * pitch;
            let row = Rect::new(Self::PADDING, y, content_w, DECK_ROW_H);
            let is_selected = i == self.selected_deck;
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if is_selected {
                    self.palette.surface1
                } else if self.hover == Some(Target::DeckRow(i)) {
                    self.palette.surface2
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(8.0),
            });
            if is_selected {
                f.push(RenderCommand::StrokeRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: self.palette.blue,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(8.0),
                });
            }
            f.push(RenderCommand::Text {
                x: Self::PADDING + 16.0,
                y: y + 12.0,
                text: deck.name.clone(),
                font_size: 16.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w - 200.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: Self::PADDING + 16.0,
                y: y + 36.0,
                text: format!(
                    "{} cards | {} reviews | {}% accuracy",
                    deck.cards.len(),
                    deck.total_reviews(),
                    deck.average_accuracy()
                ),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 48.0),
                overflow: TextOverflow::Ellipsis,
            });
            let due_count = deck.due_cards(self.current_day).len();
            if due_count > 0 {
                let badge_x = self.width - Self::PADDING - 90.0;
                f.push(RenderCommand::FillRect {
                    x: badge_x,
                    y: y + 14.0,
                    width: 72.0,
                    height: 24.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::all(12.0),
                });
                f.push(RenderCommand::Text {
                    x: badge_x + 8.0,
                    y: y + 18.0,
                    text: format!("{due_count} due"),
                    font_size: 12.0,
                    color: self.palette.crust,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            if !deck.description.is_empty() {
                f.push(RenderCommand::Text {
                    x: Self::PADDING + 16.0,
                    y: y + 52.0,
                    text: deck.description.clone(),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w - 48.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            // A press chooses the deck; a press on the chosen one opens it.
            f.hit(Target::DeckRow(i), row);
        }
        if self.decks.len() > visible {
            f.push(RenderCommand::Text {
                x: self.width - 180.0,
                y: pane.bottom() + 2.0,
                text: format!(
                    "Showing {}-{} of {}",
                    self.deck_scroll.saturating_add(1),
                    self.deck_scroll
                        .saturating_add(visible)
                        .min(self.decks.len()),
                    self.decks.len()
                ),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(170.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Where the decks came from, and what is kept. It was drawn at the
        // top of the window before the header, which painted over it.
        let notice_y = self.height - Self::STATUS_H - NOTICE_H + 2.0;
        for (i, line) in SAMPLE_AND_PROGRESS_LINES.iter().enumerate() {
            f.push(RenderCommand::Text {
                x: Self::PADDING,
                y: notice_y + i as f32 * 15.0,
                text: (*line).to_string(),
                color: if i == 0 {
                    self.palette.subtext0
                } else {
                    self.palette.ink(self.palette.yellow)
                },
                font_size: 11.0,
                font_weight: if i == 0 {
                    FontWeightHint::Regular
                } else {
                    FontWeightHint::Bold
                },
                max_width: Some(content_w.max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_deck_detail(&self, f: &mut Frame<Target>) {
        let Some(deck) = self.current_deck() else {
            return;
        };
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: deck.name.clone(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        // The deck's commands. The line of key hints this replaces advertised
        // `[D]ay+`, a key that was removed when the day came from the clock.
        let matching = self.matching_card_indices();
        let due = deck.due_cards(self.current_day).len();
        let study_due = format!("Study due ({due})");
        self.button_row(
            f,
            Self::PADDING,
            top + 30.0,
            &[
                (study_due.as_str(), 120.0, Target::StudyDue, due > 0),
                ("Study all", 86.0, Target::StudyAll, !deck.cards.is_empty()),
                ("New card", 86.0, Target::NewCard, true),
                ("Edit", 56.0, Target::EditCard, !matching.is_empty()),
                ("Delete", 66.0, Target::DeleteCard, !matching.is_empty()),
                ("Shuffle", 72.0, Target::Shuffle, deck.cards.len() > 1),
                ("Statistics", 90.0, Target::Stats, true),
            ],
        );

        // Search. A press starts typing into it, as `/` does.
        let search_y = top + 64.0;
        let search = Rect::new(Self::PADDING, search_y, content_w * 0.6, 28.0);
        self.palette.push_surface(
            f,
            search.x,
            search.y,
            search.w,
            search.h,
            4.0,
            Surface::Card,
        );
        if self.search_active {
            f.push(RenderCommand::StrokeRect {
                x: search.x,
                y: search.y,
                width: search.w,
                height: search.h,
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        let search_display = match (self.search_active, self.search_query.is_empty()) {
            (true, true) => String::from("Type to filter..."),
            (false, true) => String::from("Search [/]"),
            (_, false) => self.search_query.clone(),
        };
        f.push(RenderCommand::Text {
            x: search.x + 8.0,
            y: search_y + 7.0,
            text: search_display,
            font_size: 12.0,
            color: if self.search_query.is_empty() {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search.w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Search, search);

        // The tag filter: a chip that steps through the deck's tags as `T`
        // does. It was drawn only while a filter was set, so there was
        // nothing to press to set one.
        let tags = deck.all_tags();
        let chip_x = search.right() + TAG_PILL_GAP;
        let room = (self.width - Self::PADDING - chip_x).max(0.0);
        let chip_label = self.tag_filter.clone().unwrap_or_else(|| {
            String::from(if tags.is_empty() {
                "No tags"
            } else {
                "All tags"
            })
        });
        let pill_w = text::padded_width(
            &chip_label,
            TAG_PILL_PAD,
            CARD_TAG_SIZE,
            FontWeightHint::Bold,
        )
        .min(room);
        if pill_w > 0.0 {
            let chip = Rect::new(chip_x, search_y + 2.0, pill_w, 24.0);
            f.push(RenderCommand::FillRect {
                x: chip.x,
                y: chip.y,
                width: chip.w,
                height: chip.h,
                color: if self.tag_filter.is_some() {
                    self.palette.mauve
                } else if self.hover == Some(Target::TagChip) {
                    self.palette.surface2
                } else {
                    self.palette.surface1
                },
                corner_radii: CornerRadii::all(12.0),
            });
            let ink = if self.tag_filter.is_some() {
                self.palette.crust
            } else if tags.is_empty() {
                self.palette.overlay0
            } else {
                self.palette.text
            };
            f.draw_with(|c| {
                Table::fitted(
                    c,
                    chip.x + TAG_PILL_PAD,
                    (pill_w - TAG_PILL_PAD * 2.0).max(0.0),
                    search_y + 6.0,
                    &chip_label,
                    ink,
                    CARD_TAG_SIZE,
                    Fit::Start,
                    FontWeightHint::Bold,
                );
            });
            if !tags.is_empty() {
                f.hit(Target::TagChip, chip);
            }
        }

        let list_top = top + 100.0;
        if matching.is_empty() {
            f.push(RenderCommand::Text {
                x: Self::PADDING + 8.0,
                y: list_top + 20.0,
                text: String::from(if deck.cards.is_empty() {
                    "This deck has no cards yet -- New card (N) makes one."
                } else {
                    "No cards match your search."
                }),
                font_size: 14.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let card_cols = self.card_columns();
        let table = Self::card_table(&card_cols);
        f.draw_with(|c| {
            table.header_weighted(
                c,
                list_top,
                self.palette.subtext0,
                CARD_HEADING_SIZE,
                FontWeightHint::Bold,
            );
        });

        let (rows_top, visible_count) = self.card_list_geometry();
        let pane = Rect::new(
            Self::PADDING,
            rows_top,
            content_w,
            visible_count as f32 * Self::CARD_ROW_H,
        );
        f.hit(Target::CardList, pane);
        let end = self
            .scroll_offset
            .saturating_add(visible_count)
            .min(matching.len());
        for (vis_i, list_i) in (self.scroll_offset..end).enumerate() {
            let Some(card) = matching.get(list_i).and_then(|&i| deck.cards.get(i)) else {
                continue;
            };
            let y = rows_top + (vis_i as f32) * Self::CARD_ROW_H;
            let row = Rect::new(Self::PADDING, y, content_w, Self::CARD_ROW_H - 4.0);
            let is_selected = list_i == self.selected_card;
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if is_selected {
                    self.palette.surface1
                } else if self.hover == Some(Target::CardRow(list_i)) {
                    self.palette.surface2
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(4.0),
            });
            // Front. Elided by measured width rather than by byte index:
            // language flashcards are the one thing guaranteed to carry
            // non-ASCII text.
            let tags_str = card.tags.join(", ");
            let (status_text, status_color) = if card.review.total_reviews == 0 {
                (String::from("New"), self.palette.ink(self.palette.yellow))
            } else if card.review.repetitions >= 3 {
                (
                    String::from("Mastered"),
                    self.palette.ink(self.palette.green),
                )
            } else {
                (
                    format!("{}d", card.review.interval_days),
                    self.palette.subtext0,
                )
            };
            f.draw_with(|c| {
                table.cell(
                    c,
                    Self::CARD_FRONT,
                    y + 8.0,
                    &card.front,
                    self.palette.text,
                    CARD_FRONT_SIZE,
                    Fit::Start,
                );
                table.cell(
                    c,
                    Self::CARD_TAGS,
                    y + 8.0,
                    if tags_str.is_empty() { "-" } else { &tags_str },
                    self.palette.ink(self.palette.mauve),
                    CARD_TAG_SIZE,
                    Fit::Start,
                );
                table.cell_weighted(
                    c,
                    Self::CARD_STATUS,
                    y + 8.0,
                    &status_text,
                    status_color,
                    CARD_STATUS_SIZE,
                    Fit::Start,
                    FontWeightHint::Bold,
                );
            });
            // The back, on the row's second line, in the readable grey: it
            // was overlay0, the grey of a switched-off control.
            let (back_x, back_w) = self.card_row_span();
            f.draw_with(|c| {
                Table::fitted(
                    c,
                    back_x,
                    back_w,
                    y + 26.0,
                    &card.back,
                    self.palette.subtext0,
                    CARD_BACK_SIZE,
                    Fit::Start,
                    FontWeightHint::Regular,
                );
            });
            // A press chooses the card; a press on the chosen one edits it.
            f.hit(Target::CardRow(list_i), row);
        }
        if matching.len() > visible_count {
            f.push(RenderCommand::Text {
                x: self.width - 160.0,
                y: pane.bottom() + 4.0,
                text: format!(
                    "Showing {}-{} of {}",
                    self.scroll_offset.saturating_add(1),
                    end,
                    matching.len()
                ),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// The card editor and the deck editor: a labelled field each, the one
    /// the keyboard is in marked and holding the caret, and Save and Cancel.
    fn render_editor(&self, f: &mut Frame<Target>) {
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;
        let field_w = (content_w - 32.0).max(0.0);
        let (title, fields): (&str, &[(Field, &str, &str)]) = match self.view {
            AppView::DeckEditor => (
                if self.editing_deck.is_some() {
                    "Edit Deck"
                } else {
                    "New Deck"
                },
                &[
                    (Field::Name, "Name:", "The deck's name"),
                    (Field::About, "Description:", "What it covers (optional)"),
                ],
            ),
            _ => (
                if self.editing_card_id.is_some() {
                    "Edit Card"
                } else {
                    "New Card"
                },
                &[
                    (Field::Front, "Front (Question):", "Enter question text..."),
                    (Field::Back, "Back (Answer):", "Enter answer text..."),
                    (Field::Tags, "Tags (comma-separated):", "e.g. math, algebra"),
                ],
            ),
        };
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: String::from(title),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 26.0,
            text: String::from("[Tab] next field  [Enter] save  [Esc] cancel"),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
            overflow: TextOverflow::Ellipsis,
        });
        let mut y = top + 56.0;
        for (field, label, placeholder) in fields {
            f.push(RenderCommand::Text {
                x: Self::PADDING + 16.0,
                y,
                text: String::from(*label),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(field_w),
                overflow: TextOverflow::Ellipsis,
            });
            let rect = Rect::new(Self::PADDING + 16.0, y + 20.0, field_w, 36.0);
            let focused = self.field == *field;
            self.palette
                .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
            f.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if focused {
                    self.palette.blue
                } else {
                    self.palette.surface1
                },
                line_width: if focused { 2.0 } else { 1.0 },
                corner_radii: CornerRadii::all(4.0),
            });
            let input = self.input_ref(*field);
            if input.text().is_empty() && !focused {
                f.push(RenderCommand::Text {
                    x: rect.x + 8.0,
                    y: rect.y + 10.0,
                    text: String::from(*placeholder),
                    font_size: 13.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((rect.w - 16.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            } else {
                let mut tree = RenderTree::new();
                textedit::draw(
                    &mut tree,
                    &textedit::SingleLine {
                        text: input.text(),
                        cursor: if focused {
                            input.cursor()
                        } else {
                            TextCursor::default()
                        },
                        selection_anchor: if focused {
                            input.selection_anchor()
                        } else {
                            None
                        },
                        focused,
                        x: rect.x + 8.0,
                        y: rect.y + 9.0,
                        width: (rect.w - 16.0).max(0.0),
                        line_height: 18.0,
                        font_size: 13.0,
                        weight: FontWeightHint::Regular,
                        color: self.palette.text,
                        selection_bg: self.palette.blue,
                        selection_fg: self.palette.crust,
                        caret_width: textedit::CARET_WIDTH,
                    },
                );
                f.extend(tree.commands);
            }
            // A press puts the keyboard in the field.
            f.hit(Target::Field(*field), rect);
            y += 72.0;
        }
        self.button_row(
            f,
            Self::PADDING + 16.0,
            y + 6.0,
            &[
                ("Save", 80.0, Target::Save, true),
                ("Cancel", 80.0, Target::Cancel, true),
            ],
        );
    }

    fn render_study_mode(&self, f: &mut Frame<Target>) {
        let Some(session) = &self.study_session else {
            return;
        };
        let Some(deck) = self.current_deck() else {
            return;
        };
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        let total = session.queue.len() as f32;
        let done = session.current_pos as f32;
        self.palette.push_surface(
            f,
            Self::PADDING,
            top,
            content_w,
            8.0,
            4.0,
            Surface::ControlTrack,
        );
        if total > 0.0 {
            let filled = (done / total) * content_w;
            if filled > 0.0 {
                f.push(RenderCommand::FillRect {
                    x: Self::PADDING,
                    y: top,
                    width: filled,
                    height: 8.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 14.0,
            text: format!(
                "{} / {} cards  |  {} remaining",
                session.current_pos,
                session.queue.len(),
                session.remaining()
            ),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((content_w - 140.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if !session.is_complete() {
            self.button(
                f,
                Rect::new(self.width - Self::PADDING - 120.0, top + 12.0, 120.0, 24.0),
                "End session",
                Target::EndSession,
                true,
            );
        }

        if session.is_complete() {
            self.render_session_summary(f, session, top + 50.0, content_w);
            return;
        }
        let Some(card) = session.current_card_idx().and_then(|i| deck.cards.get(i)) else {
            return;
        };

        let card_rect = Rect::new(
            Self::PADDING + 40.0,
            top + 50.0,
            (content_w - 80.0).max(0.0),
            260.0,
        );
        self.palette.push_surface(
            f,
            card_rect.x,
            card_rect.y,
            card_rect.w,
            card_rect.h,
            12.0,
            Surface::Card,
        );
        let side_color = if session.flipped {
            self.palette.green
        } else {
            self.palette.blue
        };
        f.push(RenderCommand::StrokeRect {
            x: card_rect.x,
            y: card_rect.y,
            width: card_rect.w,
            height: card_rect.h,
            color: side_color,
            line_width: 2.0,
            corner_radii: CornerRadii::all(12.0),
        });
        f.push(RenderCommand::Text {
            x: card_rect.x + 20.0,
            y: card_rect.y + 16.0,
            text: String::from(if session.flipped {
                "ANSWER"
            } else {
                "QUESTION"
            }),
            font_size: 11.0,
            color: self.palette.ink(side_color),
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });
        // The text wrapped to the card: it was one line cut with an ellipsis,
        // so a long answer lost its end in the one place it has to be read.
        let words = if session.flipped {
            &card.back
        } else {
            &card.front
        };
        let width = (card_rect.w - 40.0).max(1.0);
        let mut lines = Vec::new();
        for line in words.split('\n') {
            if line.is_empty() || text::measure(line, 18.0, FontWeightHint::Regular) <= width {
                lines.push(line.to_owned());
            } else {
                lines.extend(text::wrap(line, width, 18.0, FontWeightHint::Regular));
            }
        }
        let room = 7usize;
        let more = lines.len() > room;
        for (i, line) in lines.iter().take(room).enumerate() {
            let last = i.saturating_add(1) == room;
            f.push(RenderCommand::Text {
                x: card_rect.x + 20.0,
                y: card_rect.y + 50.0 + i as f32 * 24.0,
                text: if more && last {
                    format!("{line}\u{2026}")
                } else {
                    line.clone()
                },
                font_size: 18.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
        }
        if !card.tags.is_empty() {
            f.push(RenderCommand::Text {
                x: card_rect.x + 20.0,
                y: card_rect.bottom() - 30.0,
                text: card.tags.join(" | "),
                font_size: 10.0,
                color: self.palette.ink(self.palette.mauve),
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
        }
        // A press on the card turns it over.
        if !session.flipped {
            f.hit(Target::StudyCard, card_rect);
        }

        let btn_y = card_rect.bottom() + 20.0;
        if session.flipped {
            let btn_w = 120.0;
            let gap = 16.0;
            let start_x = (self.width - (btn_w * 4.0 + gap * 3.0)) / 2.0;
            for (i, rating) in ALL_RATINGS.iter().enumerate() {
                let rect = Rect::new(start_x + (i as f32) * (btn_w + gap), btn_y, btn_w, 40.0);
                let lit = self.hover == Some(Target::Rate(*rating));
                f.push(RenderCommand::FillRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.w,
                    height: rect.h,
                    color: rating.color(&self.palette),
                    corner_radii: CornerRadii::all(8.0),
                });
                if lit {
                    f.push(RenderCommand::StrokeRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.w,
                        height: rect.h,
                        color: self.palette.text,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(8.0),
                    });
                }
                f.push(RenderCommand::Text {
                    x: rect.x + 10.0,
                    y: rect.y + 10.0,
                    text: format!("[{}] {}", i.saturating_add(1), rating.label()),
                    font_size: 14.0,
                    color: self.palette.crust,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(btn_w - 20.0),
                    overflow: TextOverflow::Ellipsis,
                });
                f.hit(Target::Rate(*rating), rect);
            }
        } else {
            let prompt = Rect::new((self.width - 200.0) / 2.0, btn_y, 200.0, 40.0);
            f.push(RenderCommand::FillRect {
                x: prompt.x,
                y: prompt.y,
                width: prompt.w,
                height: prompt.h,
                color: self.palette.blue,
                corner_radii: CornerRadii::all(8.0),
            });
            f.push(RenderCommand::Text {
                x: prompt.x + 20.0,
                y: prompt.y + 10.0,
                text: String::from("[Space] Flip Card"),
                font_size: 14.0,
                color: self.palette.crust,
                font_weight: FontWeightHint::Bold,
                max_width: Some(prompt.w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::StudyCard, prompt);
        }
    }

    fn render_session_summary(
        &self,
        f: &mut Frame<Target>,
        session: &StudySession,
        top: f32,
        content_w: f32,
    ) {
        let cx = self.width / 2.0;
        self.palette.push_surface(
            f,
            Self::PADDING + 60.0,
            top,
            (content_w - 120.0).max(0.0),
            280.0,
            12.0,
            Surface::Card,
        );
        f.push(RenderCommand::Text {
            x: cx - 80.0,
            y: top + 20.0,
            text: String::from("Session Complete!"),
            font_size: 20.0,
            color: self.palette.ink(self.palette.green),
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: cx - 100.0,
            y: top + 60.0,
            text: format!("Cards reviewed: {}", session.reviewed),
            font_size: 14.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: cx - 100.0,
            y: top + 84.0,
            text: format!("Accuracy: {}%", session.session_accuracy()),
            font_size: 14.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        for (i, rating) in ALL_RATINGS.iter().enumerate() {
            f.push(RenderCommand::Text {
                x: cx - 80.0,
                y: top + 120.0 + (i as f32) * 24.0,
                text: format!(
                    "{}: {}",
                    rating.label(),
                    session.session_ratings.get(i).copied().unwrap_or(0)
                ),
                font_size: 13.0,
                color: self.palette.ink(rating.color(&self.palette)),
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        self.button(
            f,
            Rect::new(cx - 70.0, top + 234.0, 140.0, 28.0),
            "Back to deck",
            Target::EndSession,
            true,
        );
    }

    fn render_statistics(&self, f: &mut Frame<Target>) {
        let Some(deck) = self.current_deck() else {
            return;
        };
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: format!("Statistics: {}", deck.name),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });
        let stats_y = top + 40.0;
        let col_w = (content_w - 32.0) / 3.0;
        let new_cards = deck
            .cards
            .iter()
            .filter(|c| c.review.total_reviews == 0)
            .count();
        let boxes = [
            (
                "Total Cards",
                format!("{}", deck.cards.len()),
                self.palette.blue,
            ),
            (
                "Total Reviews",
                format!("{}", deck.total_reviews()),
                self.palette.teal,
            ),
            (
                "Avg Accuracy",
                format!("{}%", deck.average_accuracy()),
                self.palette.green,
            ),
            (
                "Due Today",
                format!("{}", deck.due_cards(self.current_day).len()),
                self.palette.yellow,
            ),
            (
                "Mastered",
                format!("{}", deck.mastered_count()),
                self.palette.green,
            ),
            ("New", format!("{new_cards}"), self.palette.peach),
        ];
        for (i, (label, value, color)) in boxes.iter().enumerate() {
            let x = Self::PADDING + ((i % 3) as f32) * (col_w + 16.0);
            let y = stats_y + ((i / 3) as f32) * 90.0;
            self.palette
                .push_surface(f, x, y, col_w, 70.0, 8.0, Surface::Card);
            f.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 10.0,
                text: String::from(*label),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 32.0,
                text: value.clone(),
                font_size: 24.0,
                color: self.palette.ink(*color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let breakdown_y = stats_y + 180.0;
        f.push(RenderCommand::Text {
            x: Self::PADDING,
            y: breakdown_y,
            text: String::from("Card Breakdown"),
            font_size: 14.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Line {
            x1: Self::PADDING,
            y1: breakdown_y + 20.0,
            x2: self.width - Self::PADDING,
            y2: breakdown_y + 20.0,
            color: self.palette.surface1,
            width: 1.0,
        });
        let bar_y = breakdown_y + 30.0;
        let bar_h = 20.0;
        let bar_max_w = (content_w - 200.0).max(0.0);
        let ease = |c: &&Card| c.review.ease_factor;
        let counts: [(&str, Color, usize); 4] = [
            (
                "Ease < 1.8 (difficult)",
                self.palette.red,
                deck.cards.iter().filter(|c| ease(c) < 1.8).count(),
            ),
            (
                "Ease 1.8-2.2 (moderate)",
                self.palette.yellow,
                deck.cards
                    .iter()
                    .filter(|c| (1.8..2.2).contains(&ease(c)))
                    .count(),
            ),
            (
                "Ease 2.2-2.5 (good)",
                self.palette.blue,
                deck.cards
                    .iter()
                    .filter(|c| (2.2..2.5).contains(&ease(c)))
                    .count(),
            ),
            (
                "Ease > 2.5 (easy)",
                self.palette.green,
                deck.cards.iter().filter(|c| ease(c) >= 2.5).count(),
            ),
        ];
        let max_count = counts.iter().map(|(_, _, n)| *n).max().unwrap_or(1).max(1);
        for (i, (label, color, count)) in counts.iter().enumerate() {
            let y = bar_y + (i as f32) * (bar_h + 8.0);
            f.push(RenderCommand::Text {
                x: Self::PADDING,
                y: y + 3.0,
                text: String::from(*label),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
                overflow: TextOverflow::Ellipsis,
            });
            let w = (*count as f32 / max_count as f32) * bar_max_w;
            if w > 0.0 {
                f.push(RenderCommand::FillRect {
                    x: Self::PADDING + 190.0,
                    y,
                    width: w,
                    height: bar_h,
                    color: *color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            f.push(RenderCommand::Text {
                x: Self::PADDING + 196.0 + w,
                y: y + 3.0,
                text: format!("{count}"),
                font_size: 11.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// The question before a delete, with a button for each answer.
    fn render_question(&self, f: &mut Frame<Target>, doomed: Doomed) {
        let (w, h) = (self.width, self.height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });
        f.hit(Target::QuestionBackdrop, Rect::new(0.0, 0.0, w, h));
        let card = Rect::new((w - 460.0) / 2.0, (h - 150.0) / 2.0, 460.0, 150.0);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        f.hit(Target::QuestionCard, card);
        let (title, body) = match doomed {
            Doomed::Deck(idx) => {
                let deck = self.decks.get(idx);
                (
                    format!("Delete the deck {}?", deck.map_or("", |d| d.name.as_str())),
                    format!(
                        "Its {} card(s) and every review go with it, and this cannot be undone.",
                        deck.map_or(0, |d| d.cards.len())
                    ),
                )
            }
            Doomed::Card(_) => (
                String::from("Delete this card?"),
                String::from("Its review history goes with it, and this cannot be undone."),
            ),
        };
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 20.0,
            text: title,
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 50.0,
            text: body,
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        let delete = Rect::new(
            card.right() - 20.0 - 210.0,
            card.bottom() - 50.0,
            120.0,
            32.0,
        );
        f.push(RenderCommand::FillRect {
            x: delete.x,
            y: delete.y,
            width: delete.w,
            height: delete.h,
            color: self.palette.red,
            corner_radii: CornerRadii::all(6.0),
        });
        f.push(RenderCommand::Text {
            x: delete.x + 14.0,
            y: delete.y + 9.0,
            text: String::from("Delete (Y)"),
            font_size: 12.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(delete.w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::ConfirmDelete, delete);
        self.button(
            f,
            Rect::new(card.right() - 20.0 - 80.0, card.bottom() - 50.0, 80.0, 32.0),
            "Keep",
            Target::KeepIt,
            true,
        );
    }

    // ── The pointer ─────────────────────────────────────────────────

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self.frame().hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                self.press(target)
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// Back one view, as Escape does from each.
    fn go_back(&mut self) {
        match self.view {
            AppView::CardEditor | AppView::DeckEditor => self.leave_editor(),
            AppView::DeckList => {}
            AppView::DeckDetail => {
                self.search_active = false;
                self.handle_key("Escape", false, false);
            }
            AppView::StudyMode | AppView::Statistics => self.handle_key("Escape", false, false),
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Back => self.go_back(),
            Target::DeckRow(i) => {
                if i == self.selected_deck {
                    self.select_deck(i);
                } else {
                    self.selected_deck = i;
                }
            }
            Target::NewDeck => self.open_new_deck_editor(),
            Target::EditDeck => self.open_edit_deck(self.selected_deck),
            Target::DeleteDeck => self.ask_to_delete(Doomed::Deck(self.selected_deck)),
            Target::Import => self.picker.open_to_read(),
            Target::Export => self.open_save_dialog(),
            Target::CardRow(p) => {
                if p == self.selected_card {
                    self.handle_key("e", false, false);
                } else {
                    self.selected_card = p;
                }
            }
            Target::Search => {
                self.search_active = true;
                self.status_msg = String::from("Search: type to filter, Esc to clear");
            }
            Target::TagChip => self.handle_key("t", false, false),
            Target::StudyDue => self.start_study(),
            Target::StudyAll => self.start_study_all(),
            Target::NewCard => self.open_new_card_editor(),
            Target::EditCard => self.handle_key("e", false, false),
            Target::DeleteCard => self.handle_key("x", false, false),
            Target::Shuffle => self.handle_key("r", false, false),
            Target::Stats => self.view = AppView::Statistics,
            Target::Field(field) => self.field = field,
            Target::Save => {
                if self.view == AppView::DeckEditor {
                    self.save_deck_edits();
                } else {
                    self.save_card();
                }
            }
            Target::Cancel => self.leave_editor(),
            Target::StudyCard => self.flip_card(),
            Target::Rate(rating) => self.rate_card(rating),
            Target::EndSession => self.end_study(),
            Target::ConfirmDelete => {
                let Some(doomed) = self.pending_delete.take() else {
                    return EventResult::Ignored;
                };
                self.delete_doomed(doomed);
            }
            Target::KeepIt | Target::QuestionBackdrop => {
                if self.pending_delete.take().is_none() {
                    return EventResult::Ignored;
                }
                self.status_msg = String::from("Kept");
            }
            Target::QuestionCard | Target::DeckList | Target::CardList => {
                return EventResult::Ignored;
            }
        }
        self.ensure_deck_visible();
        EventResult::Consumed
    }

    /// The wheel over the deck list or the card list.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let over = self.target_at(x, y);
        let decks = match over {
            Some(Target::DeckList | Target::DeckRow(_)) => true,
            Some(Target::CardList | Target::CardRow(_)) => false,
            _ => return EventResult::Ignored,
        };
        let rows = self.wheel.rows(dy);
        let (now, count, visible) = if decks {
            let (_, _, visible) = self.deck_list_geometry();
            (self.deck_scroll, self.decks.len(), visible)
        } else {
            let (_, visible) = self.card_list_geometry();
            (
                self.scroll_offset,
                self.matching_card_indices().len(),
                visible,
            )
        };
        let last = count.saturating_sub(visible);
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(last);
        if next == now {
            return EventResult::Ignored;
        }
        if decks {
            self.deck_scroll = next;
        } else {
            self.scroll_offset = next;
        }
        EventResult::Consumed
    }
}

/// Today, as days since 1970 — the unit `ReviewData` schedules on.
///
/// Falls back to day 0 only if the system clock cannot be read at all. That is
/// the same fallback the old simulated counter effectively had, and it fails in
/// the safe direction here: every card reads as due, which is a visible wrong
/// rather than a silent one.
///
/// The zone comes from `tzrules` the same way `apps/alarmclock` gets it, so a
/// real local zone arrives here on the day
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ` is fixed. It matters more than it
/// looks: "is this card due today" is a question about the user's midnight, not
/// about UTC's.
fn today() -> u32 {
    let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    let Ok(utc) = i64::try_from(since_epoch.as_secs()) else {
        return 0;
    };
    let zone = tzrules::Tz::utc();
    let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));
    u32::try_from(guitk::date::Date::from_unix_utc(local).days_since_epoch()).unwrap_or(0)
}

impl App for FlashcardsApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Flashcards".to_owned()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// Five minutes, to notice midnight.
    ///
    /// Cards come due at a day boundary and nothing else here advances on its
    /// own, so this is not an animation clock — it is the coarsest thing that
    /// still catches the one event per day that matters. `refresh_day` answers
    /// `Ignored` unless the day actually changed, so a tick that finds the same
    /// day costs a clock read rather than a frame; only the one crossing
    /// midnight redraws.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_mins(5))
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
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.width = width;
        self.height = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

fn main() -> ExitCode {
    let mut cards = FlashcardsApp::new();
    app::launch("flashcards", &mut cards)
}

// ── Tests ───────────────────────────────────────────────────────────
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
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    // ------------------------------------------------------------------
    // The day, and the event layer
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    /// **Revealing the answer has to read as a redraw.**
    ///
    /// `handle_key` reports nothing, so `state_fingerprint` is the only thing
    /// deciding whether a frame is drawn, and it held
    /// `study_session.is_some()` -- whether a session *exists*, not what it is
    /// showing. `flip_card` sets `session.flipped`, every field compared
    /// equal, `handle_event` answered `Ignored`, and the card stayed
    /// face-down: in a flashcards program, the one interaction it is for.
    ///
    /// Found by reading the fingerprint after `apps/jsonviewer` turned out to
    /// have three of these, not by anybody using the app.
    #[test]
    fn revealing_the_answer_is_a_redraw() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        assert!(
            app.study_session.is_some(),
            "no session, so this checks nothing"
        );

        assert_eq!(
            app.handle_event(&press(Key::Space)),
            EventResult::Consumed,
            "flipping the card did not read as a redraw, so the answer stays hidden"
        );
        assert!(
            app.study_session.as_ref().is_some_and(|s| s.flipped),
            "the card did not flip at all"
        );
    }

    /// **Shuffling and filtering have to read as redraws too.**
    ///
    /// Same fingerprint, same gap: `r` reorders the deck and `t` cycles the
    /// tag filter, and neither the order nor the filter was in it. The list on
    /// screen would be the old one until something else moved.
    #[test]
    fn reordering_and_filtering_are_redraws() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        // Tags, because cycling a filter over a deck that has none is a cycle
        // of one and rightly changes nothing.
        if let Some(deck) = app.current_deck_mut() {
            deck.add_card_with_tags("front", "back", &["verbs"]);
            deck.add_card_with_tags("other", "back", &["nouns"]);
        }

        assert_eq!(
            app.handle_event(&typed('t')),
            EventResult::Consumed,
            "cycling the tag filter did not read as a redraw"
        );
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            // The scancode a letter arrives with is not what the shortcuts
            // are written as; the text is.
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    /// A key with Ctrl held, for the picker tests.
    fn ctrl_press(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event; the
    /// writer test calls the writer with a path directly and never touches the
    /// dialog. This is the half routing actually decides -- with a dialog up,
    /// a keystroke belongs to the dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_deck() {
        let mut app = FlashcardsApp::new();
        let before = app.selected_deck;

        app.handle_event(&ctrl_press(Key::O));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_deck, before,
            "Down at the open dialog moved the selection behind it"
        );
    }

    #[test]
    fn the_day_comes_from_the_calendar_and_not_from_a_counter() {
        // The scheduler this app is built around measures in days; a day that
        // only moves when a key is pressed means a six-day interval comes due
        // after six presses and never otherwise.
        let day = today();
        assert!(
            day > 20_000,
            "days since 1970 should be past 2024, got {day}"
        );
        assert!(
            day < 40_000,
            "days since 1970 should be before 2079, got {day}"
        );
    }

    #[test]
    fn the_day_number_labels_as_the_date_it_stands_for() {
        // Guards the conversion: "Day 20700" means nothing to a reader, and an
        // off-by-one in the epoch shows up here as the wrong date.
        assert_eq!(FlashcardsApp::day_label(0), "1970-01-01");
        assert_eq!(FlashcardsApp::day_label(1), "1970-01-02");
        // 2000-03-01, one day past a leap day that only a correct calendar has.
        assert_eq!(FlashcardsApp::day_label(11_017), "2000-03-01");
        assert_eq!(FlashcardsApp::day_label(11_016), "2000-02-29");
    }

    #[test]
    fn a_tick_on_the_same_day_is_not_a_redraw() {
        // 288 ticks a day, of which at most one crosses midnight: the rest must
        // cost a clock read and not a frame.
        let mut app = FlashcardsApp::new();
        assert_eq!(
            app.handle_event(&Event::Tick {
                elapsed_ms: 300_000
            }),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_tick_after_midnight_takes_the_new_day() {
        let mut app = FlashcardsApp::new();
        // Yesterday, as the app would have seen it before midnight.
        app.current_day = today().saturating_sub(1);
        assert_eq!(
            app.handle_event(&Event::Tick {
                elapsed_ms: 300_000
            }),
            EventResult::Consumed
        );
        assert_eq!(app.current_day, today());
    }

    #[test]
    fn a_card_left_unreviewed_long_enough_comes_due_on_its_own() {
        // The whole point of the clock: the scheduler already worked, and had
        // no way to be told that time had passed.
        let mut review = ReviewData::new();
        let day = today();
        review.apply_rating(Rating::Good, day);
        review.apply_rating(Rating::Good, day);
        let interval = review.interval_days;
        assert!(interval >= 1, "a reviewed card gets a real interval");
        assert!(!review.is_due(day), "just reviewed, so not due today");
        assert!(
            !review.is_due(day + interval - 1),
            "not due the day before its interval elapses"
        );
        assert!(
            review.is_due(day + interval),
            "due once the interval has elapsed"
        );
    }

    #[test]
    fn a_key_the_view_has_no_use_for_is_not_consumed() {
        // An app that consumes every key stops the compositor routing any of
        // them elsewhere, and redraws on each.
        let mut app = FlashcardsApp::new();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = FlashcardsApp::new();
        let before = app.view;
        let release = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.view, before);
    }

    #[test]
    fn the_arrows_reach_the_deck_list_through_the_event_layer() {
        // The translation from a compositor `Key` to the string `handle_key`
        // matches on is the new surface, so it needs its own coverage.
        let mut app = FlashcardsApp::new();
        assert!(app.decks.len() >= 2, "the sample data should have decks");
        assert_eq!(app.selected_deck, 0);
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
        assert_eq!(app.selected_deck, 1);
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Consumed);
        assert_eq!(app.selected_deck, 0);
    }

    #[test]
    fn a_typed_character_reaches_the_shortcut_it_names() {
        // `Key::Unknown` with text is how a letter arrives, and the letter is
        // what every shortcut in this app is written as.
        let mut app = FlashcardsApp::new();
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.view, AppView::DeckDetail, "Enter opens the deck");
        assert_eq!(app.handle_event(&typed('s')), EventResult::Consumed);
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut app = FlashcardsApp::new();
        assert_eq!(
            app.handle_event(&Event::Resize {
                width: 1280,
                height: 1024
            }),
            EventResult::Ignored
        );
        assert!((app.width - 1280.0).abs() < f32::EPSILON);
        assert!((app.height - 1024.0).abs() < f32::EPSILON);
    }

    use super::*;

    /// A deck with history survives a write and a read.
    ///
    /// **This is the bug this commit exists for.** The format carried front,
    /// back and tags and nothing else, so an export and a re-import returned
    /// every card to new. In a spaced repetition program that is close to
    /// worthless, and it is invisible: a restored deck with a reset schedule
    /// looks exactly like a restored deck, until four hundred mature cards all
    /// come due on the same morning.
    #[test]
    fn a_deck_keeps_its_schedules_across_a_write_and_a_read() {
        let mut deck = Deck::new("Studied", "Desc");
        deck.add_card("front", "back");
        let studied = ReviewData {
            repetitions: 7,
            ease_factor: 2.35,
            interval_days: 21,
            last_review_day: 19_000,
            total_reviews: 11,
            rating_counts: [1, 2, 6, 2],
        };
        deck.cards.first_mut().expect("one card").review = studied.clone();

        let text = deck.export_text();
        let mut back = Deck::new("Imported", "");
        let done = back.import_text(&text);

        assert_eq!(done.cards, 1, "the card did not come back");
        assert_eq!(done.history_unreadable, 0, "its history would not parse");
        let got = back.cards.first().expect("one card").review.clone();
        assert_eq!(got.repetitions, studied.repetitions);
        assert_eq!(got.interval_days, studied.interval_days);
        assert_eq!(got.last_review_day, studied.last_review_day);
        assert_eq!(got.total_reviews, studied.total_reviews);
        assert_eq!(got.rating_counts, studied.rating_counts);
        assert!(
            (got.ease_factor - studied.ease_factor).abs() < 0.001,
            "ease factor drifted: {} -> {}",
            studied.ease_factor,
            got.ease_factor
        );
    }

    /// A card that was never reviewed writes no review line, and an older file
    /// without one imports as a new card -- which is what it is.
    #[test]
    fn a_new_card_writes_no_review_line_and_reads_back_as_new() {
        let mut deck = Deck::new("Fresh", "");
        deck.add_card("q", "a");
        let text = deck.export_text();
        assert!(
            !text.contains("R: "),
            "a new card wrote a history: {text:?}"
        );

        let mut back = Deck::new("Imported", "");
        let done = back.import_text(&text);
        assert_eq!(done.cards, 1);
        assert_eq!(done.history_unreadable, 0, "absence is not corruption");
        assert_eq!(
            back.cards.first().expect("one card").review.total_reviews,
            0,
            "a card with no history should arrive with none"
        );
    }

    /// A review line this version cannot read is COUNTED, not silently reset.
    ///
    /// The card is still imported -- losing it would be worse -- but it starts
    /// again as new, and "1 card imported" would hide that. The difference
    /// between a backup and a deck that merely looks restored is whether the
    /// program says which happened.
    #[test]
    fn an_unreadable_review_line_is_counted_rather_than_silently_reset() {
        let text = "# Deck\n## D\nQ: q\nA: a\nR: 7 not-a-number 21 19000 11 1 2 6 2\n\n";
        let mut deck = Deck::new("Imported", "");
        let done = deck.import_text(text);

        assert_eq!(done.cards, 1, "the card should still be imported");
        assert_eq!(done.history_unreadable, 1, "the loss was not reported");
        assert_eq!(
            deck.cards.first().expect("one card").review.total_reviews,
            0,
            "an unreadable history should leave a new card, not a wrong one"
        );
    }

    /// A short review line is rejected rather than half-read.
    #[test]
    fn a_short_review_line_is_rejected() {
        assert!(ReviewData::from_line("7 2.5 21").is_none());
        assert!(ReviewData::from_line("7 2.5 21 19000 11 1 2 6 2 extra").is_none());
        assert!(ReviewData::from_line("7 0 21 19000 11 1 2 6 2").is_none());
        assert!(ReviewData::from_line("7 2.5 21 19000 11 1 2 6 2").is_some());
    }

    /// An empty deck is refused rather than written. See design-decisions 854.
    #[test]
    fn a_deck_with_no_cards_is_not_written() {
        let path = std::env::temp_dir().join("slateos-flashcards-should-not-exist.deck");
        let _ = std::fs::remove_file(&path);

        let mut app = FlashcardsApp::new();
        app.decks.push(Deck::new("Empty", ""));
        app.selected_deck = app.decks.len() - 1;
        let said = app.write_deck(&path);
        assert_eq!(said, "That deck has no cards -- nothing to write");
        assert!(!path.exists(), "nothing should have been created");
    }

    /// Ctrl+S opens a picker that the window will actually draw.
    ///
    /// `handle_key_event` answers Consumed or Ignored by comparing a state
    /// fingerprint before and after. Opening a picker changes nothing that
    /// fingerprint covers, so routing it through there would answer Ignored,
    /// the frame would not be redrawn, and **the picker would be up and
    /// invisible**. apps/hexeditor nearly shipped that same bug by a different
    /// route.
    #[test]
    fn ctrl_s_opens_a_picker_and_says_the_frame_changed() {
        let mut app = FlashcardsApp::new();
        let before = app.render_commands().len();

        let mut ctrl = Modifiers::NONE;
        ctrl.ctrl = true;
        let result = app.handle_event(&Event::Key(KeyEvent {
            key: Key::S,
            pressed: true,
            modifiers: ctrl,
            text: String::from("s"),
        }));

        assert!(app.picker.is_open(), "Ctrl+S did not open the picker");
        assert_eq!(
            result,
            EventResult::Consumed,
            "the window was not told to redraw, so the picker would be invisible"
        );
        assert!(
            app.render_commands().len() > before,
            "the picker is open and nothing was drawn for it"
        );
    }

    /// The window names the decks as included and the progress as unsaved.
    ///
    /// Both scanners reached this app, and they disagreed usefully.
    /// `find-reachable-fixtures.py` flagged `sample_world_capitals` and its
    /// two siblings; `find-silent-incapacity.py` flagged the crate for
    /// reaching nothing and saying nothing.
    ///
    /// **The decks stay.** "What is the capital of France? -> Paris" is a true
    /// statement about the world, bundled as content. A fabrication is a claim
    /// about something the program cannot observe, and this is not one -- the
    /// `apps/ebook` case, not the `apps/kanban` one. They are labelled so they
    /// cannot be mistaken for the user's own.
    ///
    /// The silence was the real defect, and it is specific to this kind of
    /// program: **spaced repetition is defined by history.** A scheduler that
    /// forgets is worse than no scheduler -- it shows a card mastered last
    /// week and holds back one about to be forgotten, and the user cannot tell,
    /// because not remembering is the thing they came here about.
    #[test]
    fn the_window_names_the_decks_and_the_lost_progress() {
        let app = FlashcardsApp::new();
        let texts: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for line in SAMPLE_AND_PROGRESS_LINES {
            assert!(
                texts.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        // The PROPERTY, not the phrase. This required the words "review
        // schedule resets", which stayed true of the test after it stopped
        // being true of the program -- the third banner assertion in this
        // tree to pin wording and go on passing while the wording went wrong
        // (see apps/contacts and apps/diagram). What has to hold is that the
        // banner names BOTH the cost and the remedy: a reader who believes it
        // should know what they lose and what to do about it.
        assert!(
            SAMPLE_AND_PROGRESS_LINES
                .iter()
                .any(|l| l.contains("resets when the window closes")),
            "the message states the mechanism but not what it costs the user",
        );
        assert!(
            SAMPLE_AND_PROGRESS_LINES
                .iter()
                .any(|l| l.contains("Ctrl+S")),
            "the message states the cost but not how to avoid it",
        );
    }

    // ── ReviewData / SM-2 tests ─────────────────────────────────────

    #[test]
    fn test_review_data_initial() {
        let rd = ReviewData::new();
        assert_eq!(rd.repetitions, 0);
        assert!((rd.ease_factor - SM2_INITIAL_EASE).abs() < f32::EPSILON);
        assert_eq!(rd.interval_days, 0);
        assert_eq!(rd.total_reviews, 0);
    }

    #[test]
    fn test_review_data_is_due_when_new() {
        let rd = ReviewData::new();
        assert!(rd.is_due(1));
    }

    #[test]
    fn test_review_good_first_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        assert_eq!(rd.repetitions, 1);
        assert_eq!(rd.interval_days, 1);
        assert_eq!(rd.total_reviews, 1);
    }

    #[test]
    fn test_review_good_second_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        assert_eq!(rd.repetitions, 2);
        assert_eq!(rd.interval_days, 6);
    }

    #[test]
    fn test_review_good_third_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Good, 8);
        assert_eq!(rd.repetitions, 3);
        assert!(rd.interval_days > 6);
    }

    #[test]
    fn test_review_again_resets_repetitions() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Again, 8);
        assert_eq!(rd.repetitions, 0);
        assert_eq!(rd.interval_days, 1);
    }

    #[test]
    fn test_review_hard_resets_repetitions() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Hard, 2);
        assert_eq!(rd.repetitions, 0);
        assert_eq!(rd.interval_days, 1);
    }

    #[test]
    fn test_review_easy_increases_ease() {
        let mut rd = ReviewData::new();
        let initial = rd.ease_factor;
        rd.apply_rating(Rating::Easy, 1);
        assert!(rd.ease_factor > initial);
    }

    #[test]
    fn test_review_again_decreases_ease() {
        let mut rd = ReviewData::new();
        let initial = rd.ease_factor;
        rd.apply_rating(Rating::Again, 1);
        assert!(rd.ease_factor < initial);
    }

    #[test]
    fn test_ease_never_below_minimum() {
        let mut rd = ReviewData::new();
        for i in 0..20 {
            rd.apply_rating(Rating::Again, i);
        }
        assert!(rd.ease_factor >= SM2_MIN_EASE);
    }

    #[test]
    fn test_is_due_after_interval() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        // interval_days = 1, so due on day 2
        assert!(!rd.is_due(1)); // same day
        assert!(rd.is_due(2)); // next day
    }

    #[test]
    fn test_is_due_not_before_interval() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        // interval_days = 6, so due on day 8
        assert!(!rd.is_due(5));
        assert!(rd.is_due(8));
    }

    #[test]
    fn test_accuracy_percent_no_reviews() {
        let rd = ReviewData::new();
        assert_eq!(rd.accuracy_percent(), 0);
    }

    #[test]
    fn test_accuracy_percent_all_good() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        assert_eq!(rd.accuracy_percent(), 100);
    }

    #[test]
    fn test_accuracy_percent_mixed() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Again, 2);
        // 1 good + 1 again = 50%
        assert_eq!(rd.accuracy_percent(), 50);
    }

    #[test]
    fn test_rating_counts_tracked() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Again, 1);
        rd.apply_rating(Rating::Hard, 2);
        rd.apply_rating(Rating::Good, 3);
        rd.apply_rating(Rating::Easy, 4);
        assert_eq!(rd.rating_counts, [1, 1, 1, 1]);
    }

    // ── Card tests ──────────────────────────────────────────────────

    #[test]
    fn test_card_new() {
        let card = Card::new(1, "Q?", "A!");
        assert_eq!(card.id, 1);
        assert_eq!(card.front, "Q?");
        assert_eq!(card.back, "A!");
        assert!(card.tags.is_empty());
    }

    #[test]
    fn test_card_with_tags() {
        let card = Card::new(1, "Q", "A").with_tags(&["math", "algebra"]);
        assert_eq!(card.tags.len(), 2);
        assert_eq!(card.tags[0], "math");
    }

    #[test]
    fn test_card_matches_search_empty() {
        let card = Card::new(1, "Hello", "World");
        assert!(card.matches_search(""));
    }

    #[test]
    fn test_card_matches_search_front() {
        let card = Card::new(1, "What is Rust?", "A language");
        assert!(card.matches_search("rust"));
    }

    #[test]
    fn test_card_matches_search_back() {
        let card = Card::new(1, "What?", "Answer here");
        assert!(card.matches_search("answer"));
    }

    #[test]
    fn test_card_matches_search_tag() {
        let card = Card::new(1, "Q", "A").with_tags(&["biology"]);
        assert!(card.matches_search("bio"));
    }

    #[test]
    fn test_card_matches_search_no_match() {
        let card = Card::new(1, "Hello", "World");
        assert!(!card.matches_search("xyz"));
    }

    #[test]
    fn test_card_has_tag() {
        let card = Card::new(1, "Q", "A").with_tags(&["Math", "Science"]);
        assert!(card.has_tag("math")); // case insensitive
        assert!(card.has_tag("SCIENCE"));
        assert!(!card.has_tag("history"));
    }

    // ── Deck tests ──────────────────────────────────────────────────

    #[test]
    fn test_deck_new() {
        let deck = Deck::new("Test", "A test deck");
        assert_eq!(deck.name, "Test");
        assert!(deck.cards.is_empty());
        assert_eq!(deck.next_card_id, 1);
    }

    #[test]
    fn test_deck_add_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Front", "Back");
        assert_eq!(id, 1);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.next_card_id, 2);
    }

    #[test]
    fn test_deck_add_card_with_tags() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card_with_tags("Q", "A", &["tag1", "tag2"]);
        assert_eq!(id, 1);
        assert_eq!(deck.cards[0].tags.len(), 2);
    }

    #[test]
    fn test_deck_remove_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        assert!(deck.remove_card(id));
        assert!(deck.cards.is_empty());
    }

    #[test]
    fn test_deck_remove_nonexistent() {
        let mut deck = Deck::new("Test", "");
        assert!(!deck.remove_card(999));
    }

    #[test]
    fn test_deck_find_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        assert!(deck.find_card(id).is_some());
        assert!(deck.find_card(999).is_none());
    }

    #[test]
    fn test_deck_find_card_mut() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        if let Some(card) = deck.find_card_mut(id) {
            card.front = String::from("Updated");
        }
        assert_eq!(deck.cards[0].front, "Updated");
    }

    #[test]
    fn test_deck_due_cards_all_new() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        let due = deck.due_cards(1);
        assert_eq!(due.len(), 2);
    }

    #[test]
    fn test_deck_due_cards_after_review() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        // Card 0 is not due on day 1, card 1 is new so due
        let due = deck.due_cards(1);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0], 1);
    }

    #[test]
    fn test_deck_total_reviews() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[1].review.apply_rating(Rating::Easy, 1);
        deck.cards[1].review.apply_rating(Rating::Good, 2);
        assert_eq!(deck.total_reviews(), 3);
    }

    #[test]
    fn test_deck_average_accuracy() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[0].review.apply_rating(Rating::Good, 2);
        deck.cards[1].review.apply_rating(Rating::Again, 1);
        deck.cards[1].review.apply_rating(Rating::Again, 2);
        // Card 0: 100%, Card 1: 0% -> average 50%
        assert_eq!(deck.average_accuracy(), 50);
    }

    #[test]
    fn test_deck_mastered_count() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[0].review.apply_rating(Rating::Good, 2);
        deck.cards[0].review.apply_rating(Rating::Good, 8);
        assert_eq!(deck.mastered_count(), 1);
    }

    #[test]
    fn test_deck_all_tags() {
        let mut deck = Deck::new("Test", "");
        deck.add_card_with_tags("Q1", "A1", &["alpha", "beta"]);
        deck.add_card_with_tags("Q2", "A2", &["Beta", "gamma"]); // Beta dups beta
        let tags = deck.all_tags();
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_deck_cards_matching_all() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        let m = deck.cards_matching("", None);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn test_deck_cards_matching_search() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Rust question", "Answer");
        deck.add_card("Python question", "Answer");
        let m = deck.cards_matching("rust", None);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_deck_cards_matching_tag() {
        let mut deck = Deck::new("Test", "");
        deck.add_card_with_tags("Q1", "A1", &["alpha"]);
        deck.add_card_with_tags("Q2", "A2", &["beta"]);
        let m = deck.cards_matching("", Some("alpha"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_deck_shuffle() {
        let mut deck = Deck::new("Test", "");
        for i in 0..20 {
            deck.add_card(&format!("Q{i}"), &format!("A{i}"));
        }
        let original_ids: Vec<u32> = deck.cards.iter().map(|c| c.id).collect();
        deck.shuffle(&mut SeededRng::new(42));
        let shuffled_ids: Vec<u32> = deck.cards.iter().map(|c| c.id).collect();
        // Very unlikely that 20 cards stay in the same order
        assert_ne!(original_ids, shuffled_ids);
    }

    #[test]
    fn test_deck_shuffle_single_card() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.shuffle(&mut SeededRng::new(42));
        assert_eq!(deck.cards.len(), 1);
    }

    #[test]
    fn test_deck_shuffle_empty() {
        let mut deck = Deck::new("Test", "");
        deck.shuffle(&mut SeededRng::new(42)); // should not panic
        assert!(deck.cards.is_empty());
    }

    /// The card in each slot after each of `presses` consecutive shuffles of
    /// one deck, driven by one generator.
    ///
    /// Consecutive presses off one stream, not one press each off `presses`
    /// fresh seeds: a low-bit defect is a counter *along* one stream, so
    /// re-seeding between samples hides it behind the variety of the seeds
    /// themselves, and the test then passes on exactly the code it exists to
    /// catch.
    fn consecutive_shuffles(cards: usize, presses: usize) -> Vec<Vec<u32>> {
        let mut deck = Deck::new("Test", "");
        for i in 0..cards {
            deck.add_card(&format!("Q{i}"), &format!("A{i}"));
        }
        let mut rng = SeededRng::new(0x9E37_79B9_7F4A_7C15);
        (0..presses)
            .map(|_| {
                deck.shuffle(&mut rng);
                deck.cards.iter().map(|c| c.id).collect()
            })
            .collect()
    }

    #[test]
    fn every_card_can_end_up_last() {
        // The measurement that condemned the old shuffle. Against the app's
        // own seed schedule (42, then +7 per press) a twenty-card deck put
        // only four of its twenty cards in the last slot across forty presses
        // -- cards 3, 8, 13 and 18, step 5 -- because the last swap of a
        // Fisher-Yates draws at bound 2, and `state % 2` on a 2^32-modulus
        // LCG is the state's low bit, which is a counter. The fix reaches 18
        // of 20 at the same deck size and press count; the floor is 15, low
        // enough that honest sampling never trips it and far above 4.
        let orders = consecutive_shuffles(20, 40);
        let mut last_slot: Vec<u32> = Vec::new();
        for order in &orders {
            if let Some(id) = order.last() {
                if !last_slot.contains(id) {
                    last_slot.push(*id);
                }
            }
        }
        assert!(
            last_slot.len() >= 15,
            "only {} of 20 cards ever finished last across 40 shuffles: {last_slot:?}",
            last_slot.len()
        );
    }

    #[test]
    fn a_small_deck_reaches_most_of_its_orderings() {
        // Four cards have 24 orderings. The old shuffle reached 11 of them
        // over 40 presses; the fix reaches 20, at the same deck size and the
        // same press count. The floor is 18.
        let orders = consecutive_shuffles(4, 40);
        let mut distinct: Vec<Vec<u32>> = Vec::new();
        for order in orders {
            if !distinct.contains(&order) {
                distinct.push(order);
            }
        }
        assert!(
            distinct.len() >= 18,
            "40 shuffles of a four-card deck reached only {} of 24 orderings",
            distinct.len()
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn a_fresh_app_is_seeded_by_the_system_and_not_by_a_literal() {
        // A host `cargo test` has no SlateOS kernel to ask, so
        // `seeded_from_system` takes the fallback -- which is what makes this
        // checkable. Asserting *which* seed, not that two apps differ: a
        // variety check would pass on the old hardcoded 42 and fail on the fix.
        let draws = |app: &mut FlashcardsApp| -> Vec<usize> {
            (0..12).map(|_| app.rng.below(1000)).collect()
        };
        let from_system = draws(&mut FlashcardsApp::new());
        assert_eq!(
            from_system,
            draws(&mut FlashcardsApp::with_seed(FALLBACK_SEED)),
            "a fresh app did not ask the system for its seed"
        );
        assert_ne!(
            from_system,
            draws(&mut FlashcardsApp::with_seed(42)),
            "a fresh app still shuffles from a literal"
        );
    }

    // ── Export/Import tests ─────────────────────────────────────────

    #[test]
    fn test_export_basic() {
        let mut deck = Deck::new("Test", "Description");
        deck.add_card("Q1", "A1");
        let text = deck.export_text();
        assert!(text.contains("# Test"));
        assert!(text.contains("## Description"));
        assert!(text.contains("Q: Q1"));
        assert!(text.contains("A: A1"));
    }

    #[test]
    fn test_export_with_tags() {
        let mut deck = Deck::new("T", "D");
        deck.add_card_with_tags("Q1", "A1", &["tag1", "tag2"]);
        let text = deck.export_text();
        assert!(text.contains("T: tag1,tag2"));
    }

    #[test]
    fn test_import_basic() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: What is 1+1?\nA: 2\n\nQ: What is 2+2?\nA: 4\n";
        let count = deck.import_text(text);
        assert_eq!(count.cards, 2);
        assert_eq!(deck.cards.len(), 2);
        assert_eq!(deck.cards[0].front, "What is 1+1?");
        assert_eq!(deck.cards[0].back, "2");
    }

    #[test]
    fn test_import_with_tags() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: Question\nA: Answer\nT: math, algebra\n";
        let count = deck.import_text(text);
        assert_eq!(count.cards, 1);
        assert_eq!(deck.cards[0].tags.len(), 2);
        assert_eq!(deck.cards[0].tags[0], "math");
    }

    #[test]
    fn test_import_no_trailing_newline() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: Question\nA: Answer";
        let count = deck.import_text(text);
        assert_eq!(count.cards, 1);
    }

    /// Card text that the unescaped format could not survive. None of these is
    /// exotic for a study deck: the first is how you would actually write a
    /// two-line question, and `C:\n` is a path in a programming deck.
    const HOSTILE_TEXT: &[&str] = &[
        "line one\nA: forged answer\n\nQ: forged question",
        "trailing blank\n\nstill the same card",
        "T: forged,tags",
        "Q: not a new card",
        r"C:\n",
        r"back\slash",
        "  leading and trailing  ",
        "",
        "comma, separated, prose",
        "\r\n",
    ];

    #[test]
    fn no_card_field_can_forge_a_card() {
        for text in HOSTILE_TEXT {
            let mut original = Deck::new("Deck", "Desc");
            original.add_card(text, text);
            let exported = original.export_text();
            let mut imported = Deck::new("Imported", "");
            let count = imported.import_text(&exported);
            // One card in, one card out -- counting, not substring matching,
            // because escaped output legitimately contains the payload text.
            assert_eq!(count.cards, 1, "field forged a card: {text:?}");
            let card = imported.cards.first().expect("one card");
            assert_eq!(card.front, *text, "front changed: {text:?}");
            assert_eq!(card.back, *text, "back changed: {text:?}");
            assert!(card.tags.is_empty(), "field forged tags: {text:?}");
        }
    }

    #[test]
    fn a_hostile_deck_name_cannot_forge_a_card() {
        // Import ignores `#` lines, so a newline in the name is the cheapest
        // route into the card parser -- the value is never even looked at.
        //
        // The payload needs the trailing blank line: without it the forged
        // Q/A pair is merely overwritten by the next one and never committed,
        // which is how a first version of this test passed against unescaped
        // output. A card is only created by the blank line that ends it.
        let mut original = Deck::new("Name\nQ: forged\nA: forged\n", "D\nQ: also\nA: also\n");
        original.add_card("real", "real");
        let mut imported = Deck::new("Imported", "");
        let count = imported.import_text(&original.export_text());
        assert_eq!(count.cards, 1, "the deck name forged a card");
        let card = imported.cards.first().expect("one card");
        assert_eq!(card.front, "real");
    }

    #[test]
    fn a_tag_containing_a_comma_stays_one_tag() {
        let mut original = Deck::new("D", "");
        original.add_card_with_tags("q", "a", &["a,b", "plain", r"back\slash"]);
        let mut imported = Deck::new("Imported", "");
        imported.import_text(&original.export_text());
        let card = imported.cards.first().expect("one card");
        assert_eq!(card.tags, vec!["a,b", "plain", r"back\slash"]);
    }

    #[test]
    fn a_tag_keeps_its_own_edge_spaces() {
        // The `T:` reader trims each tag, which is the right leniency for a
        // hand-written `T: math, algebra` -- but it used to reach the value
        // too, so a tag of `" spaced "` came back as `"spaced"`. The writer
        // now spells an edge space `\s`, which is not a space and so is not
        // what the trim removes.
        let mut original = Deck::new("D", "");
        original.add_card_with_tags("q", "a", &[" spaced ", "\ttabbed"]);
        let mut imported = Deck::new("Imported", "");
        imported.import_text(&original.export_text());
        let card = imported.cards.first().expect("one card");
        assert_eq!(card.tags, vec![" spaced ", "\ttabbed"]);
    }

    #[test]
    fn a_backslash_before_an_n_survives_the_round_trip() {
        // The replace-chain decoder's signature failure: decoding `\n` before
        // `\\` turns the two-character text `\n` into a real newline.
        for text in [r"\n", r"\\n", r"\\", r"a\nb", "real\nnewline"] {
            assert_eq!(unescape_field(&escape_field(text)), text, "{text:?}");
        }
    }

    #[test]
    fn repeated_round_trips_reach_a_fixed_point() {
        let mut deck = Deck::new("D", "");
        for text in HOSTILE_TEXT {
            deck.add_card(text, text);
        }
        let once = deck.export_text();
        let mut reimported = Deck::new("D", "");
        reimported.import_text(&once);
        assert_eq!(reimported.export_text(), once, "export is not idempotent");
    }

    #[test]
    fn test_roundtrip_export_import() {
        let mut original = Deck::new("Test", "Desc");
        original.add_card_with_tags("Q1", "A1", &["t1"]);
        original.add_card("Q2", "A2");
        let exported = original.export_text();
        let mut imported = Deck::new("Imported", "");
        let count = imported.import_text(&exported);
        assert_eq!(count.cards, 2);
        assert_eq!(imported.cards[0].front, "Q1");
        assert_eq!(imported.cards[0].back, "A1");
        assert_eq!(imported.cards[0].tags.len(), 1);
        assert_eq!(imported.cards[1].front, "Q2");
    }

    // ── StudySession tests ──────────────────────────────────────────

    #[test]
    fn test_study_session_new() {
        let session = StudySession::new(vec![0, 1, 2]);
        assert_eq!(session.current_pos, 0);
        assert!(!session.flipped);
        assert_eq!(session.reviewed, 0);
        assert_eq!(session.remaining(), 3);
    }

    #[test]
    fn test_study_session_current_card_idx() {
        let session = StudySession::new(vec![5, 10, 15]);
        assert_eq!(session.current_card_idx(), Some(5));
    }

    #[test]
    fn test_study_session_empty() {
        let session = StudySession::new(vec![]);
        assert!(session.is_complete());
        assert_eq!(session.remaining(), 0);
        assert_eq!(session.current_card_idx(), None);
    }

    #[test]
    fn test_study_session_complete() {
        let mut session = StudySession::new(vec![0]);
        session.record_rating(Rating::Good);
        session.current_pos = 1;
        assert!(session.is_complete());
    }

    #[test]
    fn test_study_session_accuracy() {
        let mut session = StudySession::new(vec![0, 1, 2, 3]);
        session.record_rating(Rating::Good);
        session.record_rating(Rating::Easy);
        session.record_rating(Rating::Again);
        session.record_rating(Rating::Hard);
        assert_eq!(session.session_accuracy(), 50); // 2 good/easy out of 4
    }

    #[test]
    fn test_study_session_accuracy_empty() {
        let session = StudySession::new(vec![0]);
        assert_eq!(session.session_accuracy(), 0);
    }

    // ── FlashcardsApp tests ─────────────────────────────────────────

    #[test]
    fn test_app_new() {
        let app = FlashcardsApp::new();
        assert_eq!(app.decks.len(), 3);
        assert_eq!(app.selected_deck, 0);
        assert_eq!(app.view, AppView::DeckList);
        assert_eq!(app.current_day, today());
    }

    #[test]
    fn test_sample_decks_non_empty() {
        let app = FlashcardsApp::new();
        for deck in &app.decks {
            assert!(!deck.cards.is_empty());
            assert!(deck.cards.len() >= 10);
        }
    }

    #[test]
    fn test_sample_decks_have_tags() {
        let app = FlashcardsApp::new();
        for deck in &app.decks {
            let has_tags = deck.cards.iter().any(|c| !c.tags.is_empty());
            assert!(has_tags);
        }
    }

    #[test]
    fn test_add_deck() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.add_deck("My Deck", "Desc");
        assert_eq!(app.decks.len(), n + 1);
        assert_eq!(app.decks.last().unwrap().name, "My Deck");
    }

    #[test]
    fn test_remove_deck() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.remove_deck(0);
        assert_eq!(app.decks.len(), n - 1);
    }

    #[test]
    fn test_remove_last_deck_prevented() {
        let mut app = FlashcardsApp::new();
        app.decks.truncate(1);
        app.remove_deck(0);
        assert_eq!(app.decks.len(), 1); // cannot remove last
    }

    #[test]
    fn test_select_deck() {
        let mut app = FlashcardsApp::new();
        app.select_deck(1);
        assert_eq!(app.selected_deck, 1);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_select_deck_out_of_bounds() {
        let mut app = FlashcardsApp::new();
        app.select_deck(999);
        assert_eq!(app.selected_deck, 0); // unchanged
    }

    #[test]
    fn test_navigate_decks() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, 1);
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, 2);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_deck, 1);
    }

    #[test]
    fn test_navigate_deck_boundary_top() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_deck, 0);
    }

    #[test]
    fn test_navigate_deck_boundary_bottom() {
        let mut app = FlashcardsApp::new();
        let last = app.decks.len() - 1;
        app.selected_deck = last;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, last);
    }

    #[test]
    fn test_open_deck_enter() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Enter", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_new_deck_key() {
        // `n` names the deck first: it made one called "New Deck" at once,
        // and nothing could rename it.
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.handle_key("n", false, false);
        assert_eq!(app.view, AppView::DeckEditor);
        assert_eq!(app.decks.len(), n);
        app.deck_name.set_text("Verbs");
        assert!(app.save_deck_edits());
        assert_eq!(app.decks.len(), n + 1);
        assert_eq!(app.decks.last().unwrap().name, "Verbs");
    }

    #[test]
    fn test_delete_deck_key() {
        // It asks first now, and Y answers.
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.handle_key("Delete", false, false);
        assert_eq!(app.decks.len(), n, "deleted without asking");
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Y,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::from("y"),
        }));
        assert_eq!(app.decks.len(), n - 1);
    }

    #[test]
    fn test_deck_detail_back() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckList);
    }

    #[test]
    fn test_deck_detail_navigate_cards() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_card, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_card, 0);
    }

    #[test]
    fn test_deck_detail_new_card() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("n", false, false);
        assert_eq!(app.view, AppView::CardEditor);
        assert!(app.editing_card_id.is_none());
    }

    #[test]
    fn test_deck_detail_statistics() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("i", false, false);
        assert_eq!(app.view, AppView::Statistics);
    }

    #[test]
    fn test_statistics_back() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::Statistics;
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_study_start_due() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        assert_eq!(app.view, AppView::StudyMode);
        assert!(app.study_session.is_some());
    }

    #[test]
    fn test_study_start_all() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        assert_eq!(app.view, AppView::StudyMode);
        let session = app.study_session.as_ref().unwrap();
        assert_eq!(session.queue.len(), app.decks[0].cards.len());
    }

    #[test]
    fn test_study_start_empty_deck() {
        let mut app = FlashcardsApp::new();
        app.decks.push(Deck::new("Empty", ""));
        app.selected_deck = app.decks.len() - 1;
        app.start_study_all();
        assert_ne!(app.view, AppView::StudyMode);
    }

    #[test]
    fn test_study_flip() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        assert!(!app.study_session.as_ref().unwrap().flipped);
        app.flip_card();
        assert!(app.study_session.as_ref().unwrap().flipped);
    }

    #[test]
    fn test_study_rate_requires_flip() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        let pos_before = app.study_session.as_ref().unwrap().current_pos;
        app.rate_card(Rating::Good); // should do nothing without flip
        assert_eq!(app.study_session.as_ref().unwrap().current_pos, pos_before);
    }

    #[test]
    fn test_study_rate_advances() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        app.rate_card(Rating::Good);
        let session = app.study_session.as_ref().unwrap();
        assert_eq!(session.current_pos, 1);
        assert!(!session.flipped);
        assert_eq!(session.reviewed, 1);
    }

    #[test]
    fn test_study_full_session() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        let total = app.study_session.as_ref().unwrap().queue.len();
        for _ in 0..total {
            app.flip_card();
            app.rate_card(Rating::Good);
        }
        assert!(app.study_session.as_ref().unwrap().is_complete());
    }

    #[test]
    fn test_study_escape() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.handle_key("Escape", false, false);
        assert!(app.study_session.is_none());
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_study_space_flips() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.handle_key("Space", false, false);
        assert!(app.study_session.as_ref().unwrap().flipped);
    }

    #[test]
    fn test_study_number_keys_rate() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        app.handle_key("3", false, false); // Good
        assert_eq!(app.study_session.as_ref().unwrap().reviewed, 1);
    }

    #[test]
    fn test_card_editor_save_new() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        app.editor_front.set_text("New Q");
        app.editor_back.set_text("New A");
        app.editor_tags.set_text("tag1, tag2");
        assert!(app.save_card());
        let deck = &app.decks[app.selected_deck];
        let last = deck.cards.last().unwrap();
        assert_eq!(last.front, "New Q");
        assert_eq!(last.back, "New A");
        assert_eq!(last.tags.len(), 2);
    }

    #[test]
    fn test_card_editor_save_empty_rejected() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        assert!(!app.save_card()); // empty front/back
    }

    #[test]
    fn test_card_editor_edit_existing() {
        let mut app = FlashcardsApp::new();
        let card_id = app.decks[0].cards[0].id;
        app.open_edit_card(card_id);
        assert_eq!(app.editing_card_id, Some(card_id));
        app.editor_front.set_text("Updated question");
        app.editor_back.set_text("Updated answer");
        assert!(app.save_card());
        assert_eq!(app.decks[0].cards[0].front, "Updated question");
    }

    #[test]
    fn test_delete_selected_card() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let n = app.decks[0].cards.len();
        app.handle_key("x", false, false);
        assert_eq!(app.decks[0].cards.len(), n, "deleted without asking");
        let doomed = app.pending_delete.take().expect("x asks");
        app.delete_doomed(doomed);
        assert_eq!(app.decks[0].cards.len(), n - 1);
    }

    #[test]
    fn test_search_text() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_search_text("cap");
        assert_eq!(app.search_query, "cap");
    }

    #[test]
    fn test_search_backspace() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("test");
        app.handle_search_backspace();
        assert_eq!(app.search_query, "tes");
    }

    #[test]
    fn test_shuffle_key() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let ids_before: Vec<u32> = app.decks[0].cards.iter().map(|c| c.id).collect();
        app.handle_key("r", false, false);
        let ids_after: Vec<u32> = app.decks[0].cards.iter().map(|c| c.id).collect();
        assert_ne!(ids_before, ids_after);
    }

    #[test]
    fn test_tag_filter_cycle() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("t", false, false);
        assert!(app.tag_filter.is_some());
    }

    #[test]
    fn test_tag_filter_cleared() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        // Cycle through all tags until cleared
        let tags = app.decks[0].all_tags();
        for _ in 0..=tags.len() {
            app.handle_key("t", false, false);
        }
        assert!(app.tag_filter.is_none());
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_deck_list() {
        let app = FlashcardsApp::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_deck_detail() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_card_editor() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_mode() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_flipped() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_complete() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        let total = app.study_session.as_ref().unwrap().queue.len();
        for _ in 0..total {
            app.flip_card();
            app.rate_card(Rating::Good);
        }
        let cmds = app.render_commands();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_statistics() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::Statistics;
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_with_search() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("capital");
        let cmds = app.render_commands();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_with_tag_filter() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.tag_filter = Some(String::from("europe"));
        let cmds = app.render_commands();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_empty_search_results() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("zzzznonexistent");
        let cmds = app.render_commands();
        assert!(cmds.len() > 5);
    }

    // ── Rating tests ────────────────────────────────────────────────

    #[test]
    fn test_rating_quality_values() {
        assert_eq!(Rating::Again.quality(), 0);
        assert_eq!(Rating::Hard.quality(), 2);
        assert_eq!(Rating::Good.quality(), 3);
        assert_eq!(Rating::Easy.quality(), 5);
    }

    #[test]
    fn test_rating_labels() {
        assert_eq!(Rating::Again.label(), "Again");
        assert_eq!(Rating::Hard.label(), "Hard");
        assert_eq!(Rating::Good.label(), "Good");
        assert_eq!(Rating::Easy.label(), "Easy");
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_remove_deck_adjusts_selection() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = app.decks.len() - 1;
        let idx = app.selected_deck;
        app.remove_deck(idx);
        assert!(app.selected_deck < app.decks.len());
    }

    #[test]
    fn test_card_editor_cancel() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.open_new_card_editor();
        assert_eq!(app.view, AppView::CardEditor);
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_ensure_card_visible_scroll_down() {
        let mut app = FlashcardsApp::new();
        app.selected_card = 10;
        app.scroll_offset = 0;
        app.ensure_card_visible();
        assert!(app.scroll_offset > 0);
    }

    #[test]
    fn test_ensure_card_visible_scroll_up() {
        let mut app = FlashcardsApp::new();
        app.scroll_offset = 5;
        app.selected_card = 2;
        app.ensure_card_visible();
        assert_eq!(app.scroll_offset, 2);
    }

    #[test]
    fn test_current_deck_none() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = 999;
        assert!(app.current_deck().is_none());
    }

    #[test]
    fn test_matching_card_indices_no_deck() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = 999;
        assert!(app.matching_card_indices().is_empty());
    }

    #[test]
    fn test_sm2_long_sequence() {
        let mut rd = ReviewData::new();
        // Simulate a long study sequence
        let mut day = 1u32;
        for _ in 0..10 {
            rd.apply_rating(Rating::Good, day);
            day += rd.interval_days;
        }
        assert!(rd.repetitions > 5);
        assert!(rd.interval_days > 10);
    }

    #[test]
    fn test_sm2_mixed_ratings() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Easy, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Hard, 8);
        rd.apply_rating(Rating::Good, 9);
        rd.apply_rating(Rating::Easy, 10);
        assert_eq!(rd.total_reviews, 5);
        assert!(rd.ease_factor >= SM2_MIN_EASE);
    }

    #[test]
    fn test_import_skips_header_lines() {
        let mut deck = Deck::new("T", "D");
        let text = "# Deck Name\n## Description\n\nQ: Question\nA: Answer\n";
        let count = deck.import_text(text);
        assert_eq!(count.cards, 1);
    }

    #[test]
    fn test_import_empty_text() {
        let mut deck = Deck::new("T", "D");
        let count = deck.import_text("");
        assert_eq!(count.cards, 0);
    }

    #[test]
    fn test_study_no_due_cards() {
        let mut app = FlashcardsApp::new();
        // Review every card as of today, so none is due today.
        let day = app.current_day;
        for card in &mut app.decks[0].cards {
            card.review.apply_rating(Rating::Good, day);
        }
        app.start_study();
        assert!(app.study_session.is_none());
    }

    // ── Card list layout ────────────────────────────────────────────────

    type TextCell = (f32, String, f32, FontWeightHint);

    /// A deck whose fronts and backs are deliberately long and non-ASCII —
    /// language decks are the one thing guaranteed to carry non-Latin text,
    /// and the old code cut the front at byte 47 and the back at byte 57,
    /// both of which land inside a character for most of these.
    fn crowded_deck() -> Deck {
        let mut deck = Deck::new("Vocabulary", "long non-ASCII entries");
        let pinned_front = format!("{}é{}", "a".repeat(46), "z".repeat(40));
        let pinned_back = format!("{}é{}", "b".repeat(56), "y".repeat(40));
        let entries = [
            (
                "この漢字の読み方と意味を答えてください。よく使われる表現です。",
                "「かんじ」— 中国から伝わった文字。日本語の表記に用いられる。",
            ),
            (
                "Πώς μεταφράζεται αυτή η φράση στα αγγλικά με ακρίβεια;",
                "It is translated as \"how is this phrase rendered in English\".",
            ),
            (
                "Как правильно перевести это длинное предложение на английский?",
                "Переводится как «how do you correctly translate this sentence».",
            ),
            // Byte 47 of the front and byte 57 of the back are each a
            // continuation byte, pinning both old cut points exactly.
            (pinned_front.as_str(), pinned_back.as_str()),
            ("short", "brief"),
        ];
        for (front, back) in entries {
            let id = deck.next_card_id;
            deck.next_card_id += 1;
            let mut card = Card::new(id, front, back);
            card.tags = vec![
                String::from("语言"),
                String::from("уровень-продвинутый"),
                String::from("phrases"),
            ];
            deck.cards.push(card);
        }
        deck
    }

    fn deck_detail(width: f32, tag_filter: Option<&str>) -> Vec<RenderCommand> {
        let mut app = FlashcardsApp::new();
        app.width = width;
        app.height = 900.0;
        app.decks = vec![crowded_deck()];
        app.selected_deck = 0;
        app.selected_card = 0;
        app.scroll_offset = 0;
        app.view = AppView::DeckDetail;
        app.tag_filter = tag_filter.map(String::from);
        let mut f = Frame::new(app.width, app.height);
        app.render_deck_detail(&mut f);
        f.into_tree().commands
    }

    fn texts_of(cmds: &[RenderCommand]) -> Vec<TextCell> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    fn app_at(width: f32) -> FlashcardsApp {
        let mut app = FlashcardsApp::new();
        app.width = width;
        app
    }

    #[test]
    fn a_non_ascii_card_does_not_abort_the_deck_detail_view() {
        // Regression: the front was `&card.front[..47]` behind a `len() > 50`
        // guard and the back `&card.back[..57]` behind `len() > 60`. Both
        // abort when the byte index lands inside a multi-byte character, and
        // both guards made that *more* likely rather than less — a
        // 16-character Japanese prompt is 48 bytes, so it always took the
        // truncating branch. A Japanese vocabulary deck crashed on open.
        for width in [200.0_f32, 320.0, 480.0, 640.0, 900.0, 1400.0] {
            let cmds = deck_detail(width, Some("уровень-продвинутый"));
            assert!(!cmds.is_empty(), "at width {width} nothing was drawn");
        }
    }

    #[test]
    fn the_card_columns_fill_the_row() {
        // The point of fractions summing to 1.0: the row fills its container
        // at every width, rather than being right at exactly one size.
        let sum: f32 = FlashcardsApp::CARD_FRACTIONS.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "the card column fractions sum to {sum}, not 1"
        );
        for width in [200.0_f32, 320.0, 480.0, 640.0, 900.0, 1400.0] {
            let app = app_at(width);
            let columns = app.card_columns();
            let table = FlashcardsApp::card_table(&columns);
            let margin = width - FlashcardsApp::PADDING;
            let end = table.right(FlashcardsApp::CARD_STATUS);
            assert!(
                (end - margin).abs() < 0.01,
                "at width {width} the card row ends at {end}, not at the \
                 margin {margin}"
            );
        }
    }

    #[test]
    fn no_card_cell_escapes_its_column() {
        // The three columns used to mix proportional and absolute coordinates,
        // so Front ran into Tags below 480px and Tags into Status below 640px.
        let mut checked = 0usize;
        for width in [200.0_f32, 320.0, 480.0, 640.0, 900.0, 1400.0] {
            let app = app_at(width);
            let columns = app.card_columns();
            let table = FlashcardsApp::card_table(&columns);
            let spans: Vec<(f32, f32)> = table.spans();

            for (x, text, size, weight) in texts_of(&deck_detail(width, None)) {
                // Only cells that sit on a column's left edge belong to it;
                // everything else in this view (title, shortcuts, search) is
                // laid out separately.
                let Some(&(left, right)) = spans.iter().find(|(l, _)| (l - x).abs() < 0.01) else {
                    continue;
                };
                if size == CARD_BACK_SIZE && left == spans[FlashcardsApp::CARD_FRONT].0 {
                    // The back preview shares the Front column's left edge but
                    // spans the whole row; it has its own test.
                    continue;
                }
                let end = x + text::measure(&text, size, weight);
                assert!(
                    end <= right + 0.5,
                    "at width {width} the cell {text:?} at {left} draws to \
                     {end}, past its column edge {right}"
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 6 * 4,
            "only {checked} cells examined — at least a heading and three rows \
             per width were expected, so this test would pass without checking \
             what it claims to"
        );
    }

    #[test]
    fn the_back_preview_stays_inside_the_row() {
        // The back sits on the row's second line, so it gets the whole row
        // rather than the Front column — but it still has to stop at the
        // right margin.
        let mut checked = 0usize;
        for width in [200.0_f32, 320.0, 640.0, 1400.0] {
            let app = app_at(width);
            let (row_x, row_w) = app.card_row_span();
            for (x, text, size, weight) in texts_of(&deck_detail(width, None)) {
                if (x - row_x).abs() > 0.01 || size != CARD_BACK_SIZE {
                    continue;
                }
                let end = x + text::measure(&text, size, weight);
                assert!(
                    end <= row_x + row_w + 0.5,
                    "at width {width} the back preview {text:?} draws to {end}, \
                     past the row's right edge {}",
                    row_x + row_w
                );
                checked += 1;
            }
        }
        assert!(checked >= 4, "only {checked} back previews examined");
    }

    #[test]
    fn the_tag_filter_pill_stays_inside_the_panel() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // The pill was a flat 100px at a proportional anchor, so it ran off
        // the right edge whenever the content was narrower than 270px. A
        // `max_width` on the label inside it is not a bound on the pill.
        let mut checked = 0usize;
        for width in [180.0_f32, 220.0, 300.0, 400.0, 900.0] {
            for cmd in deck_detail(width, Some("уровень-продвинутый")) {
                let RenderCommand::FillRect {
                    x, width: w, color, ..
                } = cmd
                else {
                    continue;
                };
                if color != pal.mauve {
                    continue;
                }
                assert!(w >= 0.0, "at width {width} the pill is {w} wide");
                assert!(
                    x + w <= width - FlashcardsApp::PADDING + 0.5,
                    "at width {width} the tag pill runs to {}, past the margin {}",
                    x + w,
                    width - FlashcardsApp::PADDING
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 3,
            "only {checked} pills examined — the filter matched almost nothing"
        );
    }

    #[test]
    fn a_short_card_is_drawn_verbatim() {
        // Otherwise every "it fits" assertion above is satisfiable by drawing
        // nothing at all.
        let texts: Vec<String> = texts_of(&deck_detail(1400.0, None))
            .into_iter()
            .map(|(_, t, _, _)| t)
            .collect();
        assert!(
            texts.iter().any(|t| t == "short"),
            "the short front was cut; got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "brief"),
            "the short back was cut; got {texts:?}"
        );
    }

    // == The search box ========================================================
    //
    // `handle_search_text` and `handle_search_backspace` were written when the
    // file was and had no callers: every letter in the deck view is a command,
    // so there was no mode in which a letter meant "search".

    /// An app sitting in a deck, which is where the search box is drawn.
    fn app_in_deck() -> FlashcardsApp {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app
    }

    #[test]
    fn slash_opens_the_search_box() {
        let mut app = app_in_deck();
        assert!(!app.search_active);
        app.handle_key("/", false, false);
        assert!(app.search_active);
    }

    #[test]
    fn typing_in_the_search_box_filters_the_deck() {
        let mut app = app_in_deck();
        let all = app.matching_card_indices().len();
        assert!(all > 1, "the sample deck should have a few cards");
        app.handle_key("/", false, false);
        for ch in "zzzz".chars() {
            app.handle_key(&ch.to_string(), false, false);
        }
        assert_eq!(app.search_query, "zzzz");
        assert!(
            app.matching_card_indices().len() < all,
            "a query that matches nothing should not leave every card showing"
        );
    }

    #[test]
    fn a_letter_is_a_command_until_the_search_box_is_open() {
        let mut app = app_in_deck();
        // `s` starts a study session outside the box.
        app.handle_key("s", false, false);
        assert!(app.study_session.is_some());
        assert_eq!(app.search_query, "");
    }

    #[test]
    fn a_letter_is_text_once_the_search_box_is_open() {
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        app.handle_key("s", false, false);
        assert!(
            app.study_session.is_none(),
            "a letter typed into the search box must not also run its shortcut"
        );
        assert_eq!(app.search_query, "s");
    }

    #[test]
    fn backspace_deletes_from_the_query() {
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        for ch in "abc".chars() {
            app.handle_key(&ch.to_string(), false, false);
        }
        app.handle_key("Backspace", false, false);
        assert_eq!(app.search_query, "ab");
    }

    #[test]
    fn space_is_typed_rather_than_ignored() {
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        app.handle_key("a", false, false);
        app.handle_key("Space", false, false);
        app.handle_key("b", false, false);
        assert_eq!(app.search_query, "a b");
    }

    #[test]
    fn enter_leaves_the_box_and_keeps_the_filter() {
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        app.handle_key("a", false, false);
        app.handle_key("Enter", false, false);
        assert!(!app.search_active);
        assert_eq!(app.search_query, "a", "the filter is still on");
    }

    #[test]
    fn escape_clears_the_filter_and_leaves_the_box() {
        let mut app = app_in_deck();
        let all = app.matching_card_indices().len();
        app.handle_key("/", false, false);
        for ch in "zzzz".chars() {
            app.handle_key(&ch.to_string(), false, false);
        }
        app.handle_key("Escape", false, false);
        assert!(!app.search_active);
        assert_eq!(app.search_query, "");
        assert_eq!(
            app.matching_card_indices().len(),
            all,
            "a filter you cannot see the edge of is one you forget is on"
        );
    }

    #[test]
    fn leaving_the_deck_leaves_the_search_with_it() {
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        app.handle_key("a", false, false);
        app.handle_key("Escape", false, false);
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckList);
        assert!(!app.search_active);
        assert_eq!(app.search_query, "");
    }

    #[test]
    fn opening_the_search_box_earns_a_redraw() {
        // The fingerprint decides whether a keystroke redrew anything, and
        // entering the box changes nothing else on screen.
        let mut app = app_in_deck();
        let before = app.state_fingerprint();
        app.handle_key("/", false, false);
        assert_ne!(before, app.state_fingerprint());
    }

    // == One way to add a card, one way to remove one ==========================

    #[test]
    fn a_saved_card_gets_its_id_from_the_deck() {
        // `save_card` used to allocate the id itself and push the card
        // directly, duplicating `Deck::add_card`.
        let mut app = FlashcardsApp::new();
        let next = app.decks[app.selected_deck].next_card_id;
        app.open_new_card_editor();
        app.editor_front.set_text("Q");
        app.editor_back.set_text("A");
        assert!(app.save_card());
        let deck = &app.decks[app.selected_deck];
        assert_eq!(deck.cards.last().map(|c| c.id), Some(next));
        assert_eq!(
            deck.next_card_id,
            next.saturating_add(1),
            "the counter moves on however the card was added"
        );
    }

    #[test]
    fn deleting_a_filtered_card_deletes_the_one_that_was_selected() {
        // The index came from the filtered list and the removal happens in the
        // unfiltered one; going through `Deck::remove_card` by id is what
        // makes those the same card.
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        // Filter to something that matches exactly one card, whatever the
        // sample deck happens to hold.
        let target = {
            let deck = &app.decks[app.selected_deck];
            deck.cards
                .get(1)
                .map(|c| c.front.clone())
                .expect("a second card")
        };
        for ch in target.chars() {
            app.handle_key(&ch.to_string(), false, false);
        }
        app.handle_key("Enter", false, false);
        let matching = app.matching_card_indices();
        assert!(!matching.is_empty(), "the filter should match its own card");
        let doomed = {
            let deck = &app.decks[app.selected_deck];
            deck.cards[matching[0]].id
        };
        app.selected_card = 0;
        app.handle_key("x", false, false);
        let asked = app.pending_delete.take().expect("x asks");
        app.delete_doomed(asked);
        let deck = &app.decks[app.selected_deck];
        assert!(
            !deck.cards.iter().any(|c| c.id == doomed),
            "the card that was showing is the card that went"
        );
    }

    #[test]
    fn a_named_key_is_not_typed_into_the_search_box() {
        // "Up", "Delete" and friends reach the same arm a letter does; without
        // a length check the box would fill up with the words for arrow keys.
        let mut app = app_in_deck();
        app.handle_key("/", false, false);
        app.handle_key("a", false, false);
        for named in ["Up", "Down", "Left", "Right", "Delete", "Tab"] {
            app.handle_key(named, false, false);
        }
        assert_eq!(app.search_query, "a");
    }

    #[test]
    fn the_fingerprint_notices_the_search_box() {
        // Directly, because opening the box also sets a status message, which
        // would mask the flag being missing from the summary.
        let mut app = app_in_deck();
        let before = app.state_fingerprint();
        app.search_active = !app.search_active;
        assert_ne!(
            before,
            app.state_fingerprint(),
            "a keystroke whose only effect is opening the box still has to \
             earn its redraw"
        );
    }

    #[test]
    fn typing_a_filter_puts_the_selection_back_on_the_first_card() {
        let mut app = app_in_deck();
        app.selected_card = 2;
        app.handle_key("/", false, false);
        app.handle_key("a", false, false);
        assert_eq!(
            app.selected_card, 0,
            "a selection counted in the old list names a different card in \
             the filtered one, and may name none at all"
        );
    }

    #[test]
    fn search_text_is_refused_outside_the_deck_view() {
        // `handle_search_text` is public to the rest of the file and the box
        // is a deck-view thing; the guard is what stops a stray call filtering
        // a deck nobody is looking at.
        let mut app = FlashcardsApp::new();
        app.view = AppView::StudyMode;
        app.handle_search_text("x");
        assert_eq!(app.search_query, "");
        app.search_query = String::from("ab");
        app.handle_search_backspace();
        assert_eq!(app.search_query, "ab");
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut FlashcardsApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = FlashcardsApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }
    // ── Typing, naming, asking, and the pointer ──────────────────────

    use guitk::probe::{self, Probe};

    impl Probe for FlashcardsApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1000.0, 700.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    /// Type `text` through the event layer, as a keyboard would.
    fn type_text(app: &mut FlashcardsApp, text: &str) {
        for c in text.chars() {
            app.handle_event(&typed(c));
        }
    }

    fn shift_press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        })
    }

    /// A card can be typed and saved. The editor's keys were Enter and
    /// Escape, so no card could be made or changed: every new one was refused
    /// as empty.
    #[test]
    fn a_card_can_be_typed_and_saved() {
        let mut app = app_in_deck();
        let n = app.decks[app.selected_deck].cards.len();
        app.handle_event(&typed('n'));
        assert_eq!(app.view, AppView::CardEditor);
        type_text(&mut app, "What is 2+2?");
        app.handle_event(&press(Key::Tab));
        type_text(&mut app, "Four");
        app.handle_event(&press(Key::Backspace));
        type_text(&mut app, "r");
        app.handle_event(&press(Key::Tab));
        type_text(&mut app, "maths, easy");
        app.handle_event(&press(Key::Enter));
        let deck = &app.decks[app.selected_deck];
        assert_eq!(deck.cards.len(), n + 1);
        let card = deck.cards.last().unwrap();
        assert_eq!(card.front, "What is 2+2?");
        assert_eq!(card.back, "Four");
        assert_eq!(card.tags, vec![String::from("maths"), String::from("easy")]);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    /// Tab walks the fields and Shift+Tab walks back.
    #[test]
    fn tab_walks_the_editors_fields() {
        let mut app = app_in_deck();
        app.open_new_card_editor();
        assert_eq!(app.field, Field::Front);
        app.handle_event(&press(Key::Tab));
        assert_eq!(app.field, Field::Back);
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        assert_eq!(app.field, Field::Front, "Tab does not wrap");
        app.handle_event(&shift_press(Key::Tab));
        assert_eq!(app.field, Field::Tags);
    }

    /// An editor takes every key: `s` in an answer does not start studying,
    /// and `x` does not delete.
    #[test]
    fn an_editor_takes_every_key() {
        let mut app = app_in_deck();
        app.open_new_card_editor();
        type_text(&mut app, "sx/");
        assert_eq!(app.view, AppView::CardEditor);
        assert!(app.pending_delete.is_none());
        assert_eq!(app.editor_front.text(), "sx/");
    }

    /// A new deck is named when it is made, and can be renamed. `n` made one
    /// called "New Deck" that nothing could rename.
    #[test]
    fn a_deck_is_named_and_can_be_renamed() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.handle_event(&typed('n'));
        assert_eq!(app.view, AppView::DeckEditor);
        type_text(&mut app, "Verbs");
        app.handle_event(&press(Key::Tab));
        type_text(&mut app, "Spanish, irregular");
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.decks.len(), n + 1);
        let made = app.decks.len() - 1;
        assert_eq!(app.decks[made].name, "Verbs");
        assert_eq!(app.decks[made].description, "Spanish, irregular");
        assert_eq!(app.selected_deck, made);
        app.handle_event(&typed('e'));
        app.handle_event(&ctrl_press(Key::A));
        type_text(&mut app, "Irregular verbs");
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.decks[made].name, "Irregular verbs");
        assert_eq!(app.decks.len(), n + 1, "a rename made a deck");
    }

    /// A deck needs a name.
    #[test]
    fn a_nameless_deck_is_refused() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.open_new_deck_editor();
        type_text(&mut app, "   ");
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.decks.len(), n);
        assert_eq!(app.view, AppView::DeckEditor);
        assert_eq!(app.status_msg, "A deck needs a name");
    }

    /// Deleting a deck or a card asks first, is drawn, and only Y deletes.
    /// Both went at once, with their whole history.
    #[test]
    fn deleting_asks_first_and_only_y_deletes() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        assert_eq!(
            app.handle_event(&typed('x')),
            EventResult::Consumed,
            "the question is not drawn"
        );
        assert!(
            drawn_texts(&app)
                .iter()
                .any(|t| t.starts_with("Delete the deck"))
        );
        app.handle_event(&typed('n'));
        assert_eq!(app.decks.len(), n);
        assert_eq!(
            app.view,
            AppView::DeckList,
            "the answer was taken as a command too"
        );
        app.handle_event(&typed('x'));
        app.handle_event(&typed('y'));
        assert_eq!(app.decks.len(), n - 1);

        let mut app = app_in_deck();
        let cards = app.decks[app.selected_deck].cards.len();
        app.handle_event(&typed('x'));
        app.handle_event(&press(Key::Escape));
        assert_eq!(app.decks[app.selected_deck].cards.len(), cards);
        app.handle_event(&typed('x'));
        probe::click(&mut app, Target::ConfirmDelete);
        assert_eq!(app.decks[app.selected_deck].cards.len(), cards - 1);
        app.handle_event(&typed('x'));
        probe::click(&mut app, Target::KeepIt);
        assert_eq!(app.decks[app.selected_deck].cards.len(), cards - 1);
    }

    /// Every deck-list button answers the pointer.
    #[test]
    fn every_deck_list_button_answers_the_pointer() {
        let mut app = FlashcardsApp::new();
        probe::click(&mut app, Target::NewDeck);
        assert_eq!(app.view, AppView::DeckEditor);
        probe::click(&mut app, Target::Cancel);
        assert_eq!(app.view, AppView::DeckList);
        probe::click(&mut app, Target::EditDeck);
        assert_eq!(app.deck_name.text(), app.decks[0].name);
        probe::click(&mut app, Target::Cancel);
        probe::click(&mut app, Target::DeleteDeck);
        assert!(app.pending_delete.is_some());
        // Beside the question's card: the backdrop's middle is the card.
        assert_eq!(
            app.frame().hit_test(5.0, 60.0),
            Some(Target::QuestionBackdrop)
        );
        app.handle_event(&Event::Mouse(MouseEvent {
            x: 5.0,
            y: 60.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert!(app.pending_delete.is_none());
        probe::click(&mut app, Target::Import);
        assert!(app.picker.is_open());
        app.handle_event(&press(Key::Escape));
        probe::click(&mut app, Target::Export);
        assert!(app.picker.is_open());
    }

    /// A press on a deck chooses it, and a press on the chosen one opens it.
    #[test]
    fn a_deck_press_chooses_and_a_second_opens() {
        let mut app = FlashcardsApp::new();
        probe::click(&mut app, Target::DeckRow(2));
        assert_eq!(app.selected_deck, 2);
        assert_eq!(app.view, AppView::DeckList);
        probe::click(&mut app, Target::DeckRow(2));
        assert_eq!(app.view, AppView::DeckDetail);
    }

    /// Every deck-view button answers the pointer.
    #[test]
    fn every_deck_view_button_answers_the_pointer() {
        let mut app = app_in_deck();
        probe::click(&mut app, Target::StudyDue);
        assert_eq!(app.view, AppView::StudyMode);
        probe::click(&mut app, Target::EndSession);
        probe::click(&mut app, Target::StudyAll);
        assert_eq!(app.view, AppView::StudyMode);
        probe::click(&mut app, Target::EndSession);
        probe::click(&mut app, Target::NewCard);
        assert_eq!(app.view, AppView::CardEditor);
        probe::click(&mut app, Target::Cancel);
        probe::click(&mut app, Target::EditCard);
        assert!(app.editing_card_id.is_some());
        probe::click(&mut app, Target::Cancel);
        probe::click(&mut app, Target::DeleteCard);
        assert!(matches!(app.pending_delete, Some(Doomed::Card(_))));
        probe::click(&mut app, Target::KeepIt);
        probe::click(&mut app, Target::Shuffle);
        assert_eq!(app.status_msg, "Deck shuffled");
        probe::click(&mut app, Target::TagChip);
        assert!(app.tag_filter.is_some(), "the chip set no filter");
        probe::click(&mut app, Target::Search);
        assert!(app.search_active);
        app.handle_event(&press(Key::Escape));
        probe::click(&mut app, Target::Stats);
        assert_eq!(app.view, AppView::Statistics);
    }

    /// The tag chip is there with no filter set, or there would be nothing
    /// to press to set one.
    #[test]
    fn the_tag_chip_is_there_with_no_filter() {
        let app = app_in_deck();
        assert!(app.tag_filter.is_none());
        assert!(probe::rect_of(&app, Target::TagChip).is_some());
    }

    /// A press on a card chooses it, and a press on the chosen one edits it.
    #[test]
    fn a_card_press_chooses_and_a_second_edits() {
        let mut app = app_in_deck();
        probe::click(&mut app, Target::CardRow(2));
        assert_eq!(app.selected_card, 2);
        assert_eq!(app.view, AppView::DeckDetail);
        probe::click(&mut app, Target::CardRow(2));
        assert_eq!(app.view, AppView::CardEditor);
    }

    /// The editor's fields and buttons answer the pointer.
    #[test]
    fn the_editor_answers_the_pointer() {
        let mut app = app_in_deck();
        let n = app.decks[app.selected_deck].cards.len();
        app.open_new_card_editor();
        probe::click(&mut app, Target::Field(Field::Back));
        assert_eq!(app.field, Field::Back);
        type_text(&mut app, "An answer");
        probe::click(&mut app, Target::Field(Field::Front));
        type_text(&mut app, "A question");
        probe::click(&mut app, Target::Save);
        let card = app.decks[app.selected_deck].cards.last().unwrap();
        assert_eq!(
            (card.front.as_str(), card.back.as_str()),
            ("A question", "An answer")
        );
        assert_eq!(app.decks[app.selected_deck].cards.len(), n + 1);
    }

    /// Study answers the pointer: the card turns over, a rating rates, and
    /// the session ends.
    #[test]
    fn study_answers_the_pointer() {
        let mut app = app_in_deck();
        app.start_study_all();
        probe::click(&mut app, Target::StudyCard);
        assert!(app.study_session.as_ref().unwrap().flipped);
        assert!(
            probe::rect_of(&app, Target::StudyCard).is_none(),
            "a turned card turns again"
        );
        probe::click(&mut app, Target::Rate(Rating::Good));
        let session = app.study_session.as_ref().unwrap();
        assert_eq!((session.reviewed, session.flipped), (1, false));
        probe::click(&mut app, Target::EndSession);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    /// The header's Back goes back from every view but the deck list.
    #[test]
    fn back_goes_back_from_every_view() {
        let mut app = app_in_deck();
        assert!(probe::rect_of(&FlashcardsApp::new(), Target::Back).is_none());
        app.handle_event(&typed('/'));
        type_text(&mut app, "cap");
        probe::click(&mut app, Target::Back);
        assert_eq!(app.view, AppView::DeckList, "Back only closed the search");
        app.select_deck(0);
        for (open, from) in [
            (Target::NewCard, AppView::CardEditor),
            (Target::Stats, AppView::Statistics),
            (Target::StudyAll, AppView::StudyMode),
        ] {
            probe::click(&mut app, open);
            assert_eq!(app.view, from);
            probe::click(&mut app, Target::Back);
            assert_eq!(app.view, AppView::DeckDetail, "from {from:?}");
        }
    }

    /// The card list fits the window, and the wheel scrolls it. It showed
    /// eight rows whatever the height.
    #[test]
    fn the_card_list_fits_the_window_and_scrolls() {
        let mut app = app_in_deck();
        for i in 0..40 {
            app.decks[0].add_card(&format!("Question {i}"), &format!("Answer {i}"));
        }
        app.height = 1100.0;
        let (_, tall) = app.card_list_geometry();
        assert!(tall > 8, "{tall} rows in a 1100-pixel window");
        app.height = 700.0;
        let (_, visible) = app.card_list_geometry();
        assert!(probe::rect_of(&app, Target::CardRow(visible)).is_none());
        for _ in 0..5 {
            probe::scroll_at_point(&mut app, Target::CardList, -3.0);
        }
        assert!(app.scroll_offset > 0);
        assert!(probe::rect_of(&app, Target::CardRow(app.scroll_offset)).is_some());
    }

    /// The deck list scrolls, and follows the chosen deck. It had no
    /// scrolling: a deck past the bottom could be chosen and never seen.
    #[test]
    fn the_deck_list_scrolls_and_follows_the_chosen_deck() {
        let mut app = FlashcardsApp::new();
        for i in 0..20 {
            app.add_deck(&format!("Deck {i}"), "");
        }
        let last = app.decks.len() - 1;
        for _ in 0..last {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.selected_deck, last);
        assert!(
            probe::rect_of(&app, Target::DeckRow(last)).is_some(),
            "the chosen deck is off screen"
        );
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::DeckList, 3.0);
        }
        assert_eq!(app.deck_scroll, 0);
        assert!(probe::rect_of(&app, Target::DeckRow(0)).is_some());
    }

    /// The notice about the included decks is drawn where it can be read: it
    /// was drawn before the header, which painted over it.
    #[test]
    fn the_notice_is_drawn_below_the_header() {
        let app = FlashcardsApp::new();
        for line in SAMPLE_AND_PROGRESS_LINES {
            let y = app
                .render_commands()
                .into_iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, y, .. } if text == line => Some(y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{line:?} is not drawn"));
            assert!(y > FlashcardsApp::HEADER_H, "{line:?} at y={y}");
        }
    }

    /// No view advertises the day key that was removed.
    #[test]
    fn no_view_advertises_the_removed_day_key() {
        let app = app_in_deck();
        assert!(!drawn_texts(&app).iter().any(|t| t.contains("[D]ay")));
    }

    /// Every drawn string.
    fn drawn_texts(app: &FlashcardsApp) -> Vec<String> {
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Every key on the list does something somewhere.
    #[test]
    fn every_advertised_key_does_something() {
        let states = || {
            let list = FlashcardsApp::new();
            let deck = app_in_deck();
            let mut editor = app_in_deck();
            editor.open_new_card_editor();
            let mut studying = app_in_deck();
            studying.start_study_all();
            let mut flipped = app_in_deck();
            flipped.start_study_all();
            flipped.flip_card();
            let mut down = app_in_deck();
            down.selected_card = 1;
            vec![list, deck, editor, studying, flipped, down]
        };
        for (row, what) in SHORTCUTS {
            let strokes = guitk::shortcut::keystrokes(row).unwrap_or_else(|e| panic!("{e}"));
            for stroke in strokes {
                let taken = states().iter_mut().any(|app| {
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    taken,
                    "the list offers {row:?} ({what}) and nothing takes {:?}",
                    stroke.key
                );
            }
        }
    }

    /// F1 raises the list, which is modal, and a press puts it away.
    #[test]
    fn the_list_of_keys_is_modal() {
        let mut app = FlashcardsApp::new();
        app.handle_event(&press(Key::F1));
        assert!(app.show_help);
        let n = app.decks.len();
        assert_eq!(app.handle_event(&typed('x')), EventResult::Ignored);
        assert!(app.pending_delete.is_none() && app.decks.len() == n);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
    }

    /// A long answer wraps on the card. It was one line cut with an
    /// ellipsis, in the one place it has to be read in full.
    #[test]
    fn a_long_answer_wraps_on_the_study_card() {
        let mut app = app_in_deck();
        let long = "The mitochondrion is the organelle in which cellular respiration \
                    produces most of the adenosine triphosphate that the cell uses";
        app.decks[app.selected_deck].cards[0].back = String::from(long);
        app.start_study_all();
        app.flip_card();
        let texts = drawn_texts(&app);
        let joined = texts
            .iter()
            .filter(|t| long.contains(t.as_str()) && t.len() > 3)
            .cloned()
            .collect::<Vec<_>>();
        assert!(joined.len() >= 2, "the answer is on one line: {joined:?}");
        assert_eq!(joined.join(" "), long);
    }

    /// A letter key arrives as its letter even with no text on it, shifted
    /// or not -- as a keystroke built from its key alone does.
    #[test]
    fn a_letter_key_names_its_letter_without_text() {
        let bare = |key, shift| KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                shift,
                ..Modifiers::NONE
            },
            text: String::new(),
        };
        assert_eq!(
            FlashcardsApp::key_name(&bare(Key::S, false)).as_deref(),
            Some("s")
        );
        assert_eq!(
            FlashcardsApp::key_name(&bare(Key::S, true)).as_deref(),
            Some("S")
        );
        assert_eq!(
            FlashcardsApp::key_name(&bare(Key::Num3, false)).as_deref(),
            Some("3")
        );
        assert_eq!(
            FlashcardsApp::key_name(&bare(Key::Slash, false)).as_deref(),
            Some("/")
        );
        assert_eq!(
            FlashcardsApp::key_name(&bare(Key::Up, false)).as_deref(),
            Some("Up")
        );
    }
}
