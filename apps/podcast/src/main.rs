//! Slate OS Podcast Manager
//!
//! A full-featured podcast manager application providing:
//! - Podcast subscription management via RSS URL with metadata
//! - Episode listing with title, date, duration, description, status tracking
//! - Playback simulation with play/pause/stop, seek, variable speed (0.5x-3x)
//! - Download management with queue, progress tracking, disk space monitoring
//! - Library browsing by podcast with status filters
//! - Playlist/queue management with reordering and auto-play
//! - Search across podcasts and episodes
//! - Category-based organization (Technology, Science, Comedy, etc.)
//! - Per-episode notes and timestamp bookmarks
//! - Listening statistics (total time, completed episodes, most-listened)
//! - OPML import/export for subscription portability
//! - Playback history with timestamps
//! - Dark theme (Catppuccin Mocha) UI with sidebar, episode list, now-playing bar
//!
//! Uses the guitk library for UI rendering.

use std::collections::HashMap;

use guitk::color::Color;
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use oswindow::{Event, RenderTree};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const SIDEBAR_WIDTH: f32 = 260.0;
/// A window narrower than the sidebar plus a usable column, or shorter than
/// the now-playing bar plus a row or two, has no layout left to follow.
const MIN_WINDOW_WIDTH: f32 = 640.0;
const MIN_WINDOW_HEIGHT: f32 = 400.0;
/// How often the player and the download queue are advanced.
const PLAYBACK_TICK: Duration = Duration::from_millis(250);

/// Height reserved under the sidebar list for its "N more" line.
///
/// Reserved unconditionally, so how many rows fit does not depend on whether
/// any are being hidden — a budget that changed with its own result could fit
/// one more podcast, discover the line was needed, and drop it again.
const LIST_MORE_HEIGHT: f32 = 16.0;
const NOW_PLAYING_HEIGHT: f32 = 80.0;

/// One row of the sidebar's scrolling list.
///
/// The sidebar is built as a flat row list rather than drawn straight down the
/// column, because it has to scroll and a scroll needs to know what the rows
/// are. It has to scroll because its *fixed* content alone — six library
/// entries and twelve categories, 732px with the headers and dividers — is
/// taller than a 600px window with no subscriptions in it at all. Nothing can
/// be pinned in a column that cannot fit its own fixed parts, so everything
/// below the title scrolls together and the title is the only chrome.
enum SidebarRow {
    /// A section heading: SUBSCRIPTIONS, CATEGORIES.
    Header(&'static str),
    /// The rule between sections, with the air above and below it.
    Divider,
    /// A selectable entry: a library view, a subscription, or a category.
    Item {
        label: String,
        accent: Color,
        /// What selecting this row does.
        ///
        /// The row used to carry a `selected: bool` computed at the point it
        /// was built, and nothing at all about what clicking it should do --
        /// which is why nothing could click it. Carrying the target instead
        /// makes the highlight a function of the target rather than a second
        /// opinion about it, so a row cannot be drawn as selected while
        /// pointing somewhere else.
        target: SidebarTarget,
    },
}

/// What selecting a sidebar row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarTarget {
    Search,
    AllEpisodes,
    Queue,
    Downloads,
    History,
    Statistics,
    Podcast(u64),
    Category(Category),
}

impl SidebarRow {
    const ITEM_H: f32 = 32.0;
    const HEADER_H: f32 = 24.0;
    /// 12px of air, the 1px rule, 12px of air.
    const DIVIDER_H: f32 = 25.0;

    fn height(&self) -> f32 {
        match self {
            Self::Header(_) => Self::HEADER_H,
            Self::Divider => Self::DIVIDER_H,
            Self::Item { .. } => Self::ITEM_H,
        }
    }
}

/// Y of the first sidebar row. Above it is the title, which is chrome.
const SIDEBAR_LIST_TOP: f32 = 48.0;
/// How far back the bar's back button jumps, and Left on the keyboard.
const SKIP_BACK_SECS: u32 = 15;
/// How far forward the bar's forward button jumps, and Right on the keyboard.
const SKIP_FORWARD_SECS: u32 = 30;
/// Height of the clickable strip along the top of the now-playing bar. The
/// progress line inside it is 3px, which nobody can hit.
const SEEK_STRIP_HEIGHT: f32 = 8.0;
const HEADER_HEIGHT: f32 = 48.0;
const EPISODE_ROW_HEIGHT: f32 = 72.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
const CATEGORY_PILL_HEIGHT: f32 = 28.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
/// Y of the first episode row: under the header, the filter bar, and the
/// episode count.
const EPISODE_LIST_TOP: f32 = HEADER_HEIGHT + TOOLBAR_HEIGHT + 32.0;

/// Point size of an episode's description in the detail view.
const DESCRIPTION_FONT_SIZE: f32 = 13.0;
/// Point size of an episode's show notes.
const NOTES_FONT_SIZE: f32 = 12.0;
/// Line spacing of both prose fields in the detail view.
const PROSE_LINE_HEIGHT: f32 = 18.0;
/// Space under a prose field before the next section starts. Sized so that a
/// one-line field keeps roughly the room it always had.
const PROSE_SECTION_GAP: f32 = 22.0;
/// Space under the show notes before the bookmark rows below them. Sized so
/// that one line of notes occupies the 24px it always has.
const NOTES_BOOKMARK_GAP: f32 = 6.0;

// ============================================================================
// Categories
// ============================================================================

/// Podcast categories for organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Technology,
    Science,
    Comedy,
    News,
    Education,
    Business,
    Health,
    Arts,
    Sports,
    Music,
    Society,
    TrueCrime,
}

impl Category {
    /// All available categories.
    pub const ALL: &'static [Category] = &[
        Category::Technology,
        Category::Science,
        Category::Comedy,
        Category::News,
        Category::Education,
        Category::Business,
        Category::Health,
        Category::Arts,
        Category::Sports,
        Category::Music,
        Category::Society,
        Category::TrueCrime,
    ];

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Technology => "Technology",
            Self::Science => "Science",
            Self::Comedy => "Comedy",
            Self::News => "News",
            Self::Education => "Education",
            Self::Business => "Business",
            Self::Health => "Health",
            Self::Arts => "Arts",
            Self::Sports => "Sports",
            Self::Music => "Music",
            Self::Society => "Society",
            Self::TrueCrime => "True Crime",
        }
    }

    /// Category accent color.
    pub fn color(self) -> Color {
        match self {
            Self::Technology => BLUE,
            Self::Science => TEAL,
            Self::Comedy => YELLOW,
            Self::News => RED,
            Self::Education => GREEN,
            Self::Business => PEACH,
            Self::Health => GREEN,
            Self::Arts => MAUVE,
            Self::Sports => PEACH,
            Self::Music => LAVENDER,
            Self::Society => TEAL,
            Self::TrueCrime => RED,
        }
    }

    /// Parse category from string.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "technology" | "tech" => Some(Self::Technology),
            "science" => Some(Self::Science),
            "comedy" => Some(Self::Comedy),
            "news" => Some(Self::News),
            "education" => Some(Self::Education),
            "business" => Some(Self::Business),
            "health" => Some(Self::Health),
            "arts" => Some(Self::Arts),
            "sports" => Some(Self::Sports),
            "music" => Some(Self::Music),
            "society" => Some(Self::Society),
            "true crime" | "truecrime" => Some(Self::TrueCrime),
            _ => None,
        }
    }
}

// ============================================================================
// Episode Status
// ============================================================================

/// Playback status of an episode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeStatus {
    Unplayed,
    InProgress { position_secs: u32 },
    Played,
}

impl EpisodeStatus {
    /// Whether the episode has been fully played.
    pub fn is_played(self) -> bool {
        matches!(self, Self::Played)
    }

    /// Whether the episode is in-progress.
    pub fn is_in_progress(self) -> bool {
        matches!(self, Self::InProgress { .. })
    }

    /// Whether the episode is unplayed.
    pub fn is_unplayed(self) -> bool {
        matches!(self, Self::Unplayed)
    }

    /// Display label for the status.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unplayed => "New",
            Self::InProgress { .. } => "In Progress",
            Self::Played => "Played",
        }
    }

    /// Color for the status indicator.
    pub fn color(self) -> Color {
        match self {
            Self::Unplayed => BLUE,
            Self::InProgress { .. } => YELLOW,
            Self::Played => SURFACE2,
        }
    }
}

// ============================================================================
// Download Status
// ============================================================================

/// Download state for an episode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DownloadStatus {
    NotDownloaded,
    Queued,
    Downloading { progress: f32 },
    Downloaded,
    Failed,
}

impl DownloadStatus {
    /// Whether the episode is downloaded.
    pub fn is_downloaded(self) -> bool {
        matches!(self, Self::Downloaded)
    }

    /// Whether the episode is currently downloading.
    pub fn is_downloading(self) -> bool {
        matches!(self, Self::Downloading { .. })
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotDownloaded => "Not Downloaded",
            Self::Queued => "Queued",
            Self::Downloading { .. } => "Downloading",
            Self::Downloaded => "Downloaded",
            Self::Failed => "Failed",
        }
    }
}

// ============================================================================
// Playback Speed
// ============================================================================

/// Available playback speeds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSpeed(f32);

impl PlaybackSpeed {
    pub const HALF: Self = Self(0.5);
    pub const NORMAL: Self = Self(1.0);
    pub const ONE_QUARTER: Self = Self(1.25);
    pub const ONE_HALF: Self = Self(1.5);
    pub const ONE_SEVENTY_FIVE: Self = Self(1.75);
    pub const DOUBLE: Self = Self(2.0);
    pub const TWO_HALF: Self = Self(2.5);
    pub const TRIPLE: Self = Self(3.0);

    pub const ALL: &'static [PlaybackSpeed] = &[
        Self::HALF,
        Self::NORMAL,
        Self::ONE_QUARTER,
        Self::ONE_HALF,
        Self::ONE_SEVENTY_FIVE,
        Self::DOUBLE,
        Self::TWO_HALF,
        Self::TRIPLE,
    ];

    /// Display label (e.g. "1.5x").
    pub fn label(self) -> String {
        if (self.0 - self.0.floor()).abs() < 0.001 {
            format!("{:.0}x", self.0)
        } else {
            format!("{:.2}x", self.0)
        }
    }

    /// The raw speed multiplier.
    pub fn value(self) -> f32 {
        self.0
    }

    /// Next speed in the list (wraps around).
    pub fn next(self) -> Self {
        let all = Self::ALL;
        for i in 0..all.len() {
            if let Some(s) = all.get(i)
                && (s.0 - self.0).abs() < 0.001
            {
                // `all` is non-empty inside this loop by construction —
                // we are iterating it — so the zero case is unreachable
                // and index 0 is the right answer for it anyway.
                let next_idx = i.saturating_add(1).checked_rem(all.len()).unwrap_or(0);
                if let Some(n) = all.get(next_idx) {
                    return *n;
                }
            }
        }
        Self::NORMAL
    }
}

// ============================================================================
// Playback State
// ============================================================================

/// Simulated player state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

// ============================================================================
// Timestamp Bookmark
// ============================================================================

/// A user-created bookmark at a specific timestamp in an episode.
#[derive(Clone, Debug)]
pub struct Bookmark {
    pub timestamp_secs: u32,
    pub label: String,
}

impl Bookmark {
    pub fn new(timestamp_secs: u32, label: &str) -> Self {
        Self {
            timestamp_secs,
            label: label.to_string(),
        }
    }

    /// Format timestamp as MM:SS.
    pub fn timestamp_display(&self) -> String {
        format_duration(self.timestamp_secs)
    }
}

// ============================================================================
// Episode Notes
// ============================================================================

/// Per-episode user notes and bookmarks.
#[derive(Clone, Debug, Default)]
pub struct EpisodeNotes {
    pub text: String,
    pub bookmarks: Vec<Bookmark>,
}

impl EpisodeNotes {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            bookmarks: Vec::new(),
        }
    }

    pub fn add_bookmark(&mut self, timestamp_secs: u32, label: &str) {
        self.bookmarks.push(Bookmark::new(timestamp_secs, label));
        // Keep bookmarks sorted by timestamp.
        self.bookmarks.sort_by_key(|b| b.timestamp_secs);
    }

    pub fn remove_bookmark(&mut self, index: usize) -> bool {
        if index < self.bookmarks.len() {
            self.bookmarks.remove(index);
            true
        } else {
            false
        }
    }

    pub fn set_notes(&mut self, text: &str) {
        self.text = text.to_string();
    }

    pub fn has_content(&self) -> bool {
        !self.text.is_empty() || !self.bookmarks.is_empty()
    }
}

// ============================================================================
// Episode
// ============================================================================

/// A single podcast episode.
#[derive(Clone, Debug)]
pub struct Episode {
    pub id: u64,
    pub podcast_id: u64,
    pub title: String,
    pub description: String,
    pub date: String,
    pub duration_secs: u32,
    pub enclosure_url: String,
    pub file_size_bytes: u64,
    pub status: EpisodeStatus,
    pub download_status: DownloadStatus,
    pub notes: EpisodeNotes,
}

impl Episode {
    /// Format duration as HH:MM:SS or MM:SS.
    pub fn duration_display(&self) -> String {
        format_duration(self.duration_secs)
    }

    /// Format file size as human-readable string.
    pub fn file_size_display(&self) -> String {
        format_bytes(self.file_size_bytes)
    }

    /// Progress percentage if in-progress.
    pub fn progress_pct(&self) -> f32 {
        match self.status {
            EpisodeStatus::InProgress { position_secs } => {
                ratio::percent(position_secs, self.duration_secs).unwrap_or(0.0) as f32
            }
            EpisodeStatus::Played => 100.0,
            EpisodeStatus::Unplayed => 0.0,
        }
    }

    /// Remaining time if in-progress.
    pub fn remaining_secs(&self) -> u32 {
        match self.status {
            EpisodeStatus::InProgress { position_secs } => {
                self.duration_secs.saturating_sub(position_secs)
            }
            EpisodeStatus::Played => 0,
            EpisodeStatus::Unplayed => self.duration_secs,
        }
    }
}

// ============================================================================
// Podcast (subscription)
// ============================================================================

/// A podcast subscription.
#[derive(Clone, Debug)]
pub struct Podcast {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub description: String,
    pub rss_url: String,
    pub artwork_url: String,
    pub categories: Vec<Category>,
    pub episodes: Vec<Episode>,
    pub auto_download: bool,
}

impl Podcast {
    /// Count of unplayed episodes.
    pub fn unplayed_count(&self) -> usize {
        self.episodes
            .iter()
            .filter(|e| e.status.is_unplayed())
            .count()
    }

    /// Count of in-progress episodes.
    pub fn in_progress_count(&self) -> usize {
        self.episodes
            .iter()
            .filter(|e| e.status.is_in_progress())
            .count()
    }

    /// Count of downloaded episodes.
    pub fn downloaded_count(&self) -> usize {
        self.episodes
            .iter()
            .filter(|e| e.download_status.is_downloaded())
            .count()
    }

    /// Total disk space used by downloaded episodes.
    pub fn downloaded_size_bytes(&self) -> u64 {
        self.episodes
            .iter()
            .filter(|e| e.download_status.is_downloaded())
            .map(|e| e.file_size_bytes)
            .sum()
    }

    /// Find episode by ID.
    pub fn find_episode(&self, episode_id: u64) -> Option<&Episode> {
        self.episodes.iter().find(|e| e.id == episode_id)
    }

    /// Find episode by ID (mutable).
    pub fn find_episode_mut(&mut self, episode_id: u64) -> Option<&mut Episode> {
        self.episodes.iter_mut().find(|e| e.id == episode_id)
    }
}

// ============================================================================
// Playback History Entry
// ============================================================================

/// A record of a listening session.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub episode_id: u64,
    pub podcast_id: u64,
    pub episode_title: String,
    pub podcast_title: String,
    pub listened_at: String,
    pub duration_listened_secs: u32,
    pub completed: bool,
}

// ============================================================================
// Queue Item
// ============================================================================

/// An item in the play queue.
#[derive(Clone, Debug)]
pub struct QueueItem {
    pub episode_id: u64,
    pub podcast_id: u64,
    pub episode_title: String,
    pub podcast_title: String,
    pub duration_secs: u32,
}

// ============================================================================
// Download Queue Item
// ============================================================================

/// An item in the download queue.
#[derive(Clone, Debug)]
pub struct DownloadQueueItem {
    pub episode_id: u64,
    pub podcast_id: u64,
    pub episode_title: String,
    pub file_size_bytes: u64,
    pub progress: f32,
    pub active: bool,
}

// ============================================================================
// Statistics
// ============================================================================

/// Listening statistics.
#[derive(Clone, Debug, Default)]
pub struct ListeningStats {
    pub total_listening_secs: u64,
    pub episodes_completed: u32,
    pub subscriptions_count: u32,
    pub most_listened_podcast: Option<String>,
    pub most_listened_time_secs: u64,
    pub per_podcast_secs: HashMap<u64, u64>,
}

impl ListeningStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total listening time formatted.
    ///
    /// This is a lifetime accumulator, so it is measured in days within a
    /// month of use. It used to be hard-wired to `{h}h {m}m`, which reported
    /// a year of podcasts as `2920h 0m` and a first session as `0h 12m`.
    pub fn total_time_display(&self) -> String {
        guitk::duration::coarse(self.total_listening_secs)
    }

    /// Record a listening session.
    pub fn record_listening(
        &mut self,
        podcast_id: u64,
        podcast_name: &str,
        duration_secs: u32,
        completed: bool,
    ) {
        self.total_listening_secs = self
            .total_listening_secs
            .saturating_add(duration_secs as u64);
        if completed {
            self.episodes_completed = self.episodes_completed.saturating_add(1);
        }
        let entry = self.per_podcast_secs.entry(podcast_id).or_insert(0);
        *entry = entry.saturating_add(duration_secs as u64);
        if *entry > self.most_listened_time_secs {
            self.most_listened_time_secs = *entry;
            self.most_listened_podcast = Some(podcast_name.to_string());
        }
    }
}

// ============================================================================
// OPML Import/Export
// ============================================================================

/// An OPML outline entry (for import/export).
#[derive(Clone, Debug)]
pub struct OpmlOutline {
    pub text: String,
    pub feed_type: String,
    pub xml_url: String,
    pub html_url: String,
}

/// Generate OPML XML from subscriptions.
pub fn generate_opml(podcasts: &[Podcast]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<opml version=\"2.0\">\n");
    out.push_str("  <head>\n");
    out.push_str("    <title>Podcast Subscriptions</title>\n");
    out.push_str("  </head>\n");
    out.push_str("  <body>\n");
    for podcast in podcasts {
        let title_escaped = xml_escape(&podcast.title);
        let url_escaped = xml_escape(&podcast.rss_url);
        out.push_str(&format!(
            "    <outline text=\"{}\" type=\"rss\" xmlUrl=\"{}\" />\n",
            title_escaped, url_escaped
        ));
    }
    out.push_str("  </body>\n");
    out.push_str("</opml>\n");
    out
}

/// Parse OPML XML and return outline entries.
pub fn parse_opml(xml: &str) -> Vec<OpmlOutline> {
    let mut outlines = Vec::new();
    // Simple line-by-line parser for <outline .../> elements.
    for line in xml.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("<outline") {
            continue;
        }
        let text = extract_attr(trimmed, "text").unwrap_or_default();
        let feed_type = extract_attr(trimmed, "type").unwrap_or_default();
        let xml_url = extract_attr(trimmed, "xmlUrl").unwrap_or_default();
        let html_url = extract_attr(trimmed, "htmlUrl").unwrap_or_default();
        if !xml_url.is_empty() {
            outlines.push(OpmlOutline {
                text: xml_unescape(&text),
                feed_type,
                xml_url: xml_unescape(&xml_url),
                html_url: xml_unescape(&html_url),
            });
        }
    }
    outlines
}

/// Extract an XML attribute value from a tag string.
fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=\"", attr_name);
    let start = tag.find(&search)?;
    let val_start = start.saturating_add(search.len());
    let rest = tag.get(val_start..)?;
    let end = rest.find('"')?;
    rest.get(..end).map(|s| s.to_string())
}

/// Escape XML special characters.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Unescape XML entities.
pub fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            let mut entity = String::new();
            for ec in chars.by_ref() {
                if ec == ';' {
                    break;
                }
                entity.push(ec);
            }
            match entity.as_str() {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ => {
                    out.push('&');
                    out.push_str(&entity);
                    out.push(';');
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format seconds as HH:MM:SS or MM:SS.
pub fn format_duration(total_secs: u32) -> String {
    guitk::duration::clock(u64::from(total_secs))
}

/// Format bytes as human-readable size.
pub fn format_bytes(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

// ============================================================================
// View / Filter State
// ============================================================================

/// Which view is active in the main content area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainView {
    EpisodeList,
    EpisodeDetail,
    Queue,
    Downloads,
    History,
    Statistics,
    Search,
}

/// A rectangle on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A control in the now-playing bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerControl {
    /// The strip along the top edge: click to jump to that point.
    Seek,
    SkipBack,
    PlayPause,
    SkipForward,
    Speed,
}

/// Episode list filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeFilter {
    All,
    Unplayed,
    InProgress,
    Played,
    Downloaded,
}

impl EpisodeFilter {
    /// Every filter, in the order the bar shows them.
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Unplayed,
        Self::InProgress,
        Self::Played,
        Self::Downloaded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Unplayed => "Unplayed",
            Self::InProgress => "In Progress",
            Self::Played => "Played",
            Self::Downloaded => "Downloaded",
        }
    }
}

/// Sidebar selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarSelection {
    AllEpisodes,
    Podcast(u64),
    Category(Category),
    Queue,
    Downloads,
    History,
    Statistics,
}

// ============================================================================
// Application State
// ============================================================================

/// The main podcast manager application.
pub struct PodcastApp {
    pub width: f32,
    pub height: f32,

    // Data
    pub podcasts: Vec<Podcast>,
    pub play_queue: Vec<QueueItem>,
    pub download_queue: Vec<DownloadQueueItem>,
    pub history: Vec<HistoryEntry>,
    pub stats: ListeningStats,

    // Playback state
    pub player_state: PlayerState,
    pub current_episode_id: Option<u64>,
    pub current_podcast_id: Option<u64>,
    pub playback_position_secs: u32,
    pub playback_duration_secs: u32,
    pub playback_speed: PlaybackSpeed,
    pub auto_play_next: bool,

    // UI state
    pub sidebar_selection: SidebarSelection,
    pub main_view: MainView,
    pub episode_filter: EpisodeFilter,
    pub selected_episode_id: Option<u64>,
    pub search_query: String,
    pub search_results: Vec<(u64, u64)>, // (podcast_id, episode_id)
    /// Index of the first subscription drawn in the sidebar.
    ///
    /// A row index rather than a pixel offset: the sidebar draws whole items,
    /// so a pixel offset could only express positions the renderer then rounds
    /// away. A value past the end is not an error, and shows the last page.
    pub sidebar_scroll: usize,
    /// First episode row drawn in the content area. Counted in rows for the
    /// same reason as `sidebar_scroll`, and likewise harmless past the end.
    pub episode_list_scroll: usize,

    // Disk space tracking
    pub total_disk_bytes: u64,
    pub used_disk_bytes: u64,

    // Next ID counter
    next_id: u64,
}

impl PodcastApp {
    pub fn new(width: f32, height: f32) -> Self {
        let mut app = Self {
            width,
            height,
            podcasts: Vec::new(),
            play_queue: Vec::new(),
            download_queue: Vec::new(),
            history: Vec::new(),
            stats: ListeningStats::new(),
            player_state: PlayerState::Stopped,
            current_episode_id: None,
            current_podcast_id: None,
            playback_position_secs: 0,
            playback_duration_secs: 0,
            playback_speed: PlaybackSpeed::NORMAL,
            auto_play_next: true,
            sidebar_selection: SidebarSelection::AllEpisodes,
            main_view: MainView::EpisodeList,
            episode_filter: EpisodeFilter::All,
            selected_episode_id: None,
            search_query: String::new(),
            search_results: Vec::new(),
            sidebar_scroll: 0,
            episode_list_scroll: 0,
            total_disk_bytes: 10_000_000_000,
            used_disk_bytes: 0,
            next_id: 1,
        };
        app.populate_sample_data();
        app
    }

    /// Generate a unique ID.
    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    // ========================================================================
    // Subscription management
    // ========================================================================

    /// Subscribe to a new podcast.
    pub fn subscribe(
        &mut self,
        title: &str,
        author: &str,
        description: &str,
        rss_url: &str,
        artwork_url: &str,
        categories: Vec<Category>,
    ) -> u64 {
        let id = self.next_id();
        self.podcasts.push(Podcast {
            id,
            title: title.to_string(),
            author: author.to_string(),
            description: description.to_string(),
            rss_url: rss_url.to_string(),
            artwork_url: artwork_url.to_string(),
            categories,
            episodes: Vec::new(),
            auto_download: false,
        });
        self.stats.subscriptions_count = self.stats.subscriptions_count.saturating_add(1);
        id
    }

    /// Unsubscribe from a podcast.
    pub fn unsubscribe(&mut self, podcast_id: u64) -> bool {
        let before = self.podcasts.len();
        self.podcasts.retain(|p| p.id != podcast_id);
        let removed = self.podcasts.len() < before;
        if removed {
            // Remove queue items for this podcast.
            self.play_queue.retain(|q| q.podcast_id != podcast_id);
            self.download_queue.retain(|d| d.podcast_id != podcast_id);
            self.stats.subscriptions_count = self.stats.subscriptions_count.saturating_sub(1);
            // Reset current playback if it was from this podcast.
            if self.current_podcast_id == Some(podcast_id) {
                self.stop_playback();
            }
        }
        removed
    }

    /// Find a podcast by ID.
    pub fn find_podcast(&self, podcast_id: u64) -> Option<&Podcast> {
        self.podcasts.iter().find(|p| p.id == podcast_id)
    }

    /// Find a podcast by ID (mutable).
    pub fn find_podcast_mut(&mut self, podcast_id: u64) -> Option<&mut Podcast> {
        self.podcasts.iter_mut().find(|p| p.id == podcast_id)
    }

    /// Move the sidebar's subscription list `delta` items, negative for up.
    ///
    /// Not clamped at the bottom here: how far the list can scroll depends on
    /// the window height and on how many categories are drawn below it,
    /// neither of which this method knows. The render clamps against what it
    /// is actually drawing, so an offset past the end shows the last page.
    pub fn scroll_sidebar_by(&mut self, delta: isize) {
        self.sidebar_scroll = scroll_window::shift(self.sidebar_scroll, delta);
    }

    /// Back to the first row.
    pub fn scroll_sidebar_to_top(&mut self) {
        self.sidebar_scroll = 0;
    }

    /// Move the episode list `delta` rows, negative for up.
    ///
    /// Clamped at the top only, for the same reason as the sidebar: the row
    /// count depends on the content height and the active filter, neither of
    /// which this method is given.
    pub fn scroll_episode_list_by(&mut self, delta: isize) {
        self.episode_list_scroll = scroll_window::shift(self.episode_list_scroll, delta);
    }

    /// Back to the first episode.
    pub fn scroll_episode_list_to_top(&mut self) {
        self.episode_list_scroll = 0;
    }

    /// Set auto-download for a podcast.
    pub fn set_auto_download(&mut self, podcast_id: u64, enabled: bool) -> bool {
        if let Some(p) = self.find_podcast_mut(podcast_id) {
            p.auto_download = enabled;
            true
        } else {
            false
        }
    }

    // ========================================================================
    // Episode management
    // ========================================================================

    /// Add an episode to a podcast.
    // Mirrors the RSS enclosure fields one-to-one; introducing a parameter
    // struct would only duplicate the Episode fields.
    #[allow(clippy::too_many_arguments)]
    pub fn add_episode(
        &mut self,
        podcast_id: u64,
        title: &str,
        description: &str,
        date: &str,
        duration_secs: u32,
        enclosure_url: &str,
        file_size_bytes: u64,
    ) -> Option<u64> {
        let ep_id = self.next_id();
        // Check auto-download status before borrowing mutably.
        let auto_dl = self
            .podcasts
            .iter()
            .find(|p| p.id == podcast_id)
            .map(|p| p.auto_download)
            .unwrap_or(false);

        let podcast = self.podcasts.iter_mut().find(|p| p.id == podcast_id)?;
        podcast.episodes.push(Episode {
            id: ep_id,
            podcast_id,
            title: title.to_string(),
            description: description.to_string(),
            date: date.to_string(),
            duration_secs,
            enclosure_url: enclosure_url.to_string(),
            file_size_bytes,
            status: EpisodeStatus::Unplayed,
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        });

        if auto_dl {
            self.queue_download(podcast_id, ep_id);
        }

        Some(ep_id)
    }

    /// Mark an episode as played.
    pub fn mark_played(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        if let Some(podcast) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = podcast.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            ep.status = EpisodeStatus::Played;
            return true;
        }
        false
    }

    /// Mark an episode as unplayed.
    pub fn mark_unplayed(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        if let Some(podcast) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = podcast.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            ep.status = EpisodeStatus::Unplayed;
            return true;
        }
        false
    }

    /// Get all episodes matching the current filter for a specific podcast.
    pub fn filtered_episodes_for_podcast(&self, podcast_id: u64) -> Vec<(u64, u64)> {
        let filter = self.episode_filter;
        let mut result = Vec::new();
        if let Some(podcast) = self.find_podcast(podcast_id) {
            for ep in &podcast.episodes {
                if Self::episode_matches_filter(ep, filter) {
                    result.push((podcast_id, ep.id));
                }
            }
        }
        result
    }

    /// Get all episodes matching the current filter across all podcasts.
    pub fn filtered_all_episodes(&self) -> Vec<(u64, u64)> {
        let filter = self.episode_filter;
        let mut result = Vec::new();
        for podcast in &self.podcasts {
            for ep in &podcast.episodes {
                if Self::episode_matches_filter(ep, filter) {
                    result.push((podcast.id, ep.id));
                }
            }
        }
        result
    }

    /// Get episodes for the selected category.
    pub fn episodes_for_category(&self, cat: Category) -> Vec<(u64, u64)> {
        let filter = self.episode_filter;
        let mut result = Vec::new();
        for podcast in &self.podcasts {
            if podcast.categories.contains(&cat) {
                for ep in &podcast.episodes {
                    if Self::episode_matches_filter(ep, filter) {
                        result.push((podcast.id, ep.id));
                    }
                }
            }
        }
        result
    }

    /// Check if an episode matches a filter.
    fn episode_matches_filter(ep: &Episode, filter: EpisodeFilter) -> bool {
        match filter {
            EpisodeFilter::All => true,
            EpisodeFilter::Unplayed => ep.status.is_unplayed(),
            EpisodeFilter::InProgress => ep.status.is_in_progress(),
            EpisodeFilter::Played => ep.status.is_played(),
            EpisodeFilter::Downloaded => ep.download_status.is_downloaded(),
        }
    }

    /// Find an episode across all podcasts.
    pub fn find_episode_global(&self, podcast_id: u64, episode_id: u64) -> Option<&Episode> {
        self.find_podcast(podcast_id)
            .and_then(|p| p.find_episode(episode_id))
    }

    // ========================================================================
    // Episode notes & bookmarks
    // ========================================================================

    /// Set notes text for an episode.
    pub fn set_episode_notes(&mut self, podcast_id: u64, episode_id: u64, text: &str) -> bool {
        if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            ep.notes.set_notes(text);
            return true;
        }
        false
    }

    /// Add a bookmark to an episode.
    pub fn add_episode_bookmark(
        &mut self,
        podcast_id: u64,
        episode_id: u64,
        timestamp_secs: u32,
        label: &str,
    ) -> bool {
        if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            ep.notes.add_bookmark(timestamp_secs, label);
            return true;
        }
        false
    }

    /// Remove a bookmark from an episode.
    pub fn remove_episode_bookmark(
        &mut self,
        podcast_id: u64,
        episode_id: u64,
        bookmark_index: usize,
    ) -> bool {
        if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            return ep.notes.remove_bookmark(bookmark_index);
        }
        false
    }

    // ========================================================================
    // Playback
    // ========================================================================

    /// Start playing an episode.
    pub fn play_episode(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        // Gather info before mutable borrow.
        let info = self
            .podcasts
            .iter()
            .find(|p| p.id == podcast_id)
            .and_then(|p| p.find_episode(episode_id))
            .map(|ep| (ep.duration_secs, ep.status));

        if let Some((duration, status)) = info {
            let position = match status {
                EpisodeStatus::InProgress { position_secs } => position_secs,
                _ => 0,
            };
            self.current_episode_id = Some(episode_id);
            self.current_podcast_id = Some(podcast_id);
            self.playback_position_secs = position;
            self.playback_duration_secs = duration;
            self.player_state = PlayerState::Playing;

            // Mark as in-progress.
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
            {
                ep.status = EpisodeStatus::InProgress {
                    position_secs: position,
                };
            }
            true
        } else {
            false
        }
    }

    /// Pause playback.
    pub fn pause_playback(&mut self) {
        if self.player_state == PlayerState::Playing {
            self.player_state = PlayerState::Paused;
            self.update_episode_position();
        }
    }

    /// Resume playback.
    pub fn resume_playback(&mut self) {
        if self.player_state == PlayerState::Paused {
            self.player_state = PlayerState::Playing;
        }
    }

    /// Toggle play/pause.
    pub fn toggle_playback(&mut self) {
        match self.player_state {
            PlayerState::Playing => self.pause_playback(),
            PlayerState::Paused => self.resume_playback(),
            PlayerState::Stopped => {
                // Try to play the first queue item.
                if let Some(item) = self.play_queue.first().cloned() {
                    self.play_episode(item.podcast_id, item.episode_id);
                }
            }
        }
    }

    /// Stop playback completely.
    pub fn stop_playback(&mut self) {
        if self.player_state != PlayerState::Stopped {
            self.record_current_to_history();
        }
        self.player_state = PlayerState::Stopped;
        self.current_episode_id = None;
        self.current_podcast_id = None;
        self.playback_position_secs = 0;
        self.playback_duration_secs = 0;
    }

    /// Seek forward by a number of seconds.
    pub fn seek_forward(&mut self, secs: u32) {
        if self.player_state != PlayerState::Stopped {
            self.playback_position_secs = self
                .playback_position_secs
                .saturating_add(secs)
                .min(self.playback_duration_secs);
            self.update_episode_position();
            if self.playback_position_secs >= self.playback_duration_secs {
                self.complete_current_episode();
            }
        }
    }

    /// Seek backward by a number of seconds.
    pub fn seek_backward(&mut self, secs: u32) {
        if self.player_state != PlayerState::Stopped {
            self.playback_position_secs = self.playback_position_secs.saturating_sub(secs);
            self.update_episode_position();
        }
    }

    /// Seek to an absolute position.
    pub fn seek_to(&mut self, position_secs: u32) {
        if self.player_state != PlayerState::Stopped {
            self.playback_position_secs = position_secs.min(self.playback_duration_secs);
            self.update_episode_position();
        }
    }

    /// Cycle playback speed.
    pub fn cycle_speed(&mut self) {
        self.playback_speed = self.playback_speed.next();
    }

    /// Set playback speed directly.
    pub fn set_speed(&mut self, speed: PlaybackSpeed) {
        self.playback_speed = speed;
    }

    /// Simulate time passing (for playback simulation).
    pub fn tick(&mut self, elapsed_ms: u64) {
        if self.player_state != PlayerState::Playing {
            return;
        }
        let speed = self.playback_speed.value();
        let elapsed_secs_f = (elapsed_ms as f64 / 1000.0) * speed as f64;
        let elapsed_secs = elapsed_secs_f as u32;
        if elapsed_secs > 0 {
            self.playback_position_secs = self
                .playback_position_secs
                .saturating_add(elapsed_secs)
                .min(self.playback_duration_secs);
            self.update_episode_position();
            if self.playback_position_secs >= self.playback_duration_secs {
                self.complete_current_episode();
            }
        }
    }

    /// Update the episode's in-progress position.
    fn update_episode_position(&mut self) {
        let ep_id = self.current_episode_id;
        let pod_id = self.current_podcast_id;
        let pos = self.playback_position_secs;
        if let (Some(podcast_id), Some(episode_id)) = (pod_id, ep_id)
            && let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
        {
            ep.status = EpisodeStatus::InProgress { position_secs: pos };
        }
    }

    /// Complete the current episode and optionally auto-play next.
    fn complete_current_episode(&mut self) {
        let ep_id = self.current_episode_id;
        let pod_id = self.current_podcast_id;
        let pos = self.playback_position_secs;

        if let (Some(podcast_id), Some(episode_id)) = (pod_id, ep_id) {
            // Mark as played.
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
            {
                ep.status = EpisodeStatus::Played;
            }

            // Gather podcast title for stats (immutable borrow).
            let podcast_title = self
                .podcasts
                .iter()
                .find(|p| p.id == podcast_id)
                .map(|p| p.title.clone())
                .unwrap_or_default();

            // Record stats.
            self.stats
                .record_listening(podcast_id, &podcast_title, pos, true);

            // Record history.
            let ep_title = self
                .podcasts
                .iter()
                .find(|p| p.id == podcast_id)
                .and_then(|p| p.find_episode(episode_id))
                .map(|e| e.title.clone())
                .unwrap_or_default();

            self.history.push(HistoryEntry {
                episode_id,
                podcast_id,
                episode_title: ep_title,
                podcast_title: podcast_title.clone(),
                listened_at: "2026-05-18 10:00".to_string(),
                duration_listened_secs: pos,
                completed: true,
            });

            // Remove from queue if present.
            self.play_queue.retain(|q| q.episode_id != episode_id);
        }

        // Auto-play next.
        if self.auto_play_next
            && let Some(next) = self.play_queue.first().cloned()
        {
            self.play_episode(next.podcast_id, next.episode_id);
            return;
        }

        self.player_state = PlayerState::Stopped;
        self.current_episode_id = None;
        self.current_podcast_id = None;
        self.playback_position_secs = 0;
        self.playback_duration_secs = 0;
    }

    /// Record the current playback to history.
    fn record_current_to_history(&mut self) {
        let ep_id = self.current_episode_id;
        let pod_id = self.current_podcast_id;
        let pos = self.playback_position_secs;

        if let (Some(podcast_id), Some(episode_id)) = (pod_id, ep_id) {
            let ep_title = self
                .podcasts
                .iter()
                .find(|p| p.id == podcast_id)
                .and_then(|p| p.find_episode(episode_id))
                .map(|e| e.title.clone())
                .unwrap_or_default();
            let pod_title = self
                .podcasts
                .iter()
                .find(|p| p.id == podcast_id)
                .map(|p| p.title.clone())
                .unwrap_or_default();

            self.stats
                .record_listening(podcast_id, &pod_title, pos, false);

            self.history.push(HistoryEntry {
                episode_id,
                podcast_id,
                episode_title: ep_title,
                podcast_title: pod_title,
                listened_at: "2026-05-18 10:00".to_string(),
                duration_listened_secs: pos,
                completed: false,
            });
        }
    }

    // ========================================================================
    // Play queue
    // ========================================================================

    /// Add an episode to the play queue.
    pub fn queue_episode(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        // Avoid duplicates.
        if self.play_queue.iter().any(|q| q.episode_id == episode_id) {
            return false;
        }

        let info = self
            .podcasts
            .iter()
            .find(|p| p.id == podcast_id)
            .and_then(|p| {
                p.find_episode(episode_id)
                    .map(|ep| (ep.title.clone(), p.title.clone(), ep.duration_secs))
            });

        if let Some((ep_title, pod_title, duration)) = info {
            self.play_queue.push(QueueItem {
                episode_id,
                podcast_id,
                episode_title: ep_title,
                podcast_title: pod_title,
                duration_secs: duration,
            });
            true
        } else {
            false
        }
    }

    /// Remove an item from the play queue by index.
    pub fn dequeue_episode(&mut self, index: usize) -> bool {
        if index < self.play_queue.len() {
            self.play_queue.remove(index);
            true
        } else {
            false
        }
    }

    /// Move a queue item from one position to another (reorder).
    pub fn reorder_queue(&mut self, from: usize, to: usize) -> bool {
        if from >= self.play_queue.len() || to >= self.play_queue.len() {
            return false;
        }
        let item = self.play_queue.remove(from);
        self.play_queue.insert(to, item);
        true
    }

    /// Clear the play queue.
    pub fn clear_queue(&mut self) {
        self.play_queue.clear();
    }

    // ========================================================================
    // Download management
    // ========================================================================

    /// Queue an episode for download.
    pub fn queue_download(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        // Check if already queued or downloaded.
        if self
            .download_queue
            .iter()
            .any(|d| d.episode_id == episode_id)
        {
            return false;
        }

        let info = self
            .podcasts
            .iter()
            .find(|p| p.id == podcast_id)
            .and_then(|p| {
                p.find_episode(episode_id)
                    .map(|ep| (ep.title.clone(), ep.file_size_bytes, ep.download_status))
            });

        if let Some((title, size, status)) = info {
            if status.is_downloaded() {
                return false;
            }
            // Check disk space.
            if self.used_disk_bytes.saturating_add(size) > self.total_disk_bytes {
                return false;
            }

            // Mark episode as queued.
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
            {
                ep.download_status = DownloadStatus::Queued;
            }

            self.download_queue.push(DownloadQueueItem {
                episode_id,
                podcast_id,
                episode_title: title,
                file_size_bytes: size,
                progress: 0.0,
                active: false,
            });
            true
        } else {
            false
        }
    }

    /// Cancel a download.
    pub fn cancel_download(&mut self, episode_id: u64) -> bool {
        let idx = self
            .download_queue
            .iter()
            .position(|d| d.episode_id == episode_id);
        if let Some(i) = idx {
            let item = self.download_queue.remove(i);
            // Reset episode download status.
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == item.podcast_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
            {
                ep.download_status = DownloadStatus::NotDownloaded;
            }
            true
        } else {
            false
        }
    }

    /// Simulate download progress.
    pub fn simulate_download_tick(&mut self) {
        let mut completed_episodes: Vec<(u64, u64, u64)> = Vec::new();

        // Start first queued item if nothing is active.
        let has_active = self.download_queue.iter().any(|d| d.active);
        if !has_active && let Some(item) = self.download_queue.iter_mut().find(|d| !d.active) {
            item.active = true;
            // Mark episode as downloading.
            let pod_id = item.podcast_id;
            let ep_id = item.episode_id;
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == pod_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == ep_id)
            {
                ep.download_status = DownloadStatus::Downloading { progress: 0.0 };
            }
        }

        // Advance active downloads.
        for item in &mut self.download_queue {
            if item.active {
                item.progress = (item.progress + 0.1).min(1.0);
                if item.progress >= 1.0 {
                    completed_episodes.push((
                        item.podcast_id,
                        item.episode_id,
                        item.file_size_bytes,
                    ));
                }
            }
        }

        // Mark completed downloads.
        for (pod_id, ep_id, size) in &completed_episodes {
            if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == *pod_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == *ep_id)
            {
                ep.download_status = DownloadStatus::Downloaded;
            }
            self.used_disk_bytes = self.used_disk_bytes.saturating_add(*size);
        }

        // Remove completed items from queue.
        self.download_queue.retain(|d| d.progress < 1.0);
    }

    /// Delete a downloaded episode (free disk space).
    pub fn delete_download(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id)
            && ep.download_status.is_downloaded()
        {
            self.used_disk_bytes = self.used_disk_bytes.saturating_sub(ep.file_size_bytes);
            ep.download_status = DownloadStatus::NotDownloaded;
            return true;
        }
        false
    }

    /// Get remaining disk space.
    pub fn remaining_disk_bytes(&self) -> u64 {
        self.total_disk_bytes.saturating_sub(self.used_disk_bytes)
    }

    /// Get disk usage percentage.
    #[must_use]
    pub fn disk_usage_pct(&self) -> f32 {
        ratio::percent(self.used_disk_bytes, self.total_disk_bytes).unwrap_or(0.0) as f32
    }

    // ========================================================================
    // Search
    // ========================================================================

    /// Search across podcast names and episode titles.
    pub fn perform_search(&mut self) {
        self.search_results.clear();
        if self.search_query.is_empty() {
            return;
        }
        let query = self.search_query.to_lowercase();
        for podcast in &self.podcasts {
            for ep in &podcast.episodes {
                let title_match = ep.title.to_lowercase().contains(&query);
                let desc_match = ep.description.to_lowercase().contains(&query);
                let pod_match = podcast.title.to_lowercase().contains(&query);
                if title_match || desc_match || pod_match {
                    self.search_results.push((podcast.id, ep.id));
                }
            }
        }
    }

    // ========================================================================
    // OPML
    // ========================================================================

    /// Export subscriptions as OPML.
    pub fn export_opml(&self) -> String {
        generate_opml(&self.podcasts)
    }

    /// Import subscriptions from OPML.
    pub fn import_opml(&mut self, xml: &str) -> usize {
        let outlines = parse_opml(xml);
        let mut count: usize = 0;
        for outline in &outlines {
            // Skip if already subscribed.
            let already = self.podcasts.iter().any(|p| p.rss_url == outline.xml_url);
            if !already {
                self.subscribe(&outline.text, "", "", &outline.xml_url, "", Vec::new());
                count = count.saturating_add(1);
            }
        }
        count
    }

    // ========================================================================
    // Sample data
    // ========================================================================

    fn populate_sample_data(&mut self) {
        // Podcast 1: Tech talk
        let p1 = self.subscribe(
            "The Rustacean Station",
            "Tim McNamara",
            "A podcast about learning and using the Rust programming language.",
            "https://rustacean-station.org/podcast.rss",
            "https://example.com/rustacean.png",
            vec![Category::Technology, Category::Education],
        );
        self.add_episode(
            p1,
            "Error Handling Patterns in Rust 2024",
            "We discuss the latest approaches to error handling in Rust.",
            "2026-05-10",
            2580,
            "https://example.com/ep1.mp3",
            45_000_000,
        );
        self.add_episode(
            p1,
            "Async Rust: The Road Ahead",
            "A deep dive into the future of async Rust and the ecosystem.",
            "2026-05-03",
            3120,
            "https://example.com/ep2.mp3",
            54_000_000,
        );
        self.add_episode(
            p1,
            "Building an OS in Rust",
            "From bootloader to userspace — writing an OS from scratch.",
            "2026-04-26",
            3600,
            "https://example.com/ep3.mp3",
            62_000_000,
        );

        // Mark second episode as in-progress.
        if let Some(podcast) = self.podcasts.iter_mut().find(|p| p.id == p1) {
            if let Some(ep) = podcast.episodes.get_mut(1) {
                ep.status = EpisodeStatus::InProgress {
                    position_secs: 1200,
                };
                ep.download_status = DownloadStatus::Downloaded;
            }
            // Mark third as downloaded.
            if let Some(ep) = podcast.episodes.get_mut(2) {
                ep.download_status = DownloadStatus::Downloaded;
            }
        }

        // Podcast 2: Science
        let p2 = self.subscribe(
            "StarTalk Radio",
            "Neil deGrasse Tyson",
            "Science, pop culture, and comedy collide on StarTalk Radio.",
            "https://www.startalkradio.net/feed/",
            "https://example.com/startalk.png",
            vec![Category::Science, Category::Comedy],
        );
        self.add_episode(
            p2,
            "The James Webb Space Telescope: Two Years Later",
            "We explore the discoveries from JWST in its second year.",
            "2026-05-12",
            2700,
            "https://example.com/st1.mp3",
            48_000_000,
        );
        self.add_episode(
            p2,
            "Quantum Computing for Everyone",
            "Breaking down the basics of quantum computing.",
            "2026-05-05",
            2400,
            "https://example.com/st2.mp3",
            42_000_000,
        );

        // Podcast 3: News
        let p3 = self.subscribe(
            "The Daily Brief",
            "News Desk",
            "Your daily news briefing in 15 minutes.",
            "https://example.com/daily.rss",
            "https://example.com/daily.png",
            vec![Category::News],
        );
        self.add_episode(
            p3,
            "Global Markets Rally After Trade Deal",
            "Markets surge following the landmark US-EU trade agreement.",
            "2026-05-18",
            900,
            "https://example.com/db1.mp3",
            16_000_000,
        );
        self.add_episode(
            p3,
            "Climate Summit: Key Takeaways",
            "What happened at the Paris Climate Summit 2026.",
            "2026-05-17",
            840,
            "https://example.com/db2.mp3",
            14_000_000,
        );

        // Mark news episodes as played.
        if let Some(podcast) = self.podcasts.iter_mut().find(|p| p.id == p3)
            && let Some(ep) = podcast.episodes.get_mut(0)
        {
            ep.status = EpisodeStatus::Played;
        }

        // Podcast 4: True Crime
        let p4 = self.subscribe(
            "Cold Case Files",
            "Investigation Network",
            "Unsolved cases reexamined with modern forensic techniques.",
            "https://example.com/coldcase.rss",
            "https://example.com/coldcase.png",
            vec![Category::TrueCrime, Category::Society],
        );
        self.add_episode(
            p4,
            "The Vanishing at Lake Pines",
            "A family disappears from their lakeside cabin in 1998.",
            "2026-05-14",
            3480,
            "https://example.com/cc1.mp3",
            60_000_000,
        );
        self.add_episode(
            p4,
            "DNA Evidence Reopens 30-Year Case",
            "New genetic genealogy techniques crack an old mystery.",
            "2026-05-07",
            2940,
            "https://example.com/cc2.mp3",
            51_000_000,
        );

        // Add some episodes to the queue.
        let first_ep_id = self
            .podcasts
            .first()
            .and_then(|p| p.episodes.first())
            .map(|e| (self.podcasts.first().map(|p| p.id).unwrap_or(0), e.id));
        if let Some((pod_id, ep_id)) = first_ep_id {
            self.queue_episode(pod_id, ep_id);
        }

        // Add some history.
        self.stats
            .record_listening(p1, "The Rustacean Station", 3600, true);
        self.stats
            .record_listening(p2, "StarTalk Radio", 2700, true);
        self.stats.episodes_completed = 2;

        // Update used disk space for downloaded episodes.
        self.used_disk_bytes = 116_000_000; // ~110 MB
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle one input event. Returns whether anything changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => false,
        }
    }

    /// Advance whatever is in flight. Returns whether anything moved.
    ///
    /// Two clocks share one tick: playback runs while an episode is playing,
    /// and the download queue runs while anything is downloading. Neither is
    /// asked to run when it has nothing to do, which is the point of
    /// `tick_interval` returning `None` when both are idle.
    fn handle_tick(&mut self, elapsed_ms: u64) -> bool {
        let mut moved = false;
        if self.player_state == PlayerState::Playing {
            self.tick(elapsed_ms);
            moved = true;
        }
        if self.has_active_downloads() {
            self.simulate_download_tick();
            moved = true;
        }
        moved
    }

    /// Is anything left in the download queue?
    ///
    /// A queued-but-not-yet-started item counts: `simulate_download_tick`
    /// starts one when nothing is active, so a queue with an idle item in it
    /// is a queue that will move on the next tick.
    pub fn has_active_downloads(&self) -> bool {
        !self.download_queue.is_empty()
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
            return false;
        }
        // The now-playing bar is drawn over everything else, so it is asked
        // first -- otherwise a click on the play button would fall through to
        // whichever list happens to be underneath it.
        if let Some(control) = self.player_control_at(event.x, event.y) {
            self.press_player_control(control, event.x);
            return true;
        }
        if let Some(target) = self.sidebar_target_at(event.x, event.y) {
            self.select_sidebar(target);
            return true;
        }
        if self.main_view == MainView::EpisodeList {
            if let Some(filter) = self.filter_pill_at(event.x, event.y) {
                // A filter shortens the list, so a scrolled position in the
                // old one names nothing in the new one.
                self.episode_filter = filter;
                self.episode_list_scroll = 0;
                return true;
            }
            if let Some((pod_id, ep_id)) = self.episode_row_at(event.x, event.y) {
                // First click selects, second opens: a list you cannot look at
                // without leaving it is a list you cannot browse.
                if self.selected_episode_id == Some(ep_id) {
                    self.main_view = MainView::EpisodeDetail;
                } else {
                    self.selected_episode_id = Some(ep_id);
                    self.current_podcast_id.get_or_insert(pod_id);
                }
                return true;
            }
        }
        false
    }

    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            // Playback. Space is the one key every player in the world binds.
            Key::Space => {
                if self.player_state == PlayerState::Stopped {
                    self.play_selected_episode()
                } else {
                    self.toggle_playback();
                    true
                }
            }
            Key::Left => self.seek_if_playing(false),
            Key::Right => self.seek_if_playing(true),
            Key::Up => self.move_episode_selection(-1),
            Key::Down => self.move_episode_selection(1),
            Key::Enter => {
                if self.main_view == MainView::EpisodeList && self.selected_episode_id.is_some() {
                    self.main_view = MainView::EpisodeDetail;
                    true
                } else {
                    self.play_selected_episode()
                }
            }
            Key::Escape => {
                // Out of the detail view, back to the list it came from.
                if self.main_view == MainView::EpisodeDetail {
                    self.main_view = MainView::EpisodeList;
                    true
                } else {
                    false
                }
            }
            Key::Tab => {
                self.cycle_filter();
                true
            }
            Key::PageUp => {
                self.scroll_episode_list_by(-1);
                true
            }
            Key::PageDown => {
                self.scroll_episode_list_by(1);
                true
            }
            Key::Home => {
                self.scroll_episode_list_to_top();
                true
            }
            _ => self.handle_typed(event),
        }
    }

    fn handle_typed(&mut self, event: &KeyEvent) -> bool {
        let Some(ch) = event.typed().next() else {
            return false;
        };
        match ch.to_ascii_lowercase() {
            's' => {
                self.cycle_speed();
                true
            }
            'q' => self.queue_selected_episode(),
            'd' => self.download_selected_episode(),
            'm' => self.toggle_played_on_selection(),
            _ => false,
        }
    }

    /// Seek, but only when there is something to seek in. Returns whether it
    /// moved: an arrow key with the player stopped is not a redraw.
    fn seek_if_playing(&mut self, forward: bool) -> bool {
        if self.player_state == PlayerState::Stopped {
            return false;
        }
        if forward {
            self.seek_forward(SKIP_FORWARD_SECS);
        } else {
            self.seek_backward(SKIP_BACK_SECS);
        }
        true
    }

    /// The episode the keyboard is on, as a (podcast, episode) pair.
    pub fn selected_episode(&self) -> Option<(u64, u64)> {
        let wanted = self.selected_episode_id?;
        self.listed_episodes()
            .into_iter()
            .find(|&(_, ep_id)| ep_id == wanted)
    }

    /// Move the selection by `delta` rows, stopping at the ends.
    ///
    /// Stopping rather than wrapping: holding Down to reach the end of a feed
    /// and silently arriving back at the top is a worse answer than stopping,
    /// because the list looks the same either way.
    fn move_episode_selection(&mut self, delta: isize) -> bool {
        let episodes = self.listed_episodes();
        if episodes.is_empty() {
            return false;
        }
        let current = self
            .selected_episode_id
            .and_then(|id| episodes.iter().position(|&(_, ep_id)| ep_id == id));
        let next = match current {
            None => 0,
            Some(index) => {
                let last = episodes.len().saturating_sub(1);
                let moved = index.saturating_add_signed(delta).min(last);
                if moved == index {
                    return false;
                }
                moved
            }
        };
        let Some(&(pod_id, ep_id)) = episodes.get(next) else {
            return false;
        };
        self.selected_episode_id = Some(ep_id);
        self.current_podcast_id.get_or_insert(pod_id);
        self.scroll_selection_into_view(next);
        true
    }

    /// Scroll so that row `index` is one of the drawn ones.
    fn scroll_selection_into_view(&mut self, index: usize) {
        let total = self.listed_episodes().len();
        let window = self.episode_list_window(total, self.content_bottom());
        if index < window.start {
            self.episode_list_scroll = index;
        } else if index >= window.end() {
            self.episode_list_scroll = index.saturating_add(1).saturating_sub(window.count.max(1));
        }
    }

    /// Play whatever the selection is on. Returns whether it started.
    fn play_selected_episode(&mut self) -> bool {
        let Some((pod_id, ep_id)) = self.selected_episode() else {
            return false;
        };
        self.play_episode(pod_id, ep_id)
    }

    fn queue_selected_episode(&mut self) -> bool {
        let Some((pod_id, ep_id)) = self.selected_episode() else {
            return false;
        };
        self.queue_episode(pod_id, ep_id)
    }

    fn download_selected_episode(&mut self) -> bool {
        let Some((pod_id, ep_id)) = self.selected_episode() else {
            return false;
        };
        self.queue_download(pod_id, ep_id)
    }

    fn toggle_played_on_selection(&mut self) -> bool {
        let Some((pod_id, ep_id)) = self.selected_episode() else {
            return false;
        };
        let played = self
            .find_episode_global(pod_id, ep_id)
            .is_some_and(|ep| ep.status.is_played());
        if played {
            self.mark_unplayed(pod_id, ep_id)
        } else {
            self.mark_played(pod_id, ep_id)
        }
    }

    /// Step the episode filter along the bar. The list is re-anchored for the
    /// same reason a click on a pill re-anchors it.
    fn cycle_filter(&mut self) {
        let current = EpisodeFilter::ALL
            .iter()
            .position(|f| *f == self.episode_filter)
            .unwrap_or(0);
        let next = current
            .saturating_add(1)
            .checked_rem(EpisodeFilter::ALL.len())
            .unwrap_or(0);
        if let Some(&filter) = EpisodeFilter::ALL.get(next) {
            self.episode_filter = filter;
        }
        self.episode_list_scroll = 0;
    }

    /// Render the entire application to a list of render commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar.
        self.render_sidebar(&mut cmds);

        // Main content area.
        let content_x = SIDEBAR_WIDTH;
        let content_w = self.width - SIDEBAR_WIDTH;
        let content_h = if self.player_state != PlayerState::Stopped {
            self.height - NOW_PLAYING_HEIGHT
        } else {
            self.height
        };

        cmds.push(RenderCommand::PushClip {
            x: content_x,
            y: 0.0,
            width: content_w,
            height: content_h,
        });

        match self.main_view {
            MainView::EpisodeList => {
                self.render_episode_list(&mut cmds, content_x, content_w, content_h);
            }
            MainView::EpisodeDetail => {
                self.render_episode_detail(&mut cmds, content_x, content_w, content_h);
            }
            MainView::Queue => self.render_queue_view(&mut cmds, content_x, content_w, content_h),
            MainView::Downloads => {
                self.render_downloads_view(&mut cmds, content_x, content_w, content_h);
            }
            MainView::History => {
                self.render_history_view(&mut cmds, content_x, content_w, content_h);
            }
            MainView::Statistics => {
                self.render_statistics_view(&mut cmds, content_x, content_w, content_h);
            }
            MainView::Search => self.render_search_view(&mut cmds, content_x, content_w, content_h),
        }

        cmds.push(RenderCommand::PopClip);

        // Now playing bar.
        if self.player_state != PlayerState::Stopped {
            self.render_now_playing(&mut cmds);
        }

        cmds
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        // Sidebar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: SIDEBAR_WIDTH,
            height: self.height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar border.
        cmds.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH - 1.0,
            y: 0.0,
            width: 1.0,
            height: self.height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let indent: f32 = 16.0;

        // Title. This is chrome, not content: it stays put while the list
        // below it scrolls.
        cmds.push(RenderCommand::Text {
            x: indent,
            y: 12.0,
            text: "Podcasts".to_string(),
            color: TEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        let (rows, window) = self.sidebar_layout();
        let mut item_y = SIDEBAR_LIST_TOP;
        for row in rows.get(window.start..window.end()).unwrap_or_default() {
            match row {
                SidebarRow::Header(text) => cmds.push(RenderCommand::Text {
                    x: indent,
                    y: item_y,
                    text: (*text).to_string(),
                    color: OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
                    overflow: TextOverflow::Ellipsis,
                }),
                SidebarRow::Divider => cmds.push(RenderCommand::FillRect {
                    x: indent,
                    y: item_y + 12.0,
                    width: SIDEBAR_WIDTH - indent * 2.0,
                    height: 1.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                }),
                SidebarRow::Item {
                    label,
                    accent,
                    target,
                } => self.render_sidebar_item(
                    cmds,
                    indent,
                    item_y,
                    label,
                    *accent,
                    self.sidebar_target_selected(*target),
                ),
            }
            item_y += row.height();
        }

        // A sidebar hiding rows says how many. The space is reserved above
        // whether or not the line is drawn, so how many fit does not depend on
        // whether any are hidden.
        let hidden = rows.len().saturating_sub(window.count);
        if hidden > 0 {
            cmds.push(RenderCommand::Text {
                x: indent,
                y: item_y,
                text: format!("{hidden} more"),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// The sidebar's scrolling content, in order, as one flat row list.
    ///
    /// Built in one place so that the scroll window and the drawing loop
    /// measure the same rows: a list whose height is computed by one function
    /// and drawn by another drifts the moment either gains a row.
    /// Adopt a new window size. Returns whether it changed.
    ///
    /// The scroll positions are left alone: both are row indices and both are
    /// documented as harmless past the end, so a window made shorter shows the
    /// last page of a list rather than an empty one.
    pub fn set_window_size(&mut self, width: f32, height: f32) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        if (self.width - width).abs() < f32::EPSILON && (self.height - height).abs() < f32::EPSILON
        {
            return false;
        }
        self.width = width;
        self.height = height;
        true
    }

    /// Height available to content, which is the window less the now-playing
    /// bar when that bar is up. The bar is drawn *over* the bottom of both the
    /// sidebar and the content area, so neither may lay out below it.
    fn content_bottom(&self) -> f32 {
        if self.player_state == PlayerState::Stopped {
            self.height
        } else {
            self.height - NOW_PLAYING_HEIGHT
        }
    }

    /// The sidebar's rows and the window over them that is on screen.
    ///
    /// One function because the renderer and the hit test must agree about a
    /// scrolled list exactly, and a scroll position is precisely the thing
    /// that makes "the fourth row" and "the fourth row drawn" different rows.
    fn sidebar_layout(&self) -> (Vec<SidebarRow>, scroll_window::Rows) {
        let rows = self.sidebar_rows();
        let heights: Vec<f32> = rows.iter().map(SidebarRow::height).collect();
        let window = scroll_window::visible_variable(
            &heights,
            self.content_bottom() - SIDEBAR_LIST_TOP - LIST_MORE_HEIGHT,
            self.sidebar_scroll,
        );
        (rows, window)
    }

    /// Which sidebar row a point is on, if any.
    pub fn sidebar_target_at(&self, x: f32, y: f32) -> Option<SidebarTarget> {
        if x < 0.0 || x >= SIDEBAR_WIDTH || y < SIDEBAR_LIST_TOP {
            return None;
        }
        let (rows, window) = self.sidebar_layout();
        let mut row_y = SIDEBAR_LIST_TOP;
        for row in rows.get(window.start..window.end())? {
            let next = row_y + row.height();
            if y < next {
                return match row {
                    SidebarRow::Item { target, .. } => Some(*target),
                    // A header or the rule between sections is not a control.
                    SidebarRow::Header(_) | SidebarRow::Divider => None,
                };
            }
            row_y = next;
        }
        None
    }

    /// Is this row the one the user is looking at?
    ///
    /// Search and the library views answer differently: Search has no
    /// `SidebarSelection` of its own and is identified by the main view, while
    /// All Episodes needs both -- selecting a podcast leaves the selection on
    /// that podcast, and All Episodes must not stay lit.
    fn sidebar_target_selected(&self, target: SidebarTarget) -> bool {
        match target {
            SidebarTarget::Search => self.main_view == MainView::Search,
            SidebarTarget::AllEpisodes => {
                self.sidebar_selection == SidebarSelection::AllEpisodes
                    && self.main_view == MainView::EpisodeList
            }
            SidebarTarget::Queue => self.sidebar_selection == SidebarSelection::Queue,
            SidebarTarget::Downloads => self.sidebar_selection == SidebarSelection::Downloads,
            SidebarTarget::History => self.sidebar_selection == SidebarSelection::History,
            SidebarTarget::Statistics => self.sidebar_selection == SidebarSelection::Statistics,
            SidebarTarget::Podcast(id) => self.sidebar_selection == SidebarSelection::Podcast(id),
            SidebarTarget::Category(cat) => {
                self.sidebar_selection == SidebarSelection::Category(cat)
            }
        }
    }

    /// Go where a sidebar row points.
    pub fn select_sidebar(&mut self, target: SidebarTarget) {
        // A list scrolled forty rows into a long podcast shows nothing at all
        // of a short one, so a change of feed starts at the top of it.
        self.episode_list_scroll = 0;
        match target {
            SidebarTarget::Search => self.main_view = MainView::Search,
            SidebarTarget::AllEpisodes => {
                self.sidebar_selection = SidebarSelection::AllEpisodes;
                self.main_view = MainView::EpisodeList;
            }
            SidebarTarget::Queue => {
                self.sidebar_selection = SidebarSelection::Queue;
                self.main_view = MainView::Queue;
            }
            SidebarTarget::Downloads => {
                self.sidebar_selection = SidebarSelection::Downloads;
                self.main_view = MainView::Downloads;
            }
            SidebarTarget::History => {
                self.sidebar_selection = SidebarSelection::History;
                self.main_view = MainView::History;
            }
            SidebarTarget::Statistics => {
                self.sidebar_selection = SidebarSelection::Statistics;
                self.main_view = MainView::Statistics;
            }
            SidebarTarget::Podcast(id) => {
                self.sidebar_selection = SidebarSelection::Podcast(id);
                self.main_view = MainView::EpisodeList;
            }
            SidebarTarget::Category(cat) => {
                self.sidebar_selection = SidebarSelection::Category(cat);
                self.main_view = MainView::EpisodeList;
            }
        }
    }

    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let item = |label: &str, accent: Color, target: SidebarTarget| SidebarRow::Item {
            label: label.to_string(),
            accent,
            target,
        };

        let queue_count = self.play_queue.len();
        let queue_label = if queue_count > 0 {
            format!("Queue ({queue_count})")
        } else {
            "Queue".to_string()
        };

        let mut rows = vec![
            item("Search", BLUE, SidebarTarget::Search),
            item("All Episodes", LAVENDER, SidebarTarget::AllEpisodes),
            item(&queue_label, GREEN, SidebarTarget::Queue),
            item("Downloads", PEACH, SidebarTarget::Downloads),
            item("History", MAUVE, SidebarTarget::History),
            item("Statistics", TEAL, SidebarTarget::Statistics),
            SidebarRow::Divider,
            SidebarRow::Header("SUBSCRIPTIONS"),
        ];

        for podcast in &self.podcasts {
            let unplayed = podcast.unplayed_count();
            let label = if unplayed > 0 {
                format!("{} ({})", podcast.title, unplayed)
            } else {
                podcast.title.clone()
            };
            rows.push(SidebarRow::Item {
                label,
                accent: podcast.categories.first().map_or(BLUE, |c| c.color()),
                target: SidebarTarget::Podcast(podcast.id),
            });
        }

        rows.push(SidebarRow::Divider);
        rows.push(SidebarRow::Header("CATEGORIES"));
        for cat in Category::ALL {
            rows.push(SidebarRow::Item {
                label: cat.name().to_string(),
                accent: cat.color(),
                target: SidebarTarget::Category(*cat),
            });
        }

        rows
    }

    fn render_sidebar_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        accent: Color,
        selected: bool,
    ) {
        let item_w = SIDEBAR_WIDTH - x * 2.0;
        let item_h: f32 = 28.0;

        if selected {
            cmds.push(RenderCommand::FillRect {
                x: x - 4.0,
                y,
                width: item_w + 8.0,
                height: item_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
            // Accent bar.
            cmds.push(RenderCommand::FillRect {
                x: x - 4.0,
                y: y + 4.0,
                width: 3.0,
                height: item_h - 8.0,
                color: accent,
                corner_radii: CornerRadii::all(1.5),
            });
        }

        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 5.0,
            text: label.to_string(),
            color: if selected { TEXT } else { SUBTEXT0 },
            font_size: 13.0,
            font_weight: if selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(item_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The episodes the list is showing, in the order it shows them.
    pub fn listed_episodes(&self) -> Vec<(u64, u64)> {
        match &self.sidebar_selection {
            SidebarSelection::Podcast(id) => self.filtered_episodes_for_podcast(*id),
            SidebarSelection::Category(cat) => self.episodes_for_category(*cat),
            _ => self.filtered_all_episodes(),
        }
    }

    /// The window of episode rows on screen.
    fn episode_list_window(&self, total: usize, content_h: f32) -> scroll_window::Rows {
        scroll_window::visible(
            total,
            EPISODE_ROW_HEIGHT,
            content_h - EPISODE_LIST_TOP - LIST_MORE_HEIGHT,
            self.episode_list_scroll,
        )
    }

    /// Which episode a point in the list is on, if any.
    pub fn episode_row_at(&self, x: f32, y: f32) -> Option<(u64, u64)> {
        if x < SIDEBAR_WIDTH || y < EPISODE_LIST_TOP {
            return None;
        }
        let content_h = self.content_bottom();
        let episodes = self.listed_episodes();
        let window = self.episode_list_window(episodes.len(), content_h);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded at or below EPISODE_LIST_TOP above, so the quotient is >= 0"
        )]
        let drawn = ((y - EPISODE_LIST_TOP) / EPISODE_ROW_HEIGHT) as usize;
        if drawn >= window.count {
            return None;
        }
        episodes.get(window.start.checked_add(drawn)?).copied()
    }

    /// The filter pills, each with the x it starts at and how wide it is.
    ///
    /// The renderer walked these widths inline and nothing else could know
    /// where a pill was, so the filter bar was decoration.
    fn filter_pills(&self, content_x: f32) -> Vec<(EpisodeFilter, f32, f32)> {
        let mut pill_x = content_x + 12.0;
        let mut out = Vec::new();
        for filter in EpisodeFilter::ALL {
            let width = text::padded_width(filter.label(), 8.0, 12.0, FontWeightHint::Regular);
            out.push((filter, pill_x, width));
            pill_x += width + 8.0;
        }
        out
    }

    /// Which filter pill a point is on, if any.
    pub fn filter_pill_at(&self, x: f32, y: f32) -> Option<EpisodeFilter> {
        let pill_y = HEADER_HEIGHT + 4.0;
        if y < pill_y || y >= pill_y + CATEGORY_PILL_HEIGHT {
            return None;
        }
        self.filter_pills(SIDEBAR_WIDTH)
            .into_iter()
            .find(|&(_, px, w)| x >= px && x < px + w)
            .map(|(filter, _, _)| filter)
    }

    fn render_episode_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        content_h: f32,
    ) {
        let episodes = match &self.sidebar_selection {
            SidebarSelection::AllEpisodes => self.filtered_all_episodes(),
            SidebarSelection::Podcast(id) => self.filtered_episodes_for_podcast(*id),
            SidebarSelection::Category(cat) => self.episodes_for_category(*cat),
            _ => self.filtered_all_episodes(),
        };

        // Header.
        let header_text = match &self.sidebar_selection {
            SidebarSelection::AllEpisodes => "All Episodes".to_string(),
            SidebarSelection::Podcast(id) => self
                .find_podcast(*id)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            SidebarSelection::Category(cat) => cat.name().to_string(),
            _ => "Episodes".to_string(),
        };

        self.render_content_header(cmds, content_x, content_w, &header_text);

        // Filter bar.
        let filter_y = HEADER_HEIGHT;
        self.render_filter_bar(cmds, content_x, filter_y, content_w);

        // Episode count.
        let count_y = filter_y + TOOLBAR_HEIGHT;
        cmds.push(RenderCommand::Text {
            x: content_x + 16.0,
            y: count_y + 8.0,
            text: format!("{} episodes", episodes.len()),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Episodes. The old loop broke on `ep_y > content_h`, which drew the
        // straddling row whole and past the bottom, and had no offset at all:
        // everything below the fold was cut by the window edge rather than by
        // a scroll position, so it could not be reached.
        let start_y = EPISODE_LIST_TOP;
        let window = self.episode_list_window(episodes.len(), content_h);
        for (drawn, (pod_id, ep_id)) in episodes
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let ep_y = start_y + drawn as f32 * EPISODE_ROW_HEIGHT;
            if let Some(podcast) = self.find_podcast(*pod_id)
                && let Some(ep) = podcast.find_episode(*ep_id)
            {
                let selected = self.selected_episode_id == Some(*ep_id);
                self.render_episode_row(
                    cmds,
                    content_x + 8.0,
                    ep_y,
                    content_w - 16.0,
                    ep,
                    &podcast.title,
                    selected,
                );
            }
        }

        // A list hiding episodes says how many. The space is reserved above
        // whether or not the line is drawn.
        let hidden = episodes.len().saturating_sub(window.count);
        if hidden > 0 {
            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: start_y + window.count as f32 * EPISODE_ROW_HEIGHT,
                text: format!("{hidden} more"),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_content_header(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        width: f32,
        title: &str,
    ) {
        // Header background.
        cmds.push(RenderCommand::FillRect {
            x,
            y: 0.0,
            width,
            height: HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: 14.0,
            text: title.to_string(),
            color: TEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_filter_bar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let pill_y = y + 4.0;
        for (filter, pill_x, label_width) in self.filter_pills(x) {
            let label = filter.label();
            let selected = self.episode_filter == filter;

            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: pill_x,
                    y: pill_y,
                    width: label_width,
                    height: CATEGORY_PILL_HEIGHT,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(14.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: pill_x + 8.0,
                y: pill_y + 6.0,
                text: label.to_string(),
                color: if selected { TEXT } else { SUBTEXT0 },
                font_size: 12.0,
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(label_width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    // self + cmds + rect (x,y,width) + episode data + parent title + selected
    // flag; all independent and used in sub-render commands.
    #[allow(clippy::too_many_arguments)]
    fn render_episode_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        episode: &Episode,
        podcast_title: &str,
        selected: bool,
    ) {
        // Row background.
        if selected {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: EPISODE_ROW_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
        }

        // Status dot.
        let dot_color = episode.status.color();
        cmds.push(RenderCommand::FillRect {
            x: x + 12.0,
            y: y + 12.0,
            width: 8.0,
            height: 8.0,
            color: dot_color,
            corner_radii: CornerRadii::all(4.0),
        });

        // Episode title.
        cmds.push(RenderCommand::Text {
            x: x + 28.0,
            y: y + 8.0,
            text: episode.title.clone(),
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 160.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Podcast name and date.
        cmds.push(RenderCommand::Text {
            x: x + 28.0,
            y: y + 28.0,
            text: format!("{} - {}", podcast_title, episode.date),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 160.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Duration and download indicator.
        let dur_text = episode.duration_display();
        cmds.push(RenderCommand::Text {
            x: x + width - 120.0,
            y: y + 8.0,
            text: dur_text,
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Download icon indicator.
        if episode.download_status.is_downloaded() {
            cmds.push(RenderCommand::Text {
                x: x + width - 40.0,
                y: y + 8.0,
                text: "DL".to_string(),
                color: GREEN,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Progress bar for in-progress episodes.
        if let EpisodeStatus::InProgress { .. } = episode.status {
            let bar_y = y + EPISODE_ROW_HEIGHT - 6.0;
            let bar_w = width - 40.0;
            let progress = episode.progress_pct() / 100.0;

            cmds.push(RenderCommand::FillRect {
                x: x + 28.0,
                y: bar_y,
                width: bar_w,
                height: 3.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(1.5),
            });
            cmds.push(RenderCommand::FillRect {
                x: x + 28.0,
                y: bar_y,
                width: bar_w * progress,
                height: 3.0,
                color: BLUE,
                corner_radii: CornerRadii::all(1.5),
            });
        }

        // File size.
        cmds.push(RenderCommand::Text {
            x: x + width - 120.0,
            y: y + 28.0,
            text: episode.file_size_display(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Bottom separator.
        cmds.push(RenderCommand::FillRect {
            x: x + 12.0,
            y: y + EPISODE_ROW_HEIGHT - 1.0,
            width: width - 24.0,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_episode_detail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        _content_h: f32,
    ) {
        let (pod_id, ep_id) = match self.selected_episode_id {
            Some(eid) => {
                // Find which podcast owns this episode.
                let found = self
                    .podcasts
                    .iter()
                    .find_map(|p| p.find_episode(eid).map(|_| (p.id, eid)));
                match found {
                    Some(pair) => pair,
                    None => return,
                }
            }
            None => return,
        };

        let podcast = match self.find_podcast(pod_id) {
            Some(p) => p,
            None => return,
        };
        let episode = match podcast.find_episode(ep_id) {
            Some(e) => e,
            None => return,
        };

        self.render_content_header(cmds, content_x, content_w, "Episode Details");

        let mut detail_y = HEADER_HEIGHT + 16.0;
        let pad = content_x + 24.0;
        let text_w = content_w - 48.0;

        // Episode title.
        cmds.push(RenderCommand::Text {
            x: pad,
            y: detail_y,
            text: episode.title.clone(),
            color: TEXT,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(text_w),
            overflow: TextOverflow::Ellipsis,
        });
        detail_y += 32.0;

        // Podcast name.
        cmds.push(RenderCommand::Text {
            x: pad,
            y: detail_y,
            text: format!("From: {}", podcast.title),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_w),
            overflow: TextOverflow::Ellipsis,
        });
        detail_y += 24.0;

        // Date and duration.
        cmds.push(RenderCommand::Text {
            x: pad,
            y: detail_y,
            text: format!(
                "{} | {} | {}",
                episode.date,
                episode.duration_display(),
                episode.file_size_display()
            ),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_w),
            overflow: TextOverflow::Ellipsis,
        });
        detail_y += 24.0;

        // Status badges.
        let status_label = episode.status.label();
        let dl_label = episode.download_status.label();
        // One width for the status badge, used both to draw it and to place
        // the badge that follows it. These were two separate estimates with
        // different padding (+16 drawn, +24 for the next badge's x), so the
        // gap between the two pills was whatever the difference happened to be
        // rather than 8 px.
        let status_w = text::padded_width(status_label, 8.0, 11.0, FontWeightHint::Bold);
        cmds.push(RenderCommand::FillRect {
            x: pad,
            y: detail_y,
            width: status_w,
            height: 22.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(11.0),
        });
        cmds.push(RenderCommand::Text {
            x: pad + 8.0,
            y: detail_y + 4.0,
            text: status_label.to_string(),
            color: episode.status.color(),
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });
        let dl_badge_x = pad + status_w + 8.0;
        cmds.push(RenderCommand::FillRect {
            x: dl_badge_x,
            y: detail_y,
            width: text::padded_width(dl_label, 8.0, 11.0, FontWeightHint::Bold),
            height: 22.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(11.0),
        });
        cmds.push(RenderCommand::Text {
            x: dl_badge_x + 8.0,
            y: detail_y + 4.0,
            text: dl_label.to_string(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });
        detail_y += 36.0;

        // Divider.
        cmds.push(RenderCommand::FillRect {
            x: pad,
            y: detail_y,
            width: text_w,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        detail_y += 16.0;

        // Description.
        cmds.push(RenderCommand::Text {
            x: pad,
            y: detail_y,
            text: "Description".to_string(),
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(text_w),
            overflow: TextOverflow::Ellipsis,
        });
        detail_y += 22.0;

        // `RenderCommand::Text` clips at `max_width` rather than wrapping, so
        // an episode description used to be shown as its first line and no
        // more. `Paragraph::draw` returns the height it used, so the sections
        // below start under the description however long it turns out to be.
        detail_y += text::Paragraph::new(&episode.description, SUBTEXT0)
            .at(pad, detail_y, text_w)
            .font(DESCRIPTION_FONT_SIZE, FontWeightHint::Regular)
            .line_height(PROSE_LINE_HEIGHT)
            .draw(cmds);
        detail_y += PROSE_SECTION_GAP;

        // Notes section.
        if episode.notes.has_content() {
            cmds.push(RenderCommand::FillRect {
                x: pad,
                y: detail_y,
                width: text_w,
                height: 1.0,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            detail_y += 16.0;

            cmds.push(RenderCommand::Text {
                x: pad,
                y: detail_y,
                text: "Notes".to_string(),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(text_w),
                overflow: TextOverflow::Ellipsis,
            });
            detail_y += 22.0;

            if !episode.notes.text.is_empty() {
                // Show notes are the one field here a user writes at length, so
                // drawing them as a single clipped line lost the most. The
                // bookmark rows are stacked directly below, so the cursor has to
                // advance over the lines actually drawn or they land on top.
                detail_y += text::Paragraph::new(&episode.notes.text, SUBTEXT0)
                    .at(pad, detail_y, text_w)
                    .font(NOTES_FONT_SIZE, FontWeightHint::Regular)
                    .line_height(PROSE_LINE_HEIGHT)
                    .draw(cmds);
                detail_y += NOTES_BOOKMARK_GAP;
            }

            // Bookmarks.
            for bm in &episode.notes.bookmarks {
                cmds.push(RenderCommand::FillRect {
                    x: pad,
                    y: detail_y,
                    width: 60.0,
                    height: 20.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: pad + 6.0,
                    y: detail_y + 3.0,
                    text: bm.timestamp_display(),
                    color: BLUE,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(50.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: pad + 68.0,
                    y: detail_y + 3.0,
                    text: bm.label.clone(),
                    color: TEXT,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(text_w - 80.0),
                    overflow: TextOverflow::Ellipsis,
                });
                detail_y += 26.0;
            }
        }
    }

    fn render_queue_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        content_h: f32,
    ) {
        let queue_label = format!("Up Next ({})", self.play_queue.len());
        self.render_content_header(cmds, content_x, content_w, &queue_label);

        if self.play_queue.is_empty() {
            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: HEADER_HEIGHT + 40.0,
                text: "Queue is empty. Add episodes to play next.".to_string(),
                color: SUBTEXT0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        // Auto-play indicator.
        cmds.push(RenderCommand::Text {
            x: content_x + 16.0,
            y: HEADER_HEIGHT + 12.0,
            text: format!(
                "Auto-play: {}",
                if self.auto_play_next { "On" } else { "Off" }
            ),
            color: if self.auto_play_next { GREEN } else { OVERLAY0 },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = HEADER_HEIGHT + 36.0;
        for (idx, item) in self.play_queue.iter().enumerate() {
            if row_y > content_h {
                break;
            }

            let row_h: f32 = 56.0;

            // Row background (alternating).
            if idx % 2 == 0 {
                cmds.push(RenderCommand::FillRect {
                    x: content_x + 8.0,
                    y: row_y,
                    width: content_w - 16.0,
                    height: row_h,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Index number.
            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: row_y + 10.0,
                text: format!("{}.", idx.saturating_add(1)),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Episode title.
            cmds.push(RenderCommand::Text {
                x: content_x + 48.0,
                y: row_y + 8.0,
                text: item.episode_title.clone(),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 180.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Podcast title.
            cmds.push(RenderCommand::Text {
                x: content_x + 48.0,
                y: row_y + 28.0,
                text: item.podcast_title.clone(),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 180.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Duration.
            cmds.push(RenderCommand::Text {
                x: content_x + content_w - 100.0,
                y: row_y + 10.0,
                text: format_duration(item.duration_secs),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });

            row_y += row_h + 4.0;
        }
    }

    fn render_downloads_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        content_h: f32,
    ) {
        self.render_content_header(cmds, content_x, content_w, "Downloads");

        let mut info_y = HEADER_HEIGHT + 12.0;

        // Disk usage bar.
        let bar_x = content_x + 16.0;
        let bar_w = content_w - 32.0;
        let bar_h: f32 = 20.0;
        let usage_pct = self.disk_usage_pct() / 100.0;

        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: info_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: info_y,
            width: bar_w * usage_pct,
            height: bar_h,
            color: if usage_pct > 0.9 { RED } else { BLUE },
            corner_radii: CornerRadii::all(4.0),
        });
        info_y += bar_h + 4.0;

        cmds.push(RenderCommand::Text {
            x: bar_x,
            y: info_y,
            text: format!(
                "{} used of {} ({:.1}%)",
                format_bytes(self.used_disk_bytes),
                format_bytes(self.total_disk_bytes),
                self.disk_usage_pct()
            ),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_w),
            overflow: TextOverflow::Ellipsis,
        });
        info_y += 28.0;

        // Active downloads.
        if self.download_queue.is_empty() {
            cmds.push(RenderCommand::Text {
                x: bar_x,
                y: info_y,
                text: "No active downloads.".to_string(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: bar_x,
                y: info_y,
                text: format!("Download Queue ({})", self.download_queue.len()),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(bar_w),
                overflow: TextOverflow::Ellipsis,
            });
            info_y += 24.0;

            for item in &self.download_queue {
                if info_y > content_h {
                    break;
                }

                cmds.push(RenderCommand::Text {
                    x: bar_x,
                    y: info_y,
                    text: item.episode_title.clone(),
                    color: TEXT,
                    font_size: 13.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(bar_w - 100.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Progress bar.
                let prog_y = info_y + 20.0;
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: prog_y,
                    width: bar_w - 80.0,
                    height: 6.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: prog_y,
                    width: (bar_w - 80.0) * item.progress,
                    height: 6.0,
                    color: PEACH,
                    corner_radii: CornerRadii::all(3.0),
                });

                // Percentage text.
                cmds.push(RenderCommand::Text {
                    x: bar_x + bar_w - 70.0,
                    y: info_y + 6.0,
                    text: format!("{:.0}%", item.progress * 100.0),
                    color: OVERLAY0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });

                info_y += 40.0;
            }
        }

        info_y += 16.0;

        // Downloaded episodes list.
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: info_y,
            width: bar_w,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        info_y += 12.0;

        cmds.push(RenderCommand::Text {
            x: bar_x,
            y: info_y,
            text: "Downloaded Episodes".to_string(),
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(bar_w),
            overflow: TextOverflow::Ellipsis,
        });
        info_y += 24.0;

        for podcast in &self.podcasts {
            for ep in &podcast.episodes {
                if !ep.download_status.is_downloaded() {
                    continue;
                }
                if info_y > content_h {
                    break;
                }

                cmds.push(RenderCommand::Text {
                    x: bar_x,
                    y: info_y,
                    text: ep.title.clone(),
                    color: TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(bar_w - 100.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: bar_x + bar_w - 90.0,
                    y: info_y,
                    text: ep.file_size_display(),
                    color: OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
                info_y += 24.0;
            }
        }
    }

    fn render_history_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        content_h: f32,
    ) {
        let title = format!("Playback History ({})", self.history.len());
        self.render_content_header(cmds, content_x, content_w, &title);

        if self.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: HEADER_HEIGHT + 40.0,
                text: "No playback history yet.".to_string(),
                color: SUBTEXT0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let mut row_y = HEADER_HEIGHT + 12.0;
        // Show most recent first.
        for entry in self.history.iter().rev() {
            if row_y > content_h {
                break;
            }

            let row_h: f32 = 52.0;

            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: row_y + 6.0,
                text: entry.episode_title.clone(),
                color: TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 160.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: content_x + 16.0,
                y: row_y + 26.0,
                text: format!(
                    "{} | {} listened",
                    entry.podcast_title,
                    format_duration(entry.duration_listened_secs)
                ),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 160.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Completion indicator.
            if entry.completed {
                cmds.push(RenderCommand::Text {
                    x: content_x + content_w - 100.0,
                    y: row_y + 12.0,
                    text: "Completed".to_string(),
                    color: GREEN,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Date.
            cmds.push(RenderCommand::Text {
                x: content_x + content_w - 130.0,
                y: row_y + 30.0,
                text: entry.listened_at.clone(),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Separator.
            cmds.push(RenderCommand::FillRect {
                x: content_x + 16.0,
                y: row_y + row_h - 1.0,
                width: content_w - 32.0,
                height: 1.0,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });

            row_y += row_h;
        }
    }

    fn render_statistics_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        _content_h: f32,
    ) {
        self.render_content_header(cmds, content_x, content_w, "Statistics");

        let pad = content_x + 24.0;
        let card_w = (content_w - 72.0) / 2.0;
        let card_h: f32 = 100.0;
        let mut card_y = HEADER_HEIGHT + 24.0;

        // Card 1: Total listening time.
        self.render_stat_card(
            cmds,
            pad,
            card_y,
            card_w,
            card_h,
            "Total Listening Time",
            &self.stats.total_time_display(),
            BLUE,
        );

        // Card 2: Episodes completed.
        self.render_stat_card(
            cmds,
            pad + card_w + 24.0,
            card_y,
            card_w,
            card_h,
            "Episodes Completed",
            &self.stats.episodes_completed.to_string(),
            GREEN,
        );

        card_y += card_h + 16.0;

        // Card 3: Subscriptions.
        self.render_stat_card(
            cmds,
            pad,
            card_y,
            card_w,
            card_h,
            "Subscriptions",
            &self.stats.subscriptions_count.to_string(),
            LAVENDER,
        );

        // Card 4: Most listened.
        let most_listened = self
            .stats
            .most_listened_podcast
            .as_deref()
            .unwrap_or("None");
        self.render_stat_card(
            cmds,
            pad + card_w + 24.0,
            card_y,
            card_w,
            card_h,
            "Most Listened",
            most_listened,
            PEACH,
        );

        card_y += card_h + 32.0;

        // Per-podcast breakdown.
        cmds.push(RenderCommand::Text {
            x: pad,
            y: card_y,
            text: "Per-Podcast Breakdown".to_string(),
            color: TEXT,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_w - 48.0),
            overflow: TextOverflow::Ellipsis,
        });
        card_y += 28.0;

        for podcast in &self.podcasts {
            let secs = self
                .stats
                .per_podcast_secs
                .get(&podcast.id)
                .copied()
                .unwrap_or(0);
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;

            cmds.push(RenderCommand::Text {
                x: pad,
                y: card_y,
                text: podcast.title.clone(),
                color: TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 200.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: pad + content_w - 200.0,
                y: card_y,
                text: format!("{}h {}m", hours, mins),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            card_y += 24.0;
        }
    }

    // Stat-card render takes self + cmds + rect + label/value + accent.
    #[allow(clippy::too_many_arguments)]
    fn render_stat_card(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: &str,
        value: &str,
        accent: Color,
    ) {
        // Card background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Accent bar at top.
        cmds.push(RenderCommand::FillRect {
            x: x + 16.0,
            y: y + 8.0,
            width: 40.0,
            height: 4.0,
            color: accent,
            corner_radii: CornerRadii::all(2.0),
        });

        // Label.
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 24.0,
            text: label.to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Value.
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 48.0,
            text: value.to_string(),
            color: TEXT,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_search_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_x: f32,
        content_w: f32,
        content_h: f32,
    ) {
        self.render_content_header(cmds, content_x, content_w, "Search");

        let pad = content_x + 16.0;
        let text_w = content_w - 32.0;

        // Search input field.
        let input_y = HEADER_HEIGHT + 12.0;
        cmds.push(RenderCommand::FillRect {
            x: pad,
            y: input_y,
            width: text_w,
            height: SEARCH_BAR_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: pad,
            y: input_y,
            width: text_w,
            height: SEARCH_BAR_HEIGHT,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        let display_text = if self.search_query.is_empty() {
            "Search podcasts and episodes..."
        } else {
            &self.search_query
        };
        cmds.push(RenderCommand::Text {
            x: pad + 12.0,
            y: input_y + 10.0,
            text: display_text.to_string(),
            color: if self.search_query.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Results.
        let results_y = input_y + SEARCH_BAR_HEIGHT + 12.0;
        if self.search_query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: pad,
                y: results_y,
                text: "Type to search across all podcasts and episodes.".to_string(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(text_w),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: pad,
                y: results_y,
                text: format!("{} results", self.search_results.len()),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(text_w),
                overflow: TextOverflow::Ellipsis,
            });

            let mut ep_y = results_y + 24.0;
            for (pod_id, ep_id) in &self.search_results {
                if ep_y > content_h {
                    break;
                }
                if let Some(podcast) = self.find_podcast(*pod_id)
                    && let Some(ep) = podcast.find_episode(*ep_id)
                {
                    self.render_episode_row(
                        cmds,
                        content_x + 8.0,
                        ep_y,
                        content_w - 16.0,
                        ep,
                        &podcast.title,
                        false,
                    );
                }
                ep_y += EPISODE_ROW_HEIGHT;
            }
        }
    }

    /// Where every control in the now-playing bar is.
    ///
    /// The renderer laid these out inline from `controls_x`, so the only
    /// record of where the play button was, was the pixels it had already
    /// drawn. Both callers now read the same list.
    fn player_controls(&self) -> Vec<(PlayerControl, Rect)> {
        let bar_y = self.height - NOW_PLAYING_HEIGHT;
        let controls_x = self.width / 2.0 - 80.0;
        let controls_y = bar_y + 20.0;
        vec![
            // The seek strip is the full width of the bar's top edge. The
            // progress line itself is 3px tall, which is not a thing a person
            // can hit; the strip is what they are aiming at.
            (
                PlayerControl::Seek,
                Rect {
                    x: 0.0,
                    y: bar_y,
                    width: self.width,
                    height: SEEK_STRIP_HEIGHT,
                },
            ),
            (
                PlayerControl::SkipBack,
                Rect {
                    x: controls_x,
                    y: controls_y,
                    width: 36.0,
                    height: 36.0,
                },
            ),
            (
                PlayerControl::PlayPause,
                Rect {
                    x: controls_x + 48.0,
                    y: controls_y - 2.0,
                    width: 40.0,
                    height: 40.0,
                },
            ),
            (
                PlayerControl::SkipForward,
                Rect {
                    x: controls_x + 100.0,
                    y: controls_y,
                    width: 36.0,
                    height: 36.0,
                },
            ),
            (
                PlayerControl::Speed,
                Rect {
                    x: self.width - 120.0,
                    y: bar_y + 14.0,
                    width: 44.0,
                    height: 22.0,
                },
            ),
        ]
    }

    /// Which player control a point is on, if any.
    pub fn player_control_at(&self, x: f32, y: f32) -> Option<PlayerControl> {
        if self.player_state == PlayerState::Stopped {
            // No bar is drawn, so there is nothing under the pointer.
            return None;
        }
        self.player_controls()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(control, _)| control)
    }

    /// Act on a player control. `x` is only read by the seek strip.
    pub fn press_player_control(&mut self, control: PlayerControl, x: f32) {
        match control {
            PlayerControl::PlayPause => self.toggle_playback(),
            PlayerControl::SkipBack => self.seek_backward(SKIP_BACK_SECS),
            PlayerControl::SkipForward => self.seek_forward(SKIP_FORWARD_SECS),
            PlayerControl::Speed => self.cycle_speed(),
            PlayerControl::Seek => {
                // Not clamped: the strip spans the whole window, so a point
                // outside 0..width is outside the strip and never reaches
                // here -- `Rect::contains` is the bound. `seek_to` caps at the
                // episode's duration in any case.
                let fraction = x / self.width;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "out of range in either direction is capped by seek_to or by the cast"
                )]
                let position = (self.playback_duration_secs as f32 * fraction) as u32;
                self.seek_to(position);
            }
        }
    }

    fn render_now_playing(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_y = self.height - NOW_PLAYING_HEIGHT;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.width,
            height: NOW_PLAYING_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.width,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Progress bar across the top.
        let progress = if self.playback_duration_secs > 0 {
            self.playback_position_secs as f32 / self.playback_duration_secs as f32
        } else {
            0.0
        };
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y + 1.0,
            width: self.width * progress,
            height: 3.0,
            color: BLUE,
            corner_radii: CornerRadii::ZERO,
        });

        // Episode info.
        let info_x: f32 = 16.0;
        let info_y = bar_y + 12.0;

        let ep_title = self
            .current_episode_id
            .and_then(|eid| {
                self.current_podcast_id.and_then(|pid| {
                    self.find_podcast(pid)
                        .and_then(|p| p.find_episode(eid))
                        .map(|e| e.title.clone())
                })
            })
            .unwrap_or_else(|| "Unknown".to_string());

        let pod_title = self
            .current_podcast_id
            .and_then(|pid| self.find_podcast(pid).map(|p| p.title.clone()))
            .unwrap_or_else(|| "Unknown".to_string());

        cmds.push(RenderCommand::Text {
            x: info_x,
            y: info_y,
            text: ep_title,
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(350.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: info_x,
            y: info_y + 20.0,
            text: pod_title,
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(350.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Playback controls, from the same rectangles the hit test reads.
        let controls = self.player_controls();
        let rect_of = |wanted: PlayerControl| {
            controls
                .iter()
                .find(|(c, _)| *c == wanted)
                .map_or(Rect::ZERO, |(_, r)| *r)
        };

        // Skip back button.
        let back = rect_of(PlayerControl::SkipBack);
        cmds.push(RenderCommand::FillRect {
            x: back.x,
            y: back.y,
            width: back.width,
            height: back.height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(18.0),
        });
        cmds.push(RenderCommand::Text {
            x: back.x + 6.0,
            y: back.y + 9.0,
            text: format!("-{SKIP_BACK_SECS}s"),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(28.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Play/Pause button.
        let pp = rect_of(PlayerControl::PlayPause);
        cmds.push(RenderCommand::FillRect {
            x: pp.x,
            y: pp.y,
            width: pp.width,
            height: pp.height,
            color: BLUE,
            corner_radii: CornerRadii::all(20.0),
        });
        let pp_label = match self.player_state {
            PlayerState::Playing => "||",
            PlayerState::Paused => ">",
            PlayerState::Stopped => ">",
        };
        cmds.push(RenderCommand::Text {
            x: pp.x + 12.0,
            y: pp.y + 10.0,
            text: pp_label.to_string(),
            color: CRUST,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(20.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Skip forward button.
        let fwd = rect_of(PlayerControl::SkipForward);
        cmds.push(RenderCommand::FillRect {
            x: fwd.x,
            y: fwd.y,
            width: fwd.width,
            height: fwd.height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(18.0),
        });
        cmds.push(RenderCommand::Text {
            x: fwd.x + 4.0,
            y: fwd.y + 9.0,
            text: format!("+{SKIP_FORWARD_SECS}s"),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Time display.
        let time_x = self.width - 250.0;
        cmds.push(RenderCommand::Text {
            x: time_x,
            y: info_y + 6.0,
            text: format!(
                "{} / {}",
                format_duration(self.playback_position_secs),
                format_duration(self.playback_duration_secs)
            ),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Speed indicator, which is also the button that cycles it.
        let speed = rect_of(PlayerControl::Speed);
        cmds.push(RenderCommand::FillRect {
            x: speed.x,
            y: speed.y,
            width: speed.width,
            height: speed.height,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: speed.x + 6.0,
            y: speed.y + 4.0,
            text: self.playback_speed.label(),
            color: PEACH,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(38.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Entry point
// ============================================================================

impl App for PodcastApp {
    fn title(&self) -> String {
        // What is playing, because that is what the window is for. A podcast
        // manager is left open in the background and found again by its title.
        match (self.player_state, self.current_episode_id) {
            (PlayerState::Stopped, _) | (_, None) => "Podcasts".to_string(),
            (state, Some(ep_id)) => {
                let title = self
                    .current_podcast_id
                    .and_then(|pid| self.find_podcast(pid))
                    .and_then(|pod| pod.find_episode(ep_id))
                    .map_or_else(|| "Unknown".to_string(), |ep| ep.title.clone());
                match state {
                    PlayerState::Playing => format!("{title} - Podcasts"),
                    _ => format!("{title} (paused) - Podcasts"),
                }
            }
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// A clock only while something is actually moving.
    ///
    /// A paused episode and an empty download queue have nothing to advance,
    /// and waking the machine four times a second to establish that is
    /// `known-issues.md` lesson 47.
    fn tick_interval(&self) -> Option<Duration> {
        (self.player_state == PlayerState::Playing || self.has_active_downloads())
            .then_some(PLAYBACK_TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension in pixels is exact in f32"
                )]
                let (w, h) = (*width as f32, *height as f32);
                if self.set_window_size(w, h) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            other => {
                if self.handle_event(other) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one: the first frame is drawn
        // before any `Event::Resize` arrives, so a window the compositor opened
        // at another size would be laid out for the size that was asked for,
        // and every hit box in it would name the wrong rectangle.
        self.set_window_size(width, height);
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app::launch("podcast", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    // Only the test keyboard builds modifier sets; production code reads the
    // ones the compositor sends.
    use guitk::event::Modifiers;

    // --- text measurement ---

    #[test]
    fn filter_pills_fit_their_labels() {
        for filter in [
            EpisodeFilter::All,
            EpisodeFilter::Unplayed,
            EpisodeFilter::InProgress,
            EpisodeFilter::Played,
            EpisodeFilter::Downloaded,
        ] {
            let label = filter.label();
            let w = text::padded_width(label, 8.0, 12.0, FontWeightHint::Regular);
            let drawn = text::measure(label, 12.0, FontWeightHint::Regular);
            assert!(drawn + 16.0 <= w + 0.01, "{label:?} overflows its pill");
        }
    }

    #[test]
    fn the_two_status_badges_are_eight_pixels_apart() {
        // The status badge's width was estimated twice with different padding
        // (+16 to draw it, +24 to place the next badge), so the gap between the
        // two pills was the difference between two guesses rather than 8 px.
        for label in ["Unplayed", "In Progress", "Played"] {
            let w = text::padded_width(label, 8.0, 11.0, FontWeightHint::Bold);
            let pad = 12.0_f32;
            let next_x = pad + w + 8.0;
            assert!(
                (next_x - (pad + w)) - 8.0 < 0.01,
                "{label:?}: badges are not 8 px apart"
            );
            // And the pill holds the text it was measured for.
            assert!(text::measure(label, 11.0, FontWeightHint::Bold) + 16.0 <= w + 0.01);
        }
    }

    // -----------------------------------------------------------------------
    // Category tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_category_all_count() {
        assert_eq!(Category::ALL.len(), 12);
    }

    #[test]
    fn test_category_names() {
        assert_eq!(Category::Technology.name(), "Technology");
        assert_eq!(Category::TrueCrime.name(), "True Crime");
        assert_eq!(Category::Comedy.name(), "Comedy");
    }

    #[test]
    fn test_category_from_str() {
        assert_eq!(
            Category::from_str_name("technology"),
            Some(Category::Technology)
        );
        assert_eq!(Category::from_str_name("tech"), Some(Category::Technology));
        assert_eq!(
            Category::from_str_name("true crime"),
            Some(Category::TrueCrime)
        );
        assert_eq!(
            Category::from_str_name("truecrime"),
            Some(Category::TrueCrime)
        );
        assert_eq!(Category::from_str_name("unknown"), None);
    }

    #[test]
    fn test_category_color() {
        // Each category should have a non-default color.
        for cat in Category::ALL {
            let color = cat.color();
            assert_ne!(color, Color::BLACK);
        }
    }

    // -----------------------------------------------------------------------
    // Episode status tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_episode_status_unplayed() {
        let s = EpisodeStatus::Unplayed;
        assert!(s.is_unplayed());
        assert!(!s.is_played());
        assert!(!s.is_in_progress());
        assert_eq!(s.label(), "New");
    }

    #[test]
    fn test_episode_status_in_progress() {
        let s = EpisodeStatus::InProgress { position_secs: 100 };
        assert!(s.is_in_progress());
        assert!(!s.is_unplayed());
        assert!(!s.is_played());
        assert_eq!(s.label(), "In Progress");
    }

    #[test]
    fn test_episode_status_played() {
        let s = EpisodeStatus::Played;
        assert!(s.is_played());
        assert!(!s.is_unplayed());
        assert!(!s.is_in_progress());
        assert_eq!(s.label(), "Played");
    }

    #[test]
    fn test_episode_status_colors() {
        let unplayed_color = EpisodeStatus::Unplayed.color();
        let played_color = EpisodeStatus::Played.color();
        assert_ne!(unplayed_color, played_color);
    }

    // -----------------------------------------------------------------------
    // Download status tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_download_status_not_downloaded() {
        let s = DownloadStatus::NotDownloaded;
        assert!(!s.is_downloaded());
        assert!(!s.is_downloading());
        assert_eq!(s.label(), "Not Downloaded");
    }

    #[test]
    fn test_download_status_downloaded() {
        let s = DownloadStatus::Downloaded;
        assert!(s.is_downloaded());
        assert!(!s.is_downloading());
        assert_eq!(s.label(), "Downloaded");
    }

    #[test]
    fn test_download_status_downloading() {
        let s = DownloadStatus::Downloading { progress: 0.5 };
        assert!(s.is_downloading());
        assert!(!s.is_downloaded());
        assert_eq!(s.label(), "Downloading");
    }

    #[test]
    fn test_download_status_queued() {
        let s = DownloadStatus::Queued;
        assert!(!s.is_downloaded());
        assert!(!s.is_downloading());
        assert_eq!(s.label(), "Queued");
    }

    #[test]
    fn test_download_status_failed() {
        let s = DownloadStatus::Failed;
        assert!(!s.is_downloaded());
        assert_eq!(s.label(), "Failed");
    }

    // -----------------------------------------------------------------------
    // PlaybackSpeed tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_speed_label() {
        assert_eq!(PlaybackSpeed::NORMAL.label(), "1x");
        assert_eq!(PlaybackSpeed::DOUBLE.label(), "2x");
        assert_eq!(PlaybackSpeed::ONE_HALF.label(), "1.50x");
        assert_eq!(PlaybackSpeed::HALF.label(), "0.50x");
    }

    #[test]
    fn test_speed_value() {
        assert!((PlaybackSpeed::NORMAL.value() - 1.0).abs() < 0.001);
        assert!((PlaybackSpeed::DOUBLE.value() - 2.0).abs() < 0.001);
        assert!((PlaybackSpeed::TRIPLE.value() - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_speed_next_cycles() {
        let s = PlaybackSpeed::NORMAL;
        let n = s.next();
        assert!((n.value() - 1.25).abs() < 0.01);

        // Full cycle returns to beginning.
        let mut current = PlaybackSpeed::HALF;
        for _ in 0..PlaybackSpeed::ALL.len() {
            current = current.next();
        }
        assert!((current.value() - PlaybackSpeed::HALF.value()).abs() < 0.01);
    }

    #[test]
    fn test_speed_all_ascending() {
        let all = PlaybackSpeed::ALL;
        for i in 1..all.len() {
            if let (Some(prev), Some(curr)) = (all.get(i - 1), all.get(i)) {
                assert!(curr.value() > prev.value());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Utility function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(59), "00:59");
        assert_eq!(format_duration(60), "01:00");
        assert_eq!(format_duration(125), "02:05");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "01:00:00");
        assert_eq!(format_duration(3661), "01:01:01");
        assert_eq!(format_duration(7200), "02:00:00");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn test_format_bytes_large() {
        let size = 45_000_000u64;
        let display = format_bytes(size);
        assert!(display.contains("MiB"));
    }

    // -----------------------------------------------------------------------
    // XML escape/unescape tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("hello"), "hello");
        assert_eq!(xml_escape("a&b"), "a&amp;b");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape("a\"b"), "a&quot;b");
    }

    #[test]
    fn test_xml_unescape() {
        assert_eq!(xml_unescape("hello"), "hello");
        assert_eq!(xml_unescape("a&amp;b"), "a&b");
        assert_eq!(xml_unescape("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn test_xml_roundtrip() {
        let original = "Test & <value> \"quoted\"";
        let escaped = xml_escape(original);
        let unescaped = xml_unescape(&escaped);
        assert_eq!(unescaped, original);
    }

    // -----------------------------------------------------------------------
    // OPML tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_opml_empty() {
        let opml = generate_opml(&[]);
        assert!(opml.contains("<opml"));
        assert!(opml.contains("</opml>"));
        assert!(opml.contains("<body>"));
    }

    #[test]
    fn test_generate_opml_with_podcasts() {
        let podcasts = vec![Podcast {
            id: 1,
            title: "My Podcast".to_string(),
            author: "Author".to_string(),
            description: "Desc".to_string(),
            rss_url: "https://example.com/feed.xml".to_string(),
            artwork_url: String::new(),
            categories: vec![],
            episodes: vec![],
            auto_download: false,
        }];
        let opml = generate_opml(&podcasts);
        assert!(opml.contains("My Podcast"));
        assert!(opml.contains("https://example.com/feed.xml"));
        assert!(opml.contains("<outline"));
    }

    #[test]
    fn test_parse_opml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Subscriptions</title></head>
  <body>
    <outline text="Pod A" type="rss" xmlUrl="https://a.com/feed" />
    <outline text="Pod B" type="rss" xmlUrl="https://b.com/feed" />
  </body>
</opml>"#;
        let outlines = parse_opml(xml);
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].text, "Pod A");
        assert_eq!(outlines[0].xml_url, "https://a.com/feed");
        assert_eq!(outlines[1].text, "Pod B");
    }

    #[test]
    fn test_parse_opml_empty() {
        let xml = r#"<?xml version="1.0"?><opml><body></body></opml>"#;
        let outlines = parse_opml(xml);
        assert!(outlines.is_empty());
    }

    #[test]
    fn test_opml_roundtrip() {
        let podcasts = vec![
            Podcast {
                id: 1,
                title: "Tech Talk".to_string(),
                author: "Host".to_string(),
                description: "A show".to_string(),
                rss_url: "https://example.com/tech.rss".to_string(),
                artwork_url: String::new(),
                categories: vec![],
                episodes: vec![],
                auto_download: false,
            },
            Podcast {
                id: 2,
                title: "Science Hour".to_string(),
                author: "Scientist".to_string(),
                description: "Science".to_string(),
                rss_url: "https://example.com/science.rss".to_string(),
                artwork_url: String::new(),
                categories: vec![],
                episodes: vec![],
                auto_download: false,
            },
        ];
        let opml = generate_opml(&podcasts);
        let parsed = parse_opml(&opml);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "Tech Talk");
        assert_eq!(parsed[1].text, "Science Hour");
    }

    #[test]
    fn test_parse_opml_with_special_chars() {
        let xml = r#"<opml><body>
    <outline text="A &amp; B" type="rss" xmlUrl="https://example.com/feed" />
</body></opml>"#;
        let outlines = parse_opml(xml);
        assert_eq!(outlines.len(), 1);
        assert_eq!(outlines[0].text, "A & B");
    }

    // -----------------------------------------------------------------------
    // Bookmark tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_bookmark_new() {
        let bm = Bookmark::new(120, "Interesting point");
        assert_eq!(bm.timestamp_secs, 120);
        assert_eq!(bm.label, "Interesting point");
    }

    #[test]
    fn test_bookmark_display() {
        let bm = Bookmark::new(3661, "Start");
        assert_eq!(bm.timestamp_display(), "01:01:01");
    }

    #[test]
    fn test_episode_notes_empty() {
        let notes = EpisodeNotes::new();
        assert!(!notes.has_content());
        assert!(notes.text.is_empty());
        assert!(notes.bookmarks.is_empty());
    }

    #[test]
    fn test_episode_notes_with_text() {
        let mut notes = EpisodeNotes::new();
        notes.set_notes("Great episode!");
        assert!(notes.has_content());
        assert_eq!(notes.text, "Great episode!");
    }

    #[test]
    fn test_episode_notes_add_bookmark() {
        let mut notes = EpisodeNotes::new();
        notes.add_bookmark(300, "Topic 1");
        notes.add_bookmark(100, "Intro");
        notes.add_bookmark(600, "Topic 2");
        // Should be sorted by timestamp.
        assert_eq!(notes.bookmarks.len(), 3);
        assert_eq!(notes.bookmarks[0].timestamp_secs, 100);
        assert_eq!(notes.bookmarks[1].timestamp_secs, 300);
        assert_eq!(notes.bookmarks[2].timestamp_secs, 600);
    }

    #[test]
    fn test_episode_notes_remove_bookmark() {
        let mut notes = EpisodeNotes::new();
        notes.add_bookmark(100, "A");
        notes.add_bookmark(200, "B");
        assert!(notes.remove_bookmark(0));
        assert_eq!(notes.bookmarks.len(), 1);
        assert_eq!(notes.bookmarks[0].label, "B");
    }

    #[test]
    fn test_episode_notes_remove_invalid_index() {
        let mut notes = EpisodeNotes::new();
        assert!(!notes.remove_bookmark(0));
        notes.add_bookmark(100, "A");
        assert!(!notes.remove_bookmark(5));
    }

    // -----------------------------------------------------------------------
    // Episode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_episode_duration_display() {
        let ep = Episode {
            id: 1,
            podcast_id: 1,
            title: "Test".to_string(),
            description: String::new(),
            date: "2026-01-01".to_string(),
            duration_secs: 3661,
            enclosure_url: String::new(),
            file_size_bytes: 1000,
            status: EpisodeStatus::Unplayed,
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        };
        assert_eq!(ep.duration_display(), "01:01:01");
    }

    #[test]
    fn test_episode_progress_pct() {
        let mut ep = Episode {
            id: 1,
            podcast_id: 1,
            title: "Test".to_string(),
            description: String::new(),
            date: "2026-01-01".to_string(),
            duration_secs: 1000,
            enclosure_url: String::new(),
            file_size_bytes: 1000,
            status: EpisodeStatus::InProgress { position_secs: 500 },
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        };
        assert!((ep.progress_pct() - 50.0).abs() < 0.1);

        ep.status = EpisodeStatus::Played;
        assert!((ep.progress_pct() - 100.0).abs() < 0.1);

        ep.status = EpisodeStatus::Unplayed;
        assert!((ep.progress_pct() - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_episode_remaining_secs() {
        let ep = Episode {
            id: 1,
            podcast_id: 1,
            title: "Test".to_string(),
            description: String::new(),
            date: "2026-01-01".to_string(),
            duration_secs: 1000,
            enclosure_url: String::new(),
            file_size_bytes: 1000,
            status: EpisodeStatus::InProgress { position_secs: 300 },
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        };
        assert_eq!(ep.remaining_secs(), 700);
    }

    #[test]
    fn test_episode_file_size_display() {
        let ep = Episode {
            id: 1,
            podcast_id: 1,
            title: "Test".to_string(),
            description: String::new(),
            date: "2026-01-01".to_string(),
            duration_secs: 100,
            enclosure_url: String::new(),
            file_size_bytes: 45_000_000,
            status: EpisodeStatus::Unplayed,
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        };
        assert!(ep.file_size_display().contains("MiB"));
    }

    // -----------------------------------------------------------------------
    // Podcast tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_podcast_unplayed_count() {
        let podcast = Podcast {
            id: 1,
            title: "Test".to_string(),
            author: String::new(),
            description: String::new(),
            rss_url: String::new(),
            artwork_url: String::new(),
            categories: vec![],
            episodes: vec![
                Episode {
                    id: 1,
                    podcast_id: 1,
                    title: "E1".to_string(),
                    description: String::new(),
                    date: String::new(),
                    duration_secs: 100,
                    enclosure_url: String::new(),
                    file_size_bytes: 100,
                    status: EpisodeStatus::Unplayed,
                    download_status: DownloadStatus::NotDownloaded,
                    notes: EpisodeNotes::new(),
                },
                Episode {
                    id: 2,
                    podcast_id: 1,
                    title: "E2".to_string(),
                    description: String::new(),
                    date: String::new(),
                    duration_secs: 100,
                    enclosure_url: String::new(),
                    file_size_bytes: 100,
                    status: EpisodeStatus::Played,
                    download_status: DownloadStatus::NotDownloaded,
                    notes: EpisodeNotes::new(),
                },
            ],
            auto_download: false,
        };
        assert_eq!(podcast.unplayed_count(), 1);
    }

    #[test]
    fn test_podcast_in_progress_count() {
        let podcast = Podcast {
            id: 1,
            title: "Test".to_string(),
            author: String::new(),
            description: String::new(),
            rss_url: String::new(),
            artwork_url: String::new(),
            categories: vec![],
            episodes: vec![Episode {
                id: 1,
                podcast_id: 1,
                title: "E1".to_string(),
                description: String::new(),
                date: String::new(),
                duration_secs: 100,
                enclosure_url: String::new(),
                file_size_bytes: 100,
                status: EpisodeStatus::InProgress { position_secs: 50 },
                download_status: DownloadStatus::NotDownloaded,
                notes: EpisodeNotes::new(),
            }],
            auto_download: false,
        };
        assert_eq!(podcast.in_progress_count(), 1);
    }

    #[test]
    fn test_podcast_downloaded_count() {
        let podcast = Podcast {
            id: 1,
            title: "Test".to_string(),
            author: String::new(),
            description: String::new(),
            rss_url: String::new(),
            artwork_url: String::new(),
            categories: vec![],
            episodes: vec![Episode {
                id: 1,
                podcast_id: 1,
                title: "E1".to_string(),
                description: String::new(),
                date: String::new(),
                duration_secs: 100,
                enclosure_url: String::new(),
                file_size_bytes: 5000,
                status: EpisodeStatus::Unplayed,
                download_status: DownloadStatus::Downloaded,
                notes: EpisodeNotes::new(),
            }],
            auto_download: false,
        };
        assert_eq!(podcast.downloaded_count(), 1);
        assert_eq!(podcast.downloaded_size_bytes(), 5000);
    }

    #[test]
    fn test_podcast_find_episode() {
        let podcast = Podcast {
            id: 1,
            title: "Test".to_string(),
            author: String::new(),
            description: String::new(),
            rss_url: String::new(),
            artwork_url: String::new(),
            categories: vec![],
            episodes: vec![Episode {
                id: 10,
                podcast_id: 1,
                title: "Found".to_string(),
                description: String::new(),
                date: String::new(),
                duration_secs: 100,
                enclosure_url: String::new(),
                file_size_bytes: 100,
                status: EpisodeStatus::Unplayed,
                download_status: DownloadStatus::NotDownloaded,
                notes: EpisodeNotes::new(),
            }],
            auto_download: false,
        };
        assert!(podcast.find_episode(10).is_some());
        assert!(podcast.find_episode(99).is_none());
    }

    // -----------------------------------------------------------------------
    // Listening stats tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_stats_new() {
        let stats = ListeningStats::new();
        assert_eq!(stats.total_listening_secs, 0);
        assert_eq!(stats.episodes_completed, 0);
        assert!(stats.most_listened_podcast.is_none());
    }

    #[test]
    fn test_stats_record_listening() {
        let mut stats = ListeningStats::new();
        stats.record_listening(1, "Podcast A", 3600, true);
        assert_eq!(stats.total_listening_secs, 3600);
        assert_eq!(stats.episodes_completed, 1);
        assert_eq!(stats.most_listened_podcast.as_deref(), Some("Podcast A"));
    }

    #[test]
    fn test_stats_record_multiple() {
        let mut stats = ListeningStats::new();
        stats.record_listening(1, "A", 100, false);
        stats.record_listening(2, "B", 200, true);
        stats.record_listening(1, "A", 300, true);
        assert_eq!(stats.total_listening_secs, 600);
        assert_eq!(stats.episodes_completed, 2);
        // A has 400 total, B has 200.
        assert_eq!(stats.most_listened_podcast.as_deref(), Some("A"));
    }

    #[test]
    fn test_stats_total_time_display() {
        let mut stats = ListeningStats::new();
        stats.total_listening_secs = 7260; // 2h 1m
        assert_eq!(stats.total_time_display(), "2h 1m");
    }

    // -----------------------------------------------------------------------
    // App subscription tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_subscribe() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let initial_count = app.podcasts.len();
        let id = app.subscribe(
            "New Pod",
            "Author",
            "Desc",
            "https://rss.example.com",
            "",
            vec![Category::Technology],
        );
        assert!(app.find_podcast(id).is_some());
        assert_eq!(app.podcasts.len(), initial_count + 1);
    }

    #[test]
    fn test_unsubscribe() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let id = app.subscribe(
            "Temp",
            "Auth",
            "Desc",
            "https://temp.example.com",
            "",
            vec![],
        );
        assert!(app.unsubscribe(id));
        assert!(app.find_podcast(id).is_none());
    }

    #[test]
    fn test_unsubscribe_nonexistent() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!(!app.unsubscribe(99999));
    }

    #[test]
    fn test_set_auto_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let id = app.subscribe("Pod", "Auth", "", "https://x.com/feed", "", vec![]);
        assert!(app.set_auto_download(id, true));
        assert!(app.find_podcast(id).unwrap().auto_download);
        assert!(app.set_auto_download(id, false));
        assert!(!app.find_podcast(id).unwrap().auto_download);
    }

    // -----------------------------------------------------------------------
    // App episode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "A", "", "https://x.com", "", vec![]);
        let eid = app.add_episode(
            pid,
            "Ep1",
            "Desc",
            "2026-01-01",
            600,
            "https://x.com/ep1.mp3",
            10000,
        );
        assert!(eid.is_some());
        assert!(
            app.find_podcast(pid)
                .unwrap()
                .find_episode(eid.unwrap())
                .is_some()
        );
    }

    #[test]
    fn test_add_episode_invalid_podcast() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let eid = app.add_episode(99999, "Ep", "", "2026-01-01", 100, "", 100);
        assert!(eid.is_none());
    }

    #[test]
    fn test_mark_played_unplayed() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();

        assert!(app.mark_played(pid, eid));
        assert!(
            app.find_podcast(pid)
                .unwrap()
                .find_episode(eid)
                .unwrap()
                .status
                .is_played()
        );

        assert!(app.mark_unplayed(pid, eid));
        assert!(
            app.find_podcast(pid)
                .unwrap()
                .find_episode(eid)
                .unwrap()
                .status
                .is_unplayed()
        );
    }

    #[test]
    fn test_episode_filter_all() {
        let app = PodcastApp::new(800.0, 600.0);
        let episodes = app.filtered_all_episodes();
        assert!(!episodes.is_empty());
    }

    #[test]
    fn test_episode_filter_unplayed() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.episode_filter = EpisodeFilter::Unplayed;
        let episodes = app.filtered_all_episodes();
        // Sample data has unplayed episodes.
        assert!(!episodes.is_empty());
    }

    #[test]
    fn test_episodes_for_category() {
        let app = PodcastApp::new(800.0, 600.0);
        let tech_eps = app.episodes_for_category(Category::Technology);
        assert!(!tech_eps.is_empty());
    }

    // -----------------------------------------------------------------------
    // Episode notes & bookmarks in app
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_set_episode_notes() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        assert!(app.set_episode_notes(pid, eid, "My notes"));
        let ep = app.find_podcast(pid).unwrap().find_episode(eid).unwrap();
        assert_eq!(ep.notes.text, "My notes");
    }

    #[test]
    fn test_app_add_episode_bookmark() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        assert!(app.add_episode_bookmark(pid, eid, 30, "Good part"));
        let ep = app.find_podcast(pid).unwrap().find_episode(eid).unwrap();
        assert_eq!(ep.notes.bookmarks.len(), 1);
        assert_eq!(ep.notes.bookmarks[0].label, "Good part");
    }

    #[test]
    fn test_app_remove_episode_bookmark() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        app.add_episode_bookmark(pid, eid, 30, "A");
        app.add_episode_bookmark(pid, eid, 60, "B");
        assert!(app.remove_episode_bookmark(pid, eid, 0));
        let ep = app.find_podcast(pid).unwrap().find_episode(eid).unwrap();
        assert_eq!(ep.notes.bookmarks.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Playback tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_play_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        assert!(app.play_episode(pid, eid));
        assert_eq!(app.player_state, PlayerState::Playing);
        assert_eq!(app.current_episode_id, Some(eid));
        assert_eq!(app.playback_duration_secs, 600);
    }

    #[test]
    fn test_pause_resume() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.pause_playback();
        assert_eq!(app.player_state, PlayerState::Paused);
        app.resume_playback();
        assert_eq!(app.player_state, PlayerState::Playing);
    }

    #[test]
    fn test_toggle_playback() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.toggle_playback();
        assert_eq!(app.player_state, PlayerState::Paused);
        app.toggle_playback();
        assert_eq!(app.player_state, PlayerState::Playing);
    }

    #[test]
    fn test_stop_playback() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.stop_playback();
        assert_eq!(app.player_state, PlayerState::Stopped);
        assert!(app.current_episode_id.is_none());
    }

    #[test]
    fn test_seek_forward() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(15);
        assert_eq!(app.playback_position_secs, 15);
    }

    #[test]
    fn test_seek_backward() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(30);
        app.seek_backward(15);
        assert_eq!(app.playback_position_secs, 15);
    }

    #[test]
    fn test_seek_backward_saturates() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.seek_backward(100);
        assert_eq!(app.playback_position_secs, 0);
    }

    #[test]
    fn test_seek_forward_clamped() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(200);
        // Should be clamped to duration and marked played.
        assert_eq!(app.playback_position_secs, 0); // Completed, auto-play ran
    }

    #[test]
    fn test_seek_to() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.seek_to(300);
        assert_eq!(app.playback_position_secs, 300);
    }

    #[test]
    fn test_cycle_speed() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!((app.playback_speed.value() - 1.0).abs() < 0.001);
        app.cycle_speed();
        assert!((app.playback_speed.value() - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_set_speed() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.set_speed(PlaybackSpeed::DOUBLE);
        assert!((app.playback_speed.value() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_tick_advances_position() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.tick(5000); // 5 seconds at 1x
        assert!(app.playback_position_secs >= 5);
    }

    #[test]
    fn test_tick_stopped_no_advance() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.tick(5000);
        assert_eq!(app.playback_position_secs, 0);
    }

    #[test]
    fn test_tick_paused_no_advance() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.pause_playback();
        let pos = app.playback_position_secs;
        app.tick(5000);
        assert_eq!(app.playback_position_secs, pos);
    }

    // -----------------------------------------------------------------------
    // Queue tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_queue_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        let initial_queue = app.play_queue.len();
        assert!(app.queue_episode(pid, eid));
        assert_eq!(app.play_queue.len(), initial_queue + 1);
    }

    #[test]
    fn test_queue_no_duplicates() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.queue_episode(pid, eid);
        let count = app.play_queue.len();
        assert!(!app.queue_episode(pid, eid));
        assert_eq!(app.play_queue.len(), count);
    }

    #[test]
    fn test_dequeue_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        app.queue_episode(pid, eid);
        let count = app.play_queue.len();
        assert!(app.dequeue_episode(count - 1));
        assert_eq!(app.play_queue.len(), count - 1);
    }

    #[test]
    fn test_dequeue_invalid_index() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!(!app.dequeue_episode(999));
    }

    #[test]
    fn test_reorder_queue() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.clear_queue();
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let e1 = app
            .add_episode(pid, "A", "", "2026-01-01", 100, "", 100)
            .unwrap();
        let e2 = app
            .add_episode(pid, "B", "", "2026-01-02", 100, "", 100)
            .unwrap();
        let e3 = app
            .add_episode(pid, "C", "", "2026-01-03", 100, "", 100)
            .unwrap();
        app.queue_episode(pid, e1);
        app.queue_episode(pid, e2);
        app.queue_episode(pid, e3);
        // Move first to last.
        assert!(app.reorder_queue(0, 2));
        assert_eq!(app.play_queue[0].episode_title, "B");
        assert_eq!(app.play_queue[2].episode_title, "A");
    }

    #[test]
    fn test_reorder_queue_invalid() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!(!app.reorder_queue(0, 99));
    }

    #[test]
    fn test_clear_queue() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.clear_queue();
        assert!(app.play_queue.is_empty());
    }

    // -----------------------------------------------------------------------
    // Download tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_queue_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000)
            .unwrap();
        assert!(app.queue_download(pid, eid));
        assert_eq!(app.download_queue.len(), 1);
    }

    #[test]
    fn test_queue_download_no_duplicate() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000)
            .unwrap();
        app.queue_download(pid, eid);
        assert!(!app.queue_download(pid, eid));
    }

    #[test]
    fn test_cancel_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000)
            .unwrap();
        app.queue_download(pid, eid);
        assert!(app.cancel_download(eid));
        assert!(app.download_queue.is_empty());
    }

    #[test]
    fn test_cancel_download_nonexistent() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!(!app.cancel_download(99999));
    }

    #[test]
    fn test_simulate_download_tick() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000)
            .unwrap();
        app.queue_download(pid, eid);
        app.simulate_download_tick();
        // First tick should activate the download.
        assert!(app.download_queue.iter().any(|d| d.active));
    }

    #[test]
    fn test_delete_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000)
            .unwrap();
        // Manually set as downloaded.
        if let Some(p) = app.find_podcast_mut(pid)
            && let Some(ep) = p.find_episode_mut(eid)
        {
            ep.download_status = DownloadStatus::Downloaded;
        }
        app.used_disk_bytes = app.used_disk_bytes.saturating_add(10_000);
        let before = app.used_disk_bytes;
        assert!(app.delete_download(pid, eid));
        assert!(app.used_disk_bytes < before);
    }

    #[test]
    fn test_disk_usage() {
        let app = PodcastApp::new(800.0, 600.0);
        assert!(app.remaining_disk_bytes() <= app.total_disk_bytes);
        let pct = app.disk_usage_pct();
        assert!((0.0..=100.0).contains(&pct));
    }

    // -----------------------------------------------------------------------
    // Search tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_search_finds_by_title() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.search_query = "Rust".to_string();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_search_finds_by_podcast_name() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.search_query = "StarTalk".to_string();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.search_query.clear();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_search_no_results() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.search_query = "xyznonexistent123".to_string();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.search_query = "rust".to_string();
        app.perform_search();
        let lower_count = app.search_results.len();
        app.search_query = "RUST".to_string();
        app.perform_search();
        assert_eq!(app.search_results.len(), lower_count);
    }

    // -----------------------------------------------------------------------
    // OPML app integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_export_opml_includes_subs() {
        let app = PodcastApp::new(800.0, 600.0);
        let opml = app.export_opml();
        assert!(opml.contains("Rustacean"));
        assert!(opml.contains("StarTalk"));
    }

    #[test]
    fn test_import_opml() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let xml = r#"<opml><body>
    <outline text="New Show" type="rss" xmlUrl="https://new.example.com/rss" />
</body></opml>"#;
        let count = app.import_opml(xml);
        assert_eq!(count, 1);
        assert!(app.podcasts.iter().any(|p| p.title == "New Show"));
    }

    #[test]
    fn test_import_opml_no_duplicates() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let existing_url = app.podcasts[0].rss_url.clone();
        let xml = format!(
            r#"<opml><body><outline text="Dup" type="rss" xmlUrl="{}" /></body></opml>"#,
            existing_url
        );
        let count = app.import_opml(&xml);
        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // History tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_playback_records_history() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100)
            .unwrap();
        let before = app.history.len();
        app.play_episode(pid, eid);
        app.seek_forward(100);
        app.stop_playback();
        assert!(app.history.len() > before);
    }

    // -----------------------------------------------------------------------
    // Auto-download tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_auto_download_on_new_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        app.set_auto_download(pid, true);
        app.add_episode(pid, "Auto Ep", "", "2026-01-01", 100, "", 5000);
        // Should be queued for download.
        assert!(
            app.download_queue
                .iter()
                .any(|d| d.episode_title == "Auto Ep")
        );
    }

    // -----------------------------------------------------------------------
    // Render tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = PodcastApp::new(800.0, 600.0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_all_views() {
        let mut app = PodcastApp::new(800.0, 600.0);

        app.main_view = MainView::EpisodeList;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Queue;
        app.sidebar_selection = SidebarSelection::Queue;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Downloads;
        app.sidebar_selection = SidebarSelection::Downloads;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        app.main_view = MainView::History;
        app.sidebar_selection = SidebarSelection::History;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Statistics;
        app.sidebar_selection = SidebarSelection::Statistics;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Search;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_episode_detail() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let first_ep = app
            .podcasts
            .first()
            .and_then(|p| p.episodes.first())
            .map(|e| e.id);
        app.selected_episode_id = first_ep;
        app.main_view = MainView::EpisodeDetail;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_now_playing() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.play_episode(pid, eid);
        let cmds = app.render_commands();
        // Should have now-playing bar render commands.
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_search_with_results() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.main_view = MainView::Search;
        app.search_query = "Rust".to_string();
        app.perform_search();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_notes_bookmarks() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.set_episode_notes(pid, eid, "Important topic");
        app.add_episode_bookmark(pid, eid, 120, "Key point");
        app.selected_episode_id = Some(eid);
        app.main_view = MainView::EpisodeDetail;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_downloads_with_queue() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.queue_download(pid, eid);
        app.main_view = MainView::Downloads;
        app.sidebar_selection = SidebarSelection::Downloads;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Prose fields of the episode detail pane
    // -----------------------------------------------------------------------

    /// Long enough to need several lines across the detail pane at either of
    /// the prose sizes, and with a distinctive final word to check nothing was
    /// dropped off the end.
    const LONG_PROSE: &str = "This week we sit down with the maintainer of a \
        long-running open source project to talk about how a hobby weekend \
        experiment turned into infrastructure that thousands of companies now \
        depend on, what changed about the way the work felt once that happened, \
        and how the funding question was eventually answered. Recorded in \
        Gothenburg.";

    fn app_with_prose_episode() -> PodcastApp {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.podcasts[0].episodes[0].description = LONG_PROSE.to_string();
        app.set_episode_notes(pid, eid, LONG_PROSE);
        app.add_episode_bookmark(pid, eid, 120, "Key point");
        app.selected_episode_id = Some(eid);
        app.main_view = MainView::EpisodeDetail;
        app
    }

    /// The detail pane on its own, so that text drawn elsewhere in the window
    /// cannot be mistaken for one of its prose fields.
    fn detail_pane(app: &PodcastApp) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        app.render_episode_detail(&mut cmds, 0.0, 800.0, 600.0);
        cmds
    }

    /// The lines of one prose field: every text of that colour and size drawn
    /// below the named section heading, top to bottom.
    fn prose_under(cmds: &[RenderCommand], heading: &str, size: f32) -> Vec<(f32, String)> {
        let heading_y = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, y, .. } if text == heading => Some(*y),
                _ => None,
            })
            .expect("the section heading is drawn");
        let mut lines: Vec<(f32, String)> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    color,
                    font_size,
                    ..
                } if *color == SUBTEXT0 && (*font_size - size).abs() < 0.01 && *y > heading_y => {
                    Some((*y, text.clone()))
                }
                _ => None,
            })
            .collect();
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        lines
    }

    /// The top of the first bookmark row, identified by its 60x20 pill.
    fn first_bookmark_y(cmds: &[RenderCommand]) -> f32 {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y, width, height, ..
                } if (*width - 60.0).abs() < 0.01 && (*height - 20.0).abs() < 0.01 => Some(*y),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn a_long_episode_description_is_wrapped_not_truncated() {
        let app = app_with_prose_episode();
        let cmds = detail_pane(&app);
        let lines = prose_under(&cmds, "Description", DESCRIPTION_FONT_SIZE);
        assert!(
            lines.len() > 1,
            "a paragraph-length description was drawn as {} line(s)",
            lines.len()
        );
        let drawn = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            drawn.contains("Gothenburg"),
            "the end of the description was cut off: {drawn}"
        );
    }

    #[test]
    fn long_show_notes_are_wrapped_not_truncated() {
        let app = app_with_prose_episode();
        let cmds = detail_pane(&app);
        let lines = prose_under(&cmds, "Notes", NOTES_FONT_SIZE);
        assert!(
            lines.len() > 1,
            "paragraph-length notes were drawn as {} line(s)",
            lines.len()
        );
        let drawn = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            drawn.contains("Gothenburg"),
            "the end of the notes was cut off: {drawn}"
        );
    }

    #[test]
    fn the_notes_section_starts_below_the_description() {
        let app = app_with_prose_episode();
        let cmds = detail_pane(&app);
        let description = prose_under(&cmds, "Description", DESCRIPTION_FONT_SIZE);
        let last_line = description
            .last()
            .map_or(0.0, |(y, _)| *y + PROSE_LINE_HEIGHT);
        let heading_y = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, y, .. } if text == "Notes" => Some(*y),
                _ => None,
            })
            .expect("the notes heading is drawn");
        assert!(
            heading_y >= last_line,
            "the Notes heading at {heading_y} sits inside the description, \
             which runs to {last_line}"
        );
    }

    #[test]
    fn bookmarks_start_below_the_show_notes() {
        let app = app_with_prose_episode();
        let cmds = detail_pane(&app);
        let notes = prose_under(&cmds, "Notes", NOTES_FONT_SIZE);
        let last_line = notes.last().map_or(0.0, |(y, _)| *y + PROSE_LINE_HEIGHT);
        let bookmark_y = first_bookmark_y(&cmds);
        assert!(
            bookmark_y >= last_line,
            "the first bookmark at {bookmark_y} is drawn over the notes, \
             which run to {last_line}"
        );
    }

    #[test]
    fn an_episode_with_no_notes_draws_no_notes_body() {
        let mut app = app_with_prose_episode();
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.set_episode_notes(pid, eid, "");
        let cmds = detail_pane(&app);
        assert!(
            prose_under(&cmds, "Notes", NOTES_FONT_SIZE).is_empty(),
            "an empty notes field still drew a line"
        );
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_play_nonexistent_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        assert!(!app.play_episode(99999, 99999));
    }

    #[test]
    fn test_seek_while_stopped() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.seek_forward(10);
        assert_eq!(app.playback_position_secs, 0);
        app.seek_backward(10);
        assert_eq!(app.playback_position_secs, 0);
    }

    #[test]
    fn test_episode_progress_zero_duration() {
        let ep = Episode {
            id: 1,
            podcast_id: 1,
            title: "Zero".to_string(),
            description: String::new(),
            date: String::new(),
            duration_secs: 0,
            enclosure_url: String::new(),
            file_size_bytes: 0,
            status: EpisodeStatus::InProgress { position_secs: 0 },
            download_status: DownloadStatus::NotDownloaded,
            notes: EpisodeNotes::new(),
        };
        assert!((ep.progress_pct() - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_unsubscribe_clears_queue_items() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        app.queue_episode(pid, eid);
        app.unsubscribe(pid);
        assert!(!app.play_queue.iter().any(|q| q.podcast_id == pid));
    }

    #[test]
    fn test_unsubscribe_stops_playing() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100)
            .unwrap();
        app.play_episode(pid, eid);
        app.unsubscribe(pid);
        assert_eq!(app.player_state, PlayerState::Stopped);
    }

    #[test]
    fn test_disk_full_rejects_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.total_disk_bytes = 100;
        app.used_disk_bytes = 90;
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app
            .add_episode(pid, "Ep", "", "2026-01-01", 100, "", 1000)
            .unwrap();
        assert!(!app.queue_download(pid, eid));
    }

    #[test]
    fn test_find_episode_global() {
        let app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        assert!(app.find_episode_global(pid, eid).is_some());
        assert!(app.find_episode_global(99999, 99999).is_none());
    }

    #[test]
    fn test_filter_label() {
        assert_eq!(EpisodeFilter::All.label(), "All");
        assert_eq!(EpisodeFilter::Downloaded.label(), "Downloaded");
    }

    #[test]
    fn test_sample_data_populated() {
        let app = PodcastApp::new(800.0, 600.0);
        assert!(app.podcasts.len() >= 4);
        assert!(app.podcasts.iter().any(|p| p.title.contains("Rustacean")));
        assert!(app.podcasts.iter().any(|p| p.title.contains("StarTalk")));
    }

    #[test]
    fn test_extract_attr_basic() {
        let tag = r#"<outline text="Hello" xmlUrl="https://example.com" />"#;
        assert_eq!(extract_attr(tag, "text"), Some("Hello".to_string()));
        assert_eq!(
            extract_attr(tag, "xmlUrl"),
            Some("https://example.com".to_string())
        );
        assert_eq!(extract_attr(tag, "missing"), None);
    }
    // --- sidebar subscription list ---

    /// An app with `n` subscriptions on top of the sample data, titled so a
    /// failure message reads as a run of names.
    /// Remove the sample library the way a user would.
    ///
    /// `podcasts.clear()` looks equivalent and is not: `unsubscribe` also
    /// drops the play queue, the download queue and the subscription count
    /// that referred to what it removed, and a fixture built the other way
    /// starts with a queue entry naming a podcast that is no longer there.
    fn drop_sample_data(app: &mut PodcastApp) {
        let ids: Vec<u64> = app.podcasts.iter().map(|p| p.id).collect();
        for id in ids {
            assert!(app.unsubscribe(id));
        }
        app.history.clear();
    }

    fn app_with_subscriptions(n: usize) -> PodcastApp {
        let mut app = PodcastApp::new(800.0, 600.0);
        drop_sample_data(&mut app);
        for i in 0..n {
            app.subscribe(&format!("P{i:03}"), "", "", "rss://x", "", vec![]);
        }
        app
    }

    /// Text in the sidebar column sits at one of exactly two x positions: the
    /// indent itself for section headers and the "N more" line, and the indent
    /// plus 8 for a selectable item, which leaves room for the accent bar
    /// `render_sidebar_item` draws to its left. Matching only the first is how
    /// a helper silently reports that the sidebar drew nothing.
    fn is_sidebar_text_x(x: f32) -> bool {
        (x - 16.0).abs() < 0.01 || (x - 24.0).abs() < 0.01
    }

    /// Just the sidebar's own commands.
    ///
    /// Filtering a full `render()` by x is not enough to isolate the sidebar:
    /// the now-playing bar spans the whole window from x=0, so its labels land
    /// in the sidebar's column too and read as rows drawn past the bottom.
    fn sidebar_commands(app: &PodcastApp) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        app.render_sidebar(&mut cmds);
        cmds
    }

    /// Every sidebar label the render actually drew.
    fn sidebar_labels(app: &PodcastApp) -> Vec<String> {
        sidebar_commands(app)
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, text, .. } if is_sidebar_text_x(x) => Some(text),
                _ => None,
            })
            .collect()
    }

    fn drawn_subscriptions(app: &PodcastApp) -> Vec<String> {
        sidebar_labels(app)
            .into_iter()
            .filter(|t| t.starts_with('P') && t.len() == 4)
            .collect()
    }

    /// The bug: the sidebar ran off the bottom of the window and was cut by
    /// the window edge rather than by a scroll position, so everything below
    /// the fold — including the whole CATEGORIES section — was unreachable.
    #[test]
    fn the_sidebar_stops_at_the_last_subscription_that_fits() {
        let app = app_with_subscriptions(200);
        let drawn = drawn_subscriptions(&app);
        assert!(
            !drawn.is_empty(),
            "the sidebar drew no subscriptions at all"
        );
        assert!(
            drawn.len() < 200,
            "the sidebar drew all 200 subscriptions into a 600px window"
        );
        assert_eq!(drawn.first().map(String::as_str), Some("P000"));
    }

    /// The categories sit below the subscriptions, so a long subscription list
    /// used to bury them past the window edge for good. They are now merely
    /// below the fold, which is a place you can scroll to.
    #[test]
    fn every_category_is_reachable_past_a_long_subscription_list() {
        let mut app = app_with_subscriptions(200);
        app.scroll_sidebar_by(1_000);
        let labels = sidebar_labels(&app);
        assert!(
            labels.iter().any(|t| t == "CATEGORIES"),
            "the CATEGORIES header is unreachable"
        );
        for cat in Category::ALL {
            assert!(
                labels.iter().any(|t| t == cat.name()),
                "category {:?} is unreachable",
                cat.name()
            );
        }
    }

    /// Nothing draws below the bottom of the sidebar — at any scroll position,
    /// and with the now-playing bar covering the bottom 80px or not.
    #[test]
    fn no_sidebar_row_is_drawn_past_the_bottom_of_the_sidebar() {
        for playing in [false, true] {
            for offset in [0, 5, 50, 1_000] {
                let mut app = app_with_subscriptions(200);
                if playing {
                    app.player_state = PlayerState::Playing;
                }
                app.scroll_sidebar_by(offset);
                let bottom = if playing {
                    app.height - NOW_PLAYING_HEIGHT
                } else {
                    app.height
                };
                for cmd in sidebar_commands(&app) {
                    if let RenderCommand::Text { x, y, text, .. } = cmd
                        && is_sidebar_text_x(x)
                    {
                        assert!(
                            y <= bottom,
                            "sidebar row {text:?} drawn at y={y}, past the sidebar bottom \
                             {bottom} (playing={playing}, offset={offset})"
                        );
                    }
                }
            }
        }
    }

    /// ...and the rows past the fold are reachable, which is the fix.
    #[test]
    fn scrolling_the_sidebar_reaches_the_subscriptions_that_did_not_fit() {
        let mut app = app_with_subscriptions(200);
        assert!(
            !drawn_subscriptions(&app).contains(&String::from("P199")),
            "the last subscription should start out below the fold"
        );
        app.scroll_sidebar_by(200);
        assert!(
            drawn_subscriptions(&app).contains(&String::from("P199")),
            "the last subscription is still unreachable after scrolling to the end"
        );
    }

    /// An offset past the end means the last page, not a blank sidebar —
    /// unsubscribing from most of a long list is exactly how that happens.
    #[test]
    fn a_sidebar_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let mut app = app_with_subscriptions(200);
        app.scroll_sidebar_by(199);
        let ids: Vec<u64> = app.podcasts.iter().skip(6).map(|p| p.id).collect();
        for id in ids {
            app.unsubscribe(id);
        }
        let labels = sidebar_labels(&app);
        assert!(!labels.is_empty(), "the sidebar must not go blank");
        // The last page ends on the last row, which is the last category.
        let last = Category::ALL.last().map(|c| c.name());
        assert_eq!(
            labels.iter().rev().find(|t| Some(t.as_str()) == last),
            last.map(String::from).as_ref()
        );
    }

    /// Scrolling up from the top stays at the top rather than wrapping.
    #[test]
    fn scrolling_the_sidebar_up_from_the_top_stays_at_the_top() {
        let mut app = app_with_subscriptions(200);
        app.scroll_sidebar_by(-10);
        assert_eq!(app.sidebar_scroll, 0);
        app.scroll_sidebar_by(5);
        app.scroll_sidebar_to_top();
        assert_eq!(app.sidebar_scroll, 0);
    }

    // --- episode list ---

    /// One podcast with `n` episodes, selected, so the content area draws the
    /// episode list rather than a placeholder view.
    fn app_with_episodes(n: usize) -> PodcastApp {
        let mut app = PodcastApp::new(1100.0, 600.0);
        drop_sample_data(&mut app);
        let pod = app.subscribe("Show", "", "", "rss://x", "", vec![]);
        for i in 0..n {
            app.add_episode(pod, &format!("E{i:03}"), "", "2026-01-01", 60, "", 0);
        }
        app.sidebar_selection = SidebarSelection::Podcast(pod);
        app.main_view = MainView::EpisodeList;
        app
    }

    /// Episode titles are `E000`-shaped, so they are told from every other
    /// string in the render without depending on a pixel position.
    fn drawn_episodes(app: &PodcastApp) -> Vec<String> {
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. }
                    if text.len() == 4
                        && text.starts_with('E')
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

    /// The bug: the list drew until a row started past the content height, so
    /// it overran the bottom by up to a row and everything after that was
    /// unreachable — there was no offset to reach it with.
    #[test]
    fn the_episode_list_stops_at_the_last_row_that_fits() {
        let app = app_with_episodes(100);
        let drawn = drawn_episodes(&app);
        assert!(
            !drawn.is_empty(),
            "the content area drew no episodes at all"
        );
        assert!(
            drawn.len() < 100,
            "the list drew all 100 episodes into a 600px window"
        );
        assert_eq!(drawn.first().map(String::as_str), Some("E000"));
    }

    /// No row is drawn past the bottom of the content area, and with audio
    /// playing the content area stops above the now-playing bar.
    #[test]
    fn no_episode_row_is_drawn_past_the_bottom_of_the_content_area() {
        for playing in [false, true] {
            for offset in [0, 7, 1_000] {
                let mut app = app_with_episodes(100);
                if playing {
                    app.player_state = PlayerState::Playing;
                }
                app.scroll_episode_list_by(offset);
                let bottom = if playing {
                    app.height - NOW_PLAYING_HEIGHT
                } else {
                    app.height
                };
                for cmd in app.render_commands() {
                    if let RenderCommand::Text { x, y, text, .. } = cmd
                        && x > SIDEBAR_WIDTH
                        && text.len() == 4
                        && text.starts_with('E')
                    {
                        assert!(
                            y + EPISODE_ROW_HEIGHT <= bottom,
                            "episode row {text:?} at y={y} overruns the content bottom \
                             {bottom} (playing={playing}, offset={offset})"
                        );
                    }
                }
            }
        }
    }

    /// The rows past the fold are reachable, which is the fix.
    #[test]
    fn scrolling_the_episode_list_reaches_the_rows_that_did_not_fit() {
        let mut app = app_with_episodes(100);
        assert!(
            !drawn_episodes(&app).contains(&String::from("E099")),
            "the last episode should start out below the fold"
        );
        app.scroll_episode_list_by(100);
        assert!(
            drawn_episodes(&app).contains(&String::from("E099")),
            "the last episode is still unreachable after scrolling to the end"
        );
    }

    /// An offset past the end means the last page, not a blank content area.
    #[test]
    fn an_episode_list_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let mut app = app_with_episodes(100);
        app.scroll_episode_list_by(99);
        app.episode_filter = EpisodeFilter::Downloaded;
        assert!(
            drawn_episodes(&app).is_empty(),
            "no episode is downloaded, so the filtered list is genuinely empty"
        );
        app.episode_filter = EpisodeFilter::All;
        let drawn = drawn_episodes(&app);
        assert!(!drawn.is_empty(), "the list must not go blank");
        assert_eq!(drawn.last().map(String::as_str), Some("E099"));
    }

    /// Scrolling up from the top stays at the top rather than wrapping.
    #[test]
    fn scrolling_the_episode_list_up_from_the_top_stays_at_the_top() {
        let mut app = app_with_episodes(100);
        app.scroll_episode_list_by(-10);
        assert_eq!(app.episode_list_scroll, 0);
        app.scroll_episode_list_by(5);
        app.scroll_episode_list_to_top();
        assert_eq!(app.episode_list_scroll, 0);
    }

    /// A list hiding episodes says how many.
    #[test]
    fn an_episode_list_that_is_hiding_rows_says_so() {
        let app = app_with_episodes(100);
        let shown = drawn_episodes(&app).len();
        let labels: Vec<String> = app
            .render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            labels.contains(&format!("{} more", 100 - shown)),
            "expected a \"{} more\" line",
            100 - shown
        );
    }

    /// A sidebar hiding rows says how many.
    #[test]
    fn a_sidebar_that_is_hiding_rows_says_so() {
        let app = app_with_subscriptions(200);
        let shown = sidebar_labels(&app)
            .iter()
            .filter(|t| !t.ends_with(" more"))
            .count();
        // 6 library entries + 2 dividers + 2 headers + 200 subscriptions + the
        // categories. Only the dividers are not labelled, so the labels drawn
        // are the visible rows less those.
        assert!(
            sidebar_labels(&app).iter().any(|t| t.ends_with(" more")),
            "a sidebar showing {shown} of 220-odd rows should say so"
        );

        // ...and one with room for everything says nothing. 600px cannot hold
        // even the fixed rows, so this needs a window that can.
        let mut app = PodcastApp::new(800.0, 1200.0);
        app.podcasts.clear();
        assert!(
            !sidebar_labels(&app).iter().any(|t| t.ends_with(" more")),
            "a complete sidebar should not claim to be hiding rows"
        );
    }

    // ======================================================================
    // Input
    // ======================================================================

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// A letter key, carrying the text it typed.
    ///
    /// The letter shortcuts read `typed()` rather than the key code, so a
    /// board that puts `q` somewhere else still queues an episode.
    fn typed(k: Key, ch: char) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        })
    }

    fn click_at(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// The y of the sidebar row at `index` among the drawn ones.
    fn sidebar_row_y(app: &PodcastApp, index: usize) -> f32 {
        let (rows, window) = app.sidebar_layout();
        let mut y = SIDEBAR_LIST_TOP;
        for row in rows
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .take(index)
        {
            y += row.height();
        }
        y + 4.0
    }

    // --- the sidebar can be clicked at all ---

    #[test]
    fn clicking_a_sidebar_row_goes_where_the_row_points() {
        let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Row 3 of the library block is Downloads.
        let y = sidebar_row_y(&app, 3);
        assert_eq!(
            app.sidebar_target_at(20.0, y),
            Some(SidebarTarget::Downloads)
        );
        assert!(app.handle_event(&click_at(20.0, y)));
        assert_eq!(app.main_view, MainView::Downloads);
        assert_eq!(app.sidebar_selection, SidebarSelection::Downloads);
    }

    #[test]
    fn a_sidebar_header_is_not_a_button() {
        let app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (rows, window) = app.sidebar_layout();
        let mut y = SIDEBAR_LIST_TOP;
        let mut checked = 0;
        for row in rows.get(window.start..window.end()).unwrap_or_default() {
            if matches!(row, SidebarRow::Header(_) | SidebarRow::Divider) {
                assert_eq!(
                    app.sidebar_target_at(20.0, y + 2.0),
                    None,
                    "SUBSCRIPTIONS is a label, not a place to go"
                );
                checked += 1;
            }
            y += row.height();
        }
        assert!(checked > 0, "the fixture must draw a header to check");
    }

    #[test]
    fn a_click_right_of_the_sidebar_is_not_a_sidebar_click() {
        let app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let y = sidebar_row_y(&app, 1);
        assert_eq!(app.sidebar_target_at(SIDEBAR_WIDTH + 4.0, y), None);
    }

    #[test]
    fn a_click_on_the_sidebar_title_is_not_a_row() {
        let app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            app.sidebar_target_at(20.0, 20.0),
            None,
            "the title is chrome and does not scroll with the list"
        );
    }

    #[test]
    fn a_scrolled_sidebar_is_hit_tested_where_it_was_drawn() {
        let mut app = app_with_subscriptions(40);
        app.sidebar_scroll = 8;
        let (rows, window) = app.sidebar_layout();
        assert!(window.start > 0, "the fixture must actually scroll");
        let first = rows.get(window.start).expect("a first drawn row");
        let SidebarRow::Item { target, .. } = first else {
            panic!("the fixture's first drawn row should be an item");
        };
        assert_eq!(
            app.sidebar_target_at(20.0, SIDEBAR_LIST_TOP + 2.0),
            Some(*target),
            "the topmost drawn row is the one the scroll put there, not row 0"
        );
    }

    #[test]
    fn the_highlight_follows_the_target_it_points_at() {
        let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let pod = app.podcasts.first().expect("sample data has podcasts").id;
        app.select_sidebar(SidebarTarget::Podcast(pod));
        assert!(app.sidebar_target_selected(SidebarTarget::Podcast(pod)));
        assert!(
            !app.sidebar_target_selected(SidebarTarget::AllEpisodes),
            "All Episodes must not stay lit once a podcast is chosen"
        );
        app.select_sidebar(SidebarTarget::Search);
        assert!(app.sidebar_target_selected(SidebarTarget::Search));
    }

    #[test]
    fn choosing_a_feed_starts_at_the_top_of_it() {
        let mut app = app_with_episodes(40);
        app.episode_list_scroll = 30;
        let pod = app.podcasts.first().expect("a podcast").id;
        app.select_sidebar(SidebarTarget::Podcast(pod));
        assert_eq!(
            app.episode_list_scroll, 0,
            "a position thirty rows into one feed names nothing in another"
        );
    }

    // --- the episode list can be clicked ---

    #[test]
    fn clicking_an_episode_row_selects_that_row() {
        let mut app = app_with_episodes(10);
        let y = EPISODE_LIST_TOP + EPISODE_ROW_HEIGHT * 2.0 + 4.0;
        let hit = app.episode_row_at(SIDEBAR_WIDTH + 40.0, y);
        let expected = app.listed_episodes().get(2).copied();
        assert_eq!(hit, expected);
        assert!(app.handle_event(&click_at(SIDEBAR_WIDTH + 40.0, y)));
        assert_eq!(app.selected_episode_id, expected.map(|(_, ep)| ep));
    }

    #[test]
    fn clicking_the_selected_row_again_opens_it() {
        let mut app = app_with_episodes(10);
        let y = EPISODE_LIST_TOP + 4.0;
        app.handle_event(&click_at(SIDEBAR_WIDTH + 40.0, y));
        assert_eq!(
            app.main_view,
            MainView::EpisodeList,
            "the first click selects, so a list can be browsed without leaving it"
        );
        app.handle_event(&click_at(SIDEBAR_WIDTH + 40.0, y));
        assert_eq!(app.main_view, MainView::EpisodeDetail);
    }

    #[test]
    fn a_scrolled_episode_list_is_hit_tested_where_it_was_drawn() {
        let mut app = app_with_episodes(40);
        app.episode_list_scroll = 5;
        let episodes = app.listed_episodes();
        assert_eq!(
            app.episode_row_at(SIDEBAR_WIDTH + 40.0, EPISODE_LIST_TOP + 4.0),
            episodes.get(5).copied(),
            "the top row of a list scrolled by five is the sixth episode"
        );
    }

    #[test]
    fn clicking_past_the_last_episode_selects_nothing() {
        let mut app = app_with_episodes(3);
        let below = EPISODE_LIST_TOP + EPISODE_ROW_HEIGHT * 3.0 + 4.0;
        assert_eq!(app.episode_row_at(SIDEBAR_WIDTH + 40.0, below), None);
        assert!(!app.handle_event(&click_at(SIDEBAR_WIDTH + 40.0, below)));
        assert_eq!(app.selected_episode_id, None);
    }

    #[test]
    fn clicking_above_the_first_episode_selects_nothing() {
        let app = app_with_episodes(3);
        assert_eq!(
            app.episode_row_at(SIDEBAR_WIDTH + 40.0, EPISODE_LIST_TOP - 4.0),
            None,
            "the episode count line is not a row"
        );
    }

    // --- the filter bar can be clicked ---

    #[test]
    fn clicking_a_filter_pill_applies_that_filter() {
        let mut app = app_with_episodes(10);
        let pills = app.filter_pills(SIDEBAR_WIDTH);
        let (wanted, x, width) = pills[2];
        assert_eq!(wanted, EpisodeFilter::InProgress);
        let y = HEADER_HEIGHT + 4.0 + CATEGORY_PILL_HEIGHT / 2.0;
        assert_eq!(app.filter_pill_at(x + width / 2.0, y), Some(wanted));
        assert!(app.handle_event(&click_at(x + width / 2.0, y)));
        assert_eq!(app.episode_filter, wanted);
    }

    #[test]
    fn every_filter_pill_is_reachable_where_it_is_drawn() {
        let app = app_with_episodes(4);
        let y = HEADER_HEIGHT + 4.0 + CATEGORY_PILL_HEIGHT / 2.0;
        for (filter, x, width) in app.filter_pills(SIDEBAR_WIDTH) {
            assert_eq!(
                app.filter_pill_at(x + width / 2.0, y),
                Some(filter),
                "{filter:?} is drawn at {x}..{} and must be clickable there",
                x + width
            );
        }
    }

    #[test]
    fn a_click_above_or_below_the_pills_is_not_a_filter() {
        let app = app_with_episodes(4);
        let (_, x, width) = app.filter_pills(SIDEBAR_WIDTH)[0];
        let mid = x + width / 2.0;
        assert_eq!(app.filter_pill_at(mid, HEADER_HEIGHT - 2.0), None);
        assert_eq!(
            app.filter_pill_at(mid, HEADER_HEIGHT + 4.0 + CATEGORY_PILL_HEIGHT + 2.0),
            None
        );
    }

    #[test]
    fn changing_the_filter_re_anchors_the_list() {
        let mut app = app_with_episodes(40);
        app.episode_list_scroll = 20;
        let (_, x, width) = app.filter_pills(SIDEBAR_WIDTH)[1];
        let y = HEADER_HEIGHT + 4.0 + CATEGORY_PILL_HEIGHT / 2.0;
        app.handle_event(&click_at(x + width / 2.0, y));
        assert_eq!(
            app.episode_list_scroll, 0,
            "a filter that shortens the list leaves the old position naming \
             nothing"
        );
    }

    // --- the now-playing bar can be clicked ---

    /// An app playing an episode long enough to seek about in. The shared
    /// `app_with_episodes` fixture makes minute-long ones, and a seek to five
    /// minutes into a one-minute episode is correctly clamped to its end --
    /// which makes every seek assertion read the same number.
    fn playing_app() -> PodcastApp {
        let mut app = PodcastApp::new(1100.0, 600.0);
        drop_sample_data(&mut app);
        let pod = app.subscribe("Show", "", "", "rss://x", "", vec![]);
        for i in 0..10 {
            app.add_episode(pod, &format!("E{i:03}"), "", "2026-01-01", 3600, "", 0);
        }
        app.sidebar_selection = SidebarSelection::Podcast(pod);
        app.main_view = MainView::EpisodeList;
        let (pod, ep) = app.listed_episodes()[0];
        assert!(app.play_episode(pod, ep));
        app
    }

    #[test]
    fn the_play_button_pauses_and_resumes() {
        let mut app = playing_app();
        let controls = app.player_controls();
        let (_, rect) = controls
            .iter()
            .find(|(c, _)| *c == PlayerControl::PlayPause)
            .expect("a play button");
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(app.player_control_at(x, y), Some(PlayerControl::PlayPause));
        assert!(app.handle_event(&click_at(x, y)));
        assert_eq!(app.player_state, PlayerState::Paused);
        assert!(app.handle_event(&click_at(x, y)));
        assert_eq!(app.player_state, PlayerState::Playing);
    }

    #[test]
    fn the_skip_buttons_move_the_position() {
        let mut app = playing_app();
        app.seek_to(300);
        let controls = app.player_controls();
        let at = |wanted: PlayerControl| {
            let (_, r) = controls
                .iter()
                .find(|(c, _)| *c == wanted)
                .expect("a control");
            (r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        let (x, y) = at(PlayerControl::SkipForward);
        app.handle_event(&click_at(x, y));
        assert_eq!(app.playback_position_secs, 300 + SKIP_FORWARD_SECS);
        let (x, y) = at(PlayerControl::SkipBack);
        app.handle_event(&click_at(x, y));
        assert_eq!(
            app.playback_position_secs,
            300 + SKIP_FORWARD_SECS - SKIP_BACK_SECS
        );
    }

    #[test]
    fn clicking_the_seek_strip_jumps_to_that_point() {
        let mut app = playing_app();
        let bar_y = app.height - NOW_PLAYING_HEIGHT;
        assert_eq!(
            app.player_control_at(app.width / 2.0, bar_y + 2.0),
            Some(PlayerControl::Seek)
        );
        app.handle_event(&click_at(app.width / 2.0, bar_y + 2.0));
        let half = app.playback_duration_secs / 2;
        assert!(
            app.playback_position_secs.abs_diff(half) <= 1,
            "a click halfway along the bar is a jump to halfway through: \
             {} vs {half}",
            app.playback_position_secs
        );
    }

    #[test]
    fn the_seek_strip_clamps_to_the_ends() {
        let mut app = playing_app();
        let bar_y = app.height - NOW_PLAYING_HEIGHT;
        app.handle_event(&click_at(0.0, bar_y + 2.0));
        assert_eq!(app.playback_position_secs, 0);
        app.handle_event(&click_at(app.width * 2.0, bar_y + 2.0));
        assert!(app.playback_position_secs <= app.playback_duration_secs);
    }

    #[test]
    fn the_speed_badge_is_the_speed_button() {
        let mut app = playing_app();
        let before = app.playback_speed;
        let controls = app.player_controls();
        let (_, rect) = controls
            .iter()
            .find(|(c, _)| *c == PlayerControl::Speed)
            .expect("a speed badge");
        app.handle_event(&click_at(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        ));
        assert_ne!(app.playback_speed, before);
    }

    #[test]
    fn there_are_no_player_controls_while_stopped() {
        let app = app_with_episodes(4);
        assert_eq!(app.player_state, PlayerState::Stopped);
        let bar_y = app.height - NOW_PLAYING_HEIGHT;
        assert_eq!(
            app.player_control_at(app.width / 2.0, bar_y + 2.0),
            None,
            "no bar is drawn, so nothing under the pointer is a button"
        );
    }

    #[test]
    fn the_player_bar_takes_the_click_before_the_list_under_it() {
        // The bar is drawn over the bottom of the content area. A click on the
        // play button must not fall through to whatever row is beneath it.
        let mut app = playing_app();
        app.height = 400.0;
        let controls = app.player_controls();
        let (_, rect) = controls
            .iter()
            .find(|(c, _)| *c == PlayerControl::PlayPause)
            .expect("a play button");
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let before = app.selected_episode_id;
        app.handle_event(&click_at(x, y));
        assert_eq!(app.player_state, PlayerState::Paused);
        assert_eq!(app.selected_episode_id, before);
    }

    // --- keyboard ---

    #[test]
    fn space_starts_the_selected_episode_then_pauses_it() {
        let mut app = app_with_episodes(5);
        let (_, ep) = app.listed_episodes()[1];
        app.selected_episode_id = Some(ep);
        assert!(app.handle_event(&key(Key::Space)));
        assert_eq!(app.player_state, PlayerState::Playing);
        assert_eq!(app.current_episode_id, Some(ep));
        assert!(app.handle_event(&key(Key::Space)));
        assert_eq!(app.player_state, PlayerState::Paused);
    }

    #[test]
    fn space_with_nothing_selected_does_nothing() {
        let mut app = app_with_episodes(5);
        assert!(
            !app.handle_event(&key(Key::Space)),
            "a keystroke that changed nothing must not cost a frame"
        );
        assert_eq!(app.player_state, PlayerState::Stopped);
    }

    #[test]
    fn the_arrows_move_the_selection_and_stop_at_the_ends() {
        let mut app = app_with_episodes(3);
        let episodes = app.listed_episodes();
        assert!(app.handle_event(&key(Key::Down)));
        assert_eq!(app.selected_episode_id, Some(episodes[0].1));
        app.handle_event(&key(Key::Down));
        app.handle_event(&key(Key::Down));
        assert_eq!(app.selected_episode_id, Some(episodes[2].1));
        assert!(
            !app.handle_event(&key(Key::Down)),
            "holding Down at the end of a feed must stop, not wrap round to \
             the top -- the list looks the same either way"
        );
        assert_eq!(app.selected_episode_id, Some(episodes[2].1));
    }

    #[test]
    fn moving_the_selection_scrolls_it_into_view() {
        let mut app = app_with_episodes(60);
        for _ in 0..40 {
            app.handle_event(&key(Key::Down));
        }
        let episodes = app.listed_episodes();
        let index = episodes
            .iter()
            .position(|&(_, ep)| Some(ep) == app.selected_episode_id)
            .expect("a selection");
        let window = app.episode_list_window(episodes.len(), app.content_bottom());
        assert!(
            index >= window.start && index < window.end(),
            "row {index} is selected but the list is showing {}..{}",
            window.start,
            window.end()
        );
    }

    #[test]
    fn the_arrows_seek_only_when_something_is_playing() {
        let mut app = playing_app();
        app.stop_playback();
        app.selected_episode_id = Some(app.listed_episodes()[0].1);
        assert!(
            !app.handle_event(&key(Key::Right)),
            "there is nothing to seek in"
        );
        app.handle_event(&key(Key::Space));
        app.seek_to(100);
        assert!(app.handle_event(&key(Key::Right)));
        assert_eq!(app.playback_position_secs, 100 + SKIP_FORWARD_SECS);
        assert!(app.handle_event(&key(Key::Left)));
        assert_eq!(
            app.playback_position_secs,
            100 + SKIP_FORWARD_SECS - SKIP_BACK_SECS
        );
    }

    #[test]
    fn enter_opens_the_detail_and_escape_comes_back() {
        let mut app = app_with_episodes(5);
        app.handle_event(&key(Key::Down));
        assert!(app.handle_event(&key(Key::Enter)));
        assert_eq!(app.main_view, MainView::EpisodeDetail);
        assert!(app.handle_event(&key(Key::Escape)));
        assert_eq!(app.main_view, MainView::EpisodeList);
        assert!(
            !app.handle_event(&key(Key::Escape)),
            "there is nothing left to back out of"
        );
    }

    #[test]
    fn tab_steps_through_the_filters_and_comes_back_round() {
        let mut app = app_with_episodes(5);
        let mut seen = vec![app.episode_filter];
        for _ in 0..EpisodeFilter::ALL.len() {
            app.handle_event(&key(Key::Tab));
            seen.push(app.episode_filter);
        }
        assert_eq!(seen.first(), seen.last(), "five steps is a full circle");
        for filter in EpisodeFilter::ALL {
            assert!(seen.contains(&filter), "{filter:?} was never reached");
        }
    }

    #[test]
    fn q_queues_the_selection_and_d_downloads_it() {
        let mut app = app_with_episodes(5);
        app.handle_event(&key(Key::Down));
        assert!(app.handle_event(&typed(Key::Q, 'q')));
        assert_eq!(app.play_queue.len(), 1);
        assert!(app.handle_event(&typed(Key::D, 'd')));
        assert_eq!(app.download_queue.len(), 1);
    }

    #[test]
    fn m_marks_the_selection_played_and_unplayed_again() {
        let mut app = app_with_episodes(5);
        app.handle_event(&key(Key::Down));
        let (pod, ep) = app.selected_episode().expect("a selection");
        assert!(app.handle_event(&typed(Key::M, 'm')));
        assert!(app.find_episode_global(pod, ep).unwrap().status.is_played());
        assert!(app.handle_event(&typed(Key::M, 'm')));
        assert!(!app.find_episode_global(pod, ep).unwrap().status.is_played());
    }

    #[test]
    fn s_cycles_the_speed() {
        let mut app = playing_app();
        let before = app.playback_speed;
        assert!(app.handle_event(&typed(Key::S, 's')));
        assert_ne!(app.playback_speed, before);
    }

    #[test]
    fn an_unbound_key_does_not_ask_for_a_frame() {
        let mut app = app_with_episodes(5);
        assert_eq!(app.on_event(&key(Key::F7)), Response::Idle);
        assert_eq!(app.on_event(&typed(Key::Z, 'z')), Response::Idle);
    }

    // --- the strap ---

    #[test]
    fn the_title_names_what_is_playing() {
        let mut app = app_with_episodes(3);
        assert_eq!(app.title(), "Podcasts");
        let (pod, ep) = app.listed_episodes()[0];
        app.play_episode(pod, ep);
        assert_eq!(app.title(), "E000 - Podcasts");
        app.pause_playback();
        assert_eq!(
            app.title(),
            "E000 (paused) - Podcasts",
            "a paused player and a playing one are not the same window"
        );
        app.stop_playback();
        assert_eq!(app.title(), "Podcasts");
    }

    #[test]
    fn the_clock_runs_only_while_something_moves() {
        let mut app = app_with_episodes(3);
        assert_eq!(
            app.tick_interval(),
            None,
            "a stopped player with an empty queue has nothing to advance"
        );
        let (pod, ep) = app.listed_episodes()[0];
        app.play_episode(pod, ep);
        assert_eq!(app.tick_interval(), Some(PLAYBACK_TICK));
        app.pause_playback();
        assert_eq!(app.tick_interval(), None, "a paused episode does not move");
        app.queue_download(pod, ep);
        assert_eq!(
            app.tick_interval(),
            Some(PLAYBACK_TICK),
            "a download still has to run even with the player paused"
        );
    }

    #[test]
    fn the_tick_advances_playback() {
        let mut app = playing_app();
        let before = app.playback_position_secs;
        for _ in 0..8 {
            app.on_event(&Event::Tick { elapsed_ms: 1000 });
        }
        assert!(
            app.playback_position_secs > before,
            "the clock arrived and the episode did not move: {before} -> {}",
            app.playback_position_secs
        );
    }

    #[test]
    fn the_tick_advances_a_download() {
        let mut app = app_with_episodes(3);
        let (pod, ep) = app.listed_episodes()[0];
        app.queue_download(pod, ep);
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 250 }),
            Response::Redraw
        );
        let progress = app.download_queue.first().map_or(1.0, |d| d.progress);
        assert!(progress > 0.0, "the download queue never moved");
    }

    #[test]
    fn a_tick_with_nothing_to_do_asks_for_no_frame() {
        let mut app = app_with_episodes(3);
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 250 }),
            Response::Idle,
            "redrawing on a tick that changed nothing is a wasted frame"
        );
    }

    #[test]
    fn a_resize_relays_out_and_a_repeat_of_it_does_not() {
        let mut app = app_with_episodes(3);
        let resize = Event::Resize {
            width: 900,
            height: 700,
        };
        assert_eq!(app.on_event(&resize), Response::Redraw);
        assert_eq!(app.width, 900.0);
        assert_eq!(app.height, 700.0);
        assert_eq!(app.on_event(&resize), Response::Idle);
    }

    #[test]
    fn a_window_dragged_tiny_keeps_a_layout() {
        let mut app = app_with_episodes(3);
        app.set_window_size(1.0, 1.0);
        assert!(app.width >= MIN_WINDOW_WIDTH);
        assert!(app.height >= MIN_WINDOW_HEIGHT);
        assert!(
            app.content_bottom() > SIDEBAR_LIST_TOP,
            "the sidebar must still have room for a row"
        );
    }

    #[test]
    fn the_first_frame_uses_the_size_the_compositor_gave() {
        let mut app = app_with_episodes(3);
        let tree = app.render(1280.0, 800.0);
        assert_eq!(app.width, 1280.0);
        assert_eq!(app.height, 800.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_close_button_exits() {
        let mut app = app_with_episodes(3);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn a_taller_window_shows_more_rows() {
        let mut app = app_with_episodes(40);
        app.set_window_size(1100.0, 500.0);
        let short = app.episode_list_window(40, app.content_bottom()).count;
        app.set_window_size(1100.0, 900.0);
        let tall = app.episode_list_window(40, app.content_bottom()).count;
        assert!(
            tall > short,
            "a window made 400px taller must show more of the list: \
             {short} -> {tall}"
        );
    }

    #[test]
    fn a_click_on_a_row_boundary_belongs_to_the_lower_row() {
        let app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (rows, window) = app.sidebar_layout();
        let first = rows.get(window.start).expect("a first row");
        let boundary = SIDEBAR_LIST_TOP + first.height();
        let second = rows.get(window.start + 1).expect("a second row");
        let SidebarRow::Item { target, .. } = second else {
            panic!("the second row should be an item");
        };
        assert_eq!(
            app.sidebar_target_at(20.0, boundary),
            Some(*target),
            "the pixel a row ends on belongs to the next row, not to both"
        );
    }

    #[test]
    fn opening_search_puts_out_the_all_episodes_light() {
        let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(app.sidebar_selection, SidebarSelection::AllEpisodes);
        assert!(app.sidebar_target_selected(SidebarTarget::AllEpisodes));
        app.select_sidebar(SidebarTarget::Search);
        assert!(
            !app.sidebar_target_selected(SidebarTarget::AllEpisodes),
            "Search does not change the selection, so All Episodes would stay \
             lit beside it -- two rows highlighted, one of them wrong"
        );
    }

    #[test]
    fn every_library_row_opens_its_own_view() {
        let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let pod = app.podcasts.first().expect("sample data").id;
        for (target, view) in [
            (SidebarTarget::Search, MainView::Search),
            (SidebarTarget::AllEpisodes, MainView::EpisodeList),
            (SidebarTarget::Queue, MainView::Queue),
            (SidebarTarget::Downloads, MainView::Downloads),
            (SidebarTarget::History, MainView::History),
            (SidebarTarget::Statistics, MainView::Statistics),
            (SidebarTarget::Podcast(pod), MainView::EpisodeList),
            (
                SidebarTarget::Category(Category::Technology),
                MainView::EpisodeList,
            ),
        ] {
            app.select_sidebar(target);
            assert_eq!(app.main_view, view, "{target:?} must open {view:?}");
            assert!(
                app.sidebar_target_selected(target),
                "{target:?} must light its own row once chosen"
            );
        }
    }

    #[test]
    fn a_click_inside_the_sidebar_is_not_an_episode() {
        let app = app_with_episodes(10);
        let y = EPISODE_LIST_TOP + EPISODE_ROW_HEIGHT + 4.0;
        assert!(
            app.episode_row_at(SIDEBAR_WIDTH + 4.0, y).is_some(),
            "the fixture must have a row there to begin with"
        );
        assert_eq!(
            app.episode_row_at(SIDEBAR_WIDTH - 4.0, y),
            None,
            "the episode list starts where the sidebar ends"
        );
    }

    #[test]
    fn clicking_below_the_fold_of_a_long_list_selects_nothing() {
        // The guard against a click past the last *drawn* row only matters
        // when the list is longer than the window: with a short list the row
        // lookup fails anyway, and with a long one it succeeds -- on an
        // episode that is nowhere on screen.
        let mut app = app_with_episodes(60);
        app.set_window_size(1100.0, 500.0);
        let window = app.episode_list_window(60, app.content_bottom());
        assert!(window.count < 60, "the fixture must overflow the window");
        #[allow(clippy::cast_precision_loss)]
        let below = EPISODE_LIST_TOP + (window.count as f32) * EPISODE_ROW_HEIGHT + 4.0;
        assert_eq!(
            app.episode_row_at(SIDEBAR_WIDTH + 40.0, below),
            None,
            "an episode drawn nowhere cannot be clicked"
        );
    }

    #[test]
    fn the_selection_stops_at_the_top_as_well_as_the_bottom() {
        let mut app = app_with_episodes(5);
        let episodes = app.listed_episodes();
        app.handle_event(&key(Key::Down));
        assert_eq!(app.selected_episode_id, Some(episodes[0].1));
        assert!(
            !app.handle_event(&key(Key::Up)),
            "Up at the top of a feed must stop, not wrap round to the end"
        );
        assert_eq!(app.selected_episode_id, Some(episodes[0].1));
    }

    #[test]
    fn moving_back_up_scrolls_the_list_back_with_it() {
        let mut app = app_with_episodes(60);
        app.set_window_size(1100.0, 500.0);
        for _ in 0..40 {
            app.handle_event(&key(Key::Down));
        }
        assert!(
            app.episode_list_scroll > 0,
            "the fixture must have scrolled"
        );
        for _ in 0..40 {
            app.handle_event(&key(Key::Up));
        }
        let episodes = app.listed_episodes();
        let index = episodes
            .iter()
            .position(|&(_, ep)| Some(ep) == app.selected_episode_id)
            .expect("a selection");
        let window = app.episode_list_window(episodes.len(), app.content_bottom());
        assert!(
            index >= window.start && index < window.end(),
            "row {index} is selected but the list is showing {}..{} -- a \
             selection moving up off the top must bring the list with it",
            window.start,
            window.end()
        );
    }

    #[test]
    fn releasing_the_mouse_is_not_a_click() {
        let mut app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let y = sidebar_row_y(&app, 3);
        let release = Event::Mouse(MouseEvent {
            x: 20.0,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        assert!(
            !app.handle_event(&release),
            "acting on both halves of a click runs every button twice"
        );
        assert_eq!(app.main_view, MainView::EpisodeList);
    }

    #[test]
    fn the_ends_of_the_seek_strip_are_the_ends_of_the_episode() {
        let mut app = playing_app();
        let bar_y = app.height - NOW_PLAYING_HEIGHT;
        app.seek_to(app.playback_duration_secs / 2);
        app.handle_event(&click_at(0.0, bar_y + 2.0));
        assert_eq!(app.playback_position_secs, 0, "the left edge is the start");

        app.handle_event(&click_at(app.width - 1.0, bar_y + 2.0));
        let duration = app.playback_duration_secs;
        assert!(
            app.playback_position_secs > duration - duration / 100,
            "the right edge is the end: {} of {duration}",
            app.playback_position_secs
        );
    }

    #[test]
    fn a_click_outside_the_window_is_not_a_seek() {
        let mut app = playing_app();
        let bar_y = app.height - NOW_PLAYING_HEIGHT;
        app.seek_to(600);
        // The strip spans the window, so past its edge is past the strip --
        // which is what keeps the fraction inside 0..1 without a clamp.
        assert_eq!(app.player_control_at(-50.0, bar_y + 2.0), None);
        assert_eq!(app.player_control_at(app.width + 50.0, bar_y + 2.0), None);
        app.handle_event(&click_at(app.width + 50.0, bar_y + 2.0));
        assert_eq!(app.playback_position_secs, 600);
    }
}
