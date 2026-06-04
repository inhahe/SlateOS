//! OurOS Podcast Manager
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

#![allow(dead_code)]

use std::collections::HashMap;

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
const NOW_PLAYING_HEIGHT: f32 = 80.0;
const HEADER_HEIGHT: f32 = 48.0;
const EPISODE_ROW_HEIGHT: f32 = 72.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
const CATEGORY_PILL_HEIGHT: f32 = 28.0;
const TOOLBAR_HEIGHT: f32 = 36.0;

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
                && (s.0 - self.0).abs() < 0.001 {
                    let next_idx = (i + 1) % all.len();
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
                if self.duration_secs == 0 {
                    0.0
                } else {
                    (position_secs as f32 / self.duration_secs as f32) * 100.0
                }
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
        self.episodes.iter().filter(|e| e.status.is_unplayed()).count()
    }

    /// Count of in-progress episodes.
    pub fn in_progress_count(&self) -> usize {
        self.episodes.iter().filter(|e| e.status.is_in_progress()).count()
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
    pub fn total_time_display(&self) -> String {
        let hours = self.total_listening_secs / 3600;
        let mins = (self.total_listening_secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
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
    let val_start = start + search.len();
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
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Format bytes as human-readable size.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{:.1} KB", kb);
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{:.1} MB", mb);
    }
    let gb = mb / 1024.0;
    format!("{:.2} GB", gb)
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
    pub sidebar_scroll: f32,
    pub episode_list_scroll: f32,

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
            sidebar_scroll: 0.0,
            episode_list_scroll: 0.0,
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
            self.stats.subscriptions_count =
                self.stats.subscriptions_count.saturating_sub(1);
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
            && let Some(ep) = podcast.episodes.iter_mut().find(|e| e.id == episode_id) {
                ep.status = EpisodeStatus::Played;
                return true;
            }
        false
    }

    /// Mark an episode as unplayed.
    pub fn mark_unplayed(&mut self, podcast_id: u64, episode_id: u64) -> bool {
        if let Some(podcast) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = podcast.episodes.iter_mut().find(|e| e.id == episode_id) {
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
    pub fn set_episode_notes(
        &mut self,
        podcast_id: u64,
        episode_id: u64,
        text: &str,
    ) -> bool {
        if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == podcast_id)
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
            && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
            self.playback_position_secs =
                self.playback_position_secs.saturating_sub(secs);
            self.update_episode_position();
        }
    }

    /// Seek to an absolute position.
    pub fn seek_to(&mut self, position_secs: u32) {
        if self.player_state != PlayerState::Stopped {
            self.playback_position_secs =
                position_secs.min(self.playback_duration_secs);
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
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
                    ep.status = EpisodeStatus::InProgress {
                        position_secs: pos,
                    };
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
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
            && let Some(next) = self.play_queue.first().cloned() {
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
        if self
            .play_queue
            .iter()
            .any(|q| q.episode_id == episode_id)
        {
            return false;
        }

        let info = self
            .podcasts
            .iter()
            .find(|p| p.id == podcast_id)
            .and_then(|p| {
                p.find_episode(episode_id).map(|ep| {
                    (
                        ep.title.clone(),
                        p.title.clone(),
                        ep.duration_secs,
                    )
                })
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
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
            if let Some(p) = self
                .podcasts
                .iter_mut()
                .find(|p| p.id == item.podcast_id)
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == episode_id) {
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
        if !has_active
            && let Some(item) = self.download_queue.iter_mut().find(|d| !d.active) {
                item.active = true;
                // Mark episode as downloading.
                let pod_id = item.podcast_id;
                let ep_id = item.episode_id;
                if let Some(p) = self.podcasts.iter_mut().find(|p| p.id == pod_id)
                    && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == ep_id) {
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
                && let Some(ep) = p.episodes.iter_mut().find(|e| e.id == *ep_id) {
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
                && ep.download_status.is_downloaded() {
                    self.used_disk_bytes =
                        self.used_disk_bytes.saturating_sub(ep.file_size_bytes);
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
    pub fn disk_usage_pct(&self) -> f32 {
        if self.total_disk_bytes == 0 {
            return 0.0;
        }
        (self.used_disk_bytes as f64 / self.total_disk_bytes as f64 * 100.0) as f32
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
            let already = self
                .podcasts
                .iter()
                .any(|p| p.rss_url == outline.xml_url);
            if !already {
                self.subscribe(
                    &outline.text,
                    "",
                    "",
                    &outline.xml_url,
                    "",
                    Vec::new(),
                );
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
            && let Some(ep) = podcast.episodes.get_mut(0) {
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
        self.stats.record_listening(p1, "The Rustacean Station", 3600, true);
        self.stats.record_listening(p2, "StarTalk Radio", 2700, true);
        self.stats.episodes_completed = 2;

        // Update used disk space for downloaded episodes.
        self.used_disk_bytes = 116_000_000; // ~110 MB
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire application to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
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
            MainView::EpisodeList => self.render_episode_list(&mut cmds, content_x, content_w, content_h),
            MainView::EpisodeDetail => self.render_episode_detail(&mut cmds, content_x, content_w, content_h),
            MainView::Queue => self.render_queue_view(&mut cmds, content_x, content_w, content_h),
            MainView::Downloads => self.render_downloads_view(&mut cmds, content_x, content_w, content_h),
            MainView::History => self.render_history_view(&mut cmds, content_x, content_w, content_h),
            MainView::Statistics => self.render_statistics_view(&mut cmds, content_x, content_w, content_h),
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

        let mut item_y: f32 = 12.0;
        let item_h: f32 = 32.0;
        let indent: f32 = 16.0;

        // Title.
        cmds.push(RenderCommand::Text {
            x: indent,
            y: item_y,
            text: "Podcasts".to_string(),
            color: TEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
        });
        item_y += 36.0;

        // Search item.
        let search_selected = matches!(self.main_view, MainView::Search);
        self.render_sidebar_item(cmds, indent, item_y, "Search", BLUE, search_selected);
        item_y += item_h;

        // All Episodes.
        let all_selected = matches!(self.sidebar_selection, SidebarSelection::AllEpisodes)
            && matches!(self.main_view, MainView::EpisodeList);
        self.render_sidebar_item(cmds, indent, item_y, "All Episodes", LAVENDER, all_selected);
        item_y += item_h;

        // Queue.
        let queue_count = self.play_queue.len();
        let queue_label = if queue_count > 0 {
            format!("Queue ({})", queue_count)
        } else {
            "Queue".to_string()
        };
        let queue_selected = matches!(self.sidebar_selection, SidebarSelection::Queue);
        self.render_sidebar_item(cmds, indent, item_y, &queue_label, GREEN, queue_selected);
        item_y += item_h;

        // Downloads.
        let dl_selected = matches!(self.sidebar_selection, SidebarSelection::Downloads);
        self.render_sidebar_item(cmds, indent, item_y, "Downloads", PEACH, dl_selected);
        item_y += item_h;

        // History.
        let hist_selected = matches!(self.sidebar_selection, SidebarSelection::History);
        self.render_sidebar_item(cmds, indent, item_y, "History", MAUVE, hist_selected);
        item_y += item_h;

        // Statistics.
        let stats_selected = matches!(self.sidebar_selection, SidebarSelection::Statistics);
        self.render_sidebar_item(cmds, indent, item_y, "Statistics", TEAL, stats_selected);
        item_y += item_h + 8.0;

        // Divider.
        cmds.push(RenderCommand::FillRect {
            x: indent,
            y: item_y,
            width: SIDEBAR_WIDTH - indent * 2.0,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        item_y += 12.0;

        // Subscriptions header.
        cmds.push(RenderCommand::Text {
            x: indent,
            y: item_y,
            text: "SUBSCRIPTIONS".to_string(),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
        });
        item_y += 24.0;

        // Podcast list.
        for podcast in &self.podcasts {
            let selected = matches!(&self.sidebar_selection, SidebarSelection::Podcast(id) if *id == podcast.id);
            let unplayed = podcast.unplayed_count();
            let label = if unplayed > 0 {
                format!("{} ({})", podcast.title, unplayed)
            } else {
                podcast.title.clone()
            };
            let accent = podcast
                .categories
                .first()
                .map(|c| c.color())
                .unwrap_or(BLUE);
            self.render_sidebar_item(cmds, indent, item_y, &label, accent, selected);
            item_y += item_h;
        }

        item_y += 12.0;

        // Divider.
        cmds.push(RenderCommand::FillRect {
            x: indent,
            y: item_y,
            width: SIDEBAR_WIDTH - indent * 2.0,
            height: 1.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        item_y += 12.0;

        // Categories header.
        cmds.push(RenderCommand::Text {
            x: indent,
            y: item_y,
            text: "CATEGORIES".to_string(),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - indent * 2.0),
        });
        item_y += 24.0;

        for cat in Category::ALL {
            let selected = matches!(&self.sidebar_selection, SidebarSelection::Category(c) if *c == *cat);
            self.render_sidebar_item(cmds, indent, item_y, cat.name(), cat.color(), selected);
            item_y += item_h;
        }
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
        });
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
        });

        // Episodes.
        let start_y = count_y + 32.0;
        let mut ep_y = start_y;
        for (pod_id, ep_id) in &episodes {
            if ep_y > content_h {
                break;
            }
            if let Some(podcast) = self.find_podcast(*pod_id)
                && let Some(ep) = podcast.find_episode(*ep_id) {
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
            ep_y += EPISODE_ROW_HEIGHT;
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
        });
    }

    fn render_filter_bar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let filters = [
            EpisodeFilter::All,
            EpisodeFilter::Unplayed,
            EpisodeFilter::InProgress,
            EpisodeFilter::Played,
            EpisodeFilter::Downloaded,
        ];

        let mut pill_x = x + 12.0;
        let pill_y = y + 4.0;
        for filter in &filters {
            let label = filter.label();
            let selected = self.episode_filter == *filter;
            let label_width = label.len() as f32 * 7.5 + 16.0;

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
            });

            pill_x += label_width + 8.0;
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
                let found = self.podcasts.iter().find_map(|p| {
                    p.find_episode(eid).map(|_| (p.id, eid))
                });
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
        });
        detail_y += 24.0;

        // Status badges.
        let status_label = episode.status.label();
        let dl_label = episode.download_status.label();
        cmds.push(RenderCommand::FillRect {
            x: pad,
            y: detail_y,
            width: status_label.len() as f32 * 7.0 + 16.0,
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
        });
        let dl_badge_x = pad + status_label.len() as f32 * 7.0 + 24.0;
        cmds.push(RenderCommand::FillRect {
            x: dl_badge_x,
            y: detail_y,
            width: dl_label.len() as f32 * 7.0 + 16.0,
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
        });
        detail_y += 22.0;

        cmds.push(RenderCommand::Text {
            x: pad,
            y: detail_y,
            text: episode.description.clone(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_w),
        });
        detail_y += 40.0;

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
            });
            detail_y += 22.0;

            if !episode.notes.text.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: pad,
                    y: detail_y,
                    text: episode.notes.text.clone(),
                    color: SUBTEXT0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(text_w),
                });
                detail_y += 24.0;
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
                });
                cmds.push(RenderCommand::Text {
                    x: pad + 68.0,
                    y: detail_y + 3.0,
                    text: bm.label.clone(),
                    color: TEXT,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(text_w - 80.0),
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
                text: format!("{}.", idx + 1),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
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
                });
                cmds.push(RenderCommand::Text {
                    x: bar_x + bar_w - 90.0,
                    y: info_y,
                    text: ep.file_size_display(),
                    color: OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
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
            });
            cmds.push(RenderCommand::Text {
                x: pad + content_w - 200.0,
                y: card_y,
                text: format!("{}h {}m", hours, mins),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
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
            });

            let mut ep_y = results_y + 24.0;
            for (pod_id, ep_id) in &self.search_results {
                if ep_y > content_h {
                    break;
                }
                if let Some(podcast) = self.find_podcast(*pod_id)
                    && let Some(ep) = podcast.find_episode(*ep_id) {
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
        });

        cmds.push(RenderCommand::Text {
            x: info_x,
            y: info_y + 20.0,
            text: pod_title,
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(350.0),
        });

        // Playback controls.
        let controls_x = self.width / 2.0 - 80.0;
        let controls_y = bar_y + 20.0;

        // Skip back button.
        cmds.push(RenderCommand::FillRect {
            x: controls_x,
            y: controls_y,
            width: 36.0,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(18.0),
        });
        cmds.push(RenderCommand::Text {
            x: controls_x + 6.0,
            y: controls_y + 9.0,
            text: "-15s".to_string(),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(28.0),
        });

        // Play/Pause button.
        let pp_x = controls_x + 48.0;
        cmds.push(RenderCommand::FillRect {
            x: pp_x,
            y: controls_y - 2.0,
            width: 40.0,
            height: 40.0,
            color: BLUE,
            corner_radii: CornerRadii::all(20.0),
        });
        let pp_label = match self.player_state {
            PlayerState::Playing => "||",
            PlayerState::Paused => ">",
            PlayerState::Stopped => ">",
        };
        cmds.push(RenderCommand::Text {
            x: pp_x + 12.0,
            y: controls_y + 8.0,
            text: pp_label.to_string(),
            color: CRUST,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(20.0),
        });

        // Skip forward button.
        let sf_x = controls_x + 100.0;
        cmds.push(RenderCommand::FillRect {
            x: sf_x,
            y: controls_y,
            width: 36.0,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(18.0),
        });
        cmds.push(RenderCommand::Text {
            x: sf_x + 4.0,
            y: controls_y + 9.0,
            text: "+30s".to_string(),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(32.0),
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
        });

        // Speed indicator.
        cmds.push(RenderCommand::FillRect {
            x: time_x + 130.0,
            y: info_y + 2.0,
            width: 44.0,
            height: 22.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: time_x + 136.0,
            y: info_y + 6.0,
            text: self.playback_speed.label(),
            color: PEACH,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(38.0),
        });
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let app = PodcastApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    let cmds = app.render();
    // In the actual OS, render commands are submitted to the compositor.
    let _cmd_count = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(Category::from_str_name("technology"), Some(Category::Technology));
        assert_eq!(Category::from_str_name("tech"), Some(Category::Technology));
        assert_eq!(Category::from_str_name("true crime"), Some(Category::TrueCrime));
        assert_eq!(Category::from_str_name("truecrime"), Some(Category::TrueCrime));
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
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
    }

    #[test]
    fn test_format_bytes_large() {
        let size = 45_000_000u64;
        let display = format_bytes(size);
        assert!(display.contains("MB"));
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
        assert!(ep.file_size_display().contains("MB"));
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
                    id: 1, podcast_id: 1, title: "E1".to_string(),
                    description: String::new(), date: String::new(),
                    duration_secs: 100, enclosure_url: String::new(),
                    file_size_bytes: 100,
                    status: EpisodeStatus::Unplayed,
                    download_status: DownloadStatus::NotDownloaded,
                    notes: EpisodeNotes::new(),
                },
                Episode {
                    id: 2, podcast_id: 1, title: "E2".to_string(),
                    description: String::new(), date: String::new(),
                    duration_secs: 100, enclosure_url: String::new(),
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
            episodes: vec![
                Episode {
                    id: 1, podcast_id: 1, title: "E1".to_string(),
                    description: String::new(), date: String::new(),
                    duration_secs: 100, enclosure_url: String::new(),
                    file_size_bytes: 100,
                    status: EpisodeStatus::InProgress { position_secs: 50 },
                    download_status: DownloadStatus::NotDownloaded,
                    notes: EpisodeNotes::new(),
                },
            ],
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
            episodes: vec![
                Episode {
                    id: 1, podcast_id: 1, title: "E1".to_string(),
                    description: String::new(), date: String::new(),
                    duration_secs: 100, enclosure_url: String::new(),
                    file_size_bytes: 5000,
                    status: EpisodeStatus::Unplayed,
                    download_status: DownloadStatus::Downloaded,
                    notes: EpisodeNotes::new(),
                },
            ],
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
            episodes: vec![
                Episode {
                    id: 10, podcast_id: 1, title: "Found".to_string(),
                    description: String::new(), date: String::new(),
                    duration_secs: 100, enclosure_url: String::new(),
                    file_size_bytes: 100,
                    status: EpisodeStatus::Unplayed,
                    download_status: DownloadStatus::NotDownloaded,
                    notes: EpisodeNotes::new(),
                },
            ],
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
        let id = app.subscribe("New Pod", "Author", "Desc", "https://rss.example.com", "", vec![Category::Technology]);
        assert!(app.find_podcast(id).is_some());
        assert_eq!(app.podcasts.len(), initial_count + 1);
    }

    #[test]
    fn test_unsubscribe() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let id = app.subscribe("Temp", "Auth", "Desc", "https://temp.example.com", "", vec![]);
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
        let eid = app.add_episode(pid, "Ep1", "Desc", "2026-01-01", 600, "https://x.com/ep1.mp3", 10000);
        assert!(eid.is_some());
        assert!(app.find_podcast(pid).unwrap().find_episode(eid.unwrap()).is_some());
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();

        assert!(app.mark_played(pid, eid));
        assert!(app.find_podcast(pid).unwrap().find_episode(eid).unwrap().status.is_played());

        assert!(app.mark_unplayed(pid, eid));
        assert!(app.find_podcast(pid).unwrap().find_episode(eid).unwrap().status.is_unplayed());
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
        assert!(app.set_episode_notes(pid, eid, "My notes"));
        let ep = app.find_podcast(pid).unwrap().find_episode(eid).unwrap();
        assert_eq!(ep.notes.text, "My notes");
    }

    #[test]
    fn test_app_add_episode_bookmark() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
        assert!(app.add_episode_bookmark(pid, eid, 30, "Good part"));
        let ep = app.find_podcast(pid).unwrap().find_episode(eid).unwrap();
        assert_eq!(ep.notes.bookmarks.len(), 1);
        assert_eq!(ep.notes.bookmarks[0].label, "Good part");
    }

    #[test]
    fn test_app_remove_episode_bookmark() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        assert!(app.play_episode(pid, eid));
        assert_eq!(app.player_state, PlayerState::Playing);
        assert_eq!(app.current_episode_id, Some(eid));
        assert_eq!(app.playback_duration_secs, 600);
    }

    #[test]
    fn test_pause_resume() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        app.play_episode(pid, eid);
        app.stop_playback();
        assert_eq!(app.player_state, PlayerState::Stopped);
        assert!(app.current_episode_id.is_none());
    }

    #[test]
    fn test_seek_forward() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(15);
        assert_eq!(app.playback_position_secs, 15);
    }

    #[test]
    fn test_seek_backward() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(30);
        app.seek_backward(15);
        assert_eq!(app.playback_position_secs, 15);
    }

    #[test]
    fn test_seek_backward_saturates() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        app.play_episode(pid, eid);
        app.seek_backward(100);
        assert_eq!(app.playback_position_secs, 0);
    }

    #[test]
    fn test_seek_forward_clamped() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
        app.play_episode(pid, eid);
        app.seek_forward(200);
        // Should be clamped to duration and marked played.
        assert_eq!(app.playback_position_secs, 0); // Completed, auto-play ran
    }

    #[test]
    fn test_seek_to() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        let initial_queue = app.play_queue.len();
        assert!(app.queue_episode(pid, eid));
        assert_eq!(app.play_queue.len(), initial_queue + 1);
    }

    #[test]
    fn test_queue_no_duplicates() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
        app.queue_episode(pid, eid);
        let count = app.play_queue.len();
        assert!(!app.queue_episode(pid, eid));
        assert_eq!(app.play_queue.len(), count);
    }

    #[test]
    fn test_dequeue_episode() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        let e1 = app.add_episode(pid, "A", "", "2026-01-01", 100, "", 100).unwrap();
        let e2 = app.add_episode(pid, "B", "", "2026-01-02", 100, "", 100).unwrap();
        let e3 = app.add_episode(pid, "C", "", "2026-01-03", 100, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000).unwrap();
        assert!(app.queue_download(pid, eid));
        assert_eq!(app.download_queue.len(), 1);
    }

    #[test]
    fn test_queue_download_no_duplicate() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000).unwrap();
        app.queue_download(pid, eid);
        assert!(!app.queue_download(pid, eid));
    }

    #[test]
    fn test_cancel_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000).unwrap();
        app.queue_download(pid, eid);
        app.simulate_download_tick();
        // First tick should activate the download.
        assert!(app.download_queue.iter().any(|d| d.active));
    }

    #[test]
    fn test_delete_download() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 10_000).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 600, "", 100).unwrap();
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
        assert!(app.download_queue.iter().any(|d| d.episode_title == "Auto Ep"));
    }

    // -----------------------------------------------------------------------
    // Render tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = PodcastApp::new(800.0, 600.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_all_views() {
        let mut app = PodcastApp::new(800.0, 600.0);

        app.main_view = MainView::EpisodeList;
        let cmds = app.render();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Queue;
        app.sidebar_selection = SidebarSelection::Queue;
        let cmds = app.render();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Downloads;
        app.sidebar_selection = SidebarSelection::Downloads;
        let cmds = app.render();
        assert!(!cmds.is_empty());

        app.main_view = MainView::History;
        app.sidebar_selection = SidebarSelection::History;
        let cmds = app.render();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Statistics;
        app.sidebar_selection = SidebarSelection::Statistics;
        let cmds = app.render();
        assert!(!cmds.is_empty());

        app.main_view = MainView::Search;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_episode_detail() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let first_ep = app.podcasts.first()
            .and_then(|p| p.episodes.first())
            .map(|e| e.id);
        app.selected_episode_id = first_ep;
        app.main_view = MainView::EpisodeDetail;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_now_playing() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.podcasts[0].id;
        let eid = app.podcasts[0].episodes[0].id;
        app.play_episode(pid, eid);
        let cmds = app.render();
        // Should have now-playing bar render commands.
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_search_with_results() {
        let mut app = PodcastApp::new(800.0, 600.0);
        app.main_view = MainView::Search;
        app.search_query = "Rust".to_string();
        app.perform_search();
        let cmds = app.render();
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
        let cmds = app.render();
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
        app.queue_episode(pid, eid);
        app.unsubscribe(pid);
        assert!(!app.play_queue.iter().any(|q| q.podcast_id == pid));
    }

    #[test]
    fn test_unsubscribe_stops_playing() {
        let mut app = PodcastApp::new(800.0, 600.0);
        let pid = app.subscribe("P", "", "", "rss://x", "", vec![]);
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 100).unwrap();
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
        let eid = app.add_episode(pid, "Ep", "", "2026-01-01", 100, "", 1000).unwrap();
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
        assert_eq!(extract_attr(tag, "xmlUrl"), Some("https://example.com".to_string()));
        assert_eq!(extract_attr(tag, "missing"), None);
    }
}
