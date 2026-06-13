//! SlateOS Login Manager — Display Manager and Session Launcher
//!
//! Presents the login screen, authenticates users against the local user
//! database, and starts user sessions. Also provides a lock screen that
//! protects running sessions from unauthorized access.
//!
//! # Features
//!
//! - Multi-user account management (load from /etc/users.yaml)
//! - Password hashing via SHA-256 with salt
//! - Account lockout after repeated failures (5 attempts / 5 minute cooldown)
//! - Auto-login support for configured accounts
//! - Guest login (no password)
//! - Lock screen with idle timeout
//! - Accessibility options (high contrast, large text)
//! - Power menu (shutdown, restart, sleep)
//! - Full keyboard navigation

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Theme colors (dark theme — Catppuccin Mocha inspired)
// ============================================================================

/// Deep background
const COL_BG_DARK: Color = Color::from_hex(0x11111B);
/// Background gradient top
const COL_BG_TOP: Color = Color::from_hex(0x1E1E2E);
/// Background gradient bottom
const COL_BG_BOTTOM: Color = Color::from_hex(0x181825);
/// Login box background
const COL_PANEL: Color = Color::from_hex(0x313244);
/// Input field background
const COL_INPUT_BG: Color = Color::from_hex(0x45475A);
/// Input field border (focused)
const COL_INPUT_FOCUS: Color = Color::from_hex(0x89B4FA);
/// Input field border (normal)
#[allow(dead_code)]
const COL_INPUT_BORDER: Color = Color::from_hex(0x585B70);
/// Primary text
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Secondary / dim text
const COL_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
/// Accent color (buttons, selected items)
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
/// Accent hover
#[allow(dead_code)]
const COL_ACCENT_HOVER: Color = Color::from_hex(0xB4D0FB);
/// Error / red
const COL_ERROR: Color = Color::from_hex(0xF38BA8);
/// Success / green
#[allow(dead_code)]
const COL_SUCCESS: Color = Color::from_hex(0xA6E3A1);
/// Warning / peach
const COL_WARNING: Color = Color::from_hex(0xFAB387);
/// Avatar circle colors (assigned by uid)
const COL_AVATAR_PALETTE: [Color; 6] = [
    Color::from_hex(0x89B4FA), // blue
    Color::from_hex(0xCBA6F7), // mauve
    Color::from_hex(0xF38BA8), // red
    Color::from_hex(0xA6E3A1), // green
    Color::from_hex(0xF9E2AF), // yellow
    Color::from_hex(0xFAB387), // peach
];
/// Power menu background
const COL_POWER_BG: Color = Color::rgba(17, 17, 27, 220);

// High contrast overrides
const COL_HC_BG: Color = Color::BLACK;
const COL_HC_TEXT: Color = Color::WHITE;
const COL_HC_ACCENT: Color = Color::from_hex(0x00FFFF);
const COL_HC_ERROR: Color = Color::from_hex(0xFF4444);
const COL_HC_PANEL: Color = Color::from_hex(0x222222);

// ============================================================================
// Layout constants
// ============================================================================

const SCREEN_WIDTH: f32 = 1920.0;
const SCREEN_HEIGHT: f32 = 1080.0;

const LOGIN_BOX_WIDTH: f32 = 400.0;
const LOGIN_BOX_HEIGHT: f32 = 480.0;
const LOGIN_BOX_RADIUS: f32 = 16.0;

const AVATAR_SIZE: f32 = 80.0;
const AVATAR_Y_OFFSET: f32 = 40.0;

const INPUT_WIDTH: f32 = 320.0;
const INPUT_HEIGHT: f32 = 44.0;
const INPUT_RADIUS: f32 = 8.0;

const BUTTON_WIDTH: f32 = 320.0;
const BUTTON_HEIGHT: f32 = 44.0;
const BUTTON_RADIUS: f32 = 8.0;

const POWER_BUTTON_SIZE: f32 = 48.0;
const POWER_MENU_WIDTH: f32 = 200.0;

const FONT_SIZE_LARGE: f32 = 24.0;
const FONT_SIZE_NORMAL: f32 = 16.0;
const FONT_SIZE_SMALL: f32 = 13.0;
const FONT_SIZE_CLOCK: f32 = 18.0;

const LARGE_FONT_SCALE: f32 = 1.4;

// ============================================================================
// SHA-256 (inline implementation for password hashing)
// ============================================================================

/// SHA-256 initial hash values.
const SHA256_H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// SHA-256 round constants.
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 compression function.
#[allow(clippy::many_single_char_names)]
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];

    for (w_slot, word_bytes) in w.iter_mut().take(16).zip(block.chunks_exact(4)) {
        // chunks_exact(4) yields a &[u8] of length 4; try_into is infallible.
        let arr: [u8; 4] = word_bytes.try_into().unwrap_or([0; 4]);
        *w_slot = u32::from_be_bytes(arr);
    }

    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Compute SHA-256 of a byte slice.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = SHA256_H0;
    let mut offset = 0usize;

    // Process full 64-byte blocks.
    while offset + 64 <= data.len() {
        let mut block = [0u8; 64];
        if let Some(src) = data.get(offset..offset + 64) {
            block.copy_from_slice(src);
        }
        sha256_compress(&mut state, &block);
        offset += 64;
    }

    // Final block with padding.
    let remaining = data.len().saturating_sub(offset);
    let mut buffer = [0u8; 128]; // Two blocks max for final padding.
    if let (Some(dest), Some(src)) = (buffer.get_mut(..remaining), data.get(offset..)) {
        dest.copy_from_slice(src);
    }

    // Append 0x80.
    if let Some(b) = buffer.get_mut(remaining) {
        *b = 0x80;
    }

    let total_bits = (data.len() as u64).wrapping_mul(8);
    let pad_len = if remaining + 1 > 56 { 128 } else { 64 };

    // Write length in bits (big-endian) at end of final block(s).
    let len_bytes = total_bits.to_be_bytes();
    if let Some(dest) = buffer.get_mut(pad_len - 8..pad_len) {
        dest.copy_from_slice(&len_bytes);
    }

    // Compress final block(s).
    if pad_len == 128 {
        let mut block1 = [0u8; 64];
        if let Some(src) = buffer.get(..64) {
            block1.copy_from_slice(src);
        }
        sha256_compress(&mut state, &block1);
        let mut block2 = [0u8; 64];
        if let Some(src) = buffer.get(64..128) {
            block2.copy_from_slice(src);
        }
        sha256_compress(&mut state, &block2);
    } else {
        let mut block = [0u8; 64];
        if let Some(src) = buffer.get(..64) {
            block.copy_from_slice(src);
        }
        sha256_compress(&mut state, &block);
    }

    // Produce digest.
    let mut digest = [0u8; 32];
    for (i, &word) in state.iter().enumerate() {
        let bytes = word.to_be_bytes();
        let off = i * 4;
        if let Some(dest) = digest.get_mut(off..off + 4) {
            dest.copy_from_slice(&bytes);
        }
    }
    digest
}

/// Convert a byte slice to a lowercase hex string.
fn bytes_to_hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for &byte in data {
        out.push(HEX_CHARS[(byte >> 4) as usize]);
        out.push(HEX_CHARS[(byte & 0x0F) as usize]);
    }
    out
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Parse a hex string into bytes. Returns None on invalid input.
fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut result = Vec::with_capacity(hex.len() / 2);
    let chars: Vec<char> = hex.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let high = hex_digit(chars[i])?;
        let low = hex_digit(chars[i + 1])?;
        result.push((high << 4) | low);
        i += 2;
    }
    Some(result)
}

fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// Password hashing: sha256(salt || password)
// ============================================================================

/// Salt length in bytes.
const SALT_LENGTH: usize = 16;

/// Hash a password with a given salt.
fn hash_password(salt: &[u8], password: &str) -> String {
    let mut input = Vec::with_capacity(salt.len() + password.len());
    input.extend_from_slice(salt);
    input.extend_from_slice(password.as_bytes());
    bytes_to_hex(&sha256(&input))
}

/// Generate a deterministic salt from a uid and username (used for default accounts).
/// In production, a CSPRNG-derived salt would be used when a user sets their password.
fn generate_default_salt(uid: u32, username: &str) -> [u8; SALT_LENGTH] {
    let mut seed = Vec::new();
    seed.extend_from_slice(&uid.to_le_bytes());
    seed.extend_from_slice(username.as_bytes());
    seed.extend_from_slice(b"slateos-default-salt");
    let hash = sha256(&seed);
    let mut salt = [0u8; SALT_LENGTH];
    if let Some(src) = hash.get(..SALT_LENGTH) {
        salt.copy_from_slice(src);
    }
    salt
}

// ============================================================================
// User account model
// ============================================================================

/// A user account on the system.
#[derive(Clone, Debug)]
pub struct UserAccount {
    /// Unique user identifier.
    pub uid: u32,
    /// Login username.
    pub username: String,
    /// Display name (shown on login screen).
    pub display_name: String,
    /// SHA-256 hash of (salt || password).
    pub password_hash: String,
    /// Salt for the password (hex-encoded).
    pub password_salt: String,
    /// Optional avatar image path.
    pub avatar_path: Option<String>,
    /// User's preferred shell.
    pub shell: String,
    /// Home directory.
    pub home_dir: String,
    /// Whether this user has admin privileges.
    pub is_admin: bool,
    /// Whether this account should auto-login.
    pub auto_login: bool,
    /// Unix timestamp of last successful login.
    pub last_login_timestamp: u64,
    /// Total successful logins.
    pub login_count: u32,
}

impl UserAccount {
    /// Create a new user account with a plaintext password (will be hashed).
    fn new_with_password(
        uid: u32,
        username: &str,
        display_name: &str,
        password: &str,
        is_admin: bool,
    ) -> Self {
        let salt = generate_default_salt(uid, username);
        let hash = hash_password(&salt, password);
        Self {
            uid,
            username: username.to_string(),
            display_name: display_name.to_string(),
            password_hash: hash,
            password_salt: bytes_to_hex(&salt),
            avatar_path: None,
            shell: "/bin/nush".to_string(),
            home_dir: format!("/home/{}", username),
            is_admin,
            auto_login: false,
            last_login_timestamp: 0,
            login_count: 0,
        }
    }

    /// Create the root (admin) account.
    fn root_account() -> Self {
        let mut account = Self::new_with_password(0, "root", "Administrator", "root", true);
        account.home_dir = "/root".to_string();
        account
    }

    /// Create the guest account (no password required).
    fn guest_account() -> Self {
        Self {
            uid: 65534,
            username: "guest".to_string(),
            display_name: "Guest".to_string(),
            password_hash: String::new(),
            password_salt: String::new(),
            avatar_path: None,
            shell: "/bin/nush".to_string(),
            home_dir: "/tmp/guest".to_string(),
            is_admin: false,
            auto_login: false,
            last_login_timestamp: 0,
            login_count: 0,
        }
    }

    /// Check if this account requires a password (guest does not).
    fn requires_password(&self) -> bool {
        !self.password_hash.is_empty()
    }

    /// Get initials for the avatar circle (first letters of display name words).
    fn initials(&self) -> String {
        self.display_name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .map(|c| c.to_uppercase().to_string())
            .collect()
    }

    /// Get the avatar color based on uid.
    fn avatar_color(&self) -> Color {
        let idx = (self.uid as usize) % COL_AVATAR_PALETTE.len();
        COL_AVATAR_PALETTE[idx]
    }
}

// ============================================================================
// User database (YAML-based)
// ============================================================================

/// Simple YAML serialization for user database.
/// Format: /etc/users.yaml
fn serialize_users_yaml(users: &[UserAccount]) -> String {
    let mut yaml = String::from("# Slate OS User Database\n# DO NOT EDIT MANUALLY\n\nusers:\n");
    for user in users {
        yaml.push_str(&format!("  - uid: {}\n", user.uid));
        yaml.push_str(&format!("    username: \"{}\"\n", user.username));
        yaml.push_str(&format!("    display_name: \"{}\"\n", user.display_name));
        yaml.push_str(&format!("    password_hash: \"{}\"\n", user.password_hash));
        yaml.push_str(&format!("    password_salt: \"{}\"\n", user.password_salt));
        match &user.avatar_path {
            Some(path) => yaml.push_str(&format!("    avatar_path: \"{}\"\n", path)),
            None => yaml.push_str("    avatar_path: null\n"),
        }
        yaml.push_str(&format!("    shell: \"{}\"\n", user.shell));
        yaml.push_str(&format!("    home_dir: \"{}\"\n", user.home_dir));
        yaml.push_str(&format!("    is_admin: {}\n", user.is_admin));
        yaml.push_str(&format!("    auto_login: {}\n", user.auto_login));
        yaml.push_str(&format!(
            "    last_login_timestamp: {}\n",
            user.last_login_timestamp
        ));
        yaml.push_str(&format!("    login_count: {}\n", user.login_count));
    }
    yaml
}

/// Parse user accounts from YAML text.
/// Returns default accounts if parsing fails.
fn parse_users_yaml(yaml: &str) -> Vec<UserAccount> {
    let mut users = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("- uid:") {
            let mut uid = 0u32;
            let mut username = String::new();
            let mut display_name = String::new();
            let mut password_hash = String::new();
            let mut password_salt = String::new();
            let mut avatar_path: Option<String> = None;
            let mut shell = "/bin/nush".to_string();
            let mut home_dir = String::new();
            let mut is_admin = false;
            let mut auto_login = false;
            let mut last_login_timestamp = 0u64;
            let mut login_count = 0u32;

            // Parse uid from this line.
            if let Some(val) = line.strip_prefix("- uid:") {
                uid = val.trim().parse().unwrap_or(0);
            }
            i += 1;

            // Parse subsequent indented fields.
            while i < lines.len() {
                let field = lines[i].trim();
                if field.starts_with("- uid:")
                    || field.is_empty()
                        && i + 1 < lines.len()
                        && lines[i + 1].trim().starts_with("- uid:")
                {
                    break;
                }
                if field.is_empty() || field.starts_with('#') {
                    i += 1;
                    continue;
                }

                if let Some(val) = field.strip_prefix("username:") {
                    username = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("display_name:") {
                    display_name = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("password_hash:") {
                    password_hash = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("password_salt:") {
                    password_salt = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("avatar_path:") {
                    let v = strip_yaml_string(val);
                    avatar_path = if v == "null" || v.is_empty() {
                        None
                    } else {
                        Some(v)
                    };
                } else if let Some(val) = field.strip_prefix("shell:") {
                    shell = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("home_dir:") {
                    home_dir = strip_yaml_string(val);
                } else if let Some(val) = field.strip_prefix("is_admin:") {
                    is_admin = val.trim() == "true";
                } else if let Some(val) = field.strip_prefix("auto_login:") {
                    auto_login = val.trim() == "true";
                } else if let Some(val) = field.strip_prefix("last_login_timestamp:") {
                    last_login_timestamp = val.trim().parse().unwrap_or(0);
                } else if let Some(val) = field.strip_prefix("login_count:") {
                    login_count = val.trim().parse().unwrap_or(0);
                }
                i += 1;
            }

            users.push(UserAccount {
                uid,
                username,
                display_name,
                password_hash,
                password_salt,
                avatar_path,
                shell,
                home_dir,
                is_admin,
                auto_login,
                last_login_timestamp,
                login_count,
            });
        } else {
            i += 1;
        }
    }

    users
}

/// Strip quotes and whitespace from a YAML string value.
fn strip_yaml_string(val: &str) -> String {
    let trimmed = val.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Load user database, falling back to default accounts.
fn load_user_database() -> Vec<UserAccount> {
    // In a real system, we would read /etc/users.yaml.
    // For initial implementation, return default accounts.
    match std::fs::read_to_string("/etc/users.yaml") {
        Ok(content) => {
            let users = parse_users_yaml(&content);
            if users.is_empty() {
                default_accounts()
            } else {
                users
            }
        }
        Err(_) => default_accounts(),
    }
}

/// Save user database to /etc/users.yaml.
fn save_user_database(users: &[UserAccount]) -> Result<(), std::io::Error> {
    let yaml = serialize_users_yaml(users);
    std::fs::write("/etc/users.yaml", yaml)
}

/// Default user accounts for a fresh system.
fn default_accounts() -> Vec<UserAccount> {
    vec![UserAccount::root_account(), UserAccount::guest_account()]
}

// ============================================================================
// Session management
// ============================================================================

/// Information about a running user session.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    /// User who owns this session.
    pub user_uid: u32,
    /// Unique session identifier.
    pub session_id: u64,
    /// Timestamp when the session started.
    pub started_at: u64,
    /// Path to the user's shell.
    pub shell_path: String,
    /// User's home directory.
    pub home_dir: String,
    /// Environment variables for this session.
    pub environment: HashMap<String, String>,
}

impl SessionInfo {
    /// Create a new session for a user.
    fn new(user: &UserAccount, session_id: u64, timestamp: u64) -> Self {
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), user.home_dir.clone());
        env.insert("USER".to_string(), user.username.clone());
        env.insert("LOGNAME".to_string(), user.username.clone());
        env.insert("SHELL".to_string(), user.shell.clone());
        env.insert(
            "PATH".to_string(),
            "/bin:/usr/bin:/usr/local/bin".to_string(),
        );
        env.insert(
            "XDG_RUNTIME_DIR".to_string(),
            format!("/run/user/{}", user.uid),
        );
        env.insert(
            "XDG_DATA_HOME".to_string(),
            format!("{}/.local/share", user.home_dir),
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            format!("{}/.config", user.home_dir),
        );
        env.insert(
            "XDG_CACHE_HOME".to_string(),
            format!("{}/.cache", user.home_dir),
        );
        env.insert("XDG_SESSION_TYPE".to_string(), "graphical".to_string());

        Self {
            user_uid: user.uid,
            session_id,
            started_at: timestamp,
            shell_path: user.shell.clone(),
            home_dir: user.home_dir.clone(),
            environment: env,
        }
    }
}

// ============================================================================
// Account lockout tracking
// ============================================================================

/// Tracks failed login attempts and lockout state for an account.
#[derive(Clone, Debug)]
pub struct LockoutState {
    /// Number of consecutive failed attempts.
    failed_attempts: u32,
    /// Timestamp when the lockout expires (0 = not locked).
    locked_until: u64,
}

impl LockoutState {
    fn new() -> Self {
        Self {
            failed_attempts: 0,
            locked_until: 0,
        }
    }

    /// Record a failed attempt; returns true if account is now locked.
    fn record_failure(&mut self, now: u64) -> bool {
        self.failed_attempts += 1;
        if self.failed_attempts >= MAX_FAILED_ATTEMPTS {
            self.locked_until = now.saturating_add(LOCKOUT_DURATION_SECS);
            true
        } else {
            false
        }
    }

    /// Check if the account is currently locked.
    fn is_locked(&self, now: u64) -> bool {
        self.locked_until > now
    }

    /// Remaining lockout seconds.
    fn remaining_lockout_secs(&self, now: u64) -> u64 {
        self.locked_until.saturating_sub(now)
    }

    /// Reset on successful login.
    fn reset(&mut self) {
        self.failed_attempts = 0;
        self.locked_until = 0;
    }
}

const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECS: u64 = 300; // 5 minutes

// ============================================================================
// Login view state machine
// ============================================================================

/// Which screen the login manager is currently displaying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginView {
    /// Show the user list for selection.
    UserSelect,
    /// Show the password entry for the selected user.
    PasswordEntry,
    /// Screen is locked (session active, user must unlock).
    Locked,
    /// Power menu overlay is visible.
    PowerMenu,
    /// System is shutting down (or restarting).
    ShuttingDown,
}

/// Power menu options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PowerAction {
    Shutdown,
    Restart,
    Sleep,
}

// ============================================================================
// Accessibility settings
// ============================================================================

/// Accessibility options for the login screen.
#[derive(Clone, Debug)]
pub struct AccessibilitySettings {
    /// High contrast mode.
    pub high_contrast: bool,
    /// Large text (1.4x scale).
    pub large_text: bool,
    /// On-screen keyboard visible.
    pub onscreen_keyboard: bool,
    /// Screen reader announcements (collect for output).
    pub screen_reader_enabled: bool,
    /// Pending announcements for screen reader.
    pub announcements: Vec<String>,
}

impl AccessibilitySettings {
    fn new() -> Self {
        Self {
            high_contrast: false,
            large_text: false,
            onscreen_keyboard: false,
            screen_reader_enabled: false,
            announcements: Vec::new(),
        }
    }

    /// Announce a text description for the screen reader.
    fn announce(&mut self, text: &str) {
        if self.screen_reader_enabled {
            self.announcements.push(text.to_string());
        }
    }

    /// Drain pending announcements.
    #[allow(dead_code)]
    fn drain_announcements(&mut self) -> Vec<String> {
        std::mem::take(&mut self.announcements)
    }
}

// ============================================================================
// Login Manager (main state)
// ============================================================================

/// The login manager state machine. Holds all state for the display manager,
/// including user database, authentication, session tracking, and UI state.
pub struct LoginManager {
    /// Current view being displayed.
    pub current_view: LoginView,
    /// All user accounts on the system.
    pub users: Vec<UserAccount>,
    /// Index of the currently selected user in the user list.
    pub selected_user_index: usize,
    /// Current password input (masked on screen).
    pub password_input: String,
    /// Whether to show the password in cleartext.
    pub password_visible: bool,
    /// Current error message to display (cleared on input).
    pub error_message: Option<String>,
    /// Per-account lockout state, keyed by uid.
    pub locked_accounts: HashMap<u32, LockoutState>,
    /// Active sessions, keyed by session_id.
    pub sessions: HashMap<u64, SessionInfo>,
    /// Next session ID to assign.
    next_session_id: u64,
    /// Lock screen idle timeout in seconds.
    pub lock_timeout_secs: u64,
    /// Seconds since last user input (for idle timeout).
    pub idle_seconds: u64,
    /// Whether the screen is dimmed (30s warning before lock).
    pub screen_dimmed: bool,
    /// Current timestamp (updated by tick()).
    current_time: u64,
    /// Accessibility settings.
    pub accessibility: AccessibilitySettings,
    /// Power menu selection index.
    power_menu_selection: usize,
    /// The uid of the user whose session is locked (only used in Locked view).
    locked_session_uid: Option<u32>,
    /// Clock string for display.
    clock_display: String,
}

impl Default for LoginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginManager {
    /// Create a new login manager with accounts loaded from the database.
    pub fn new() -> Self {
        let users = load_user_database();
        Self {
            current_view: LoginView::UserSelect,
            users,
            selected_user_index: 0,
            password_input: String::new(),
            password_visible: false,
            error_message: None,
            locked_accounts: HashMap::new(),
            sessions: HashMap::new(),
            next_session_id: 1,
            lock_timeout_secs: 300, // 5 minutes default
            idle_seconds: 0,
            screen_dimmed: false,
            current_time: 0,
            accessibility: AccessibilitySettings::new(),
            power_menu_selection: 0,
            locked_session_uid: None,
            clock_display: "00:00".to_string(),
        }
    }

    /// Create a login manager with specific accounts (for testing).
    pub fn with_users(users: Vec<UserAccount>) -> Self {
        Self {
            users,
            ..Self::new_internal()
        }
    }

    /// Internal constructor without loading from disk.
    fn new_internal() -> Self {
        Self {
            current_view: LoginView::UserSelect,
            users: Vec::new(),
            selected_user_index: 0,
            password_input: String::new(),
            password_visible: false,
            error_message: None,
            locked_accounts: HashMap::new(),
            sessions: HashMap::new(),
            next_session_id: 1,
            lock_timeout_secs: 300,
            idle_seconds: 0,
            screen_dimmed: false,
            current_time: 0,
            accessibility: AccessibilitySettings::new(),
            power_menu_selection: 0,
            locked_session_uid: None,
            clock_display: "00:00".to_string(),
        }
    }

    /// Check for an auto-login user and bypass the login screen if found.
    pub fn check_auto_login(&mut self) -> Option<SessionInfo> {
        let auto_user = self.users.iter().find(|u| u.auto_login).cloned();
        if let Some(user) = auto_user {
            self.start_session(user.uid).ok()
        } else {
            None
        }
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    /// Authenticate a user with the given password.
    /// Returns Ok(()) on success, Err(message) on failure.
    pub fn authenticate(&mut self, username: &str, password: &str) -> Result<(), String> {
        let now = self.current_time;

        // Find the user.
        let user = self.users.iter().find(|u| u.username == username).cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("User not found".to_string()),
        };

        // Check lockout.
        if let Some(lockout) = self.locked_accounts.get(&user.uid)
            && lockout.is_locked(now) {
                let remaining = lockout.remaining_lockout_secs(now);
                return Err(format!(
                    "Account locked. Try again in {} seconds.",
                    remaining
                ));
            }

        // Guest accounts don't need a password.
        if !user.requires_password() {
            return Ok(());
        }

        // Verify password.
        let salt_bytes = hex_to_bytes(&user.password_salt).unwrap_or_default();
        let computed_hash = hash_password(&salt_bytes, password);

        if computed_hash == user.password_hash {
            // Success: reset lockout, update login stats.
            self.locked_accounts
                .entry(user.uid)
                .and_modify(|l| l.reset());
            if let Some(u) = self.users.iter_mut().find(|u| u.uid == user.uid) {
                u.last_login_timestamp = now;
                u.login_count = u.login_count.saturating_add(1);
            }
            Ok(())
        } else {
            // Failure: record attempt.
            let lockout = self
                .locked_accounts
                .entry(user.uid)
                .or_insert_with(LockoutState::new);
            let now_locked = lockout.record_failure(now);
            if now_locked {
                Err("Account locked after too many attempts. Wait 5 minutes.".to_string())
            } else {
                let remaining = MAX_FAILED_ATTEMPTS.saturating_sub(lockout.failed_attempts);
                Err(format!(
                    "Incorrect password. {} attempts remaining.",
                    remaining
                ))
            }
        }
    }

    // ========================================================================
    // Session management
    // ========================================================================

    /// Start a new session for the given user.
    pub fn start_session(&mut self, uid: u32) -> Result<SessionInfo, String> {
        let user = self.users.iter().find(|u| u.uid == uid).cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("User not found".to_string()),
        };

        let session_id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        let session = SessionInfo::new(&user, session_id, self.current_time);
        self.sessions.insert(session_id, session.clone());

        // Save updated login stats.
        let _ = save_user_database(&self.users);

        Ok(session)
    }

    /// End a session and return to the login screen.
    pub fn end_session(&mut self, session_id: u64) {
        self.sessions.remove(&session_id);
        self.current_view = LoginView::UserSelect;
        self.password_input.clear();
        self.error_message = None;
        self.idle_seconds = 0;
        self.screen_dimmed = false;
    }

    /// Lock the screen for the given session.
    pub fn lock_screen(&mut self, session_uid: u32) {
        self.current_view = LoginView::Locked;
        self.locked_session_uid = Some(session_uid);
        self.password_input.clear();
        self.error_message = None;
        self.screen_dimmed = false;
        self.accessibility
            .announce("Screen locked. Enter password to unlock.");
    }

    /// Attempt to unlock the screen with a password.
    pub fn unlock_screen(&mut self, password: &str) -> Result<(), String> {
        let uid = match self.locked_session_uid {
            Some(uid) => uid,
            None => return Err("No locked session".to_string()),
        };

        let user = self.users.iter().find(|u| u.uid == uid).cloned();
        let user = match user {
            Some(u) => u,
            None => return Err("Session user not found".to_string()),
        };

        // Verify password (same flow as authenticate).
        let salt_bytes = hex_to_bytes(&user.password_salt).unwrap_or_default();
        let computed_hash = hash_password(&salt_bytes, password);

        if computed_hash == user.password_hash {
            self.current_view = LoginView::UserSelect; // Returns to desktop in real system.
            self.locked_session_uid = None;
            self.idle_seconds = 0;
            self.screen_dimmed = false;
            Ok(())
        } else {
            Err("Incorrect password".to_string())
        }
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event. Returns EventResult::Consumed if handled.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // Any input resets idle timer.
        self.idle_seconds = 0;
        self.screen_dimmed = false;

        match event {
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => {
                self.tick(*elapsed_ms);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Global accessibility shortcuts.
        if key.modifiers.ctrl && key.modifiers.alt {
            match key.key {
                Key::H => {
                    self.accessibility.high_contrast = !self.accessibility.high_contrast;
                    return EventResult::Consumed;
                }
                Key::L => {
                    self.accessibility.large_text = !self.accessibility.large_text;
                    return EventResult::Consumed;
                }
                Key::K => {
                    self.accessibility.onscreen_keyboard = !self.accessibility.onscreen_keyboard;
                    return EventResult::Consumed;
                }
                Key::S => {
                    self.accessibility.screen_reader_enabled =
                        !self.accessibility.screen_reader_enabled;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        match self.current_view {
            LoginView::UserSelect => self.handle_key_user_select(key),
            LoginView::PasswordEntry => self.handle_key_password_entry(key),
            LoginView::Locked => self.handle_key_locked(key),
            LoginView::PowerMenu => self.handle_key_power_menu(key),
            LoginView::ShuttingDown => EventResult::Consumed,
        }
    }

    fn handle_key_user_select(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up | Key::Left => {
                if self.selected_user_index > 0 {
                    self.selected_user_index -= 1;
                } else if !self.users.is_empty() {
                    self.selected_user_index = self.users.len() - 1;
                }
                EventResult::Consumed
            }
            Key::Down | Key::Right => {
                if !self.users.is_empty() {
                    self.selected_user_index = (self.selected_user_index + 1) % self.users.len();
                }
                EventResult::Consumed
            }
            Key::Enter => {
                if let Some(user) = self.users.get(self.selected_user_index).cloned() {
                    if user.requires_password() {
                        self.current_view = LoginView::PasswordEntry;
                        self.password_input.clear();
                        self.error_message = None;
                        self.accessibility.announce(&format!(
                            "Password entry for {}. Type your password.",
                            user.display_name
                        ));
                    } else {
                        // Guest or no-password account: log in directly.
                        let uid = user.uid;
                        match self.start_session(uid) {
                            Ok(_) => {
                                self.accessibility.announce("Logged in as guest.");
                            }
                            Err(msg) => {
                                self.error_message = Some(msg);
                            }
                        }
                    }
                }
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::PowerMenu;
                self.power_menu_selection = 0;
                EventResult::Consumed
            }
            Key::Tab => {
                // Cycle focus to accessibility options in the future.
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key_password_entry(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => {
                self.attempt_login();
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::UserSelect;
                self.password_input.clear();
                self.error_message = None;
                EventResult::Consumed
            }
            Key::Backspace => {
                self.password_input.pop();
                self.error_message = None;
                EventResult::Consumed
            }
            _ => {
                // Type character into password field.
                if let Some(ch) = key.text
                    && !ch.is_control() {
                        self.password_input.push(ch);
                        self.error_message = None;
                    }
                EventResult::Consumed
            }
        }
    }

    fn handle_key_locked(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => {
                let password = self.password_input.clone();
                match self.unlock_screen(&password) {
                    Ok(()) => {
                        self.password_input.clear();
                        self.accessibility.announce("Screen unlocked.");
                    }
                    Err(msg) => {
                        self.error_message = Some(msg);
                        self.password_input.clear();
                    }
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                self.password_input.pop();
                self.error_message = None;
                EventResult::Consumed
            }
            Key::Escape => {
                // Cannot escape lock screen, just clear input.
                self.password_input.clear();
                self.error_message = None;
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text
                    && !ch.is_control() {
                        self.password_input.push(ch);
                        self.error_message = None;
                    }
                EventResult::Consumed
            }
        }
    }

    fn handle_key_power_menu(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                if self.power_menu_selection > 0 {
                    self.power_menu_selection -= 1;
                } else {
                    self.power_menu_selection = 2;
                }
                EventResult::Consumed
            }
            Key::Down => {
                self.power_menu_selection = (self.power_menu_selection + 1) % 3;
                EventResult::Consumed
            }
            Key::Enter => {
                let action = match self.power_menu_selection {
                    0 => PowerAction::Shutdown,
                    1 => PowerAction::Restart,
                    _ => PowerAction::Sleep,
                };
                self.execute_power_action(action);
                EventResult::Consumed
            }
            Key::Escape => {
                self.current_view = LoginView::UserSelect;
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Handle mouse events (click detection on UI elements).
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(mouse.x, mouse.y),
            _ => EventResult::Ignored,
        }
    }

    /// Handle a left-click at the given screen coordinates.
    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        match self.current_view {
            LoginView::UserSelect => {
                // Check if clicking on a user avatar/name.
                let center_x = SCREEN_WIDTH / 2.0;
                let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
                let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

                // User list starts at avatar area.
                let list_y_start = box_y + AVATAR_Y_OFFSET + AVATAR_SIZE + 20.0;
                let item_height = 50.0;

                for (i, _user) in self.users.iter().enumerate() {
                    let item_y = list_y_start + (i as f32) * item_height;
                    if x >= box_x
                        && x <= box_x + LOGIN_BOX_WIDTH
                        && y >= item_y
                        && y <= item_y + item_height
                    {
                        self.selected_user_index = i;
                        // Double-click or single press both select.
                        return EventResult::Consumed;
                    }
                }

                // Check power button (bottom-right corner).
                let power_x = SCREEN_WIDTH - POWER_BUTTON_SIZE - 20.0;
                let power_y = SCREEN_HEIGHT - POWER_BUTTON_SIZE - 20.0;
                if x >= power_x
                    && x <= power_x + POWER_BUTTON_SIZE
                    && y >= power_y
                    && y <= power_y + POWER_BUTTON_SIZE
                {
                    self.current_view = LoginView::PowerMenu;
                    self.power_menu_selection = 0;
                    return EventResult::Consumed;
                }

                // Check accessibility toggle buttons (bottom-left).
                let acc_y = SCREEN_HEIGHT - 50.0;
                if y >= acc_y && y <= acc_y + 30.0 {
                    if (20.0..=60.0).contains(&x) {
                        self.accessibility.high_contrast = !self.accessibility.high_contrast;
                        return EventResult::Consumed;
                    }
                    if (70.0..=110.0).contains(&x) {
                        self.accessibility.large_text = !self.accessibility.large_text;
                        return EventResult::Consumed;
                    }
                    if (120.0..=160.0).contains(&x) {
                        self.accessibility.onscreen_keyboard =
                            !self.accessibility.onscreen_keyboard;
                        return EventResult::Consumed;
                    }
                }

                EventResult::Ignored
            }
            LoginView::PasswordEntry => {
                // Check login button.
                let center_x = SCREEN_WIDTH / 2.0;
                let btn_x = center_x - BUTTON_WIDTH / 2.0;
                let btn_y = SCREEN_HEIGHT / 2.0 + 60.0;
                if x >= btn_x
                    && x <= btn_x + BUTTON_WIDTH
                    && y >= btn_y
                    && y <= btn_y + BUTTON_HEIGHT
                {
                    self.attempt_login();
                    return EventResult::Consumed;
                }

                // Check password visibility toggle.
                let toggle_x = center_x + INPUT_WIDTH / 2.0 - 40.0;
                let toggle_y = SCREEN_HEIGHT / 2.0 - 10.0;
                if x >= toggle_x
                    && x <= toggle_x + 30.0
                    && y >= toggle_y
                    && y <= toggle_y + INPUT_HEIGHT
                {
                    self.password_visible = !self.password_visible;
                    return EventResult::Consumed;
                }

                EventResult::Consumed
            }
            LoginView::Locked => {
                // Similar to password entry.
                EventResult::Consumed
            }
            LoginView::PowerMenu => {
                // Check power menu item clicks.
                let menu_x = SCREEN_WIDTH / 2.0 - POWER_MENU_WIDTH / 2.0;
                let menu_y = SCREEN_HEIGHT / 2.0 - 75.0;
                let item_h = 50.0;

                for i in 0..3 {
                    let item_y = menu_y + (i as f32) * item_h;
                    if x >= menu_x
                        && x <= menu_x + POWER_MENU_WIDTH
                        && y >= item_y
                        && y <= item_y + item_h
                    {
                        self.power_menu_selection = i;
                        let action = match i {
                            0 => PowerAction::Shutdown,
                            1 => PowerAction::Restart,
                            _ => PowerAction::Sleep,
                        };
                        self.execute_power_action(action);
                        return EventResult::Consumed;
                    }
                }

                // Click outside menu to dismiss.
                self.current_view = LoginView::UserSelect;
                EventResult::Consumed
            }
            LoginView::ShuttingDown => EventResult::Consumed,
        }
    }

    /// Attempt login with the current password input.
    fn attempt_login(&mut self) {
        let username = self
            .users
            .get(self.selected_user_index)
            .map(|u| u.username.clone())
            .unwrap_or_default();
        let password = self.password_input.clone();

        match self.authenticate(&username, &password) {
            Ok(()) => {
                let uid = self
                    .users
                    .get(self.selected_user_index)
                    .map(|u| u.uid)
                    .unwrap_or(0);
                match self.start_session(uid) {
                    Ok(_session) => {
                        self.password_input.clear();
                        self.error_message = None;
                        self.current_view = LoginView::UserSelect;
                        self.accessibility.announce("Login successful.");
                    }
                    Err(msg) => {
                        self.error_message = Some(msg);
                    }
                }
            }
            Err(msg) => {
                self.error_message = Some(msg);
                self.password_input.clear();
            }
        }
    }

    /// Execute a power action.
    fn execute_power_action(&mut self, action: PowerAction) {
        match action {
            PowerAction::Shutdown | PowerAction::Restart => {
                self.current_view = LoginView::ShuttingDown;
                self.accessibility
                    .announce(if action == PowerAction::Shutdown {
                        "Shutting down..."
                    } else {
                        "Restarting..."
                    });
                // In a real system, this would invoke the init system.
            }
            PowerAction::Sleep => {
                // Return to login screen; the system would suspend.
                self.current_view = LoginView::UserSelect;
                self.accessibility.announce("System going to sleep.");
            }
        }
    }

    // ========================================================================
    // Tick (idle timeout, lockout countdown)
    // ========================================================================

    /// Called periodically with elapsed time. Handles idle timeout and lockout expiry.
    pub fn tick(&mut self, elapsed_ms: u64) {
        let elapsed_secs = elapsed_ms / 1000;
        self.current_time = self.current_time.saturating_add(elapsed_secs);
        self.idle_seconds = self.idle_seconds.saturating_add(elapsed_secs);

        // Update clock display (HH:MM format from timestamp).
        let total_minutes = self.current_time / 60;
        let hours = (total_minutes / 60) % 24;
        let minutes = total_minutes % 60;
        self.clock_display = format!("{:02}:{:02}", hours, minutes);

        // Idle timeout → lock screen (only if a session is active).
        if !self.sessions.is_empty() && self.current_view != LoginView::Locked {
            let dim_threshold = self.lock_timeout_secs.saturating_sub(30);
            if self.idle_seconds >= self.lock_timeout_secs {
                // Find the active session to lock.
                if let Some(session) = self.sessions.values().next() {
                    let uid = session.user_uid;
                    self.lock_screen(uid);
                }
            } else if self.idle_seconds >= dim_threshold {
                self.screen_dimmed = true;
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the current login screen state to a RenderTree.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background.
        self.render_background(&mut tree);

        // Clock (top-right).
        self.render_clock(&mut tree);

        // Main content based on view.
        match self.current_view {
            LoginView::UserSelect => self.render_user_select(&mut tree),
            LoginView::PasswordEntry => self.render_password_entry(&mut tree),
            LoginView::Locked => self.render_lock_screen(&mut tree),
            LoginView::PowerMenu => {
                // Show user select behind the overlay.
                self.render_user_select(&mut tree);
                self.render_power_menu_overlay(&mut tree);
            }
            LoginView::ShuttingDown => self.render_shutdown_screen(&mut tree),
        }

        // Accessibility buttons (bottom-left).
        self.render_accessibility_buttons(&mut tree);

        // Power button (bottom-right, except during power menu / shutdown).
        if self.current_view != LoginView::PowerMenu && self.current_view != LoginView::ShuttingDown
        {
            self.render_power_button(&mut tree);
        }

        // Screen dim overlay (idle warning).
        if self.screen_dimmed {
            tree.fill_rect(
                0.0,
                0.0,
                SCREEN_WIDTH,
                SCREEN_HEIGHT,
                Color::rgba(0, 0, 0, 128),
            );
        }

        tree
    }

    /// Render the gradient background.
    fn render_background(&self, tree: &mut RenderTree) {
        let (bg_top, bg_bottom) = if self.accessibility.high_contrast {
            (COL_HC_BG, COL_HC_BG)
        } else {
            (COL_BG_TOP, COL_BG_BOTTOM)
        };

        // Render as two halves for a simple gradient approximation.
        tree.fill_rect(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT / 2.0, bg_top);
        tree.fill_rect(
            0.0,
            SCREEN_HEIGHT / 2.0,
            SCREEN_WIDTH,
            SCREEN_HEIGHT / 2.0,
            bg_bottom,
        );

        // Subtle decorative circles in background (disabled in high contrast).
        if !self.accessibility.high_contrast {
            tree.fill_rounded_rect(
                -100.0,
                -100.0,
                400.0,
                400.0,
                Color::rgba(137, 180, 250, 8),
                CornerRadii::all(200.0),
            );
            tree.fill_rounded_rect(
                SCREEN_WIDTH - 200.0,
                SCREEN_HEIGHT - 200.0,
                500.0,
                500.0,
                Color::rgba(203, 166, 247, 6),
                CornerRadii::all(250.0),
            );
        }
    }

    /// Render the clock in the top-right corner.
    fn render_clock(&self, tree: &mut RenderTree) {
        let font_size = self.scaled_font(FONT_SIZE_CLOCK);
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        tree.push(RenderCommand::Text {
            x: SCREEN_WIDTH - 100.0,
            y: 20.0,
            text: self.clock_display.clone(),
            color: text_color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the user selection screen.
    fn render_user_select(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };

        // Login box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Login box background.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // Title.
        let title_y = box_y + 30.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        tree.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: title_y,
            text: "Sign In".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_LARGE),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // User list.
        let list_y_start = title_y + 50.0;
        let item_height = 60.0;

        for (i, user) in self.users.iter().enumerate() {
            let item_y = list_y_start + (i as f32) * item_height;
            let is_selected = i == self.selected_user_index;

            // Selection highlight.
            if is_selected {
                let accent = if self.accessibility.high_contrast {
                    COL_HC_ACCENT
                } else {
                    COL_ACCENT
                };
                tree.fill_rounded_rect(
                    box_x + 16.0,
                    item_y,
                    LOGIN_BOX_WIDTH - 32.0,
                    item_height - 4.0,
                    Color::rgba(accent.r, accent.g, accent.b, 30),
                    CornerRadii::all(8.0),
                );
                // Left accent bar.
                tree.fill_rounded_rect(
                    box_x + 16.0,
                    item_y + 8.0,
                    3.0,
                    item_height - 20.0,
                    accent,
                    CornerRadii::all(2.0),
                );
            }

            // Avatar circle.
            let avatar_x = box_x + 36.0;
            let avatar_y = item_y + (item_height - 36.0) / 2.0;
            let avatar_radius = 18.0;
            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                avatar_radius * 2.0,
                avatar_radius * 2.0,
                user.avatar_color(),
                CornerRadii::all(avatar_radius),
            );

            // Initials in avatar.
            let initials = user.initials();
            tree.push(RenderCommand::Text {
                x: avatar_x + avatar_radius - 8.0,
                y: avatar_y + avatar_radius - 6.0,
                text: initials,
                color: COL_BG_DARK,
                font_size: self.scaled_font(12.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Username and display name.
            let name_x = avatar_x + avatar_radius * 2.0 + 12.0;
            tree.push(RenderCommand::Text {
                x: name_x,
                y: item_y + 14.0,
                text: user.display_name.clone(),
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(LOGIN_BOX_WIDTH - 120.0),
            });

            let subtext_color = if self.accessibility.high_contrast {
                COL_HC_TEXT
            } else {
                COL_SUBTEXT
            };
            tree.push(RenderCommand::Text {
                x: name_x,
                y: item_y + 34.0,
                text: format!("@{}", user.username),
                color: subtext_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(LOGIN_BOX_WIDTH - 120.0),
            });

            // Admin badge.
            if user.is_admin {
                let badge_x = box_x + LOGIN_BOX_WIDTH - 70.0;
                tree.fill_rounded_rect(
                    badge_x,
                    item_y + 18.0,
                    40.0,
                    20.0,
                    Color::rgba(250, 179, 135, 40),
                    CornerRadii::all(4.0),
                );
                tree.push(RenderCommand::Text {
                    x: badge_x + 5.0,
                    y: item_y + 21.0,
                    text: "admin".to_string(),
                    color: COL_WARNING,
                    font_size: self.scaled_font(10.0),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // Hint text at bottom.
        let hint_y = box_y + LOGIN_BOX_HEIGHT - 40.0;
        let hint_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };
        tree.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: hint_y,
            text: "Press Enter to sign in | Esc for power menu".to_string(),
            color: hint_color,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: Some(LOGIN_BOX_WIDTH - 40.0),
        });
    }

    /// Render the password entry screen.
    fn render_password_entry(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Login box.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // Selected user avatar (large, centered).
        if let Some(user) = self.users.get(self.selected_user_index) {
            let avatar_x = center_x - AVATAR_SIZE / 2.0;
            let avatar_y = box_y + AVATAR_Y_OFFSET;

            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                AVATAR_SIZE,
                AVATAR_SIZE,
                user.avatar_color(),
                CornerRadii::all(AVATAR_SIZE / 2.0),
            );

            // Initials.
            let initials = user.initials();
            tree.push(RenderCommand::Text {
                x: avatar_x + AVATAR_SIZE / 2.0 - 14.0,
                y: avatar_y + AVATAR_SIZE / 2.0 - 10.0,
                text: initials,
                color: COL_BG_DARK,
                font_size: self.scaled_font(22.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Display name.
            let name_y = avatar_y + AVATAR_SIZE + 16.0;
            tree.push(RenderCommand::Text {
                x: center_x - 60.0,
                y: name_y,
                text: user.display_name.clone(),
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_LARGE),
                font_weight: FontWeightHint::Bold,
                max_width: Some(LOGIN_BOX_WIDTH - 40.0),
            });

            // Username.
            tree.push(RenderCommand::Text {
                x: center_x - 40.0,
                y: name_y + 30.0,
                text: format!("@{}", user.username),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Password input field.
        let input_x = center_x - INPUT_WIDTH / 2.0;
        let input_y = box_y + 240.0;
        let input_border_color = if self.error_message.is_some() {
            if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            }
        } else {
            COL_INPUT_FOCUS
        };

        // Input background.
        tree.fill_rounded_rect(
            input_x,
            input_y,
            INPUT_WIDTH,
            INPUT_HEIGHT,
            COL_INPUT_BG,
            CornerRadii::all(INPUT_RADIUS),
        );

        // Input border.
        tree.push(RenderCommand::StrokeRect {
            x: input_x,
            y: input_y,
            width: INPUT_WIDTH,
            height: INPUT_HEIGHT,
            color: input_border_color,
            line_width: 2.0,
            corner_radii: CornerRadii::all(INPUT_RADIUS),
        });

        // Password text (masked or visible).
        let display_text = if self.password_visible {
            self.password_input.clone()
        } else {
            "\u{2022}".repeat(self.password_input.len())
        };

        if display_text.is_empty() {
            // Placeholder.
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: "Password".to_string(),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 60.0),
            });
        } else {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: display_text,
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 60.0),
            });
        }

        // Show/hide password toggle.
        let toggle_x = input_x + INPUT_WIDTH - 40.0;
        let toggle_text = if self.password_visible {
            "Hide"
        } else {
            "Show"
        };
        tree.push(RenderCommand::Text {
            x: toggle_x,
            y: input_y + 14.0,
            text: toggle_text.to_string(),
            color: accent,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Error message.
        if let Some(ref error) = self.error_message {
            let err_color = if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            };
            tree.push(RenderCommand::Text {
                x: input_x,
                y: input_y + INPUT_HEIGHT + 8.0,
                text: error.clone(),
                color: err_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH),
            });
        }

        // Login button.
        let btn_x = center_x - BUTTON_WIDTH / 2.0;
        let btn_y = input_y + INPUT_HEIGHT + 40.0;
        tree.fill_rounded_rect(
            btn_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            accent,
            CornerRadii::all(BUTTON_RADIUS),
        );
        tree.push(RenderCommand::Text {
            x: center_x - 20.0,
            y: btn_y + 13.0,
            text: "Sign In".to_string(),
            color: COL_BG_DARK,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // "Back" link.
        let back_y = btn_y + BUTTON_HEIGHT + 16.0;
        tree.push(RenderCommand::Text {
            x: center_x - 30.0,
            y: back_y,
            text: "< Back to user list".to_string(),
            color: accent,
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the lock screen.
    fn render_lock_screen(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let box_x = center_x - LOGIN_BOX_WIDTH / 2.0;
        let box_y = SCREEN_HEIGHT / 2.0 - LOGIN_BOX_HEIGHT / 2.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Box shadow.
        if !self.accessibility.high_contrast {
            tree.push(RenderCommand::BoxShadow {
                x: box_x,
                y: box_y,
                width: LOGIN_BOX_WIDTH,
                height: LOGIN_BOX_HEIGHT,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: CornerRadii::all(LOGIN_BOX_RADIUS),
            });
        }

        // Box.
        tree.fill_rounded_rect(
            box_x,
            box_y,
            LOGIN_BOX_WIDTH,
            LOGIN_BOX_HEIGHT,
            panel_color,
            CornerRadii::all(LOGIN_BOX_RADIUS),
        );

        // "Locked" badge.
        let badge_y = box_y + 20.0;
        tree.fill_rounded_rect(
            center_x - 40.0,
            badge_y,
            80.0,
            28.0,
            Color::rgba(243, 139, 168, 30),
            CornerRadii::all(14.0),
        );
        let lock_color = if self.accessibility.high_contrast {
            COL_HC_ERROR
        } else {
            COL_ERROR
        };
        tree.push(RenderCommand::Text {
            x: center_x - 28.0,
            y: badge_y + 6.0,
            text: "Locked".to_string(),
            color: lock_color,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Show locked user avatar.
        if let Some(uid) = self.locked_session_uid
            && let Some(user) = self.users.iter().find(|u| u.uid == uid) {
                let avatar_x = center_x - AVATAR_SIZE / 2.0;
                let avatar_y = box_y + 70.0;

                tree.fill_rounded_rect(
                    avatar_x,
                    avatar_y,
                    AVATAR_SIZE,
                    AVATAR_SIZE,
                    user.avatar_color(),
                    CornerRadii::all(AVATAR_SIZE / 2.0),
                );

                let initials = user.initials();
                tree.push(RenderCommand::Text {
                    x: avatar_x + AVATAR_SIZE / 2.0 - 14.0,
                    y: avatar_y + AVATAR_SIZE / 2.0 - 10.0,
                    text: initials,
                    color: COL_BG_DARK,
                    font_size: self.scaled_font(22.0),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Display name.
                tree.push(RenderCommand::Text {
                    x: center_x - 60.0,
                    y: avatar_y + AVATAR_SIZE + 16.0,
                    text: user.display_name.clone(),
                    color: text_color,
                    font_size: self.scaled_font(FONT_SIZE_LARGE),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(LOGIN_BOX_WIDTH - 40.0),
                });
            }

        // Password input.
        let input_x = center_x - INPUT_WIDTH / 2.0;
        let input_y = box_y + 260.0;
        let input_border = if self.error_message.is_some() {
            if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            }
        } else {
            COL_INPUT_FOCUS
        };

        tree.fill_rounded_rect(
            input_x,
            input_y,
            INPUT_WIDTH,
            INPUT_HEIGHT,
            COL_INPUT_BG,
            CornerRadii::all(INPUT_RADIUS),
        );
        tree.push(RenderCommand::StrokeRect {
            x: input_x,
            y: input_y,
            width: INPUT_WIDTH,
            height: INPUT_HEIGHT,
            color: input_border,
            line_width: 2.0,
            corner_radii: CornerRadii::all(INPUT_RADIUS),
        });

        // Password text.
        let display_text = "\u{2022}".repeat(self.password_input.len());
        if display_text.is_empty() {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: "Enter password to unlock".to_string(),
                color: if self.accessibility.high_contrast {
                    COL_HC_TEXT
                } else {
                    COL_SUBTEXT
                },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 32.0),
            });
        } else {
            tree.push(RenderCommand::Text {
                x: input_x + 16.0,
                y: input_y + 13.0,
                text: display_text,
                color: text_color,
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH - 32.0),
            });
        }

        // Error message.
        if let Some(ref error) = self.error_message {
            let err_color = if self.accessibility.high_contrast {
                COL_HC_ERROR
            } else {
                COL_ERROR
            };
            tree.push(RenderCommand::Text {
                x: input_x,
                y: input_y + INPUT_HEIGHT + 8.0,
                text: error.clone(),
                color: err_color,
                font_size: self.scaled_font(FONT_SIZE_SMALL),
                font_weight: FontWeightHint::Regular,
                max_width: Some(INPUT_WIDTH),
            });
        }

        // Unlock button.
        let btn_y = input_y + INPUT_HEIGHT + 40.0;
        let btn_x = center_x - BUTTON_WIDTH / 2.0;
        tree.fill_rounded_rect(
            btn_x,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            accent,
            CornerRadii::all(BUTTON_RADIUS),
        );
        tree.push(RenderCommand::Text {
            x: center_x - 22.0,
            y: btn_y + 13.0,
            text: "Unlock".to_string(),
            color: COL_BG_DARK,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the power menu overlay.
    fn render_power_menu_overlay(&self, tree: &mut RenderTree) {
        // Dim overlay.
        tree.fill_rect(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT, COL_POWER_BG);

        let center_x = SCREEN_WIDTH / 2.0;
        let menu_x = center_x - POWER_MENU_WIDTH / 2.0;
        let menu_y = SCREEN_HEIGHT / 2.0 - 100.0;
        let item_height = 56.0;

        let panel_color = if self.accessibility.high_contrast {
            COL_HC_PANEL
        } else {
            COL_PANEL
        };
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };
        let accent = if self.accessibility.high_contrast {
            COL_HC_ACCENT
        } else {
            COL_ACCENT
        };

        // Menu background.
        tree.fill_rounded_rect(
            menu_x - 16.0,
            menu_y - 16.0,
            POWER_MENU_WIDTH + 32.0,
            item_height * 3.0 + 48.0,
            panel_color,
            CornerRadii::all(12.0),
        );

        // Title.
        tree.push(RenderCommand::Text {
            x: center_x - 30.0,
            y: menu_y - 8.0,
            text: "Power".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let items = ["Shut Down", "Restart", "Sleep"];
        let icons = ["\u{23FB}", "\u{21BB}", "\u{263E}"]; // Unicode power, refresh, moon

        for (i, (label, icon)) in items.iter().zip(icons.iter()).enumerate() {
            let item_y = menu_y + 20.0 + (i as f32) * item_height;
            let is_selected = i == self.power_menu_selection;

            if is_selected {
                tree.fill_rounded_rect(
                    menu_x,
                    item_y,
                    POWER_MENU_WIDTH,
                    item_height - 4.0,
                    Color::rgba(accent.r, accent.g, accent.b, 30),
                    CornerRadii::all(8.0),
                );
            }

            // Icon.
            tree.push(RenderCommand::Text {
                x: menu_x + 16.0,
                y: item_y + 16.0,
                text: icon.to_string(),
                color: if is_selected { accent } else { text_color },
                font_size: self.scaled_font(20.0),
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Label.
            tree.push(RenderCommand::Text {
                x: menu_x + 48.0,
                y: item_y + 18.0,
                text: label.to_string(),
                color: if is_selected { accent } else { text_color },
                font_size: self.scaled_font(FONT_SIZE_NORMAL),
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }

        // Escape hint.
        let hint_y = menu_y + 20.0 + 3.0 * item_height + 8.0;
        tree.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: hint_y,
            text: "Esc to cancel".to_string(),
            color: if self.accessibility.high_contrast {
                COL_HC_TEXT
            } else {
                COL_SUBTEXT
            },
            font_size: self.scaled_font(FONT_SIZE_SMALL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the shutdown/restart screen.
    fn render_shutdown_screen(&self, tree: &mut RenderTree) {
        let center_x = SCREEN_WIDTH / 2.0;
        let center_y = SCREEN_HEIGHT / 2.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_TEXT
        };

        // Simple centered message.
        tree.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y - 20.0,
            text: "Shutting down...".to_string(),
            color: text_color,
            font_size: self.scaled_font(FONT_SIZE_LARGE),
            font_weight: FontWeightHint::Light,
            max_width: None,
        });

        // Spinner placeholder (just a dot animation would go here).
        tree.push(RenderCommand::Text {
            x: center_x - 10.0,
            y: center_y + 20.0,
            text: "\u{25CF} \u{25CB} \u{25CB}".to_string(),
            color: if self.accessibility.high_contrast {
                COL_HC_ACCENT
            } else {
                COL_ACCENT
            },
            font_size: self.scaled_font(FONT_SIZE_NORMAL),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the accessibility toggle buttons (bottom-left corner).
    fn render_accessibility_buttons(&self, tree: &mut RenderTree) {
        let y = SCREEN_HEIGHT - 50.0;
        let btn_size = 36.0;
        let spacing = 8.0;
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };

        let buttons = [
            ("HC", self.accessibility.high_contrast),
            ("Aa", self.accessibility.large_text),
            ("KB", self.accessibility.onscreen_keyboard),
        ];

        for (i, (label, active)) in buttons.iter().enumerate() {
            let x = 20.0 + (i as f32) * (btn_size + spacing);
            let bg = if *active {
                Color::rgba(137, 180, 250, 60)
            } else {
                Color::rgba(88, 91, 112, 40)
            };

            tree.fill_rounded_rect(x, y, btn_size, btn_size, bg, CornerRadii::all(6.0));
            tree.push(RenderCommand::Text {
                x: x + 6.0,
                y: y + 10.0,
                text: label.to_string(),
                color: if *active {
                    if self.accessibility.high_contrast {
                        COL_HC_ACCENT
                    } else {
                        COL_ACCENT
                    }
                } else {
                    text_color
                },
                font_size: self.scaled_font(12.0),
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    /// Render the power button (bottom-right corner).
    fn render_power_button(&self, tree: &mut RenderTree) {
        let x = SCREEN_WIDTH - POWER_BUTTON_SIZE - 20.0;
        let y = SCREEN_HEIGHT - POWER_BUTTON_SIZE - 20.0;

        tree.fill_rounded_rect(
            x,
            y,
            POWER_BUTTON_SIZE,
            POWER_BUTTON_SIZE,
            Color::rgba(88, 91, 112, 40),
            CornerRadii::all(POWER_BUTTON_SIZE / 2.0),
        );

        // Power icon (Unicode symbol).
        let text_color = if self.accessibility.high_contrast {
            COL_HC_TEXT
        } else {
            COL_SUBTEXT
        };
        tree.push(RenderCommand::Text {
            x: x + POWER_BUTTON_SIZE / 2.0 - 8.0,
            y: y + POWER_BUTTON_SIZE / 2.0 - 8.0,
            text: "\u{23FB}".to_string(),
            color: text_color,
            font_size: self.scaled_font(18.0),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    /// Apply font scaling for large text accessibility mode.
    fn scaled_font(&self, base_size: f32) -> f32 {
        if self.accessibility.large_text {
            base_size * LARGE_FONT_SCALE
        } else {
            base_size
        }
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let mut manager = LoginManager::new();

    // Check for auto-login configuration.
    if let Some(_session) = manager.check_auto_login() {
        // Auto-login succeeded; in a real system, we would launch the compositor.
        return;
    }

    // In a real SlateOS environment, this enters the compositor event loop.
    // For now, render one frame to verify the UI builds correctly.
    let tree = manager.render();
    assert!(!tree.is_empty(), "Login UI must produce render commands");

    // Verify password entry view renders too.
    manager.current_view = LoginView::PasswordEntry;
    let tree2 = manager.render();
    assert!(
        !tree2.is_empty(),
        "Password entry UI must produce render commands"
    );

    // Verify lock screen renders.
    manager.lock_screen(0);
    let tree3 = manager.render();
    assert!(
        !tree3.is_empty(),
        "Lock screen UI must produce render commands"
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test login manager with known accounts.
    fn test_manager() -> LoginManager {
        let users = vec![
            UserAccount::new_with_password(1000, "alice", "Alice Smith", "password123", false),
            UserAccount::new_with_password(1001, "bob", "Bob Jones", "hunter2", true),
            UserAccount::guest_account(),
        ];
        LoginManager::with_users(users)
    }

    // ========================================================================
    // Authentication tests
    // ========================================================================

    #[test]
    fn test_authenticate_success() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("alice", "password123");
        assert!(
            result.is_ok(),
            "Valid password should authenticate: {:?}",
            result
        );
    }

    #[test]
    fn test_authenticate_wrong_password() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("alice", "wrongpass");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Incorrect password"));
    }

    #[test]
    fn test_authenticate_nonexistent_user() {
        let mut mgr = test_manager();
        let result = mgr.authenticate("nobody", "pass");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("User not found"));
    }

    #[test]
    fn test_authenticate_guest_no_password() {
        let mut mgr = test_manager();
        // Guest should authenticate with any password (including empty).
        let result = mgr.authenticate("guest", "");
        assert!(result.is_ok());
        let result2 = mgr.authenticate("guest", "anything");
        assert!(result2.is_ok());
    }

    #[test]
    fn test_authenticate_updates_login_stats() {
        let mut mgr = test_manager();
        mgr.current_time = 1000;
        let _ = mgr.authenticate("alice", "password123");
        let alice = mgr.users.iter().find(|u| u.username == "alice");
        assert!(alice.is_some());
        let alice = alice.unwrap();
        assert_eq!(alice.last_login_timestamp, 1000);
        assert_eq!(alice.login_count, 1);
    }

    // ========================================================================
    // Account lockout tests
    // ========================================================================

    #[test]
    fn test_lockout_after_max_failures() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        // Fail 5 times.
        for i in 0..5 {
            let result = mgr.authenticate("alice", "wrong");
            if i < 4 {
                assert!(result.unwrap_err().contains("attempts remaining"));
            } else {
                assert!(result.unwrap_err().contains("locked"));
            }
        }

        // Now even the correct password should fail (locked).
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Account locked"));
    }

    #[test]
    fn test_lockout_expires() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        // Lock the account.
        for _ in 0..5 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Advance time past lockout (5 minutes = 300 seconds).
        mgr.current_time = 100 + 301;

        // Should be able to authenticate now.
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_lockout_reset_on_success() {
        let mut mgr = test_manager();

        // Fail a few times (but not enough to lock).
        for _ in 0..3 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Succeed.
        let result = mgr.authenticate("alice", "password123");
        assert!(result.is_ok());

        // Lockout state should be reset; we should get 5 fresh attempts.
        let lockout = mgr.locked_accounts.get(&1000);
        assert!(lockout.is_none() || lockout.unwrap().failed_attempts == 0);
    }

    #[test]
    fn test_lockout_countdown() {
        let mut mgr = test_manager();
        mgr.current_time = 100;

        for _ in 0..5 {
            let _ = mgr.authenticate("alice", "wrong");
        }

        // Check countdown.
        mgr.current_time = 200;
        let result = mgr.authenticate("alice", "password123");
        let err = result.unwrap_err();
        assert!(err.contains("Account locked"));
        // Should show approximately 200 seconds remaining.
        assert!(err.contains("200"));
    }

    // ========================================================================
    // Session management tests
    // ========================================================================

    #[test]
    fn test_start_session() {
        let mut mgr = test_manager();
        mgr.current_time = 5000;
        let session = mgr.start_session(1000);
        assert!(session.is_ok());
        let session = session.unwrap();
        assert_eq!(session.user_uid, 1000);
        assert_eq!(session.started_at, 5000);
        assert_eq!(session.home_dir, "/home/alice");
        assert_eq!(session.environment.get("USER"), Some(&"alice".to_string()));
        assert_eq!(
            session.environment.get("HOME"),
            Some(&"/home/alice".to_string())
        );
    }

    #[test]
    fn test_start_session_nonexistent_user() {
        let mut mgr = test_manager();
        let result = mgr.start_session(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_end_session() {
        let mut mgr = test_manager();
        let session = mgr.start_session(1000).unwrap();
        let sid = session.session_id;
        assert!(mgr.sessions.contains_key(&sid));

        mgr.end_session(sid);
        assert!(!mgr.sessions.contains_key(&sid));
        assert_eq!(mgr.current_view, LoginView::UserSelect);
    }

    #[test]
    fn test_session_ids_increment() {
        let mut mgr = test_manager();
        let s1 = mgr.start_session(1000).unwrap();
        let s2 = mgr.start_session(1001).unwrap();
        assert_eq!(s1.session_id + 1, s2.session_id);
    }

    // ========================================================================
    // Lock screen tests
    // ========================================================================

    #[test]
    fn test_lock_screen() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        assert_eq!(mgr.current_view, LoginView::Locked);
        assert_eq!(mgr.locked_session_uid, Some(1000));
    }

    #[test]
    fn test_unlock_screen_correct_password() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let result = mgr.unlock_screen("password123");
        assert!(result.is_ok());
        assert_ne!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_unlock_screen_wrong_password() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let result = mgr.unlock_screen("wrongpass");
        assert!(result.is_err());
        assert_eq!(mgr.current_view, LoginView::Locked);
    }

    // ========================================================================
    // Idle timeout tests
    // ========================================================================

    #[test]
    fn test_idle_timeout_locks_screen() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.lock_timeout_secs = 60;

        // Tick past the timeout.
        mgr.tick(61000); // 61 seconds in ms.
        assert_eq!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_idle_dimming_before_lock() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.lock_timeout_secs = 60;

        // Tick to 30s before lock (within dim threshold).
        mgr.tick(31000);
        assert!(mgr.screen_dimmed);
        assert_ne!(mgr.current_view, LoginView::Locked);
    }

    #[test]
    fn test_input_resets_idle() {
        let mut mgr = test_manager();
        let _ = mgr.start_session(1000);
        mgr.idle_seconds = 50;
        mgr.screen_dimmed = true;

        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        mgr.handle_event(&event);

        assert_eq!(mgr.idle_seconds, 0);
        assert!(!mgr.screen_dimmed);
    }

    // ========================================================================
    // SHA-256 tests
    // ========================================================================

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let hex = bytes_to_hex(&hash);
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc() {
        let hash = sha256(b"abc");
        let hex = bytes_to_hex(&hash);
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_long_message() {
        // "a" repeated 1_000_000 times.
        let data = vec![b'a'; 1_000_000];
        let hash = sha256(&data);
        let hex = bytes_to_hex(&hash);
        assert_eq!(
            hex,
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    // ========================================================================
    // Password hashing tests
    // ========================================================================

    #[test]
    fn test_password_hash_deterministic() {
        let salt = b"fixed_salt_value";
        let hash1 = hash_password(salt, "mypassword");
        let hash2 = hash_password(salt, "mypassword");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_password_hash_different_passwords() {
        let salt = b"same_salt";
        let hash1 = hash_password(salt, "password1");
        let hash2 = hash_password(salt, "password2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_password_hash_different_salts() {
        let hash1 = hash_password(b"salt_a", "samepass");
        let hash2 = hash_password(b"salt_b", "samepass");
        assert_ne!(hash1, hash2);
    }

    // ========================================================================
    // User database serialization tests
    // ========================================================================

    #[test]
    fn test_serialize_and_parse_roundtrip() {
        let users = vec![
            UserAccount::new_with_password(1000, "testuser", "Test User", "pass", false),
            UserAccount::guest_account(),
        ];
        let yaml = serialize_users_yaml(&users);
        let parsed = parse_users_yaml(&yaml);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].uid, 1000);
        assert_eq!(parsed[0].username, "testuser");
        assert_eq!(parsed[0].display_name, "Test User");
        assert!(!parsed[0].is_admin);
        assert_eq!(parsed[1].uid, 65534);
        assert_eq!(parsed[1].username, "guest");
    }

    #[test]
    fn test_parse_empty_yaml() {
        let parsed = parse_users_yaml("");
        assert!(parsed.is_empty());
    }

    // ========================================================================
    // Rendering tests
    // ========================================================================

    #[test]
    fn test_render_user_select() {
        let mgr = test_manager();
        let tree = mgr.render();
        assert!(!tree.is_empty());
        // Should have background + clock + user list + accessibility buttons + power button.
        assert!(tree.len() > 10);
    }

    #[test]
    fn test_render_password_entry() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "secret".to_string();
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_lock_screen() {
        let mut mgr = test_manager();
        mgr.lock_screen(1000);
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_power_menu() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PowerMenu;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_shutdown() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::ShuttingDown;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_high_contrast() {
        let mut mgr = test_manager();
        mgr.accessibility.high_contrast = true;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_large_text() {
        let mut mgr = test_manager();
        mgr.accessibility.large_text = true;
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_error_message() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.error_message = Some("Incorrect password. 4 attempts remaining.".to_string());
        let tree = mgr.render();
        assert!(!tree.is_empty());
    }

    // ========================================================================
    // Event handling tests
    // ========================================================================

    #[test]
    fn test_navigate_user_list() {
        let mut mgr = test_manager();
        assert_eq!(mgr.selected_user_index, 0);

        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 1);

        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 2);

        // Wraps around.
        mgr.handle_event(&down);
        assert_eq!(mgr.selected_user_index, 0);
    }

    #[test]
    fn test_enter_selects_user() {
        let mut mgr = test_manager();
        // Alice requires password.
        let enter = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&enter);
        assert_eq!(mgr.current_view, LoginView::PasswordEntry);
    }

    #[test]
    fn test_escape_opens_power_menu() {
        let mut mgr = test_manager();
        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&esc);
        assert_eq!(mgr.current_view, LoginView::PowerMenu);
    }

    #[test]
    fn test_escape_in_password_goes_back() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "typed".to_string();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&esc);
        assert_eq!(mgr.current_view, LoginView::UserSelect);
        assert!(mgr.password_input.is_empty());
    }

    #[test]
    fn test_typing_password() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;

        for ch in "hello".chars() {
            let event = Event::Key(KeyEvent {
                key: Key::A, // Key code doesn't matter for text input.
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some(ch),
            });
            mgr.handle_event(&event);
        }
        assert_eq!(mgr.password_input, "hello");
    }

    #[test]
    fn test_backspace_in_password() {
        let mut mgr = test_manager();
        mgr.current_view = LoginView::PasswordEntry;
        mgr.password_input = "abc".to_string();

        let backspace = Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        mgr.handle_event(&backspace);
        assert_eq!(mgr.password_input, "ab");
    }

    #[test]
    fn test_accessibility_toggle_shortcut() {
        let mut mgr = test_manager();
        assert!(!mgr.accessibility.high_contrast);

        let event = Event::Key(KeyEvent {
            key: Key::H,
            pressed: true,
            modifiers: Modifiers {
                shift: false,
                ctrl: true,
                alt: true,
                super_key: false,
            },
            text: None,
        });
        mgr.handle_event(&event);
        assert!(mgr.accessibility.high_contrast);

        // Toggle off.
        mgr.handle_event(&event);
        assert!(!mgr.accessibility.high_contrast);
    }

    // ========================================================================
    // Hex conversion tests
    // ========================================================================

    #[test]
    fn test_hex_roundtrip() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        let hex = bytes_to_hex(&data);
        assert_eq!(hex, "deadbeef");
        let back = hex_to_bytes(&hex).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn test_hex_invalid() {
        assert!(hex_to_bytes("xyz").is_none());
        assert!(hex_to_bytes("0").is_none()); // Odd length.
    }

    // ========================================================================
    // UserAccount helper tests
    // ========================================================================

    #[test]
    fn test_user_initials() {
        let user = UserAccount::new_with_password(1, "jd", "John Doe", "pass", false);
        assert_eq!(user.initials(), "JD");

        let single = UserAccount::new_with_password(2, "x", "Xavier", "pass", false);
        assert_eq!(single.initials(), "X");
    }

    #[test]
    fn test_guest_requires_no_password() {
        let guest = UserAccount::guest_account();
        assert!(!guest.requires_password());

        let normal = UserAccount::new_with_password(1, "u", "User", "p", false);
        assert!(normal.requires_password());
    }

    // ========================================================================
    // Auto-login tests
    // ========================================================================

    #[test]
    fn test_auto_login() {
        let mut users = vec![UserAccount::new_with_password(
            1000,
            "auto",
            "Auto User",
            "pass",
            false,
        )];
        users[0].auto_login = true;

        let mut mgr = LoginManager::with_users(users);
        let session = mgr.check_auto_login();
        assert!(session.is_some());
        assert_eq!(session.unwrap().user_uid, 1000);
    }

    #[test]
    fn test_no_auto_login() {
        let mgr_users = vec![UserAccount::new_with_password(
            1000,
            "normal",
            "Normal User",
            "pass",
            false,
        )];
        let mut mgr = LoginManager::with_users(mgr_users);
        let session = mgr.check_auto_login();
        assert!(session.is_none());
    }
}
