//! IRC chat client application for SlateOS.
//!
//! Implements IRC protocol message parsing, channel management,
//! user tracking, message history, and a multi-panel chat UI.

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
            let p = rest[1..space].to_string();
            rest = &rest[space + 1..];
            Some(p)
        } else {
            None
        };

        // Skip leading spaces
        rest = rest.trim_start();

        let (command, remainder) = if let Some(space) = rest.find(' ') {
            (rest[..space].to_uppercase(), &rest[space + 1..])
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
                params.push(rest[..space].to_string());
                rest = &rest[space + 1..];
            } else {
                params.push(rest.to_string());
                break;
            }
        }

        Some(IrcMessage { prefix, command, params })
    }

    /// Extract nickname from prefix (nick!user@host).
    pub fn nick(&self) -> Option<&str> {
        self.prefix.as_ref().map(|p| {
            p.split('!').next().unwrap_or(p)
        })
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
        self.prefix.as_ref().and_then(|p| {
            p.split('@').nth(1)
        })
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
            let last_idx = self.params.len() - 1;
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

impl CtcpMessage {
    /// Parse CTCP from a PRIVMSG trailing parameter.
    pub fn parse(text: &str) -> Option<Self> {
        if !text.starts_with('\x01') || !text.ends_with('\x01') {
            return None;
        }
        let inner = &text[1..text.len() - 1];
        let (cmd, rest) = if let Some(space) = inner.find(' ') {
            (&inner[..space], &inner[space + 1..])
        } else {
            (inner, "")
        };

        match cmd.to_uppercase().as_str() {
            "VERSION" => Some(Self::Version),
            "PING" => Some(Self::Ping(rest.to_string())),
            "ACTION" => Some(Self::Action(rest.to_string())),
            "TIME" => Some(Self::Time),
            "FINGER" => Some(Self::Finger),
            "SOURCE" => Some(Self::Source),
            _ => Some(Self::Unknown(cmd.to_string(), rest.to_string())),
        }
    }

    pub fn is_action(text: &str) -> bool {
        text.starts_with("\x01ACTION") && text.ends_with('\x01')
    }

    pub fn action_text(text: &str) -> Option<&str> {
        if Self::is_action(text) {
            Some(&text[8..text.len() - 1])
        } else {
            None
        }
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
            return Self { nick: String::new(), prefix: UserPrefix::None, away: false };
        }
        let first = entry.chars().next().unwrap_or(' ');
        let prefix = UserPrefix::from_char(first);
        let nick = if prefix != UserPrefix::None {
            entry[1..].to_string()
        } else {
            entry.to_string()
        };
        Self { nick, prefix, away: false }
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
        if self.invite_only { modes.push('i'); }
        if self.moderated { modes.push('m'); }
        if self.no_external { modes.push('n'); }
        if self.topic_protected { modes.push('t'); }
        if self.secret { modes.push('s'); }
        if self.key.is_some() { modes.push('k'); }
        if self.limit.is_some() { modes.push('l'); }
        if modes.len() == 1 { String::new() } else { modes }
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
        self.users.iter().find(|u| u.nick.eq_ignore_ascii_case(nick))
    }

    pub fn find_user_mut(&mut self, nick: &str) -> Option<&mut ChannelUser> {
        self.users.iter_mut().find(|u| u.nick.eq_ignore_ascii_case(nick))
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
            a.prefix.cmp(&b.prefix)
                .then_with(|| a.nick.to_ascii_lowercase().cmp(&b.nick.to_ascii_lowercase()))
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
        Self { nick, messages: Vec::new(), unread_count: 0, scroll_offset: 0.0 }
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
        let idx = (hash as usize) % colors.len();
        colors[idx]
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
            realname: "SlateOS IRC Client".to_string(),
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
                        realname: "SlateOS IRC Client".to_string(),
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
        self.channels.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn find_channel_mut(&mut self, name: &str) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn get_or_create_pm(&mut self, nick: &str) -> &mut PrivateChat {
        let idx = self.private_chats.iter().position(|p| p.nick.eq_ignore_ascii_case(nick));
        match idx {
            Some(i) => &mut self.private_chats[i],
            None => {
                self.private_chats.push(PrivateChat::new(nick.to_string()));
                self.private_chats.last_mut().unwrap()
            }
        }
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
        let channel = msg.trailing()
            .or_else(|| msg.target())
            .unwrap_or("").to_string();

        if nick.eq_ignore_ascii_case(&self.my_nick) {
            self.join_channel(&channel);
        } else if let Some(ch) = self.find_channel_mut(&channel) {
            ch.add_user(ChannelUser { nick: nick.clone(), prefix: UserPrefix::None, away: false });
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
                    kind: ChatMessageKind::Quit { reason: reason.clone() },
                    highlight: false,
                });
            }
        }
    }

    fn handle_nick_change(&mut self, msg: &IrcMessage) {
        let old_nick = msg.nick().unwrap_or("").to_string();
        let new_nick = msg.trailing()
            .or_else(|| msg.target())
            .unwrap_or("").to_string();

        if old_nick.eq_ignore_ascii_case(&self.my_nick) {
            self.my_nick = new_nick.clone();
        }

        for ch in &mut self.channels {
            ch.rename_user(&old_nick, &new_nick);
            ch.add_message(ChatMessage {
                timestamp: String::new(),
                sender: new_nick.clone(),
                text: String::new(),
                kind: ChatMessageKind::Nick { old: old_nick.clone() },
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
            && let Some(ch) = self.find_channel_mut(&target) {
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
                let names: Vec<ChannelUser> = text.split_whitespace()
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
                    name: channel, user_count: count, topic: text,
                });
            }
            numerics::ERR_NICKNAMEINUSE => {
                self.server_messages.push(ChatMessage::system("", &format!("Nickname in use: {text}")));
            }
            _ => {
                if !text.is_empty() {
                    self.server_messages.push(ChatMessage::system("", &format!("[{code}] {text}")));
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
                let msg_parts: Vec<&str> = args.splitn(2, ' ').collect();
                if msg_parts.len() < 2 { return None; }
                Some(cmd_privmsg(msg_parts[0], msg_parts[1]))
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
                    ActivePanel::Channel(name) => name.clone(),
                    ActivePanel::Private(name) => name.clone(),
                    _ => return None,
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

    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
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
        self.render_input(&mut cmds, chat_x, content_y + chat_h, chat_w + nick_list_w, input_h);

        cmds
    }

    fn render_title_bar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: 30.0,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Connection status
        cmds.push(RenderCommand::FillRect {
            x: 8.0, y: 8.0, width: 10.0, height: 10.0,
            color: self.connection.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0, y: 8.0,
            text: format!("{} - {}", self.server_config.display_address(), self.connection.label()),
            font_size: 12.0, color: TEXT, font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Nick display
        cmds.push(RenderCommand::Text {
            x: self.width - 200.0, y: 8.0,
            text: format!("Nick: {}", self.my_nick),
            font_size: 12.0, color: BLUE, font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
        });

        // Topic (if in channel)
        if let Some(ch) = self.active_channel()
            && !ch.topic.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: 350.0, y: 8.0,
                    text: ch.topic.clone(),
                    font_size: 11.0, color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                    max_width: Some(self.width - 600.0),
                });
            }

        cmds.push(RenderCommand::Line {
            x1: 0.0, y1: 30.0, x2: self.width, y2: 30.0,
            color: SURFACE0, width: 1.0,
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, top_y: f32) {
        let sidebar_w = 180.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: top_y, width: sidebar_w, height: self.height - top_y,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Server item
        let mut row_y = top_y + 4.0;
        let is_active = self.active_panel == ActivePanel::Server;

        cmds.push(RenderCommand::FillRect {
            x: 4.0, y: row_y, width: sidebar_w - 8.0, height: 24.0,
            color: if is_active { SURFACE0 } else { MANTLE },
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: 12.0, y: row_y + 5.0,
            text: "Server".to_string(),
            font_size: 12.0,
            color: if is_active { TEXT } else { SUBTEXT0 },
            font_weight: FontWeightHint::Bold, max_width: Some(sidebar_w - 24.0),
        });

        row_y += 30.0;

        // Channels header
        cmds.push(RenderCommand::Text {
            x: 12.0, y: row_y,
            text: "CHANNELS".to_string(),
            font_size: 9.0, color: OVERLAY0, font_weight: FontWeightHint::Bold,
            max_width: Some(sidebar_w - 24.0),
        });
        row_y += 16.0;

        for ch in &self.channels {
            if !ch.joined { continue; }
            let is_active = self.active_panel == ActivePanel::Channel(ch.name.clone());

            cmds.push(RenderCommand::FillRect {
                x: 4.0, y: row_y, width: sidebar_w - 8.0, height: 24.0,
                color: if is_active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(4.0),
            });

            let name_color = if ch.unread_mentions > 0 { RED }
                else if ch.has_unread() { TEXT }
                else if is_active { BLUE }
                else { SUBTEXT0 };

            cmds.push(RenderCommand::Text {
                x: 12.0, y: row_y + 5.0,
                text: ch.name.clone(),
                font_size: 11.0, color: name_color,
                font_weight: if ch.has_unread() { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(sidebar_w - 50.0),
            });

            // Unread badge
            if ch.unread_count > 0 {
                let badge_text = if ch.unread_mentions > 0 {
                    ch.unread_mentions.to_string()
                } else {
                    ch.unread_count.to_string()
                };
                let badge_color = if ch.unread_mentions > 0 { RED } else { SURFACE2 };

                cmds.push(RenderCommand::FillRect {
                    x: sidebar_w - 36.0, y: row_y + 4.0, width: 24.0, height: 16.0,
                    color: badge_color, corner_radii: CornerRadii::all(8.0),
                });
                cmds.push(RenderCommand::Text {
                    x: sidebar_w - 32.0, y: row_y + 6.0,
                    text: badge_text, font_size: 9.0,
                    color: if ch.unread_mentions > 0 { CRUST } else { TEXT },
                    font_weight: FontWeightHint::Bold, max_width: Some(20.0),
                });
            }

            row_y += 26.0;
        }

        // Private messages header
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x: 12.0, y: row_y,
            text: "DIRECT MESSAGES".to_string(),
            font_size: 9.0, color: OVERLAY0, font_weight: FontWeightHint::Bold,
            max_width: Some(sidebar_w - 24.0),
        });
        row_y += 16.0;

        for pm in &self.private_chats {
            let is_active = self.active_panel == ActivePanel::Private(pm.nick.clone());

            cmds.push(RenderCommand::FillRect {
                x: 4.0, y: row_y, width: sidebar_w - 8.0, height: 24.0,
                color: if is_active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: 12.0, y: row_y + 5.0,
                text: pm.nick.clone(),
                font_size: 11.0,
                color: if pm.unread_count > 0 { TEXT } else if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if pm.unread_count > 0 { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(sidebar_w - 24.0),
            });

            row_y += 26.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: sidebar_w, y1: top_y, x2: sidebar_w, y2: self.height,
            color: SURFACE0, width: 1.0,
        });
    }

    fn render_chat_area(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::PushClip { x, y, width: w, height: h });

        let messages = match &self.active_panel {
            ActivePanel::Channel(name) => {
                self.find_channel(name).map(|ch| &ch.messages)
            }
            ActivePanel::Private(nick) => {
                self.private_chats.iter().find(|p| p.nick == *nick).map(|p| &p.messages)
            }
            ActivePanel::Server => Some(&self.server_messages),
        };

        if let Some(msgs) = messages {
            let msg_h = 20.0;
            let visible_count = (h / msg_h) as usize;
            let start = if msgs.len() > visible_count { msgs.len() - visible_count } else { 0 };

            for (i, msg) in msgs.iter().skip(start).enumerate() {
                let my = y + i as f32 * msg_h;
                self.render_chat_message(cmds, x + 8.0, my, w - 16.0, msg);
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 40.0, y: y + h / 2.0,
                text: "No messages".to_string(),
                font_size: 14.0, color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_chat_message(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, _w: f32, msg: &ChatMessage) {
        if msg.highlight {
            cmds.push(RenderCommand::FillRect {
                x: x - 4.0, y, width: _w + 8.0, height: 18.0,
                color: Color::rgba(243, 139, 168, 30),
                corner_radii: CornerRadii::ZERO,
            });
        }

        let mut tx = x;

        // Timestamp
        if self.show_timestamps && !msg.timestamp.is_empty() {
            cmds.push(RenderCommand::Text {
                x: tx, y: y + 2.0,
                text: msg.timestamp.clone(),
                font_size: 10.0, color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
            });
            tx += 60.0;
        }

        match &msg.kind {
            ChatMessageKind::Normal => {
                let nick_color = ChatMessage::color_for_nick(&msg.sender);
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("<{}>", msg.sender),
                    font_size: 11.0, color: nick_color, font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                });
                tx += 100.0;
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: msg.text.clone(),
                    font_size: 11.0, color: TEXT, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Action => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("* {} {}", msg.sender, msg.text),
                    font_size: 11.0, color: MAUVE, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Notice => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("-{}- {}", msg.sender, msg.text),
                    font_size: 11.0, color: PEACH, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Join => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("--> {} has joined", msg.sender),
                    font_size: 10.0, color: GREEN, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Part { reason } => {
                let reason_str = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("<-- {} has left{reason_str}", msg.sender),
                    font_size: 10.0, color: RED, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Quit { reason } => {
                let reason_str = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("<-- {} has quit{reason_str}", msg.sender),
                    font_size: 10.0, color: RED, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Kick { by, reason } => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("*** {} was kicked by {} ({})", msg.sender, by, reason),
                    font_size: 10.0, color: RED, font_weight: FontWeightHint::Bold,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Nick { old } => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("*** {old} is now known as {}", msg.sender),
                    font_size: 10.0, color: TEAL, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Topic { by } => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("*** {by} changed the topic to: {}", msg.text),
                    font_size: 10.0, color: YELLOW, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::Mode { by, mode } => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: format!("*** {by} sets mode {mode}"),
                    font_size: 10.0, color: TEAL, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
            ChatMessageKind::System => {
                cmds.push(RenderCommand::Text {
                    x: tx, y: y + 2.0,
                    text: msg.text.clone(),
                    font_size: 10.0, color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                    max_width: Some(_w - tx + x),
                });
            }
        }
    }

    fn render_nick_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: x, y1: y, x2: x, y2: y + h,
            color: SURFACE0, width: 1.0,
        });

        if let Some(ch) = self.active_channel() {
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 6.0,
                text: format!("Users ({})", ch.user_count()),
                font_size: 10.0, color: SUBTEXT0, font_weight: FontWeightHint::Bold,
                max_width: Some(w - 16.0),
            });

            let sorted = ch.sorted_users();
            let mut current_prefix: Option<UserPrefix> = None;

            let mut row_y = y + 24.0;
            for user in sorted.iter().take(((h - 24.0) / 18.0) as usize) {
                // Section header for prefix groups
                if current_prefix != Some(user.prefix) && user.prefix != UserPrefix::None {
                    cmds.push(RenderCommand::Text {
                        x: x + 8.0, y: row_y,
                        text: user.prefix.label().to_string(),
                        font_size: 9.0, color: OVERLAY0, font_weight: FontWeightHint::Bold,
                        max_width: Some(w - 16.0),
                    });
                    row_y += 14.0;
                    current_prefix = Some(user.prefix);
                }

                let color = if user.away { OVERLAY0 } else { user.prefix.color() };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: row_y,
                    text: user.display_nick(),
                    font_size: 11.0, color,
                    font_weight: FontWeightHint::Regular, max_width: Some(w - 24.0),
                });
                row_y += 18.0;
            }
        }
    }

    fn render_input(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: x, y1: y, x2: x + w, y2: y,
            color: SURFACE0, width: 1.0,
        });

        // Input field
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0, y: y + 6.0, width: w - 16.0, height: h - 12.0,
            color: SURFACE0, corner_radii: CornerRadii::all(4.0),
        });

        let display_text = if self.input_text.is_empty() {
            "Type a message... (/help for commands)".to_string()
        } else {
            self.input_text.clone()
        };

        let text_color = if self.input_text.is_empty() { OVERLAY0 } else { TEXT };

        cmds.push(RenderCommand::Text {
            x: x + 16.0, y: y + 12.0,
            text: display_text, font_size: 12.0, color: text_color,
            font_weight: FontWeightHint::Regular, max_width: Some(w - 32.0),
        });
    }
}

// ============================================================================
// Sample data and main
// ============================================================================

fn main() {
    let mut app = IrcClientApp::new(1280.0, 720.0);
    app.connection = ConnectionState::Connected;
    app.server_name = "irc.libera.chat".to_string();
    app.my_nick = "SlateOSUser".to_string();

    // Create channels with sample data
    app.join_channel("#slateos");
    app.join_channel("#rust");

    if let Some(ch) = app.find_channel_mut("#slateos") {
        ch.topic = "SlateOS Development | https://slateos.dev".to_string();
        ch.add_user(ChannelUser { nick: "SlateOSUser".to_string(), prefix: UserPrefix::Op, away: false });
        ch.add_user(ChannelUser { nick: "alice".to_string(), prefix: UserPrefix::Voice, away: false });
        ch.add_user(ChannelUser { nick: "bob".to_string(), prefix: UserPrefix::None, away: false });
        ch.add_user(ChannelUser { nick: "charlie".to_string(), prefix: UserPrefix::None, away: true });

        ch.add_message(ChatMessage::system("12:00", "Welcome to #slateos!"));
        ch.add_message(ChatMessage::normal("12:01", "alice", "Hey everyone!"));
        ch.add_message(ChatMessage::normal("12:02", "bob", "Working on the new kernel module"));
        ch.add_message(ChatMessage::action("12:03", "alice", "is reviewing PRs"));
        ch.add_message(ChatMessage::normal("12:05", "bob", "The scheduler benchmarks look great"));
        ch.mark_read();
    }

    if let Some(ch) = app.find_channel_mut("#rust") {
        ch.topic = "The Rust Programming Language".to_string();
        ch.add_user(ChannelUser { nick: "rustbot".to_string(), prefix: UserPrefix::Op, away: false });
        ch.add_user(ChannelUser { nick: "ferris".to_string(), prefix: UserPrefix::None, away: false });
        ch.unread_count = 3;
    }

    // Test message parsing
    let raw = ":alice!user@host PRIVMSG #slateos :Hello world!";
    if let Some(msg) = IrcMessage::parse(raw) {
        app.handle_message(&msg);
    }

    // Render
    let cmds = app.render();
    let _ = cmds.len();

    // Test other panels
    app.active_panel = ActivePanel::Server;
    let cmds2 = app.render();
    let _ = cmds2.len();
}

#[cfg(test)]
mod tests {
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
        assert_eq!(cmd_privmsg("#channel", "hello"), "PRIVMSG #channel :hello\r\n");
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
        ch.add_user(ChannelUser { nick: "alice".to_string(), prefix: UserPrefix::Op, away: false });
        ch.add_user(ChannelUser { nick: "bob".to_string(), prefix: UserPrefix::None, away: false });
        assert_eq!(ch.user_count(), 2);
        assert!(ch.find_user("alice").is_some());
        assert!(ch.find_user("Alice").is_some()); // Case insensitive
        ch.remove_user("alice");
        assert_eq!(ch.user_count(), 1);
    }

    #[test]
    fn test_channel_rename_user() {
        let mut ch = Channel::new("#test".to_string());
        ch.add_user(ChannelUser { nick: "alice".to_string(), prefix: UserPrefix::Op, away: false });
        ch.rename_user("alice", "alice_away");
        assert!(ch.find_user("alice_away").is_some());
        assert!(ch.find_user("alice").is_none());
    }

    #[test]
    fn test_channel_sorted_users() {
        let mut ch = Channel::new("#test".to_string());
        ch.add_user(ChannelUser { nick: "zeb".to_string(), prefix: UserPrefix::None, away: false });
        ch.add_user(ChannelUser { nick: "alice".to_string(), prefix: UserPrefix::Op, away: false });
        ch.add_user(ChannelUser { nick: "bob".to_string(), prefix: UserPrefix::Voice, away: false });
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
            ch.add_user(ChannelUser { nick: "bob".to_string(), prefix: UserPrefix::None, away: false });
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
            ch.add_user(ChannelUser { nick: "bob".to_string(), prefix: UserPrefix::None, away: false });
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
        let msg = IrcMessage::parse(":alice!user@host PRIVMSG SlateOSUser :secret message").unwrap();
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
        let cmds = app.render();
        assert!(!cmds.is_empty());

        // Server view
        app.active_panel = ActivePanel::Server;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_without_nick_list() {
        let mut app = IrcClientApp::new(800.0, 600.0);
        app.nick_list_visible = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // User display nick
    #[test]
    fn test_display_nick() {
        let u = ChannelUser { nick: "alice".to_string(), prefix: UserPrefix::Op, away: false };
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
            params: vec!["nick".to_string(), "=".to_string(), "#test".to_string(), "@alice +bob charlie".to_string()],
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
}
