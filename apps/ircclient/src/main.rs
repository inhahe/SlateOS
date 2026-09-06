//! IRC chat client application for SlateOS.
//!
//! Implements IRC protocol message parsing, channel management,
//! user tracking, message history, and a multi-panel chat UI.

use guitk::color::Color;
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use oswindow::{Event, RenderTree};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Layout
// ============================================================================

/// Y the panels start at, below the title bar.
const CONTENT_TOP: f32 = 32.0;
const SIDEBAR_WIDTH: f32 = 180.0;
const NICK_LIST_WIDTH: f32 = 160.0;
const INPUT_HEIGHT: f32 = 36.0;
/// Height of one line in the chat area.
const MESSAGE_HEIGHT: f32 = 20.0;
/// Height of a row in the sidebar, and the step between two of them.
const SIDEBAR_ROW_HEIGHT: f32 = 24.0;
const SIDEBAR_ROW_STEP: f32 = 26.0;
/// Height of a section heading in the sidebar.
const SIDEBAR_HEADER_HEIGHT: f32 = 16.0;
/// Height of one nick in the nick list.
const NICK_ROW_HEIGHT: f32 = 20.0;
/// Height of the "USERS (n)" heading above the nick list.
const NICK_LIST_HEADER_HEIGHT: f32 = 24.0;
/// How many lines one notch of the wheel moves the chat.
const SCROLL_LINES_PER_NOTCH: f32 = 3.0;
/// How many lines Page Up and Page Down move it.
const PAGE_SCROLL_LINES: isize = 10;
/// A window smaller than this has no chat column left between the panels.
const MIN_WINDOW_WIDTH: f32 = 560.0;
const MIN_WINDOW_HEIGHT: f32 = 320.0;
const WINDOW_WIDTH: f32 = 1280.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// A rectangle on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// One row of the sidebar, as it is laid out down the column.
///
/// The sidebar was drawn by walking a `row_y` down through four blocks with
/// different gaps, so the only record of where a row was, was the pixels
/// already drawn -- which is why none of them could be clicked.
#[derive(Clone, Debug)]
enum SidebarRow {
    /// A section heading: CHANNELS, PRIVATE.
    Header(&'static str),
    /// Air between sections.
    Gap(f32),
    /// A row that switches to a panel.
    Item(ActivePanel),
}

impl SidebarRow {
    fn height(&self) -> f32 {
        match self {
            Self::Header(_) => SIDEBAR_HEADER_HEIGHT,
            Self::Gap(h) => *h,
            Self::Item(_) => SIDEBAR_ROW_STEP,
        }
    }
}

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
#[allow(dead_code)]
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const PINK: Color = Color::from_hex(0xF5C2E7);

// ============================================================================
// IRC protocol message parsing
// ============================================================================

/// A parsed IRC protocol message.
#[derive(Debug, Clone)]
pub struct IrcMessage {
    pub prefix: Option<String>,
    pub command: String,
    pub params: Vec<String>,
}

impl IrcMessage {
    /// Parse a raw IRC line into a structured message.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return None;
        }

        let mut rest = line;
        let prefix = if rest.starts_with(':') {
            let space = rest.find(' ')?;
            let p = rest.get(1..space)?.to_string();
            rest = rest.get(space.saturating_add(1)..)?;
            Some(p)
        } else {
            None
        };

        // Skip leading spaces
        rest = rest.trim_start();

        let (command, remainder) = if let Some(space) = rest.find(' ') {
            (
                rest.get(..space)?.to_uppercase(),
                rest.get(space.saturating_add(1)..)?,
            )
        } else {
            (rest.to_uppercase(), "")
        };

        let mut params = Vec::new();
        let mut rest = remainder;
        while !rest.is_empty() {
            rest = rest.trim_start();
            if let Some(stripped) = rest.strip_prefix(':') {
                params.push(stripped.to_string());
                break;
            }
            if let Some(space) = rest.find(' ') {
                params.push(rest.get(..space)?.to_string());
                rest = rest.get(space.saturating_add(1)..)?;
            } else {
                params.push(rest.to_string());
                break;
            }
        }

        Some(IrcMessage {
            prefix,
            command,
            params,
        })
    }

    /// Extract nickname from prefix (nick!user@host).
    pub fn nick(&self) -> Option<&str> {
        self.prefix
            .as_ref()
            .map(|p| p.split('!').next().unwrap_or(p))
    }

    /// Extract user from prefix.
    pub fn user(&self) -> Option<&str> {
        self.prefix.as_ref().and_then(|p| {
            let after_bang = p.split('!').nth(1)?;
            Some(after_bang.split('@').next().unwrap_or(after_bang))
        })
    }

    /// Extract host from prefix.
    pub fn host(&self) -> Option<&str> {
        self.prefix.as_ref().and_then(|p| p.split('@').nth(1))
    }

    /// Get trailing parameter (usually the message text).
    pub fn trailing(&self) -> Option<&str> {
        self.params.last().map(|s| s.as_str())
    }

    /// Get the target (first param, usually channel or nick).
    pub fn target(&self) -> Option<&str> {
        self.params.first().map(|s| s.as_str())
    }

    /// Serialize to IRC protocol wire format.
    pub fn to_wire(&self) -> String {
        let mut result = String::new();
        if let Some(ref prefix) = self.prefix {
            result.push(':');
            result.push_str(prefix);
            result.push(' ');
        }
        result.push_str(&self.command);
        if !self.params.is_empty() {
            let last_idx = self.params.len().saturating_sub(1);
            for (i, param) in self.params.iter().enumerate() {
                result.push(' ');
                if i == last_idx && (param.contains(' ') || param.starts_with(':')) {
                    result.push(':');
                }
                result.push_str(param);
            }
        }
        result.push_str("\r\n");
        result
    }
}

/// Generate common IRC commands.
pub fn cmd_nick(nick: &str) -> String {
    format!("NICK {nick}\r\n")
}

pub fn cmd_user(username: &str, realname: &str) -> String {
    format!("USER {username} 0 * :{realname}\r\n")
}

pub fn cmd_join(channel: &str) -> String {
    format!("JOIN {channel}\r\n")
}

pub fn cmd_join_with_key(channel: &str, key: &str) -> String {
    format!("JOIN {channel} {key}\r\n")
}

pub fn cmd_part(channel: &str, reason: &str) -> String {
    if reason.is_empty() {
        format!("PART {channel}\r\n")
    } else {
        format!("PART {channel} :{reason}\r\n")
    }
}

pub fn cmd_privmsg(target: &str, message: &str) -> String {
    format!("PRIVMSG {target} :{message}\r\n")
}

pub fn cmd_notice(target: &str, message: &str) -> String {
    format!("NOTICE {target} :{message}\r\n")
}

pub fn cmd_quit(reason: &str) -> String {
    if reason.is_empty() {
        "QUIT\r\n".to_string()
    } else {
        format!("QUIT :{reason}\r\n")
    }
}

pub fn cmd_ping(token: &str) -> String {
    format!("PING :{token}\r\n")
}

pub fn cmd_pong(token: &str) -> String {
    format!("PONG :{token}\r\n")
}

pub fn cmd_topic(channel: &str) -> String {
    format!("TOPIC {channel}\r\n")
}

pub fn cmd_set_topic(channel: &str, topic: &str) -> String {
    format!("TOPIC {channel} :{topic}\r\n")
}

pub fn cmd_kick(channel: &str, nick: &str, reason: &str) -> String {
    if reason.is_empty() {
        format!("KICK {channel} {nick}\r\n")
    } else {
        format!("KICK {channel} {nick} :{reason}\r\n")
    }
}

pub fn cmd_mode(target: &str, mode: &str) -> String {
    format!("MODE {target} {mode}\r\n")
}

pub fn cmd_whois(nick: &str) -> String {
    format!("WHOIS {nick}\r\n")
}

pub fn cmd_list() -> String {
    "LIST\r\n".to_string()
}

pub fn cmd_names(channel: &str) -> String {
    format!("NAMES {channel}\r\n")
}

pub fn cmd_away(message: &str) -> String {
    if message.is_empty() {
        "AWAY\r\n".to_string()
    } else {
        format!("AWAY :{message}\r\n")
    }
}

/// IRC numeric reply codes.
pub mod numerics {
    pub const RPL_WELCOME: &str = "001";
    pub const RPL_YOURHOST: &str = "002";
    pub const RPL_CREATED: &str = "003";
    pub const RPL_MYINFO: &str = "004";
    pub const RPL_ISUPPORT: &str = "005";
    pub const RPL_TOPIC: &str = "332";
    pub const RPL_TOPICWHOTIME: &str = "333";
    pub const RPL_NAMREPLY: &str = "353";
    pub const RPL_ENDOFNAMES: &str = "366";
    pub const RPL_MOTD: &str = "372";
    pub const RPL_MOTDSTART: &str = "375";
    pub const RPL_ENDOFMOTD: &str = "376";
    pub const RPL_WHOISUSER: &str = "311";
    pub const RPL_WHOISSERVER: &str = "312";
    pub const RPL_ENDOFWHOIS: &str = "318";
    pub const RPL_LIST: &str = "322";
    pub const RPL_LISTEND: &str = "323";
    pub const RPL_CHANNELMODEIS: &str = "324";
    pub const ERR_NOSUCHNICK: &str = "401";
    pub const ERR_NOSUCHCHANNEL: &str = "403";
    pub const ERR_CANNOTSENDTOCHAN: &str = "404";
    pub const ERR_NICKNAMEINUSE: &str = "433";
    pub const ERR_NOTONCHANNEL: &str = "442";
    pub const ERR_NEEDMOREPARAMS: &str = "461";
}

// ============================================================================
// CTCP (Client-To-Client Protocol)
// ============================================================================

/// CTCP message types.
#[derive(Debug, Clone)]
pub enum CtcpMessage {
    Version,
    Ping(String),
    Action(String),
    Time,
    Finger,
    Source,
    Unknown(String, String),
}

/// Strip the `\x01` delimiters that frame a CTCP payload, or `None` if `text`
/// is not framed as one.
///
/// `strip_prefix`/`strip_suffix` rather than a `starts_with`/`ends_with` test
/// followed by `&text[1..text.len() - 1]`, because the latter puts the proof
/// that the slice is in bounds in a *different statement* from the slice. It
/// was wrong: a lone `"\x01"` starts with `\x01` and also ends with it — the
/// same byte answering both tests — so the guard passed and the slice became
/// `1..0`, which panics. `text` here is a PRIVMSG trailing parameter, i.e.
/// whatever the server sent, so that was a one-line remote client kill.
///
/// Stripping in sequence cannot make that mistake: after the prefix is
/// removed, the suffix is looked for in what is *left*, and an empty string
/// has no `\x01` to end with.
fn ctcp_payload(text: &str) -> Option<&str> {
    text.strip_prefix('\x01')?.strip_suffix('\x01')
}

/// Split a CTCP payload into its verb and the rest, per the one-space
/// convention. A payload with no space is all verb and no argument.
fn ctcp_split_verb(inner: &str) -> (&str, &str) {
    inner.split_once(' ').unwrap_or((inner, ""))
}

impl CtcpMessage {
    /// Parse CTCP from a PRIVMSG trailing parameter.
    pub fn parse(text: &str) -> Option<Self> {
        let (cmd, rest) = ctcp_split_verb(ctcp_payload(text)?);

        // ASCII folding, not `to_uppercase`: CTCP verbs are ASCII by spec, and
        // `action_text` below has to agree with this match exactly. Unicode
        // folding would let the two disagree — `"actıon"` (dotless i)
        // uppercases to `"ACTION"` but is not `eq_ignore_ascii_case` to it.
        match cmd.to_ascii_uppercase().as_str() {
            "VERSION" => Some(Self::Version),
            "PING" => Some(Self::Ping(rest.to_string())),
            "ACTION" => Some(Self::Action(rest.to_string())),
            "TIME" => Some(Self::Time),
            "FINGER" => Some(Self::Finger),
            "SOURCE" => Some(Self::Source),
            _ => Some(Self::Unknown(cmd.to_string(), rest.to_string())),
        }
    }

    /// Whether `text` is a CTCP ACTION — defined as, and therefore unable to
    /// disagree with, [`Self::action_text`] finding one.
    pub fn is_action(text: &str) -> bool {
        Self::action_text(text).is_some()
    }

    /// The text of a CTCP ACTION, borrowed from `text`, or `None`.
    ///
    /// Recognises exactly what [`Self::parse`] reports as [`Self::Action`].
    /// It used to be its own parser — `starts_with("\x01ACTION")` and then
    /// `&text[8..text.len() - 1]` — and the two disagreed three ways, all of
    /// them reachable from the wire:
    ///
    /// * `"\x01ACTION\x01"` passed the guard at 8 bytes and sliced `8..7`,
    ///   which panics.
    /// * `"\x01ACTIONfoo\x01"` was an action with text `"oo"` here (the
    ///   hard-coded `8` assumed a space that was never checked for) and the
    ///   unknown verb `ACTIONFOO` in `parse`.
    /// * `"\x01ACTIONé \x01"` put index 8 in the middle of the `é`, which
    ///   panics on the char boundary.
    pub fn action_text(text: &str) -> Option<&str> {
        let (cmd, rest) = ctcp_split_verb(ctcp_payload(text)?);
        cmd.eq_ignore_ascii_case("ACTION").then_some(rest)
    }

    pub fn format_action(text: &str) -> String {
        format!("\x01ACTION {text}\x01")
    }

    pub fn format_version_reply(version: &str) -> String {
        format!("\x01VERSION {version}\x01")
    }
}

// ============================================================================
// User and channel types
// ============================================================================

/// User modes/prefixes in a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UserPrefix {
    Owner,
    Admin,
    Op,
    HalfOp,
    Voice,
    None,
}

impl UserPrefix {
    pub fn from_char(c: char) -> Self {
        match c {
            '~' => Self::Owner,
            '&' => Self::Admin,
            '@' => Self::Op,
            '%' => Self::HalfOp,
            '+' => Self::Voice,
            _ => Self::None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Owner => "~",
            Self::Admin => "&",
            Self::Op => "@",
            Self::HalfOp => "%",
            Self::Voice => "+",
            Self::None => "",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Owner => RED,
            Self::Admin => PEACH,
            Self::Op => GREEN,
            Self::HalfOp => YELLOW,
            Self::Voice => BLUE,
            Self::None => TEXT,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Op => "Operator",
            Self::HalfOp => "Half-Op",
            Self::Voice => "Voice",
            Self::None => "User",
        }
    }
}

/// A user in a channel.
#[derive(Debug, Clone)]
pub struct ChannelUser {
    pub nick: String,
    pub prefix: UserPrefix,
    pub away: bool,
}

impl ChannelUser {
    pub fn display_nick(&self) -> String {
        format!("{}{}", self.prefix.symbol(), self.nick)
    }

    pub fn from_names_entry(entry: &str) -> Self {
        let entry = entry.trim();
        if entry.is_empty() {
            return Self {
                nick: String::new(),
                prefix: UserPrefix::None,
                away: false,
            };
        }
        let first = entry.chars().next().unwrap_or(' ');
        let prefix = UserPrefix::from_char(first);
        let nick = if prefix != UserPrefix::None {
            entry[1..].to_string()
        } else {
            entry.to_string()
        };
        Self {
            nick,
            prefix,
            away: false,
        }
    }
}

/// Channel modes.
#[derive(Debug, Clone, Default)]
pub struct ChannelModes {
    pub invite_only: bool,
    pub moderated: bool,
    pub no_external: bool,
    pub topic_protected: bool,
    pub secret: bool,
    pub key: Option<String>,
    pub limit: Option<u32>,
}

impl ChannelModes {
    pub fn mode_string(&self) -> String {
        let mut modes = "+".to_string();
        if self.invite_only {
            modes.push('i');
        }
        if self.moderated {
            modes.push('m');
        }
        if self.no_external {
            modes.push('n');
        }
        if self.topic_protected {
            modes.push('t');
        }
        if self.secret {
            modes.push('s');
        }
        if self.key.is_some() {
            modes.push('k');
        }
        if self.limit.is_some() {
            modes.push('l');
        }
        if modes.len() == 1 {
            String::new()
        } else {
            modes
        }
    }
}

/// An IRC channel.
#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub topic: String,
    pub topic_set_by: Option<String>,
    pub users: Vec<ChannelUser>,
    pub modes: ChannelModes,
    pub messages: Vec<ChatMessage>,
    pub unread_count: u32,
    pub unread_mentions: u32,
    pub joined: bool,
    pub scroll_offset: f32,
}

impl Channel {
    pub fn new(name: String) -> Self {
        Self {
            name,
            topic: String::new(),
            topic_set_by: None,
            users: Vec::new(),
            modes: ChannelModes::default(),
            messages: Vec::new(),
            unread_count: 0,
            unread_mentions: 0,
            joined: false,
            scroll_offset: 0.0,
        }
    }

    pub fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.unread_count = self.unread_count.saturating_add(1);
    }

    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    pub fn find_user(&self, nick: &str) -> Option<&ChannelUser> {
        self.users
            .iter()
            .find(|u| u.nick.eq_ignore_ascii_case(nick))
    }

    pub fn find_user_mut(&mut self, nick: &str) -> Option<&mut ChannelUser> {
        self.users
            .iter_mut()
            .find(|u| u.nick.eq_ignore_ascii_case(nick))
    }

    pub fn add_user(&mut self, user: ChannelUser) {
        if self.find_user(&user.nick).is_none() {
            self.users.push(user);
        }
    }

    pub fn remove_user(&mut self, nick: &str) {
        self.users.retain(|u| !u.nick.eq_ignore_ascii_case(nick));
    }

    pub fn rename_user(&mut self, old: &str, new_nick: &str) {
        if let Some(u) = self.find_user_mut(old) {
            u.nick = new_nick.to_string();
        }
    }

    pub fn sorted_users(&self) -> Vec<&ChannelUser> {
        let mut sorted: Vec<&ChannelUser> = self.users.iter().collect();
        sorted.sort_by(|a, b| {
            a.prefix.cmp(&b.prefix).then_with(|| {
                a.nick
                    .to_ascii_lowercase()
                    .cmp(&b.nick.to_ascii_lowercase())
            })
        });
        sorted
    }

    pub fn mark_read(&mut self) {
        self.unread_count = 0;
        self.unread_mentions = 0;
    }

    pub fn has_unread(&self) -> bool {
        self.unread_count > 0
    }
}

/// A private message conversation.
#[derive(Debug, Clone)]
pub struct PrivateChat {
    pub nick: String,
    pub messages: Vec<ChatMessage>,
    pub unread_count: u32,
    pub scroll_offset: f32,
}

impl PrivateChat {
    pub fn new(nick: String) -> Self {
        Self {
            nick,
            messages: Vec::new(),
            unread_count: 0,
            scroll_offset: 0.0,
        }
    }

    pub fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.unread_count = self.unread_count.saturating_add(1);
    }

    pub fn mark_read(&mut self) {
        self.unread_count = 0;
    }
}

// ============================================================================
// Chat messages
// ============================================================================

/// Types of chat messages for display.
#[derive(Debug, Clone)]
pub enum ChatMessageKind {
    Normal,
    Action,
    Notice,
    Join,
    Part { reason: String },
    Quit { reason: String },
    Kick { by: String, reason: String },
    Nick { old: String },
    Topic { by: String },
    Mode { by: String, mode: String },
    System,
}

/// A chat message for display.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub timestamp: String,
    pub sender: String,
    pub text: String,
    pub kind: ChatMessageKind,
    pub highlight: bool,
}

impl ChatMessage {
    pub fn normal(time: &str, sender: &str, text: &str) -> Self {
        Self {
            timestamp: time.to_string(),
            sender: sender.to_string(),
            text: text.to_string(),
            kind: ChatMessageKind::Normal,
            highlight: false,
        }
    }

    pub fn action(time: &str, sender: &str, text: &str) -> Self {
        Self {
            timestamp: time.to_string(),
            sender: sender.to_string(),
            text: text.to_string(),
            kind: ChatMessageKind::Action,
            highlight: false,
        }
    }

    pub fn system(time: &str, text: &str) -> Self {
        Self {
            timestamp: time.to_string(),
            sender: String::new(),
            text: text.to_string(),
            kind: ChatMessageKind::System,
            highlight: false,
        }
    }

    pub fn color_for_nick(nick: &str) -> Color {
        // Deterministic color based on nick hash
        let mut hash: u32 = 5381;
        for byte in nick.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
        }
        let colors = [BLUE, GREEN, PEACH, MAUVE, TEAL, SKY, PINK, LAVENDER, YELLOW];
        let idx = (hash as usize).checked_rem(colors.len()).unwrap_or(0);
        colors.get(idx).copied().unwrap_or(TEXT)
    }
}

// ============================================================================
// Server connection state
// ============================================================================

/// Connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Registering,
    Connected,
    Reconnecting,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Registering => "Registering...",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting...",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Disconnected => RED,
            Self::Connecting | Self::Registering | Self::Reconnecting => YELLOW,
            Self::Connected => GREEN,
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    pub tls: bool,
    pub nick: String,
    pub username: String,
    pub realname: String,
    pub password: Option<String>,
    pub auto_join: Vec<String>,
    pub nickserv_pass: Option<String>,
}

impl ServerConfig {
    pub fn display_address(&self) -> String {
        let scheme = if self.tls { "ircs" } else { "irc" };
        format!("{scheme}://{}:{}", self.address, self.port)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "irc.libera.chat".to_string(),
            port: 6697,
            tls: true,
            nick: "SlateOSUser".to_string(),
            username: "slateos".to_string(),
            realname: "Slate OS IRC Client".to_string(),
            password: None,
            auto_join: vec!["#slateos".to_string()],
            nickserv_pass: None,
        }
    }
}

/// A saved server/network entry.
#[derive(Debug, Clone)]
pub struct SavedNetwork {
    pub name: String,
    pub config: ServerConfig,
    pub auto_connect: bool,
}

// ============================================================================
// Channel list (from LIST command)
// ============================================================================

/// Entry from channel listing.
#[derive(Debug, Clone)]
pub struct ChannelListEntry {
    pub name: String,
    pub user_count: u32,
    pub topic: String,
}

// ============================================================================
// Main application
// ============================================================================

/// Active panel in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivePanel {
    Channel(String),
    Private(String),
    Server,
}

/// The IRC client application.
pub struct IrcClientApp {
    pub width: f32,
    pub height: f32,

    // Connection
    pub connection: ConnectionState,
    pub server_config: ServerConfig,
    pub saved_networks: Vec<SavedNetwork>,
    pub my_nick: String,
    pub server_name: String,
    pub motd: Vec<String>,

    // Channels and PMs
    pub channels: Vec<Channel>,
    pub private_chats: Vec<PrivateChat>,
    pub active_panel: ActivePanel,

    // Server messages buffer
    pub server_messages: Vec<ChatMessage>,

    // Channel list (from LIST)
    pub channel_list: Vec<ChannelListEntry>,
    pub channel_list_visible: bool,
    pub channel_list_filter: String,

    // UI state
    /// How many lines the chat has been scrolled back from the newest.
    ///
    /// Counted backwards because a chat pins to its bottom: zero is "showing
    /// the latest message", which is where every conversation opens.
    pub chat_scroll: usize,
    pub input_text: String,
    pub input_history: Vec<String>,
    pub input_history_idx: Option<usize>,
    pub nick_list_visible: bool,
    pub show_timestamps: bool,

    // Notifications
    pub highlight_words: Vec<String>,
    pub notification_sound: bool,
    pub flash_on_mention: bool,
}

impl IrcClientApp {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            connection: ConnectionState::Disconnected,
            server_config: ServerConfig::default(),
            saved_networks: vec![
                SavedNetwork {
                    name: "Libera Chat".to_string(),
                    config: ServerConfig::default(),
                    auto_connect: true,
                },
                SavedNetwork {
                    name: "OFTC".to_string(),
                    config: ServerConfig {
                        address: "irc.oftc.net".to_string(),
                        port: 6697,
                        tls: true,
                        nick: "SlateOSUser".to_string(),
                        username: "slateos".to_string(),
                        realname: "Slate OS IRC Client".to_string(),
                        password: None,
                        auto_join: vec![],
                        nickserv_pass: None,
                    },
                    auto_connect: false,
                },
            ],
            my_nick: "SlateOSUser".to_string(),
            server_name: String::new(),
            motd: Vec::new(),
            channels: Vec::new(),
            private_chats: Vec::new(),
            active_panel: ActivePanel::Server,
            server_messages: Vec::new(),
            channel_list: Vec::new(),
            channel_list_visible: false,
            channel_list_filter: String::new(),
            chat_scroll: 0,
            input_text: String::new(),
            input_history: Vec::new(),
            input_history_idx: None,
            nick_list_visible: true,
            show_timestamps: true,
            highlight_words: Vec::new(),
            notification_sound: true,
            flash_on_mention: true,
        }
    }

    // ========================================================================
    // Channel management
    // ========================================================================

    pub fn join_channel(&mut self, name: &str) {
        if self.find_channel(name).is_none() {
            let mut ch = Channel::new(name.to_string());
            ch.joined = true;
            self.channels.push(ch);
        }
        self.active_panel = ActivePanel::Channel(name.to_string());
    }

    pub fn part_channel(&mut self, name: &str) {
        if let Some(ch) = self.find_channel_mut(name) {
            ch.joined = false;
        }
        if self.active_panel == ActivePanel::Channel(name.to_string()) {
            self.active_panel = ActivePanel::Server;
        }
    }

    pub fn find_channel(&self, name: &str) -> Option<&Channel> {
        self.channels
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn find_channel_mut(&mut self, name: &str) -> Option<&mut Channel> {
        self.channels
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    #[allow(
        clippy::expect_used,
        reason = "the index is either one just found or the element just pushed"
    )]
    pub fn get_or_create_pm(&mut self, nick: &str) -> &mut PrivateChat {
        let idx = self
            .private_chats
            .iter()
            .position(|p| p.nick.eq_ignore_ascii_case(nick));
        // The index is re-derived after the push rather than held across it,
        // because a `&mut` taken before the push would not survive it.
        let index = match idx {
            Some(i) => i,
            None => {
                self.private_chats.push(PrivateChat::new(nick.to_string()));
                self.private_chats.len().saturating_sub(1)
            }
        };
        self.private_chats
            .get_mut(index)
            .expect("index is either one just found or the one just pushed")
    }

    pub fn active_channel(&self) -> Option<&Channel> {
        match &self.active_panel {
            ActivePanel::Channel(name) => self.find_channel(name),
            _ => None,
        }
    }

    /// Process a parsed IRC message, updating state accordingly.
    pub fn handle_message(&mut self, msg: &IrcMessage) {
        match msg.command.as_str() {
            "PRIVMSG" => self.handle_privmsg(msg),
            "NOTICE" => self.handle_notice(msg),
            "JOIN" => self.handle_join(msg),
            "PART" => self.handle_part(msg),
            "QUIT" => self.handle_quit(msg),
            "NICK" => self.handle_nick_change(msg),
            "KICK" => self.handle_kick(msg),
            "TOPIC" => self.handle_topic(msg),
            "MODE" => self.handle_mode(msg),
            "PING" => { /* handled at protocol level */ }
            code if code.chars().all(|c| c.is_ascii_digit()) => {
                self.handle_numeric(msg);
            }
            _ => {
                if let Some(text) = msg.trailing() {
                    self.server_messages.push(ChatMessage::system("", text));
                }
            }
        }
    }

    fn handle_privmsg(&mut self, msg: &IrcMessage) {
        let sender = msg.nick().unwrap_or("unknown").to_string();
        let target = msg.target().unwrap_or("").to_string();
        let text = msg.trailing().unwrap_or("").to_string();

        // Check for CTCP ACTION
        let (display_text, is_action) = if let Some(action) = CtcpMessage::action_text(&text) {
            (action.to_string(), true)
        } else {
            (text, false)
        };

        let is_highlight = self.is_highlight(&display_text);

        let chat_msg = if is_action {
            let mut m = ChatMessage::action("", &sender, &display_text);
            m.highlight = is_highlight;
            m
        } else {
            let mut m = ChatMessage::normal("", &sender, &display_text);
            m.highlight = is_highlight;
            m
        };

        if target.starts_with('#') || target.starts_with('&') {
            if let Some(ch) = self.find_channel_mut(&target) {
                if is_highlight {
                    ch.unread_mentions = ch.unread_mentions.saturating_add(1);
                }
                ch.add_message(chat_msg);
            }
        } else {
            let pm = self.get_or_create_pm(&sender);
            pm.add_message(chat_msg);
        }
    }

    fn handle_notice(&mut self, msg: &IrcMessage) {
        let sender = msg.nick().unwrap_or("server").to_string();
        let text = msg.trailing().unwrap_or("").to_string();

        self.server_messages.push(ChatMessage {
            timestamp: String::new(),
            sender: sender.clone(),
            text,
            kind: ChatMessageKind::Notice,
            highlight: false,
        });
    }

    fn handle_join(&mut self, msg: &IrcMessage) {
        let nick = msg.nick().unwrap_or("").to_string();
        let channel = msg
            .trailing()
            .or_else(|| msg.target())
            .unwrap_or("")
            .to_string();

        if nick.eq_ignore_ascii_case(&self.my_nick) {
            self.join_channel(&channel);
        } else if let Some(ch) = self.find_channel_mut(&channel) {
            ch.add_user(ChannelUser {
                nick: nick.clone(),
                prefix: UserPrefix::None,
                away: false,
            });
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: nick,
                text: String::new(),
                kind: ChatMessageKind::Join,
                highlight: false,
            });
        }
    }

    fn handle_part(&mut self, msg: &IrcMessage) {
        let nick = msg.nick().unwrap_or("").to_string();
        let channel = msg.target().unwrap_or("").to_string();
        let reason = msg.trailing().unwrap_or("").to_string();

        if nick.eq_ignore_ascii_case(&self.my_nick) {
            self.part_channel(&channel);
        } else if let Some(ch) = self.find_channel_mut(&channel) {
            ch.remove_user(&nick);
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: nick,
                text: String::new(),
                kind: ChatMessageKind::Part { reason },
                highlight: false,
            });
        }
    }

    fn handle_quit(&mut self, msg: &IrcMessage) {
        let nick = msg.nick().unwrap_or("").to_string();
        let reason = msg.trailing().unwrap_or("").to_string();

        for ch in &mut self.channels {
            if ch.find_user(&nick).is_some() {
                ch.remove_user(&nick);
                ch.add_message(ChatMessage {
                    timestamp: String::new(),
                    sender: nick.clone(),
                    text: String::new(),
                    kind: ChatMessageKind::Quit {
                        reason: reason.clone(),
                    },
                    highlight: false,
                });
            }
        }
    }

    fn handle_nick_change(&mut self, msg: &IrcMessage) {
        let old_nick = msg.nick().unwrap_or("").to_string();
        let new_nick = msg
            .trailing()
            .or_else(|| msg.target())
            .unwrap_or("")
            .to_string();

        if old_nick.eq_ignore_ascii_case(&self.my_nick) {
            self.my_nick = new_nick.clone();
        }

        for ch in &mut self.channels {
            ch.rename_user(&old_nick, &new_nick);
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: new_nick.clone(),
                text: String::new(),
                kind: ChatMessageKind::Nick {
                    old: old_nick.clone(),
                },
                highlight: false,
            });
        }
    }

    fn handle_kick(&mut self, msg: &IrcMessage) {
        let kicker = msg.nick().unwrap_or("").to_string();
        let channel = msg.target().unwrap_or("").to_string();
        let kicked = msg.params.get(1).cloned().unwrap_or_default();
        let reason = msg.trailing().unwrap_or("").to_string();

        if kicked.eq_ignore_ascii_case(&self.my_nick) {
            self.part_channel(&channel);
        } else if let Some(ch) = self.find_channel_mut(&channel) {
            ch.remove_user(&kicked);
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: kicked,
                text: String::new(),
                kind: ChatMessageKind::Kick { by: kicker, reason },
                highlight: false,
            });
        }
    }

    fn handle_topic(&mut self, msg: &IrcMessage) {
        let setter = msg.nick().unwrap_or("").to_string();
        let channel = msg.target().unwrap_or("").to_string();
        let topic = msg.trailing().unwrap_or("").to_string();

        if let Some(ch) = self.find_channel_mut(&channel) {
            ch.topic = topic.clone();
            ch.topic_set_by = Some(setter.clone());
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: String::new(),
                text: topic,
                kind: ChatMessageKind::Topic { by: setter },
                highlight: false,
            });
        }
    }

    fn handle_mode(&mut self, msg: &IrcMessage) {
        let setter = msg.nick().unwrap_or("").to_string();
        let target = msg.target().unwrap_or("").to_string();
        let mode = msg.params.get(1).cloned().unwrap_or_default();

        if (target.starts_with('#') || target.starts_with('&'))
            && let Some(ch) = self.find_channel_mut(&target)
        {
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: String::new(),
                text: String::new(),
                kind: ChatMessageKind::Mode { by: setter, mode },
                highlight: false,
            });
        }
    }

    fn handle_numeric(&mut self, msg: &IrcMessage) {
        let code = &msg.command;
        let text = msg.trailing().unwrap_or("").to_string();

        match code.as_str() {
            numerics::RPL_TOPIC => {
                let channel = msg.params.get(1).cloned().unwrap_or_default();
                if let Some(ch) = self.find_channel_mut(&channel) {
                    ch.topic = text;
                }
            }
            numerics::RPL_NAMREPLY => {
                // Params: <nick> = <channel> :<names>
                let channel = msg.params.get(2).cloned().unwrap_or_default();
                let names: Vec<ChannelUser> = text
                    .split_whitespace()
                    .map(ChannelUser::from_names_entry)
                    .filter(|u| !u.nick.is_empty())
                    .collect();
                if let Some(ch) = self.find_channel_mut(&channel) {
                    for user in names {
                        ch.add_user(user);
                    }
                }
            }
            numerics::RPL_MOTD | numerics::RPL_MOTDSTART => {
                self.motd.push(text.clone());
                self.server_messages.push(ChatMessage::system("", &text));
            }
            numerics::RPL_WELCOME => {
                self.connection = ConnectionState::Connected;
                self.server_messages.push(ChatMessage::system("", &text));
            }
            numerics::RPL_LIST => {
                let channel = msg.params.get(1).cloned().unwrap_or_default();
                let count: u32 = msg.params.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                self.channel_list.push(ChannelListEntry {
                    name: channel,
                    user_count: count,
                    topic: text,
                });
            }
            numerics::ERR_NICKNAMEINUSE => {
                self.server_messages
                    .push(ChatMessage::system("", &format!("Nickname in use: {text}")));
            }
            _ => {
                if !text.is_empty() {
                    self.server_messages
                        .push(ChatMessage::system("", &format!("[{code}] {text}")));
                }
            }
        }
    }

    fn is_highlight(&self, text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        if lower.contains(&self.my_nick.to_ascii_lowercase()) {
            return true;
        }
        for word in &self.highlight_words {
            if lower.contains(&word.to_ascii_lowercase()) {
                return true;
            }
        }
        false
    }

    /// Parse user slash commands (/join, /part, /msg, etc.).
    pub fn parse_command(&self, input: &str) -> Option<String> {
        if !input.starts_with('/') {
            return None;
        }
        let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
        let cmd = parts.first()?.to_ascii_lowercase();
        let args = parts.get(1).unwrap_or(&"");

        match cmd.as_str() {
            "join" | "j" => Some(cmd_join(args.trim())),
            "part" | "leave" => {
                let channel = if args.is_empty() {
                    match &self.active_panel {
                        ActivePanel::Channel(name) => name.clone(),
                        _ => return None,
                    }
                } else {
                    args.trim().to_string()
                };
                Some(cmd_part(&channel, ""))
            }
            "msg" | "privmsg" => {
                let (target, body) = args.split_once(' ')?;
                Some(cmd_privmsg(target, body))
            }
            "nick" => Some(cmd_nick(args.trim())),
            "quit" | "exit" => Some(cmd_quit(args)),
            "topic" => {
                let channel = match &self.active_panel {
                    ActivePanel::Channel(name) => name.clone(),
                    _ => return None,
                };
                if args.is_empty() {
                    Some(cmd_topic(&channel))
                } else {
                    Some(cmd_set_topic(&channel, args))
                }
            }
            "kick" => {
                let channel = match &self.active_panel {
                    ActivePanel::Channel(name) => name.clone(),
                    _ => return None,
                };
                let kick_parts: Vec<&str> = args.splitn(2, ' ').collect();
                let nick = kick_parts.first().unwrap_or(&"");
                let reason = kick_parts.get(1).unwrap_or(&"");
                Some(cmd_kick(&channel, nick, reason))
            }
            "me" => {
                let target = match &self.active_panel {
                    ActivePanel::Channel(name) | ActivePanel::Private(name) => name.clone(),
                    ActivePanel::Server => return None,
                };
                Some(cmd_privmsg(&target, &CtcpMessage::format_action(args)))
            }
            "whois" => Some(cmd_whois(args.trim())),
            "away" => Some(cmd_away(args)),
            "mode" => {
                let channel = match &self.active_panel {
                    ActivePanel::Channel(name) => name.clone(),
                    _ => return None,
                };
                Some(cmd_mode(&channel, args))
            }
            "list" => Some(cmd_list()),
            "names" => {
                let channel = match &self.active_panel {
                    ActivePanel::Channel(name) => name.clone(),
                    _ => return None,
                };
                Some(cmd_names(&channel))
            }
            _ => None,
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    // ------------------------------------------------------------------
    // Layout the renderer draws and the pointer reads
    // ------------------------------------------------------------------

    /// Adopt a new window size. Returns whether it changed.
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

    /// The chat column: everything between the sidebar and the nick list,
    /// above the input line.
    pub fn chat_rect(&self) -> Rect {
        let nick_w = if self.nick_list_visible {
            NICK_LIST_WIDTH
        } else {
            0.0
        };
        Rect {
            x: SIDEBAR_WIDTH,
            y: CONTENT_TOP,
            width: (self.width - SIDEBAR_WIDTH - nick_w).max(1.0),
            height: (self.height - CONTENT_TOP - INPUT_HEIGHT).max(1.0),
        }
    }

    /// The nick list down the right-hand side, if it is showing.
    pub fn nick_list_rect(&self) -> Option<Rect> {
        if !self.nick_list_visible {
            return None;
        }
        let chat = self.chat_rect();
        Some(Rect {
            x: chat.x + chat.width,
            y: CONTENT_TOP,
            width: NICK_LIST_WIDTH,
            height: chat.height,
        })
    }

    /// The line that is typed into.
    pub fn input_rect(&self) -> Rect {
        let chat = self.chat_rect();
        Rect {
            x: chat.x,
            y: chat.y + chat.height,
            width: self.width - SIDEBAR_WIDTH,
            height: INPUT_HEIGHT,
        }
    }

    /// The sidebar, row by row, in the order they are drawn.
    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut rows = vec![
            SidebarRow::Gap(4.0),
            SidebarRow::Item(ActivePanel::Server),
            SidebarRow::Gap(SIDEBAR_ROW_STEP - SIDEBAR_ROW_HEIGHT),
            SidebarRow::Header("CHANNELS"),
        ];
        for ch in &self.channels {
            if ch.joined {
                rows.push(SidebarRow::Item(ActivePanel::Channel(ch.name.clone())));
            }
        }
        rows.push(SidebarRow::Gap(8.0));
        rows.push(SidebarRow::Header("DIRECT MESSAGES"));
        for pm in &self.private_chats {
            rows.push(SidebarRow::Item(ActivePanel::Private(pm.nick.clone())));
        }
        rows
    }

    /// Which panel a point in the sidebar switches to, if any.
    pub fn sidebar_panel_at(&self, x: f32, y: f32) -> Option<ActivePanel> {
        if x < 0.0 || x >= SIDEBAR_WIDTH {
            return None;
        }
        let mut row_y = CONTENT_TOP;
        for row in self.sidebar_rows() {
            let next = row_y + row.height();
            if y < next {
                return match row {
                    // A row's box is shorter than its step, so the gap under
                    // one belongs to neither it nor the next.
                    SidebarRow::Item(panel) if y < row_y + SIDEBAR_ROW_HEIGHT => Some(panel),
                    _ => None,
                };
            }
            row_y = next;
        }
        None
    }

    /// The messages showing in the chat column.
    pub fn visible_messages(&self) -> &[ChatMessage] {
        let all = match &self.active_panel {
            ActivePanel::Channel(name) => self.find_channel(name).map(|ch| &ch.messages),
            ActivePanel::Private(nick) => self
                .private_chats
                .iter()
                .find(|p| p.nick == *nick)
                .map(|p| &p.messages),
            ActivePanel::Server => Some(&self.server_messages),
        };
        let Some(all) = all else {
            return &[];
        };
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "a positive height over a positive row height"
        )]
        let capacity = (self.chat_rect().height / MESSAGE_HEIGHT) as usize;
        // A chat pins to the newest line, so the scroll is counted backwards
        // from the end -- `chat_scroll` is how many lines have been scrolled
        // up out of the bottom.
        let end = all.len().saturating_sub(self.chat_scroll);
        let start = end.saturating_sub(capacity);
        all.get(start..end).unwrap_or(&[])
    }

    /// How far back the chat can be scrolled before it runs out of history.
    pub fn max_chat_scroll(&self) -> usize {
        let total = match &self.active_panel {
            ActivePanel::Channel(name) => self.find_channel(name).map_or(0, |ch| ch.messages.len()),
            ActivePanel::Private(nick) => self
                .private_chats
                .iter()
                .find(|p| p.nick == *nick)
                .map_or(0, |p| p.messages.len()),
            ActivePanel::Server => self.server_messages.len(),
        };
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "a positive height over a positive row height"
        )]
        let capacity = (self.chat_rect().height / MESSAGE_HEIGHT) as usize;
        total.saturating_sub(capacity)
    }

    /// Scroll the chat back by `lines`, stopping at the oldest message.
    pub fn scroll_chat(&mut self, lines: isize) {
        let wanted = self.chat_scroll.saturating_add_signed(lines);
        self.chat_scroll = wanted.min(self.max_chat_scroll());
    }

    /// The nicks showing in the nick list, in the order they are drawn.
    pub fn visible_nicks(&self) -> Vec<String> {
        self.active_channel()
            .map(|ch| ch.users.iter().map(|u| u.nick.clone()).collect())
            .unwrap_or_default()
    }

    /// Which nick a point in the nick list is on, if any.
    pub fn nick_at(&self, x: f32, y: f32) -> Option<String> {
        let rect = self.nick_list_rect()?;
        if !rect.contains(x, y) {
            return None;
        }
        // The list starts a heading's height below the top of the panel.
        let list_top = rect.y + NICK_LIST_HEADER_HEIGHT;
        if y < list_top {
            return None;
        }
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "guarded at or below list_top just above"
        )]
        let row = ((y - list_top) / NICK_ROW_HEIGHT) as usize;
        self.visible_nicks().get(row).cloned()
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle one input event. Returns whether anything changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        let (x, y) = (event.x, event.y);
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(panel) = self.sidebar_panel_at(x, y) {
                    self.switch_panel(panel);
                    return true;
                }
                if let Some(nick) = self.nick_at(x, y) {
                    // Clicking a nick opens a conversation with them, which is
                    // what `get_or_create_pm` was written for.
                    self.get_or_create_pm(&nick);
                    self.switch_panel(ActivePanel::Private(nick));
                    return true;
                }
                false
            }
            MouseEventKind::Scroll { dy, .. } => {
                if !self.chat_rect().contains(x, y) {
                    return false;
                }
                // `dy` is in notches, positive away from the user, which
                // scrolls towards the start -- that is, back through history.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a notch count is small and whole in every case that matters"
                )]
                let lines = (dy * SCROLL_LINES_PER_NOTCH) as isize;
                if lines == 0 {
                    return false;
                }
                let before = self.chat_scroll;
                self.scroll_chat(lines);
                self.chat_scroll != before
            }
            _ => false,
        }
    }

    /// Show a panel, and put the chat back at the newest line.
    pub fn switch_panel(&mut self, panel: ActivePanel) {
        if self.active_panel == panel {
            return;
        }
        self.active_panel = panel;
        // A position forty lines back in one conversation names nothing in
        // another, and an unread channel opened part-way up its own history
        // hides the message that made it unread.
        self.chat_scroll = 0;
        if let ActivePanel::Channel(name) = self.active_panel.clone()
            && let Some(ch) = self.find_channel_mut(&name)
        {
            ch.mark_read();
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Enter => self.submit_input(),
            Key::Backspace => self.input_text.pop().is_some(),
            Key::Up => self.recall_history(1),
            Key::Down => self.recall_history(-1),
            Key::PageUp => {
                let before = self.chat_scroll;
                self.scroll_chat(PAGE_SCROLL_LINES);
                self.chat_scroll != before
            }
            Key::PageDown => {
                let before = self.chat_scroll;
                self.scroll_chat(-PAGE_SCROLL_LINES);
                self.chat_scroll != before
            }
            Key::Escape => {
                if self.input_text.is_empty() {
                    return false;
                }
                self.input_text.clear();
                self.input_history_idx = None;
                true
            }
            Key::Tab => {
                self.nick_list_visible = !self.nick_list_visible;
                true
            }
            _ => {
                let typed: String = event.typed().collect();
                if typed.is_empty() {
                    return false;
                }
                self.input_text.push_str(&typed);
                // Typing leaves the history: the line being edited is the
                // user's own, not the recalled one it started from.
                self.input_history_idx = None;
                true
            }
        }
    }

    /// Step back (`1`) or forward (`-1`) through what has been typed before.
    ///
    /// `input_history` and `input_history_idx` have been in this struct since
    /// it was written and nothing ever pushed to or read them.
    fn recall_history(&mut self, direction: isize) -> bool {
        if self.input_history.is_empty() {
            return false;
        }
        let last = self.input_history.len().saturating_sub(1);
        let next = match (self.input_history_idx, direction) {
            // Up out of the empty line lands on the most recent thing said.
            (None, d) if d > 0 => Some(last),
            (None, _) => None,
            // Down from the newest is back to the line being written.
            (Some(i), d) if d < 0 && i >= last => None,
            (Some(i), d) => Some(if d > 0 {
                i.saturating_sub(1)
            } else {
                i.saturating_add(1)
            }),
        };
        if next == self.input_history_idx {
            return false;
        }
        self.input_history_idx = next;
        self.input_text = next
            .and_then(|i| self.input_history.get(i))
            .cloned()
            .unwrap_or_default();
        true
    }

    /// Send whatever has been typed. Returns whether anything changed.
    pub fn submit_input(&mut self) -> bool {
        let line = std::mem::take(&mut self.input_text);
        self.input_history_idx = None;
        if line.trim().is_empty() {
            return false;
        }
        self.input_history.push(line.clone());
        self.chat_scroll = 0;

        if line.starts_with('/') {
            self.run_command(&line);
        } else {
            self.say(&line);
        }
        true
    }

    /// Put a line of one's own into the conversation on screen.
    fn say(&mut self, text: &str) {
        let nick = self.my_nick.clone();
        let stamp = self.timestamp();
        let message = ChatMessage::normal(&stamp, &nick, text);
        match self.active_panel.clone() {
            ActivePanel::Channel(name) => {
                if let Some(ch) = self.find_channel_mut(&name) {
                    ch.add_message(message);
                    ch.mark_read();
                }
            }
            ActivePanel::Private(target) => {
                let pm = self.get_or_create_pm(&target);
                pm.messages.push(message);
            }
            ActivePanel::Server => {
                // The server panel is a log, not a conversation.
                self.server_messages.push(ChatMessage::system(
                    &stamp,
                    "You can only talk in a channel or a private chat.",
                ));
            }
        }
    }

    /// Carry out a slash command.
    ///
    /// This client has no socket: `parse_command` builds the line a server
    /// would be sent, and the commands whose effect is local are applied here
    /// as well, so that typing `/join #x` opens the channel rather than only
    /// composing a string. The rest are logged as what would have gone out,
    /// which is the truthful thing to show for a client that cannot send.
    fn run_command(&mut self, line: &str) {
        let stamp = self.timestamp();
        let Some(wire) = self.parse_command(line) else {
            self.server_messages.push(ChatMessage::system(
                &stamp,
                &format!("Unknown or incomplete command: {line}"),
            ));
            return;
        };

        let rest = line.get(1..).unwrap_or("");
        let (cmd, args) = rest.split_once(' ').unwrap_or((rest, ""));
        match cmd.to_ascii_lowercase().as_str() {
            "join" | "j" => {
                let name = args.trim();
                if !name.is_empty() {
                    self.join_channel(name);
                    self.switch_panel(ActivePanel::Channel(name.to_string()));
                }
            }
            "part" | "leave" => {
                let name = if args.trim().is_empty() {
                    match &self.active_panel {
                        ActivePanel::Channel(n) => n.clone(),
                        _ => String::new(),
                    }
                } else {
                    args.trim().to_string()
                };
                if !name.is_empty() {
                    self.part_channel(&name);
                    self.switch_panel(ActivePanel::Server);
                }
            }
            "nick" => {
                let new_nick = args.trim();
                if !new_nick.is_empty() {
                    self.my_nick = new_nick.to_string();
                }
            }
            "me" => {
                let nick = self.my_nick.clone();
                let message = ChatMessage::action(&stamp, &nick, args);
                if let ActivePanel::Channel(name) = self.active_panel.clone()
                    && let Some(ch) = self.find_channel_mut(&name)
                {
                    ch.add_message(message);
                }
            }
            "query" | "msg" | "privmsg" => {
                let (target, body) = args.split_once(' ').unwrap_or((args.trim(), ""));
                if !target.is_empty() {
                    let nick = self.my_nick.clone();
                    let pm = self.get_or_create_pm(target);
                    if !body.is_empty() {
                        pm.messages.push(ChatMessage::normal(&stamp, &nick, body));
                    }
                    self.switch_panel(ActivePanel::Private(target.to_string()));
                }
            }
            _ => {}
        }

        self.server_messages
            .push(ChatMessage::system(&stamp, &format!("-> {wire}")));
    }

    /// The time a message is stamped with.
    ///
    /// This tree has no clock a userspace program can read yet, so a message
    /// takes the stamp of the one before it rather than inventing a time that
    /// would be wrong in a way nobody could see. See `todo.txt`.
    fn timestamp(&self) -> String {
        self.active_channel()
            .and_then(|ch| ch.messages.last())
            .or_else(|| self.server_messages.last())
            .map_or_else(|| "--:--".to_string(), |m| m.timestamp.clone())
    }

    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title bar
        self.render_title_bar(&mut cmds);

        let content_y = 32.0;
        let sidebar_w = 180.0;
        let input_h = 36.0;
        let nick_list_w = if self.nick_list_visible { 160.0 } else { 0.0 };
        let chat_x = sidebar_w;
        let chat_w = self.width - sidebar_w - nick_list_w;
        let chat_h = self.height - content_y - input_h;

        // Channel sidebar
        self.render_sidebar(&mut cmds, content_y);

        // Chat area
        self.render_chat_area(&mut cmds, chat_x, content_y, chat_w, chat_h);

        // Nick list
        if self.nick_list_visible {
            self.render_nick_list(&mut cmds, chat_x + chat_w, content_y, nick_list_w, chat_h);
        }

        // Input area
        self.render_input(
            &mut cmds,
            chat_x,
            content_y + chat_h,
            chat_w + nick_list_w,
            input_h,
        );

        cmds
    }

    fn render_title_bar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: 30.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Connection status
        cmds.push(RenderCommand::FillRect {
            x: 8.0,
            y: 8.0,
            width: 10.0,
            height: 10.0,
            color: self.connection.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 8.0,
            text: format!(
                "{} - {}",
                self.server_config.display_address(),
                self.connection.label()
            ),
            font_size: 12.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Nick display
        cmds.push(RenderCommand::Text {
            x: self.width - 200.0,
            y: 8.0,
            text: format!("Nick: {}", self.my_nick),
            font_size: 12.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Topic (if in channel)
        if let Some(ch) = self.active_channel()
            && !ch.topic.is_empty()
        {
            cmds.push(RenderCommand::Text {
                x: 350.0,
                y: 8.0,
                text: ch.topic.clone(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 600.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: 30.0,
            x2: self.width,
            y2: 30.0,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, top_y: f32) {
        let sidebar_w = SIDEBAR_WIDTH;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top_y,
            width: sidebar_w,
            height: self.height - top_y,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Walked from the same row list the pointer is tested against, so a
        // row drawn here is a row that can be clicked.
        let mut row_y = top_y;
        for row in self.sidebar_rows() {
            match &row {
                SidebarRow::Gap(_) => {}
                SidebarRow::Header(text) => cmds.push(RenderCommand::Text {
                    x: 12.0,
                    y: row_y,
                    text: (*text).to_owned(),
                    font_size: 9.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(sidebar_w - 24.0),
                    overflow: TextOverflow::Ellipsis,
                }),
                SidebarRow::Item(panel) => {
                    self.render_sidebar_item(cmds, row_y, sidebar_w, panel);
                }
            }
            row_y += row.height();
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sidebar_w,
            y1: top_y,
            x2: sidebar_w,
            y2: self.height,
            color: SURFACE0,
            width: 1.0,
        });
    }

    /// One selectable row of the sidebar.
    fn render_sidebar_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        row_y: f32,
        sidebar_w: f32,
        panel: &ActivePanel,
    ) {
        let is_active = self.active_panel == *panel;

        cmds.push(RenderCommand::FillRect {
            x: 4.0,
            y: row_y,
            width: sidebar_w - 8.0,
            height: SIDEBAR_ROW_HEIGHT,
            color: if is_active { SURFACE0 } else { MANTLE },
            corner_radii: CornerRadii::all(4.0),
        });

        let (label, color, weight, badge) = match panel {
            ActivePanel::Server => (
                "Server".to_owned(),
                if is_active { TEXT } else { SUBTEXT0 },
                FontWeightHint::Bold,
                None,
            ),
            ActivePanel::Channel(name) => {
                let ch = self.find_channel(name);
                let unread = ch.map_or(0, |c| c.unread_count);
                let mentions = ch.map_or(0, |c| c.unread_mentions);
                let has_unread = ch.is_some_and(Channel::has_unread);
                let color = if mentions > 0 {
                    RED
                } else if has_unread {
                    TEXT
                } else if is_active {
                    BLUE
                } else {
                    SUBTEXT0
                };
                (
                    name.clone(),
                    color,
                    if has_unread {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    (unread > 0).then_some((unread, mentions)),
                )
            }
            ActivePanel::Private(nick) => {
                let unread = self
                    .private_chats
                    .iter()
                    .find(|p| p.nick == *nick)
                    .map_or(0, |p| p.unread_count);
                let color = if unread > 0 {
                    TEXT
                } else if is_active {
                    BLUE
                } else {
                    SUBTEXT0
                };
                (
                    nick.clone(),
                    color,
                    if unread > 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    None,
                )
            }
        };

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: row_y + 5.0,
            text: label,
            font_size: if matches!(panel, ActivePanel::Server) {
                12.0
            } else {
                11.0
            },
            color,
            font_weight: weight,
            max_width: Some(sidebar_w - if badge.is_some() { 50.0 } else { 24.0 }),
            overflow: TextOverflow::Ellipsis,
        });

        if let Some((unread, mentions)) = badge {
            let badge_text = if mentions > 0 {
                mentions.to_string()
            } else {
                unread.to_string()
            };
            let badge_color = if mentions > 0 { RED } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: sidebar_w - 36.0,
                y: row_y + 4.0,
                width: 24.0,
                height: 16.0,
                color: badge_color,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: sidebar_w - 32.0,
                y: row_y + 6.0,
                text: badge_text,
                font_size: 9.0,
                color: if mentions > 0 { CRUST } else { TEXT },
                font_weight: FontWeightHint::Bold,
                max_width: Some(20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
    fn render_chat_area(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: w,
            height: h,
        });

        let messages = match &self.active_panel {
            ActivePanel::Channel(name) => self.find_channel(name).map(|ch| &ch.messages),
            ActivePanel::Private(nick) => self
                .private_chats
                .iter()
                .find(|p| p.nick == *nick)
                .map(|p| &p.messages),
            ActivePanel::Server => Some(&self.server_messages),
        };

        if messages.is_some() {
            // The same window the scroll keys and the wheel move, so what is
            // drawn is what has been scrolled to.
            for (i, msg) in self.visible_messages().iter().enumerate() {
                let my = y + i as f32 * MESSAGE_HEIGHT;
                self.render_chat_message(cmds, x + 8.0, my, w - 16.0, msg);
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 40.0,
                y: y + h / 2.0,
                text: "No messages".to_string(),
                font_size: 14.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_chat_message(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        _w: f32,
        msg: &ChatMessage,
    ) {
        if msg.highlight {
            cmds.push(RenderCommand::FillRect {
                x: x - 4.0,
                y,
                width: _w + 8.0,
                height: 18.0,
                color: Color::rgba(243, 139, 168, 30),
                corner_radii: CornerRadii::ZERO,
            });
        }

        let mut tx = x;

        // Timestamp
        if self.show_timestamps && !msg.timestamp.is_empty() {
            cmds.push(RenderCommand::Text {
                x: tx,
                y: y + 2.0,
                text: msg.timestamp.clone(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += 60.0;
        }

        match &msg.kind {
            ChatMessageKind::Normal => {
                let nick_color = ChatMessage::color_for_nick(&msg.sender);
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("<{}>", msg.sender),
                    font_size: 11.0,
                    color: nick_color,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                    overflow: TextOverflow::Ellipsis,
                });
                tx += 100.0;
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: msg.text.clone(),
                    font_size: 11.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Action => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("* {} {}", msg.sender, msg.text),
                    font_size: 11.0,
                    color: MAUVE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Notice => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("-{}- {}", msg.sender, msg.text),
                    font_size: 11.0,
                    color: PEACH,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Join => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("--> {} has joined", msg.sender),
                    font_size: 10.0,
                    color: GREEN,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Part { reason } => {
                let reason_str = if reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({reason})")
                };
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("<-- {} has left{reason_str}", msg.sender),
                    font_size: 10.0,
                    color: RED,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Quit { reason } => {
                let reason_str = if reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({reason})")
                };
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("<-- {} has quit{reason_str}", msg.sender),
                    font_size: 10.0,
                    color: RED,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Kick { by, reason } => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("*** {} was kicked by {} ({})", msg.sender, by, reason),
                    font_size: 10.0,
                    color: RED,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Nick { old } => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("*** {old} is now known as {}", msg.sender),
                    font_size: 10.0,
                    color: TEAL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Topic { by } => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("*** {by} changed the topic to: {}", msg.text),
                    font_size: 10.0,
                    color: YELLOW,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::Mode { by, mode } => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: format!("*** {by} sets mode {mode}"),
                    font_size: 10.0,
                    color: TEAL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ChatMessageKind::System => {
                cmds.push(RenderCommand::Text {
                    x: tx,
                    y: y + 2.0,
                    text: msg.text.clone(),
                    font_size: 10.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_nick_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x,
            y2: y + h,
            color: SURFACE0,
            width: 1.0,
        });

        if let Some(ch) = self.active_channel() {
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 6.0,
                text: format!("Users ({})", ch.user_count()),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            let sorted = ch.sorted_users();
            let mut current_prefix: Option<UserPrefix> = None;

            let mut row_y = y + 24.0;
            for user in sorted.iter().take(((h - 24.0) / 18.0) as usize) {
                // Section header for prefix groups
                if current_prefix != Some(user.prefix) && user.prefix != UserPrefix::None {
                    cmds.push(RenderCommand::Text {
                        x: x + 8.0,
                        y: row_y,
                        text: user.prefix.label().to_string(),
                        font_size: 9.0,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(w - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    row_y += 14.0;
                    current_prefix = Some(user.prefix);
                }

                let color = if user.away {
                    OVERLAY0
                } else {
                    user.prefix.color()
                };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: row_y,
                    text: user.display_nick(),
                    font_size: 11.0,
                    color,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 24.0),
                    overflow: TextOverflow::Ellipsis,
                });
                row_y += 18.0;
            }
        }
    }

    fn render_input(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });

        // Input field
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: y + 6.0,
            width: w - 16.0,
            height: h - 12.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        let display_text = if self.input_text.is_empty() {
            "Type a message... (/help for commands)".to_string()
        } else {
            self.input_text.clone()
        };

        let text_color = if self.input_text.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: display_text,
            font_size: 12.0,
            color: text_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Sample data and main
// ============================================================================

fn seeded_client() -> IrcClientApp {
    let mut app = IrcClientApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app.connection = ConnectionState::Connected;
    app.server_name = "irc.libera.chat".to_string();
    app.my_nick = "SlateOSUser".to_string();

    // Create channels with sample data
    app.join_channel("#slateos");
    app.join_channel("#rust");

    if let Some(ch) = app.find_channel_mut("#slateos") {
        ch.topic = "Slate OS Development | https://slateos.dev".to_string();
        ch.add_user(ChannelUser {
            nick: "SlateOSUser".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "alice".to_string(),
            prefix: UserPrefix::Voice,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "bob".to_string(),
            prefix: UserPrefix::None,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "charlie".to_string(),
            prefix: UserPrefix::None,
            away: true,
        });

        ch.add_message(ChatMessage::system("12:00", "Welcome to #slateos!"));
        ch.add_message(ChatMessage::normal("12:01", "alice", "Hey everyone!"));
        ch.add_message(ChatMessage::normal(
            "12:02",
            "bob",
            "Working on the new kernel module",
        ));
        ch.add_message(ChatMessage::action("12:03", "alice", "is reviewing PRs"));
        ch.add_message(ChatMessage::normal(
            "12:05",
            "bob",
            "The scheduler benchmarks look great",
        ));
        ch.mark_read();
    }

    if let Some(ch) = app.find_channel_mut("#rust") {
        ch.topic = "The Rust Programming Language".to_string();
        ch.add_user(ChannelUser {
            nick: "rustbot".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "ferris".to_string(),
            prefix: UserPrefix::None,
            away: false,
        });
        ch.unread_count = 3;
    }

    // Test message parsing
    let raw = ":alice!user@host PRIVMSG #slateos :Hello world!";
    if let Some(msg) = IrcMessage::parse(raw) {
        app.handle_message(&msg);
    }
    app
}

impl App for IrcClientApp {
    fn title(&self) -> String {
        // Where you are talking, and how much you have not read, because that
        // is what a chat window is left open for.
        let unread: u32 = self.channels.iter().map(|ch| ch.unread_count).sum();
        let here = match &self.active_panel {
            ActivePanel::Server => self.server_name.clone(),
            ActivePanel::Channel(name) => name.clone(),
            ActivePanel::Private(nick) => nick.clone(),
        };
        if unread == 0 {
            format!("{here} - IRC")
        } else {
            format!("({unread}) {here} - IRC")
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

    /// No clock.
    ///
    /// Messages arrive from a server, not from a timer, and this client has no
    /// socket to receive them on yet. Asking the harness for a tick would wake
    /// the machine on a schedule to find the same conversation still there --
    /// `known-issues.md` lesson 47. See `todo.txt` for what changes when there
    /// is a transport.
    fn tick_interval(&self) -> Option<Duration> {
        None
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
        // before any `Event::Resize` arrives, so a window opened at another
        // size would be laid out for the size that was asked for, and every
        // hit box in it would name the wrong rectangle.
        self.set_window_size(width, height);
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    let mut app = seeded_client();
    app::launch("ircclient", &mut app)
}

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

    // IRC message parsing
    #[test]
    fn test_parse_simple() {
        let msg = IrcMessage::parse("PING :token123").unwrap();
        assert_eq!(msg.command, "PING");
        assert_eq!(msg.trailing(), Some("token123"));
    }

    #[test]
    fn test_parse_with_prefix() {
        let msg = IrcMessage::parse(":nick!user@host PRIVMSG #channel :Hello world").unwrap();
        assert_eq!(msg.prefix, Some("nick!user@host".to_string()));
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.target(), Some("#channel"));
        assert_eq!(msg.trailing(), Some("Hello world"));
    }

    #[test]
    fn test_parse_nick() {
        let msg = IrcMessage::parse(":alice!~user@example.com PRIVMSG #test :hi").unwrap();
        assert_eq!(msg.nick(), Some("alice"));
        assert_eq!(msg.user(), Some("~user"));
        assert_eq!(msg.host(), Some("example.com"));
    }

    #[test]
    fn test_parse_numeric() {
        let msg = IrcMessage::parse(":server 001 nick :Welcome to the network").unwrap();
        assert_eq!(msg.command, "001");
        assert_eq!(msg.trailing(), Some("Welcome to the network"));
    }

    #[test]
    fn test_parse_join() {
        let msg = IrcMessage::parse(":nick!user@host JOIN #channel").unwrap();
        assert_eq!(msg.command, "JOIN");
        assert_eq!(msg.target(), Some("#channel"));
    }

    #[test]
    fn test_parse_empty() {
        assert!(IrcMessage::parse("").is_none());
    }

    #[test]
    fn test_to_wire() {
        let msg = IrcMessage {
            prefix: None,
            command: "PRIVMSG".to_string(),
            params: vec!["#channel".to_string(), "Hello world".to_string()],
        };
        assert_eq!(msg.to_wire(), "PRIVMSG #channel :Hello world\r\n");
    }

    // Command generation
    #[test]
    fn test_cmd_nick() {
        assert_eq!(cmd_nick("test"), "NICK test\r\n");
    }

    #[test]
    fn test_cmd_join() {
        assert_eq!(cmd_join("#channel"), "JOIN #channel\r\n");
    }

    #[test]
    fn test_cmd_privmsg() {
        assert_eq!(
            cmd_privmsg("#channel", "hello"),
            "PRIVMSG #channel :hello\r\n"
        );
    }

    #[test]
    fn test_cmd_part() {
        assert_eq!(cmd_part("#channel", "bye"), "PART #channel :bye\r\n");
        assert_eq!(cmd_part("#channel", ""), "PART #channel\r\n");
    }

    #[test]
    fn test_cmd_quit() {
        assert_eq!(cmd_quit("leaving"), "QUIT :leaving\r\n");
        assert_eq!(cmd_quit(""), "QUIT\r\n");
    }

    // CTCP
    #[test]
    fn test_ctcp_action() {
        let text = "\x01ACTION waves\x01";
        assert!(CtcpMessage::is_action(text));
        assert_eq!(CtcpMessage::action_text(text), Some("waves"));
    }

    /// Every one of these strings used to panic the client, and every one of
    /// them is something a server can put in a PRIVMSG. The old framing test
    /// (`starts_with('\x01') && ends_with('\x01')`) let a single `\x01` answer
    /// both halves, and the old ACTION parser hard-coded the byte offset 8.
    #[test]
    fn hostile_ctcp_framing_does_not_panic() {
        // One byte answering both `starts_with` and `ends_with`: sliced 1..0.
        assert!(CtcpMessage::parse("\x01").is_none());
        assert!(CtcpMessage::action_text("\x01").is_none());
        // Empty, and unframed-but-suffixed, and prefixed-but-unterminated.
        for text in ["", "\x01ACTION", "ACTION\x01", "\x01\x01"] {
            let _ = CtcpMessage::parse(text);
            let _ = CtcpMessage::action_text(text);
        }
        // 8 bytes exactly: the old `&text[8..text.len() - 1]` was `8..7`.
        assert_eq!(CtcpMessage::action_text("\x01ACTION\x01"), Some(""));
        // Index 8 landed inside the `é`, panicking on the char boundary.
        assert_eq!(CtcpMessage::action_text("\x01ACTIONé \x01"), None);
        assert_eq!(CtcpMessage::action_text("\x01ACTION é\x01"), Some("é"));
    }

    /// `action_text` and `parse` are one recogniser, so they cannot report a
    /// different verb for the same string. `\x01ACTIONfoo\x01` used to be an
    /// action with the text `"oo"` to one and the unknown verb `ACTIONFOO` to
    /// the other.
    #[test]
    fn action_text_agrees_with_parse() {
        for text in [
            "\x01ACTION waves\x01",
            "\x01ACTIONfoo\x01",
            "\x01ACTION\x01",
            "\x01action waves\x01",
            "\x01VERSION\x01",
            "\x01\x01",
            "plain text",
        ] {
            let by_parse = match CtcpMessage::parse(text) {
                Some(CtcpMessage::Action(a)) => Some(a),
                _ => None,
            };
            assert_eq!(
                by_parse.as_deref(),
                CtcpMessage::action_text(text),
                "disagreement on {text:?}"
            );
        }
    }

    #[test]
    fn test_ctcp_parse() {
        let text = "\x01VERSION\x01";
        let ctcp = CtcpMessage::parse(text).unwrap();
        assert!(matches!(ctcp, CtcpMessage::Version));
    }

    #[test]
    fn test_ctcp_not_ctcp() {
        assert!(CtcpMessage::parse("regular text").is_none());
    }

    // User prefix
    #[test]
    fn test_user_prefix() {
        assert_eq!(UserPrefix::from_char('@'), UserPrefix::Op);
        assert_eq!(UserPrefix::from_char('+'), UserPrefix::Voice);
        assert_eq!(UserPrefix::from_char('x'), UserPrefix::None);
    }

    #[test]
    fn test_channel_user_from_names() {
        let u = ChannelUser::from_names_entry("@alice");
        assert_eq!(u.nick, "alice");
        assert_eq!(u.prefix, UserPrefix::Op);

        let u2 = ChannelUser::from_names_entry("bob");
        assert_eq!(u2.nick, "bob");
        assert_eq!(u2.prefix, UserPrefix::None);
    }

    // Channel
    #[test]
    fn test_channel_users() {
        let mut ch = Channel::new("#test".to_string());
        ch.add_user(ChannelUser {
            nick: "alice".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "bob".to_string(),
            prefix: UserPrefix::None,
            away: false,
        });
        assert_eq!(ch.user_count(), 2);
        assert!(ch.find_user("alice").is_some());
        assert!(ch.find_user("Alice").is_some()); // Case insensitive
        ch.remove_user("alice");
        assert_eq!(ch.user_count(), 1);
    }

    #[test]
    fn test_channel_rename_user() {
        let mut ch = Channel::new("#test".to_string());
        ch.add_user(ChannelUser {
            nick: "alice".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        });
        ch.rename_user("alice", "alice_away");
        assert!(ch.find_user("alice_away").is_some());
        assert!(ch.find_user("alice").is_none());
    }

    #[test]
    fn test_channel_sorted_users() {
        let mut ch = Channel::new("#test".to_string());
        ch.add_user(ChannelUser {
            nick: "zeb".to_string(),
            prefix: UserPrefix::None,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "alice".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        });
        ch.add_user(ChannelUser {
            nick: "bob".to_string(),
            prefix: UserPrefix::Voice,
            away: false,
        });
        let sorted = ch.sorted_users();
        assert_eq!(sorted[0].nick, "alice"); // Op first
        assert_eq!(sorted[1].nick, "bob"); // Voice second
        assert_eq!(sorted[2].nick, "zeb"); // Regular last
    }

    #[test]
    fn test_channel_unread() {
        let mut ch = Channel::new("#test".to_string());
        assert!(!ch.has_unread());
        ch.add_message(ChatMessage::normal("", "alice", "hello"));
        assert!(ch.has_unread());
        assert_eq!(ch.unread_count, 1);
        ch.mark_read();
        assert!(!ch.has_unread());
    }

    // Channel modes
    #[test]
    fn test_channel_modes() {
        let modes = ChannelModes {
            invite_only: true,
            moderated: false,
            no_external: true,
            topic_protected: true,
            secret: false,
            key: None,
            limit: None,
        };
        assert_eq!(modes.mode_string(), "+int");
    }

    // Chat message
    #[test]
    fn test_nick_color_deterministic() {
        let c1 = ChatMessage::color_for_nick("alice");
        let c2 = ChatMessage::color_for_nick("alice");
        assert_eq!(c1.r, c2.r);
        assert_eq!(c1.g, c2.g);
        assert_eq!(c1.b, c2.b);
    }

    #[test]
    fn test_nick_color_varies() {
        let c1 = ChatMessage::color_for_nick("alice");
        let c2 = ChatMessage::color_for_nick("bob");
        // Different nicks should (usually) get different colors
        // Not guaranteed but very likely with different names
        let _ = (c1, c2);
    }

    // App message handling
    #[test]
    fn test_handle_privmsg() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");

        let msg = IrcMessage::parse(":alice!user@host PRIVMSG #test :Hello!").unwrap();
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert_eq!(ch.messages.len(), 1);
        assert_eq!(ch.messages[0].sender, "alice");
    }

    #[test]
    fn test_handle_join() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");

        let msg = IrcMessage::parse(":bob!user@host JOIN #test").unwrap();
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert!(ch.find_user("bob").is_some());
    }

    #[test]
    fn test_handle_part() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");
        if let Some(ch) = app.find_channel_mut("#test") {
            ch.add_user(ChannelUser {
                nick: "bob".to_string(),
                prefix: UserPrefix::None,
                away: false,
            });
        }

        let msg = IrcMessage::parse(":bob!user@host PART #test :goodbye").unwrap();
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert!(ch.find_user("bob").is_none());
    }

    #[test]
    fn test_handle_nick_change() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");
        if let Some(ch) = app.find_channel_mut("#test") {
            ch.add_user(ChannelUser {
                nick: "bob".to_string(),
                prefix: UserPrefix::None,
                away: false,
            });
        }

        let msg = IrcMessage::parse(":bob!user@host NICK :bobby").unwrap();
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert!(ch.find_user("bobby").is_some());
        assert!(ch.find_user("bob").is_none());
    }

    #[test]
    fn test_handle_topic() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");

        let msg = IrcMessage::parse(":alice!user@host TOPIC #test :New topic!").unwrap();
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert_eq!(ch.topic, "New topic!");
    }

    #[test]
    fn test_highlight_detection() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.my_nick = "testuser".to_string();
        app.highlight_words = vec!["urgent".to_string()];

        assert!(app.is_highlight("Hey testuser, check this out"));
        assert!(app.is_highlight("This is urgent!"));
        assert!(!app.is_highlight("Normal message"));
    }

    // Command parsing
    #[test]
    fn test_parse_join_command() {
        let app = IrcClientApp::new(800.0, 600.0);
        let cmd = app.parse_command("/join #channel").unwrap();
        assert_eq!(cmd, "JOIN #channel\r\n");
    }

    #[test]
    fn test_parse_nick_command() {
        let app = IrcClientApp::new(800.0, 600.0);
        let cmd = app.parse_command("/nick newnick").unwrap();
        assert_eq!(cmd, "NICK newnick\r\n");
    }

    #[test]
    fn test_parse_msg_command() {
        let app = IrcClientApp::new(800.0, 600.0);
        let cmd = app.parse_command("/msg alice Hello there").unwrap();
        assert_eq!(cmd, "PRIVMSG alice :Hello there\r\n");
    }

    #[test]
    fn test_parse_not_command() {
        let app = IrcClientApp::new(800.0, 600.0);
        assert!(app.parse_command("regular text").is_none());
    }

    // Private chat
    #[test]
    fn test_private_chat() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        let msg =
            IrcMessage::parse(":alice!user@host PRIVMSG SlateOSUser :secret message").unwrap();
        app.handle_message(&msg);

        assert_eq!(app.private_chats.len(), 1);
        assert_eq!(app.private_chats[0].nick, "alice");
        assert_eq!(app.private_chats[0].messages.len(), 1);
    }

    // Server config
    #[test]
    fn test_server_config_display() {
        let config = ServerConfig::default();
        assert!(config.display_address().contains("ircs://"));
        assert!(config.display_address().contains("6697"));
    }

    // Connection state
    #[test]
    fn test_connection_state_labels() {
        assert_eq!(ConnectionState::Connected.label(), "Connected");
        assert_eq!(ConnectionState::Disconnected.label(), "Disconnected");
    }

    // Render tests
    #[test]
    fn test_render_all_panels() {
        let mut app = IrcClientApp::new(1280.0, 720.0);
        app.connection = ConnectionState::Connected;
        app.join_channel("#test");

        // Channel view
        app.active_panel = ActivePanel::Channel("#test".to_string());
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());

        // Server view
        app.active_panel = ActivePanel::Server;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_without_nick_list() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.nick_list_visible = false;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // User display nick
    #[test]
    fn test_display_nick() {
        let u = ChannelUser {
            nick: "alice".to_string(),
            prefix: UserPrefix::Op,
            away: false,
        };
        assert_eq!(u.display_nick(), "@alice");
    }

    // Numerics
    #[test]
    fn test_handle_names_reply() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.join_channel("#test");

        let msg = IrcMessage {
            prefix: Some("server".to_string()),
            command: "353".to_string(),
            params: vec![
                "nick".to_string(),
                "=".to_string(),
                "#test".to_string(),
                "@alice +bob charlie".to_string(),
            ],
        };
        app.handle_message(&msg);

        let ch = app.find_channel("#test").unwrap();
        assert!(ch.find_user("alice").is_some());
        assert_eq!(ch.find_user("alice").unwrap().prefix, UserPrefix::Op);
        assert_eq!(ch.find_user("bob").unwrap().prefix, UserPrefix::Voice);
    }

    #[test]
    fn test_handle_welcome() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.connection = ConnectionState::Registering;

        let msg = IrcMessage {
            prefix: Some("server".to_string()),
            command: "001".to_string(),
            params: vec!["nick".to_string(), "Welcome to the IRC network".to_string()],
        };
        app.handle_message(&msg);
        assert_eq!(app.connection, ConnectionState::Connected);
    }

    // Wire format roundtrip
    #[test]
    fn test_parse_wire_roundtrip() {
        let original = ":nick!user@host PRIVMSG #channel :Hello world\r\n";
        let msg = IrcMessage::parse(original).unwrap();
        let wire = msg.to_wire();
        let reparsed = IrcMessage::parse(&wire).unwrap();
        assert_eq!(reparsed.command, msg.command);
        assert_eq!(reparsed.trailing(), msg.trailing());
    }

    // ======================================================================
    // Window, sidebar and the input line
    // ======================================================================

    use guitk::event::Modifiers;

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn typed(k: Key, ch: char) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        })
    }

    fn type_line(app: &mut IrcClientApp, text: &str) {
        for ch in text.chars() {
            app.handle_event(&typed(Key::A, ch));
        }
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// A client in a channel with a couple of others in it.
    fn joined() -> IrcClientApp {
        let mut app = IrcClientApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.connection = ConnectionState::Connected;
        app.my_nick = "me".to_string();
        app.join_channel("#one");
        app.join_channel("#two");
        if let Some(ch) = app.find_channel_mut("#one") {
            ch.add_user(ChannelUser {
                nick: "alice".to_string(),
                prefix: UserPrefix::Op,
                away: false,
            });
            ch.add_user(ChannelUser {
                nick: "bob".to_string(),
                prefix: UserPrefix::None,
                away: false,
            });
            ch.add_message(ChatMessage::normal("12:00", "alice", "hello"));
        }
        // Opened the way a user would, so it starts read -- setting the
        // field directly would leave the message above it unread and every
        // title assertion below one out.
        app.switch_panel(ActivePanel::Channel("#one".to_string()));
        app
    }

    /// The y of the sidebar row that switches to `wanted`.
    fn sidebar_row_y(app: &IrcClientApp, wanted: &ActivePanel) -> f32 {
        let mut y = CONTENT_TOP;
        for row in app.sidebar_rows() {
            if let SidebarRow::Item(panel) = &row
                && panel == wanted
            {
                return y + SIDEBAR_ROW_HEIGHT / 2.0;
            }
            y += row.height();
        }
        panic!("no row for {wanted:?}");
    }

    // --- the sidebar ---

    #[test]
    fn clicking_a_channel_row_switches_to_it() {
        let mut app = joined();
        let target = ActivePanel::Channel("#two".to_string());
        let y = sidebar_row_y(&app, &target);
        assert_eq!(app.sidebar_panel_at(20.0, y), Some(target.clone()));
        assert!(app.handle_event(&click(20.0, y)));
        assert_eq!(app.active_panel, target);
    }

    #[test]
    fn every_sidebar_row_is_reachable_where_it_is_drawn() {
        let mut app = joined();
        app.get_or_create_pm("carol");
        let mut y = CONTENT_TOP;
        let mut items = 0;
        for row in app.sidebar_rows() {
            if let SidebarRow::Item(panel) = &row {
                assert_eq!(
                    app.sidebar_panel_at(20.0, y + SIDEBAR_ROW_HEIGHT / 2.0),
                    Some(panel.clone()),
                    "{panel:?} is drawn at y={y} and must be clickable there"
                );
                items += 1;
            }
            y += row.height();
        }
        assert!(items >= 4, "server, two channels and a private chat");
    }

    #[test]
    fn a_sidebar_heading_is_not_a_button() {
        let app = joined();
        let mut y = CONTENT_TOP;
        let mut checked = 0;
        for row in app.sidebar_rows() {
            if matches!(row, SidebarRow::Header(_) | SidebarRow::Gap(_)) {
                assert_eq!(app.sidebar_panel_at(20.0, y + 1.0), None);
                checked += 1;
            }
            y += row.height();
        }
        assert!(checked > 0);
    }

    #[test]
    fn a_click_right_of_the_sidebar_is_not_a_sidebar_click() {
        let app = joined();
        let y = sidebar_row_y(&app, &ActivePanel::Server);
        assert_eq!(app.sidebar_panel_at(SIDEBAR_WIDTH + 4.0, y), None);
    }

    #[test]
    fn switching_to_a_channel_marks_it_read() {
        let mut app = joined();
        if let Some(ch) = app.find_channel_mut("#two") {
            ch.unread_count = 5;
        }
        app.switch_panel(ActivePanel::Channel("#two".to_string()));
        assert_eq!(
            app.find_channel("#two").map(|ch| ch.unread_count),
            Some(0),
            "opening a channel is reading it"
        );
    }

    #[test]
    fn switching_conversations_goes_back_to_the_newest_line() {
        let mut app = joined();
        app.chat_scroll = 7;
        app.switch_panel(ActivePanel::Channel("#two".to_string()));
        assert_eq!(
            app.chat_scroll, 0,
            "a position seven lines back in one conversation names nothing \
             in another"
        );
    }

    // --- the nick list ---

    #[test]
    fn clicking_a_nick_opens_a_conversation_with_them() {
        let mut app = joined();
        let rect = app.nick_list_rect().expect("the nick list is showing");
        let y = rect.y + NICK_LIST_HEADER_HEIGHT + NICK_ROW_HEIGHT / 2.0;
        let first = app.visible_nicks().first().cloned().expect("a nick");
        assert_eq!(app.nick_at(rect.x + 20.0, y), Some(first.clone()));
        assert!(app.handle_event(&click(rect.x + 20.0, y)));
        assert_eq!(app.active_panel, ActivePanel::Private(first.clone()));
        assert!(
            app.private_chats.iter().any(|p| p.nick == first),
            "`get_or_create_pm` was written for exactly this and nothing \
             called it"
        );
    }

    #[test]
    fn every_nick_is_reachable_where_it_is_drawn() {
        let app = joined();
        let rect = app.nick_list_rect().expect("the nick list is showing");
        for (i, nick) in app.visible_nicks().iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let y = rect.y + NICK_LIST_HEADER_HEIGHT + (i as f32 + 0.5) * NICK_ROW_HEIGHT;
            assert_eq!(app.nick_at(rect.x + 20.0, y), Some(nick.clone()));
        }
    }

    #[test]
    fn the_nick_list_heading_is_not_a_nick() {
        let app = joined();
        let rect = app.nick_list_rect().expect("the nick list is showing");
        assert_eq!(app.nick_at(rect.x + 20.0, rect.y + 2.0), None);
    }

    #[test]
    fn there_is_no_nick_list_when_it_is_hidden() {
        let mut app = joined();
        app.nick_list_visible = false;
        assert_eq!(app.nick_list_rect(), None);
        assert_eq!(app.nick_at(app.width - 40.0, 200.0), None);
    }

    #[test]
    fn tab_hides_and_shows_the_nick_list() {
        let mut app = joined();
        let before = app.chat_rect().width;
        assert!(app.handle_event(&key(Key::Tab)));
        assert!(!app.nick_list_visible);
        assert!(
            app.chat_rect().width > before,
            "the chat column takes the space the list gave up"
        );
    }

    // --- typing ---

    #[test]
    fn typing_puts_characters_in_the_input_line() {
        let mut app = joined();
        type_line(&mut app, "hi");
        assert_eq!(
            app.input_text, "hi",
            "`input_text` was drawn by the renderer and written by nobody"
        );
        assert!(app.handle_event(&key(Key::Backspace)));
        assert_eq!(app.input_text, "h");
    }

    #[test]
    fn backspace_on_an_empty_line_asks_for_no_frame() {
        let mut app = joined();
        assert!(!app.handle_event(&key(Key::Backspace)));
    }

    #[test]
    fn enter_says_the_line_in_the_channel() {
        let mut app = joined();
        let before = app.find_channel("#one").map_or(0, |ch| ch.messages.len());
        type_line(&mut app, "hello everyone");
        assert!(app.handle_event(&key(Key::Enter)));
        assert_eq!(app.input_text, "", "the line is sent, not left behind");
        let ch = app.find_channel("#one").expect("the channel");
        assert_eq!(ch.messages.len(), before + 1);
        let last = ch.messages.last().expect("a message");
        assert_eq!(last.sender, "me");
        assert_eq!(last.text, "hello everyone");
    }

    #[test]
    fn an_empty_line_sends_nothing() {
        let mut app = joined();
        let before = app.find_channel("#one").map_or(0, |ch| ch.messages.len());
        assert!(!app.handle_event(&key(Key::Enter)));
        type_line(&mut app, "   ");
        assert!(!app.handle_event(&key(Key::Enter)));
        assert_eq!(
            app.find_channel("#one").map_or(0, |ch| ch.messages.len()),
            before
        );
        assert!(app.input_history.is_empty(), "and is not remembered");
    }

    #[test]
    fn escape_clears_the_line_without_sending_it() {
        let mut app = joined();
        let before = app.find_channel("#one").map_or(0, |ch| ch.messages.len());
        type_line(&mut app, "never mind");
        assert!(app.handle_event(&key(Key::Escape)));
        assert_eq!(app.input_text, "");
        assert_eq!(
            app.find_channel("#one").map_or(0, |ch| ch.messages.len()),
            before
        );
        assert!(!app.handle_event(&key(Key::Escape)));
    }

    // --- the history nothing ever wrote to ---

    #[test]
    fn up_recalls_what_was_typed_before() {
        let mut app = joined();
        type_line(&mut app, "first");
        app.handle_event(&key(Key::Enter));
        type_line(&mut app, "second");
        app.handle_event(&key(Key::Enter));

        assert!(app.handle_event(&key(Key::Up)));
        assert_eq!(
            app.input_text, "second",
            "the most recent line comes back first"
        );
        assert!(app.handle_event(&key(Key::Up)));
        assert_eq!(app.input_text, "first");
        assert!(
            !app.handle_event(&key(Key::Up)),
            "there is nothing older to recall"
        );
    }

    #[test]
    fn down_walks_back_out_of_the_history() {
        let mut app = joined();
        type_line(&mut app, "first");
        app.handle_event(&key(Key::Enter));
        type_line(&mut app, "second");
        app.handle_event(&key(Key::Enter));

        app.handle_event(&key(Key::Up));
        app.handle_event(&key(Key::Up));
        assert_eq!(app.input_text, "first");
        assert!(app.handle_event(&key(Key::Down)));
        assert_eq!(app.input_text, "second");
        assert!(app.handle_event(&key(Key::Down)));
        assert_eq!(
            app.input_text, "",
            "past the newest is the empty line again"
        );
    }

    #[test]
    fn down_with_no_history_does_nothing() {
        let mut app = joined();
        assert!(!app.handle_event(&key(Key::Down)));
        assert!(!app.handle_event(&key(Key::Up)));
    }

    #[test]
    fn typing_over_a_recalled_line_makes_it_your_own() {
        let mut app = joined();
        type_line(&mut app, "hello");
        app.handle_event(&key(Key::Enter));
        app.handle_event(&key(Key::Up));
        assert_eq!(app.input_history_idx, Some(0));
        type_line(&mut app, "!");
        assert_eq!(app.input_text, "hello!");
        assert_eq!(
            app.input_history_idx, None,
            "the line being edited is the user's, not the recalled one it \
             started from"
        );
    }

    // --- slash commands ---

    #[test]
    fn slash_join_opens_the_channel() {
        let mut app = joined();
        type_line(&mut app, "/join #three");
        assert!(app.handle_event(&key(Key::Enter)));
        assert!(app.find_channel("#three").is_some());
        assert_eq!(app.active_panel, ActivePanel::Channel("#three".to_string()));
    }

    #[test]
    fn slash_part_leaves_the_channel_you_are_in() {
        let mut app = joined();
        type_line(&mut app, "/part");
        assert!(app.handle_event(&key(Key::Enter)));
        assert_eq!(
            app.find_channel("#one").map(|ch| ch.joined),
            Some(false),
            "parting with no argument parts the channel on screen"
        );
        assert_eq!(app.active_panel, ActivePanel::Server);
    }

    #[test]
    fn slash_nick_changes_your_nick() {
        let mut app = joined();
        type_line(&mut app, "/nick newname");
        app.handle_event(&key(Key::Enter));
        assert_eq!(app.my_nick, "newname");
    }

    #[test]
    fn slash_me_puts_an_action_in_the_channel() {
        let mut app = joined();
        type_line(&mut app, "/me waves");
        app.handle_event(&key(Key::Enter));
        let ch = app.find_channel("#one").expect("the channel");
        let last = ch.messages.last().expect("a message");
        assert!(matches!(last.kind, ChatMessageKind::Action));
        assert_eq!(last.text, "waves");
    }

    #[test]
    fn slash_msg_opens_a_private_chat_and_puts_the_line_in_it() {
        let mut app = joined();
        type_line(&mut app, "/msg carol are you there");
        app.handle_event(&key(Key::Enter));
        assert_eq!(app.active_panel, ActivePanel::Private("carol".to_string()));
        let pm = app
            .private_chats
            .iter()
            .find(|p| p.nick == "carol")
            .expect("a private chat");
        assert_eq!(
            pm.messages.last().map(|m| m.text.clone()),
            Some("are you there".to_string())
        );
    }

    #[test]
    fn a_command_logs_the_line_it_would_have_sent() {
        let mut app = joined();
        type_line(&mut app, "/nick other");
        app.handle_event(&key(Key::Enter));
        let logged = app
            .server_messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        assert!(
            logged.starts_with("-> NICK"),
            "a client with no socket should show what it would have sent \
             rather than drop it: {logged:?}"
        );
    }

    #[test]
    fn an_unknown_command_says_so_rather_than_vanishing() {
        let mut app = joined();
        type_line(&mut app, "/nonsense");
        app.handle_event(&key(Key::Enter));
        let logged = app
            .server_messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        assert!(logged.contains("nonsense"), "{logged:?}");
    }

    #[test]
    fn talking_in_the_server_panel_says_where_to_talk_instead() {
        let mut app = joined();
        app.switch_panel(ActivePanel::Server);
        type_line(&mut app, "hello?");
        app.handle_event(&key(Key::Enter));
        let logged = app
            .server_messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        assert!(logged.contains("channel"), "{logged:?}");
    }

    // --- scrollback ---

    fn app_with_history(lines: usize) -> IrcClientApp {
        let mut app = joined();
        if let Some(ch) = app.find_channel_mut("#one") {
            ch.messages.clear();
            for i in 0..lines {
                ch.add_message(ChatMessage::normal("12:00", "alice", &format!("line {i}")));
            }
        }
        app
    }

    #[test]
    fn the_chat_shows_the_newest_lines_by_default() {
        let app = app_with_history(200);
        let shown = app.visible_messages();
        assert!(!shown.is_empty());
        assert_eq!(
            shown.last().map(|m| m.text.clone()),
            Some("line 199".to_string()),
            "a chat opens at the bottom"
        );
    }

    #[test]
    fn page_up_shows_older_lines() {
        let mut app = app_with_history(200);
        let newest = app.visible_messages().last().map(|m| m.text.clone());
        assert!(app.handle_event(&key(Key::PageUp)));
        let after = app.visible_messages().last().map(|m| m.text.clone());
        assert_ne!(
            newest, after,
            "the chat cut itself off at the window edge with no offset to \
             move, so nothing above the fold could be read"
        );
        assert!(app.handle_event(&key(Key::PageDown)));
        assert_eq!(
            app.visible_messages().last().map(|m| m.text.clone()),
            newest
        );
    }

    #[test]
    fn the_chat_stops_at_the_oldest_message() {
        let mut app = app_with_history(50);
        for _ in 0..50 {
            app.handle_event(&key(Key::PageUp));
        }
        assert_eq!(app.chat_scroll, app.max_chat_scroll());
        assert!(
            !app.handle_event(&key(Key::PageUp)),
            "there is nothing older, so the key costs no frame"
        );
        assert_eq!(
            app.visible_messages().first().map(|m| m.text.clone()),
            Some("line 0".to_string())
        );
    }

    #[test]
    fn the_chat_stops_at_the_newest_message() {
        let mut app = app_with_history(50);
        assert!(
            !app.handle_event(&key(Key::PageDown)),
            "already at the bottom"
        );
        assert_eq!(app.chat_scroll, 0);
    }

    #[test]
    fn a_short_conversation_cannot_be_scrolled_at_all() {
        let app = app_with_history(3);
        assert_eq!(app.max_chat_scroll(), 0, "it all fits");
    }

    #[test]
    fn the_wheel_scrolls_the_chat() {
        let mut app = app_with_history(200);
        let chat = app.chat_rect();
        let (x, y) = (chat.x + chat.width / 2.0, chat.y + chat.height / 2.0);
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        })));
        assert!(app.chat_scroll > 0, "a notch away from the user goes back");
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        })));
        assert_eq!(app.chat_scroll, 0);
    }

    #[test]
    fn the_wheel_over_the_sidebar_does_not_scroll_the_chat() {
        let mut app = app_with_history(200);
        assert!(!app.handle_event(&Event::Mouse(MouseEvent {
            x: 20.0,
            y: 200.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        })));
        assert_eq!(app.chat_scroll, 0);
    }

    #[test]
    fn saying_something_brings_the_chat_back_to_the_bottom() {
        let mut app = app_with_history(200);
        app.handle_event(&key(Key::PageUp));
        assert!(app.chat_scroll > 0);
        type_line(&mut app, "still here");
        app.handle_event(&key(Key::Enter));
        assert_eq!(
            app.chat_scroll, 0,
            "a line you just sent is one you want to see"
        );
    }

    #[test]
    fn a_taller_window_shows_more_of_the_conversation() {
        let mut app = app_with_history(200);
        app.set_window_size(WINDOW_WIDTH, 400.0);
        let short = app.visible_messages().len();
        app.set_window_size(WINDOW_WIDTH, 900.0);
        assert!(
            app.visible_messages().len() > short,
            "a window five hundred pixels taller must show more than \
             {short} lines"
        );
    }

    // --- the strap ---

    #[test]
    fn the_title_says_where_you_are_and_what_is_unread() {
        let mut app = joined();
        app.server_name = "irc.example".to_string();
        assert_eq!(app.title(), "#one - IRC");
        if let Some(ch) = app.find_channel_mut("#two") {
            ch.unread_count = 4;
        }
        assert_eq!(app.title(), "(4) #one - IRC");
        app.switch_panel(ActivePanel::Server);
        assert_eq!(app.title(), "(4) irc.example - IRC");
    }

    #[test]
    fn a_chat_client_with_no_socket_asks_for_no_clock() {
        assert_eq!(
            joined().tick_interval(),
            None,
            "messages arrive from a server, not from a timer"
        );
    }

    #[test]
    fn an_unbound_key_asks_for_no_frame() {
        let mut app = joined();
        assert_eq!(app.on_event(&key(Key::F9)), Response::Idle);
    }

    #[test]
    fn a_resize_relays_out_and_a_repeat_of_it_does_not() {
        let mut app = joined();
        let resize = Event::Resize {
            width: 900,
            height: 600,
        };
        assert_eq!(app.on_event(&resize), Response::Redraw);
        assert_eq!(app.width, 900.0);
        assert_eq!(app.on_event(&resize), Response::Idle);
    }

    #[test]
    fn a_window_dragged_tiny_keeps_a_chat_column() {
        let mut app = joined();
        app.set_window_size(1.0, 1.0);
        assert!(app.width >= MIN_WINDOW_WIDTH);
        assert!(app.height >= MIN_WINDOW_HEIGHT);
        assert!(app.chat_rect().width >= 1.0);
        assert!(app.chat_rect().height >= 1.0);
    }

    #[test]
    fn the_first_frame_uses_the_size_the_compositor_gave() {
        let mut app = joined();
        let tree = app.render(1000.0, 700.0);
        assert_eq!(app.width, 1000.0);
        assert_eq!(app.height, 700.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_close_button_exits() {
        let mut app = joined();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn the_seeded_client_opens_on_a_conversation() {
        let app = seeded_client();
        assert!(!app.channels.is_empty());
        assert!(
            app.find_channel("#slateos")
                .is_some_and(|ch| !ch.messages.is_empty()),
            "the first window should have something in it"
        );
    }

    #[test]
    fn the_gap_under_a_sidebar_row_is_not_the_row() {
        let app = joined();
        let y = sidebar_row_y(&app, &ActivePanel::Server) - SIDEBAR_ROW_HEIGHT / 2.0;
        // A row's box is shorter than the step between two of them.
        assert_eq!(
            app.sidebar_panel_at(20.0, y + SIDEBAR_ROW_HEIGHT + 1.0),
            None,
            "the air under a row belongs to neither it nor the next"
        );
    }

    #[test]
    fn a_channel_you_have_left_leaves_the_sidebar() {
        let mut app = joined();
        let target = ActivePanel::Channel("#two".to_string());
        let y = sidebar_row_y(&app, &target);
        assert_eq!(app.sidebar_panel_at(20.0, y), Some(target));
        app.part_channel("#two");
        assert!(
            !app.sidebar_rows().iter().any(|row| matches!(
                row,
                SidebarRow::Item(ActivePanel::Channel(name)) if name == "#two"
            )),
            "a channel that is not joined has no row to click"
        );
    }

    #[test]
    fn a_hidden_nick_list_answers_nothing_at_all() {
        let mut app = joined();
        let rect = app.nick_list_rect().expect("showing to begin with");
        let y = rect.y + NICK_LIST_HEADER_HEIGHT + NICK_ROW_HEIGHT / 2.0;
        assert!(app.nick_at(rect.x + 20.0, y).is_some());
        app.nick_list_visible = false;
        // Every y down the column, not one: a hidden list that still answers
        // would answer at *some* height, and which one depends on where the
        // wrong rectangle put its first row.
        let mut probe = CONTENT_TOP;
        while probe < CONTENT_TOP + 240.0 {
            assert_eq!(
                app.nick_at(rect.x + 20.0, probe),
                None,
                "the chat column reaches over y={probe} now, and a click                  there must not open a private chat with whoever used to be                  listed"
            );
            probe += 4.0;
        }
    }

    #[test]
    fn walking_off_the_end_of_the_history_stops_there() {
        let mut app = joined();
        type_line(&mut app, "one");
        app.handle_event(&key(Key::Enter));
        app.handle_event(&key(Key::Up));
        assert!(app.handle_event(&key(Key::Down)), "back to the empty line");
        assert_eq!(app.input_history_idx, None);
        assert!(
            !app.handle_event(&key(Key::Down)),
            "past the newest there is nowhere further to go, so the key              costs no frame"
        );
    }
}
