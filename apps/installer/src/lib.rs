//! OurOS Installer — unattended and interactive installation library.
//!
//! Provides YAML-based configuration parsing, validation, and install plan
//! generation for automated OurOS installations. The installer reads a YAML
//! config file describing disk layout, users, network, packages, and services,
//! validates the configuration, and produces an ordered sequence of install
//! steps that the runtime executor can carry out.
//!
//! # Architecture
//!
//! ```text
//! YAML text  -->  YamlParser  -->  YamlValue tree
//!                                      |
//!                              InstallConfig::from_yaml
//!                                      |
//!                               InstallConfig (validated)
//!                                      |
//!                              InstallPlan::from_config
//!                                      |
//!                               InstallPlan { steps }
//! ```

#![allow(dead_code)]

pub mod grub;

use std::fmt;

// ============================================================================
// Error types
// ============================================================================

/// Errors from the YAML parser.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// Unexpected end of input while parsing.
    UnexpectedEof,
    /// Expected a colon after a mapping key.
    ExpectedColon { line: usize },
    /// Inconsistent indentation level.
    BadIndentation { line: usize },
    /// A generic syntax error at the given line.
    Syntax { line: usize, message: String },
    /// Invalid numeric literal.
    InvalidNumber { line: usize, value: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
            Self::ExpectedColon { line } => {
                write!(f, "line {line}: expected ':' after mapping key")
            }
            Self::BadIndentation { line } => {
                write!(f, "line {line}: inconsistent indentation")
            }
            Self::Syntax { line, message } => {
                write!(f, "line {line}: {message}")
            }
            Self::InvalidNumber { line, value } => {
                write!(f, "line {line}: invalid number '{value}'")
            }
        }
    }
}

/// Errors from configuration parsing or validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    /// A required field is missing.
    MissingField(String),
    /// A field has an invalid value.
    InvalidValue { field: String, message: String },
    /// YAML parsing failed.
    Parse(ParseError),
    /// A type mismatch (expected map, got list, etc.).
    TypeError { field: String, expected: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(name) => write!(f, "missing required field: {name}"),
            Self::InvalidValue { field, message } => {
                write!(f, "invalid value for '{field}': {message}")
            }
            Self::Parse(e) => write!(f, "YAML parse error: {e}"),
            Self::TypeError { field, expected } => {
                write!(f, "type error for '{field}': expected {expected}")
            }
        }
    }
}

impl From<ParseError> for ConfigError {
    fn from(e: ParseError) -> Self {
        Self::Parse(e)
    }
}

// ============================================================================
// YAML value representation
// ============================================================================

/// A parsed YAML value.
#[derive(Debug, Clone, PartialEq)]
pub enum YamlValue {
    /// Null / missing value.
    Null,
    /// Boolean true/false.
    Bool(bool),
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit floating point.
    Float(f64),
    /// String value.
    Str(String),
    /// Ordered list of values.
    List(Vec<YamlValue>),
    /// Ordered map of key-value pairs (preserves insertion order).
    Map(Vec<(String, YamlValue)>),
}

impl YamlValue {
    /// Look up a key in a map value. Returns `None` if `self` is not a map
    /// or the key is absent.
    pub fn get(&self, key: &str) -> Option<&YamlValue> {
        match self {
            Self::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Try to interpret the value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Try to interpret the value as a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to interpret the value as an integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to interpret the value as a float, coercing integers.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    /// Try to interpret the value as a list.
    pub fn as_list(&self) -> Option<&[YamlValue]> {
        match self {
            Self::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Try to interpret the value as a map.
    pub fn as_map(&self) -> Option<&[(String, YamlValue)]> {
        match self {
            Self::Map(pairs) => Some(pairs.as_slice()),
            _ => None,
        }
    }

    /// Check if the value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

// ============================================================================
// YAML parser — lightweight subset
// ============================================================================

/// Lightweight YAML subset parser.
///
/// Supports:
/// - Mappings (key: value)
/// - Lists (- item)
/// - Strings (plain, single-quoted, double-quoted)
/// - Integers and floats
/// - Booleans (true/false, yes/no, on/off)
/// - Null (null, ~)
/// - Comments (#)
/// - Multi-line strings (literal `|` and folded `>`)
/// - Nested maps and lists via indentation
///
/// Does NOT support:
/// - Anchors/aliases (&, *)
/// - Tags (!!)
/// - Flow collections ({}, [])
/// - Complex mapping keys
pub struct YamlParser;

/// A single logical line from the YAML source.
#[derive(Debug, Clone)]
struct YamlLine {
    /// 1-based line number in the source.
    number: usize,
    /// Number of leading spaces (indentation level).
    indent: usize,
    /// The content after stripping leading whitespace and trailing
    /// whitespace/comments.
    content: String,
}

impl YamlParser {
    /// Parse a YAML document from a string.
    pub fn parse(input: &str) -> Result<YamlValue, ParseError> {
        let lines = Self::tokenize(input);
        if lines.is_empty() {
            return Ok(YamlValue::Null);
        }
        let mut pos = 0;
        Self::parse_value(&lines, &mut pos, 0)
    }

    // -- tokenizer ----------------------------------------------------------

    /// Split the input into logical lines, stripping blanks and pure-comment
    /// lines.  Handles inline comments but preserves quoted content.
    fn tokenize(input: &str) -> Vec<YamlLine> {
        let mut out = Vec::new();
        for (idx, raw) in input.lines().enumerate() {
            let line_no = idx.wrapping_add(1);
            // Count leading spaces (tabs are not standard YAML but tolerate).
            let indent = raw.len().saturating_sub(raw.trim_start_matches(' ').len());
            let trimmed = raw.trim();
            // Skip blank lines and pure comment lines.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Strip inline comment (not inside quotes).
            let content = Self::strip_inline_comment(trimmed);
            out.push(YamlLine {
                number: line_no,
                indent,
                content,
            });
        }
        out
    }

    /// Remove inline `#` comments that are not inside quotes.
    fn strip_inline_comment(s: &str) -> String {
        let mut in_single = false;
        let mut in_double = false;
        let mut prev_space = false;
        let bytes = s.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'\'' if !in_double => in_single = !in_single,
                b'"' if !in_single => {
                    // Check for escaped quote.
                    if i == 0 || bytes[i.wrapping_sub(1)] != b'\\' {
                        in_double = !in_double;
                    }
                }
                b'#' if !in_single && !in_double && prev_space => {
                    return s[..i].trim_end().to_string();
                }
                _ => {}
            }
            prev_space = b == b' ';
        }
        s.to_string()
    }

    // -- recursive descent ---------------------------------------------------

    /// Parse a value at the given indentation level.  `pos` is advanced past
    /// consumed lines.
    fn parse_value(
        lines: &[YamlLine],
        pos: &mut usize,
        min_indent: usize,
    ) -> Result<YamlValue, ParseError> {
        if *pos >= lines.len() {
            return Ok(YamlValue::Null);
        }
        let line = &lines[*pos];
        if line.indent < min_indent {
            return Ok(YamlValue::Null);
        }

        // List?
        if line.content.starts_with("- ") || line.content == "-" {
            return Self::parse_list(lines, pos, line.indent);
        }

        // Map?
        if Self::looks_like_mapping(&line.content) {
            return Self::parse_map(lines, pos, line.indent);
        }

        // Scalar.
        *pos = pos.wrapping_add(1);
        Ok(Self::parse_scalar(&line.content, line.number)?)
    }

    /// Detect whether a line looks like `key: value` (not a scalar that
    /// happens to contain a colon).
    fn looks_like_mapping(content: &str) -> bool {
        // Find the first unquoted colon.
        let mut in_single = false;
        let mut in_double = false;
        let bytes = content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'\'' if !in_double => in_single = !in_single,
                b'"' if !in_single => {
                    if i == 0 || bytes[i.wrapping_sub(1)] != b'\\' {
                        in_double = !in_double;
                    }
                }
                b':' if !in_single && !in_double => {
                    // Must be followed by space, EOL, or be at the end.
                    let after = i.wrapping_add(1);
                    if after >= bytes.len() || bytes[after] == b' ' {
                        // Key must not start with `- ` (that's a list item
                        // containing a colon).
                        return !content.starts_with("- ");
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Parse a YAML mapping (sequence of `key: value` pairs at the same
    /// indentation).
    fn parse_map(
        lines: &[YamlLine],
        pos: &mut usize,
        base_indent: usize,
    ) -> Result<YamlValue, ParseError> {
        let mut pairs: Vec<(String, YamlValue)> = Vec::new();

        while *pos < lines.len() {
            let line = &lines[*pos];
            if line.indent < base_indent {
                break;
            }
            if line.indent != base_indent {
                // Nested content handled by recursive calls — skip if
                // indentation is deeper (shouldn't happen at this level).
                break;
            }

            // Must be `key: ...`.
            let (key, rest) = Self::split_mapping_key(&line.content, line.number)?;

            *pos = pos.wrapping_add(1);

            if rest == "|" || rest == ">" || rest == "|+" || rest == ">+"
                || rest == "|-" || rest == ">-"
            {
                // Multi-line string block.
                let folded = rest.starts_with('>');
                let chomp = if rest.ends_with('-') {
                    Chomp::Strip
                } else if rest.ends_with('+') {
                    Chomp::Keep
                } else {
                    Chomp::Clip
                };
                let val = Self::parse_block_scalar(lines, pos, base_indent, folded, chomp);
                pairs.push((key, YamlValue::Str(val)));
            } else if rest.is_empty() {
                // Value is on subsequent indented lines (nested map or list).
                if *pos < lines.len() && lines[*pos].indent > base_indent {
                    let child_indent = lines[*pos].indent;
                    let val = Self::parse_value(lines, pos, child_indent)?;
                    pairs.push((key, val));
                } else {
                    pairs.push((key, YamlValue::Null));
                }
            } else {
                // Inline scalar value.
                let val = Self::parse_scalar(&rest, line.number)?;
                pairs.push((key, val));
            }
        }

        Ok(YamlValue::Map(pairs))
    }

    /// Parse a YAML sequence (lines starting with `- ` at the same
    /// indentation).
    fn parse_list(
        lines: &[YamlLine],
        pos: &mut usize,
        base_indent: usize,
    ) -> Result<YamlValue, ParseError> {
        let mut items: Vec<YamlValue> = Vec::new();

        while *pos < lines.len() {
            let line = &lines[*pos];
            if line.indent < base_indent {
                break;
            }
            if line.indent != base_indent {
                break;
            }
            if !line.content.starts_with("- ") && line.content != "-" {
                break;
            }

            let item_text = if line.content == "-" {
                String::new()
            } else {
                line.content[2..].to_string()
            };

            *pos = pos.wrapping_add(1);

            if item_text.is_empty() {
                // Nested block under the dash.
                if *pos < lines.len() && lines[*pos].indent > base_indent {
                    let child_indent = lines[*pos].indent;
                    let val = Self::parse_value(lines, pos, child_indent)?;
                    items.push(val);
                } else {
                    items.push(YamlValue::Null);
                }
            } else if Self::looks_like_mapping(&item_text) {
                // Inline map entry after dash: `- key: value`
                // This starts a nested map whose first key-value is on this
                // line and subsequent indented lines continue it.
                let (key, rest) = Self::split_mapping_key(&item_text, line.number)?;
                let mut map_pairs: Vec<(String, YamlValue)> = Vec::new();

                if rest.is_empty() {
                    // Value is nested below.
                    if *pos < lines.len() && lines[*pos].indent > base_indent {
                        let child_indent = lines[*pos].indent;
                        let val = Self::parse_value(lines, pos, child_indent)?;
                        map_pairs.push((key, val));
                    } else {
                        map_pairs.push((key, YamlValue::Null));
                    }
                } else {
                    let val = Self::parse_scalar(&rest, line.number)?;
                    map_pairs.push((key, val));
                }

                // Continue reading more key-value pairs at the nested indent.
                // The continuation indent for `- key: val` entries is
                // base_indent + 2.
                let continuation_indent = base_indent.wrapping_add(2);
                while *pos < lines.len()
                    && lines[*pos].indent == continuation_indent
                    && Self::looks_like_mapping(&lines[*pos].content)
                {
                    let nested_map = Self::parse_map(lines, pos, continuation_indent)?;
                    if let YamlValue::Map(extra_pairs) = nested_map {
                        map_pairs.extend(extra_pairs);
                    }
                }

                items.push(YamlValue::Map(map_pairs));
            } else {
                // Plain scalar list item.
                let val = Self::parse_scalar(&item_text, line.number)?;
                items.push(val);
            }
        }

        Ok(YamlValue::List(items))
    }

    /// Chomping mode for block scalars.
    fn parse_block_scalar(
        lines: &[YamlLine],
        pos: &mut usize,
        parent_indent: usize,
        folded: bool,
        chomp: Chomp,
    ) -> String {
        let mut collected: Vec<String> = Vec::new();
        let block_indent = if *pos < lines.len() {
            lines[*pos].indent
        } else {
            parent_indent.wrapping_add(2)
        };

        // Block content must be indented beyond the parent.
        if block_indent <= parent_indent {
            return String::new();
        }

        while *pos < lines.len() && lines[*pos].indent >= block_indent {
            // Preserve relative indentation beyond block_indent.
            let extra = lines[*pos].indent.saturating_sub(block_indent);
            let mut line_content = String::new();
            for _ in 0..extra {
                line_content.push(' ');
            }
            line_content.push_str(&lines[*pos].content);
            collected.push(line_content);
            *pos = pos.wrapping_add(1);
        }

        if collected.is_empty() {
            return String::new();
        }

        let mut result = if folded {
            // Folded: replace single newlines with spaces, preserve double
            // newlines.
            let mut out = String::new();
            for (i, line) in collected.iter().enumerate() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    if i > 0
                        && !collected[i.wrapping_sub(1)].is_empty()
                        && !out.ends_with('\n')
                    {
                        out.push(' ');
                    }
                    out.push_str(line);
                }
            }
            out
        } else {
            // Literal: join with newlines.
            collected.join("\n")
        };

        // Apply chomping.
        match chomp {
            Chomp::Clip => {
                // Single trailing newline.
                let trimmed = result.trim_end_matches('\n');
                result = format!("{trimmed}\n");
            }
            Chomp::Strip => {
                // No trailing newlines.
                let trimmed = result.trim_end_matches('\n');
                result = trimmed.to_string();
            }
            Chomp::Keep => {
                // Keep all trailing newlines — already done, just add final.
                if !result.ends_with('\n') {
                    result.push('\n');
                }
            }
        }

        result
    }

    /// Split `"key: value"` into `(key, value_remainder)`.
    fn split_mapping_key(content: &str, line: usize) -> Result<(String, String), ParseError> {
        let mut in_single = false;
        let mut in_double = false;
        let bytes = content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'\'' if !in_double => in_single = !in_single,
                b'"' if !in_single => {
                    if i == 0 || bytes[i.wrapping_sub(1)] != b'\\' {
                        in_double = !in_double;
                    }
                }
                b':' if !in_single && !in_double => {
                    let after = i.wrapping_add(1);
                    if after >= bytes.len() || bytes[after] == b' ' {
                        let key = content[..i].trim().to_string();
                        let val_start = if after < bytes.len() {
                            after.wrapping_add(1)
                        } else {
                            after
                        };
                        let val = if val_start <= content.len() {
                            content[val_start..].trim().to_string()
                        } else {
                            String::new()
                        };
                        return Ok((key, val));
                    }
                }
                _ => {}
            }
        }
        Err(ParseError::ExpectedColon { line })
    }

    /// Parse a scalar string into a `YamlValue`.
    fn parse_scalar(s: &str, _line: usize) -> Result<YamlValue, ParseError> {
        let trimmed = s.trim();

        // Null.
        if trimmed.is_empty() || trimmed == "null" || trimmed == "~" || trimmed == "Null" || trimmed == "NULL" {
            return Ok(YamlValue::Null);
        }

        // Booleans.
        match trimmed {
            "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
                return Ok(YamlValue::Bool(true));
            }
            "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
                return Ok(YamlValue::Bool(false));
            }
            _ => {}
        }

        // Quoted strings.
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            let inner = &trimmed[1..trimmed.len().wrapping_sub(1)];
            return Ok(YamlValue::Str(Self::unescape_string(inner)));
        }

        // Numbers — try integer first, then float.
        if let Ok(n) = trimmed.parse::<i64>() {
            return Ok(YamlValue::Int(n));
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            // Only accept if it looks numeric (not "inf", "nan", etc. unless
            // explicitly those values).
            if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
                return Ok(YamlValue::Float(f));
            }
        }

        // Plain string (unquoted).
        Ok(YamlValue::Str(trimmed.to_string()))
    }

    /// Process escape sequences in a double-quoted string.
    fn unescape_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('\'') => out.push('\''),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// Block scalar chomping indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chomp {
    /// Clip: single trailing newline (default).
    Clip,
    /// Strip (`-`): no trailing newline.
    Strip,
    /// Keep (`+`): preserve all trailing newlines.
    Keep,
}

// ============================================================================
// Partition and disk types
// ============================================================================

/// Partition table scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionScheme {
    /// GUID Partition Table — the only scheme currently supported.
    Gpt,
}

/// Partition size specification.
#[derive(Debug, Clone, PartialEq)]
pub enum PartitionSize {
    /// Fixed size in bytes.
    Fixed(u64),
    /// Percentage of total disk space (1-100).
    Percentage(u8),
    /// Use all remaining space after other partitions are allocated.
    Remaining,
}

/// Filesystem type for a partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    /// ext4 — primary Linux-style filesystem.
    Ext4,
    /// FAT32 — used for EFI System Partition.
    Fat32,
    /// Swap partition.
    Swap,
}

/// Partition flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionFlag {
    /// Mark as bootable.
    Boot,
    /// EFI System Partition.
    Efi,
    /// Swap partition type.
    Swap,
}

/// Configuration for a single partition.
#[derive(Debug, Clone, PartialEq)]
pub struct PartitionConfig {
    /// Human-readable label for the partition.
    pub label: String,
    /// How large the partition should be.
    pub size: PartitionSize,
    /// Filesystem to format with.
    pub filesystem: FsType,
    /// Where to mount (None for swap).
    pub mount_point: Option<String>,
    /// Partition flags (boot, efi, swap).
    pub flags: Vec<PartitionFlag>,
}

/// Disk configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskConfig {
    /// Target disk device path (e.g. "/dev/sda").
    pub target: String,
    /// Partition table scheme.
    pub scheme: PartitionScheme,
    /// Partition definitions.
    pub partitions: Vec<PartitionConfig>,
    /// Whether to wipe existing partitions before installing.
    pub wipe: bool,
}

// ============================================================================
// User configuration
// ============================================================================

/// Configuration for a user account.
#[derive(Debug, Clone, PartialEq)]
pub struct UserConfig {
    /// Login name.
    pub username: String,
    /// Optional display/full name.
    pub display_name: Option<String>,
    /// Pre-hashed password (e.g. SHA-512 crypt format).
    pub password_hash: Option<String>,
    /// Group memberships.
    pub groups: Vec<String>,
    /// Whether the user has administrator privileges.
    pub admin: bool,
    /// Whether this user auto-logs in at boot.
    pub auto_login: bool,
    /// Login shell path.
    pub shell: Option<String>,
}

// ============================================================================
// Network configuration
// ============================================================================

/// Wi-Fi security type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    /// No encryption.
    Open,
    /// WPA2-PSK.
    Wpa2,
    /// WPA3-SAE.
    Wpa3,
}

/// Wi-Fi connection details.
#[derive(Debug, Clone, PartialEq)]
pub struct WifiConfig {
    /// Network SSID.
    pub ssid: String,
    /// Pre-shared key / password.
    pub password: String,
    /// Security protocol.
    pub security: WifiSecurity,
}

/// Static network configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticNetConfig {
    /// IP address with prefix length (e.g. "192.168.1.100/24").
    pub address: String,
    /// Default gateway.
    pub gateway: String,
}

/// Network mode — DHCP or static.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkMode {
    /// Automatic configuration via DHCP.
    Dhcp,
    /// Manual static IP configuration.
    Static(StaticNetConfig),
}

/// Network configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfig {
    /// IP address assignment mode.
    pub mode: NetworkMode,
    /// DNS server addresses.
    pub dns: Vec<String>,
    /// Optional Wi-Fi configuration.
    pub wifi: Option<WifiConfig>,
}

// ============================================================================
// Top-level install configuration
// ============================================================================

/// Complete installation configuration parsed from YAML.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallConfig {
    /// System hostname.
    pub hostname: String,
    /// System locale (e.g. "en_US.UTF-8").
    pub locale: String,
    /// Timezone (e.g. "America/New_York").
    pub timezone: String,
    /// Keyboard layout (e.g. "us").
    pub keyboard_layout: String,
    /// Disk/partition configuration.
    pub disk: DiskConfig,
    /// User accounts to create.
    pub users: Vec<UserConfig>,
    /// Additional packages to install beyond the base system.
    pub packages: Vec<String>,
    /// Network configuration.
    pub network: NetworkConfig,
    /// System services to enable at boot.
    pub services: Vec<String>,
    /// Commands to execute after installation completes.
    pub post_install: Vec<String>,
    /// Whether to automatically reboot after installation.
    pub auto_reboot: bool,
}

impl InstallConfig {
    /// Parse an `InstallConfig` from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let root = YamlParser::parse(yaml)?;
        Self::from_yaml_value(&root)
    }

    /// Build an `InstallConfig` from a parsed `YamlValue` tree.
    fn from_yaml_value(root: &YamlValue) -> Result<Self, ConfigError> {
        let hostname = Self::require_str(root, "hostname")?;
        let locale = Self::opt_str(root, "locale").unwrap_or_else(|| "en_US.UTF-8".to_string());
        let timezone =
            Self::opt_str(root, "timezone").unwrap_or_else(|| "UTC".to_string());
        let keyboard_layout =
            Self::opt_str(root, "keyboard_layout").unwrap_or_else(|| "us".to_string());
        let auto_reboot = root
            .get("auto_reboot")
            .and_then(YamlValue::as_bool)
            .unwrap_or(false);

        let disk = Self::parse_disk(root)?;
        let users = Self::parse_users(root)?;
        let packages = Self::parse_string_list(root, "packages");
        let network = Self::parse_network(root)?;
        let services = Self::parse_string_list(root, "services");
        let post_install = Self::parse_string_list(root, "post_install");

        Ok(Self {
            hostname,
            locale,
            timezone,
            keyboard_layout,
            disk,
            users,
            packages,
            network,
            services,
            post_install,
            auto_reboot,
        })
    }

    /// Validate the configuration, returning a list of all errors found.
    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        // Must have at least one user.
        if self.users.is_empty() {
            errors.push(ConfigError::MissingField("users".to_string()));
        }

        // Every user must have a password hash.
        for user in &self.users {
            if user.password_hash.is_none() {
                errors.push(ConfigError::InvalidValue {
                    field: format!("users.{}.password_hash", user.username),
                    message: "password hash is required (no empty passwords)".to_string(),
                });
            }
        }

        // Must have a root partition mounted at "/".
        let has_root = self
            .disk
            .partitions
            .iter()
            .any(|p| p.mount_point.as_deref() == Some("/"));
        if !has_root {
            errors.push(ConfigError::MissingField(
                "partition with mount_point \"/\"".to_string(),
            ));
        }

        // GPT requires an EFI partition.
        if self.disk.scheme == PartitionScheme::Gpt {
            let has_efi = self
                .disk
                .partitions
                .iter()
                .any(|p| p.flags.contains(&PartitionFlag::Efi));
            if !has_efi {
                errors.push(ConfigError::MissingField(
                    "EFI partition (required for GPT)".to_string(),
                ));
            }
        }

        // Validate timezone format — must contain a slash (Area/Location).
        if self.timezone != "UTC"
            && !self.timezone.contains('/')
        {
            errors.push(ConfigError::InvalidValue {
                field: "timezone".to_string(),
                message: format!(
                    "expected Area/Location format (e.g. 'America/New_York'), got '{}'",
                    self.timezone
                ),
            });
        }

        // Validate locale format — must match xx_XX.ENCODING or similar.
        if !self.locale.contains('_') || !self.locale.contains('.') {
            errors.push(ConfigError::InvalidValue {
                field: "locale".to_string(),
                message: format!(
                    "expected format like 'en_US.UTF-8', got '{}'",
                    self.locale
                ),
            });
        }

        // Partition percentages must not exceed 100.
        let total_pct: u16 = self
            .disk
            .partitions
            .iter()
            .filter_map(|p| {
                if let PartitionSize::Percentage(pct) = &p.size {
                    Some(u16::from(*pct))
                } else {
                    None
                }
            })
            .sum();
        if total_pct > 100 {
            errors.push(ConfigError::InvalidValue {
                field: "disk.partitions".to_string(),
                message: format!("total partition percentages exceed 100% ({total_pct}%)"),
            });
        }

        // At most one partition can use Remaining size.
        let remaining_count = self
            .disk
            .partitions
            .iter()
            .filter(|p| p.size == PartitionSize::Remaining)
            .count();
        if remaining_count > 1 {
            errors.push(ConfigError::InvalidValue {
                field: "disk.partitions".to_string(),
                message: "only one partition may use 'remaining' size".to_string(),
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    // -- parsing helpers -----------------------------------------------------

    fn require_str(root: &YamlValue, key: &str) -> Result<String, ConfigError> {
        root.get(key)
            .and_then(YamlValue::as_str)
            .map(String::from)
            .ok_or_else(|| ConfigError::MissingField(key.to_string()))
    }

    fn opt_str(root: &YamlValue, key: &str) -> Option<String> {
        root.get(key)
            .and_then(YamlValue::as_str)
            .map(String::from)
    }

    fn parse_string_list(root: &YamlValue, key: &str) -> Vec<String> {
        root.get(key)
            .and_then(YamlValue::as_list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(YamlValue::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_disk(root: &YamlValue) -> Result<DiskConfig, ConfigError> {
        let disk = root
            .get("disk")
            .ok_or_else(|| ConfigError::MissingField("disk".to_string()))?;

        let target = disk
            .get("target")
            .and_then(YamlValue::as_str)
            .ok_or_else(|| ConfigError::MissingField("disk.target".to_string()))?
            .to_string();

        let scheme_str = disk
            .get("scheme")
            .and_then(YamlValue::as_str)
            .unwrap_or("gpt");
        let scheme = match scheme_str.to_ascii_lowercase().as_str() {
            "gpt" => PartitionScheme::Gpt,
            other => {
                return Err(ConfigError::InvalidValue {
                    field: "disk.scheme".to_string(),
                    message: format!("unsupported partition scheme '{other}', only 'gpt' is supported"),
                });
            }
        };

        let wipe = disk
            .get("wipe")
            .and_then(YamlValue::as_bool)
            .unwrap_or(false);

        let partitions = Self::parse_partitions(disk)?;

        Ok(DiskConfig {
            target,
            scheme,
            partitions,
            wipe,
        })
    }

    fn parse_partitions(disk: &YamlValue) -> Result<Vec<PartitionConfig>, ConfigError> {
        let parts_list = disk
            .get("partitions")
            .and_then(YamlValue::as_list)
            .ok_or_else(|| ConfigError::MissingField("disk.partitions".to_string()))?;

        let mut out = Vec::new();
        for (idx, entry) in parts_list.iter().enumerate() {
            let label = entry
                .get("label")
                .and_then(YamlValue::as_str)
                .ok_or_else(|| {
                    ConfigError::MissingField(format!("disk.partitions[{idx}].label"))
                })?
                .to_string();

            let size = Self::parse_partition_size(entry, idx)?;
            let filesystem = Self::parse_fs_type(entry, idx)?;
            let mount_point = entry
                .get("mount_point")
                .and_then(YamlValue::as_str)
                .map(String::from);
            let flags = Self::parse_partition_flags(entry);

            out.push(PartitionConfig {
                label,
                size,
                filesystem,
                mount_point,
                flags,
            });
        }

        Ok(out)
    }

    fn parse_partition_size(
        entry: &YamlValue,
        idx: usize,
    ) -> Result<PartitionSize, ConfigError> {
        let val = entry.get("size").ok_or_else(|| {
            ConfigError::MissingField(format!("disk.partitions[{idx}].size"))
        })?;

        match val {
            YamlValue::Str(s) => {
                let s_lower = s.to_ascii_lowercase();
                if s_lower == "remaining" || s_lower == "rest" {
                    return Ok(PartitionSize::Remaining);
                }
                // "50%" format.
                if let Some(stripped) = s_lower.strip_suffix('%') {
                    let pct = stripped.trim().parse::<u8>().map_err(|_| {
                        ConfigError::InvalidValue {
                            field: format!("disk.partitions[{idx}].size"),
                            message: format!("invalid percentage '{s}'"),
                        }
                    })?;
                    return Ok(PartitionSize::Percentage(pct));
                }
                // "512M", "1G", "2T" etc.
                Self::parse_size_with_unit(s, idx)
            }
            YamlValue::Int(n) => Ok(PartitionSize::Fixed(
                u64::try_from(*n).map_err(|_| ConfigError::InvalidValue {
                    field: format!("disk.partitions[{idx}].size"),
                    message: "size must be positive".to_string(),
                })?,
            )),
            _ => Err(ConfigError::TypeError {
                field: format!("disk.partitions[{idx}].size"),
                expected: "string or integer".to_string(),
            }),
        }
    }

    fn parse_size_with_unit(s: &str, idx: usize) -> Result<PartitionSize, ConfigError> {
        let s_trimmed = s.trim();
        let (num_part, unit) = if s_trimmed
            .as_bytes()
            .last()
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            let split = s_trimmed.len().wrapping_sub(1);
            // Handle two-char units like "MB", "GB", "TB".
            if split > 0
                && s_trimmed
                    .as_bytes()
                    .get(split.wrapping_sub(1))
                    .is_some_and(|b| b.is_ascii_alphabetic())
            {
                (&s_trimmed[..split.wrapping_sub(1)], &s_trimmed[split.wrapping_sub(1)..])
            } else {
                (&s_trimmed[..split], &s_trimmed[split..])
            }
        } else {
            (s_trimmed, "")
        };

        let num: u64 = num_part.trim().parse().map_err(|_| ConfigError::InvalidValue {
            field: format!("disk.partitions[{idx}].size"),
            message: format!("invalid size '{s}'"),
        })?;

        let multiplier: u64 = match unit.to_ascii_uppercase().as_str() {
            "" | "B" => 1,
            "K" | "KB" | "KIB" => 1024,
            "M" | "MB" | "MIB" => 1024 * 1024,
            "G" | "GB" | "GIB" => 1024 * 1024 * 1024,
            "T" | "TB" | "TIB" => 1024 * 1024 * 1024 * 1024,
            other => {
                return Err(ConfigError::InvalidValue {
                    field: format!("disk.partitions[{idx}].size"),
                    message: format!("unknown size unit '{other}'"),
                });
            }
        };

        let total = num.checked_mul(multiplier).ok_or_else(|| ConfigError::InvalidValue {
            field: format!("disk.partitions[{idx}].size"),
            message: "size overflow".to_string(),
        })?;

        Ok(PartitionSize::Fixed(total))
    }

    fn parse_fs_type(entry: &YamlValue, idx: usize) -> Result<FsType, ConfigError> {
        let val = entry
            .get("filesystem")
            .and_then(YamlValue::as_str)
            .ok_or_else(|| {
                ConfigError::MissingField(format!("disk.partitions[{idx}].filesystem"))
            })?;

        match val.to_ascii_lowercase().as_str() {
            "ext4" => Ok(FsType::Ext4),
            "fat32" | "vfat" => Ok(FsType::Fat32),
            "swap" => Ok(FsType::Swap),
            other => Err(ConfigError::InvalidValue {
                field: format!("disk.partitions[{idx}].filesystem"),
                message: format!("unsupported filesystem '{other}'"),
            }),
        }
    }

    fn parse_partition_flags(entry: &YamlValue) -> Vec<PartitionFlag> {
        entry
            .get("flags")
            .and_then(YamlValue::as_list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| {
                        v.as_str().and_then(|s| match s.to_ascii_lowercase().as_str() {
                            "boot" => Some(PartitionFlag::Boot),
                            "efi" | "esp" => Some(PartitionFlag::Efi),
                            "swap" => Some(PartitionFlag::Swap),
                            _ => None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_users(root: &YamlValue) -> Result<Vec<UserConfig>, ConfigError> {
        let users_list = root
            .get("users")
            .and_then(YamlValue::as_list)
            .ok_or_else(|| ConfigError::MissingField("users".to_string()))?;

        let mut out = Vec::new();
        for (idx, entry) in users_list.iter().enumerate() {
            let username = entry
                .get("username")
                .and_then(YamlValue::as_str)
                .ok_or_else(|| {
                    ConfigError::MissingField(format!("users[{idx}].username"))
                })?
                .to_string();

            let display_name = entry
                .get("display_name")
                .and_then(YamlValue::as_str)
                .map(String::from);

            let password_hash = entry
                .get("password_hash")
                .and_then(YamlValue::as_str)
                .map(String::from);

            let groups = entry
                .get("groups")
                .and_then(YamlValue::as_list)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(YamlValue::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            let admin = entry
                .get("admin")
                .and_then(YamlValue::as_bool)
                .unwrap_or(false);

            let auto_login = entry
                .get("auto_login")
                .and_then(YamlValue::as_bool)
                .unwrap_or(false);

            let shell = entry
                .get("shell")
                .and_then(YamlValue::as_str)
                .map(String::from);

            out.push(UserConfig {
                username,
                display_name,
                password_hash,
                groups,
                admin,
                auto_login,
                shell,
            });
        }

        Ok(out)
    }

    fn parse_network(root: &YamlValue) -> Result<NetworkConfig, ConfigError> {
        let net = match root.get("network") {
            Some(n) => n,
            None => {
                // Default: DHCP with no DNS/Wi-Fi.
                return Ok(NetworkConfig {
                    mode: NetworkMode::Dhcp,
                    dns: Vec::new(),
                    wifi: None,
                });
            }
        };

        let mode_str = net
            .get("mode")
            .and_then(YamlValue::as_str)
            .unwrap_or("dhcp");

        let mode = match mode_str.to_ascii_lowercase().as_str() {
            "dhcp" => NetworkMode::Dhcp,
            "static" => {
                let address = net
                    .get("address")
                    .and_then(YamlValue::as_str)
                    .ok_or_else(|| {
                        ConfigError::MissingField("network.address (required for static mode)".to_string())
                    })?
                    .to_string();
                let gateway = net
                    .get("gateway")
                    .and_then(YamlValue::as_str)
                    .ok_or_else(|| {
                        ConfigError::MissingField("network.gateway (required for static mode)".to_string())
                    })?
                    .to_string();
                NetworkMode::Static(StaticNetConfig { address, gateway })
            }
            other => {
                return Err(ConfigError::InvalidValue {
                    field: "network.mode".to_string(),
                    message: format!("unsupported network mode '{other}'"),
                });
            }
        };

        let dns = net
            .get("dns")
            .and_then(YamlValue::as_list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(YamlValue::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let wifi = Self::parse_wifi(net)?;

        Ok(NetworkConfig { mode, dns, wifi })
    }

    fn parse_wifi(net: &YamlValue) -> Result<Option<WifiConfig>, ConfigError> {
        let wifi = match net.get("wifi") {
            Some(w) => w,
            None => return Ok(None),
        };

        let ssid = wifi
            .get("ssid")
            .and_then(YamlValue::as_str)
            .ok_or_else(|| ConfigError::MissingField("network.wifi.ssid".to_string()))?
            .to_string();

        let password = wifi
            .get("password")
            .and_then(YamlValue::as_str)
            .ok_or_else(|| ConfigError::MissingField("network.wifi.password".to_string()))?
            .to_string();

        let security_str = wifi
            .get("security")
            .and_then(YamlValue::as_str)
            .unwrap_or("wpa2");

        let security = match security_str.to_ascii_lowercase().as_str() {
            "open" | "none" => WifiSecurity::Open,
            "wpa2" | "wpa2-psk" => WifiSecurity::Wpa2,
            "wpa3" | "wpa3-sae" => WifiSecurity::Wpa3,
            other => {
                return Err(ConfigError::InvalidValue {
                    field: "network.wifi.security".to_string(),
                    message: format!("unsupported Wi-Fi security type '{other}'"),
                });
            }
        };

        Ok(Some(WifiConfig {
            ssid,
            password,
            security,
        }))
    }
}

// ============================================================================
// Install plan
// ============================================================================

/// A single step in the installation process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStep {
    /// Wipe existing partition table.
    WipeDisk { target: String },
    /// Create a new partition.
    CreatePartition { label: String, size_desc: String },
    /// Format a partition with a filesystem.
    FormatPartition { label: String, fs: String },
    /// Mount a partition at a path.
    MountPartition { label: String, mount_point: String },
    /// Copy the base OurOS system files.
    CopyBaseSystem,
    /// Install additional packages from the package manager.
    InstallPackages { packages: Vec<String> },
    /// Create a user account.
    CreateUser { username: String },
    /// Configure network settings.
    ConfigureNetwork { mode: String },
    /// Set the system hostname.
    SetHostname { hostname: String },
    /// Set the system timezone.
    SetTimezone { timezone: String },
    /// Set the system locale.
    SetLocale { locale: String },
    /// Enable system services.
    EnableServices { services: Vec<String> },
    /// Run post-installation commands.
    RunPostInstall { commands: Vec<String> },
    /// Install the bootloader.
    InstallBootloader { target: String },
    /// Unmount all partitions.
    Unmount,
    /// Reboot the system.
    Reboot,
}

/// An ordered plan of installation steps generated from an `InstallConfig`.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    /// The ordered sequence of steps to execute.
    pub steps: Vec<InstallStep>,
}

impl InstallPlan {
    /// Generate an install plan from a validated configuration.
    ///
    /// Steps are ordered for correct execution:
    /// 1. Disk operations (wipe, partition, format, mount)
    /// 2. System copy
    /// 3. Configuration (hostname, timezone, locale, network)
    /// 4. User creation
    /// 5. Package installation
    /// 6. Service enablement
    /// 7. Post-install commands
    /// 8. Bootloader
    /// 9. Cleanup (unmount, optional reboot)
    pub fn from_config(config: &InstallConfig) -> Self {
        let mut steps = Vec::new();

        // Phase 1: Disk preparation.
        if config.disk.wipe {
            steps.push(InstallStep::WipeDisk {
                target: config.disk.target.clone(),
            });
        }

        // Create and format partitions, then mount them.
        // First pass: create all partitions.
        for part in &config.disk.partitions {
            let size_desc = match &part.size {
                PartitionSize::Fixed(bytes) => format_bytes(*bytes),
                PartitionSize::Percentage(pct) => format!("{pct}%"),
                PartitionSize::Remaining => "remaining".to_string(),
            };
            steps.push(InstallStep::CreatePartition {
                label: part.label.clone(),
                size_desc,
            });
        }

        // Second pass: format.
        for part in &config.disk.partitions {
            let fs = match part.filesystem {
                FsType::Ext4 => "ext4",
                FsType::Fat32 => "fat32",
                FsType::Swap => "swap",
            };
            steps.push(InstallStep::FormatPartition {
                label: part.label.clone(),
                fs: fs.to_string(),
            });
        }

        // Third pass: mount (root first, then others in path order).
        let mut mount_parts: Vec<&PartitionConfig> = config
            .disk
            .partitions
            .iter()
            .filter(|p| p.mount_point.is_some())
            .collect();
        // Sort so "/" comes first, then by path depth.
        mount_parts.sort_by(|a, b| {
            let ma = a.mount_point.as_deref().unwrap_or("");
            let mb = b.mount_point.as_deref().unwrap_or("");
            ma.cmp(mb)
        });
        for part in mount_parts {
            if let Some(mp) = &part.mount_point {
                steps.push(InstallStep::MountPartition {
                    label: part.label.clone(),
                    mount_point: mp.clone(),
                });
            }
        }

        // Phase 2: Copy base system.
        steps.push(InstallStep::CopyBaseSystem);

        // Phase 3: System configuration.
        steps.push(InstallStep::SetHostname {
            hostname: config.hostname.clone(),
        });
        steps.push(InstallStep::SetTimezone {
            timezone: config.timezone.clone(),
        });
        steps.push(InstallStep::SetLocale {
            locale: config.locale.clone(),
        });
        steps.push(InstallStep::ConfigureNetwork {
            mode: match &config.network.mode {
                NetworkMode::Dhcp => "dhcp".to_string(),
                NetworkMode::Static(s) => format!("static ({})", s.address),
            },
        });

        // Phase 4: User creation.
        for user in &config.users {
            steps.push(InstallStep::CreateUser {
                username: user.username.clone(),
            });
        }

        // Phase 5: Package installation.
        if !config.packages.is_empty() {
            steps.push(InstallStep::InstallPackages {
                packages: config.packages.clone(),
            });
        }

        // Phase 6: Service enablement.
        if !config.services.is_empty() {
            steps.push(InstallStep::EnableServices {
                services: config.services.clone(),
            });
        }

        // Phase 7: Post-install commands.
        if !config.post_install.is_empty() {
            steps.push(InstallStep::RunPostInstall {
                commands: config.post_install.clone(),
            });
        }

        // Phase 8: Bootloader.
        steps.push(InstallStep::InstallBootloader {
            target: config.disk.target.clone(),
        });

        // Phase 9: Cleanup.
        steps.push(InstallStep::Unmount);

        if config.auto_reboot {
            steps.push(InstallStep::Reboot);
        }

        Self { steps }
    }

    /// Generate a human-readable description of the plan.
    pub fn describe(&self) -> String {
        let mut out = String::from("Installation Plan\n");
        out.push_str(&"=".repeat(60));
        out.push('\n');

        for (i, step) in self.steps.iter().enumerate() {
            let num = i.wrapping_add(1);
            let desc = match step {
                InstallStep::WipeDisk { target } => {
                    format!("Wipe disk {target}")
                }
                InstallStep::CreatePartition { label, size_desc } => {
                    format!("Create partition '{label}' ({size_desc})")
                }
                InstallStep::FormatPartition { label, fs } => {
                    format!("Format partition '{label}' as {fs}")
                }
                InstallStep::MountPartition { label, mount_point } => {
                    format!("Mount partition '{label}' at {mount_point}")
                }
                InstallStep::CopyBaseSystem => "Copy base system files".to_string(),
                InstallStep::InstallPackages { packages } => {
                    format!("Install {} additional package(s)", packages.len())
                }
                InstallStep::CreateUser { username } => {
                    format!("Create user '{username}'")
                }
                InstallStep::ConfigureNetwork { mode } => {
                    format!("Configure network ({mode})")
                }
                InstallStep::SetHostname { hostname } => {
                    format!("Set hostname to '{hostname}'")
                }
                InstallStep::SetTimezone { timezone } => {
                    format!("Set timezone to '{timezone}'")
                }
                InstallStep::SetLocale { locale } => {
                    format!("Set locale to '{locale}'")
                }
                InstallStep::EnableServices { services } => {
                    format!("Enable {} service(s)", services.len())
                }
                InstallStep::RunPostInstall { commands } => {
                    format!("Run {} post-install command(s)", commands.len())
                }
                InstallStep::InstallBootloader { target } => {
                    format!("Install bootloader to {target}")
                }
                InstallStep::Unmount => "Unmount all partitions".to_string(),
                InstallStep::Reboot => "Reboot system".to_string(),
            };
            out.push_str(&format!("  {num:>3}. {desc}\n"));
        }

        out.push_str(&"=".repeat(60));
        out.push('\n');
        out.push_str(&format!("Total: {} steps\n", self.steps.len()));

        out
    }
}

// ============================================================================
// Install progress
// ============================================================================

/// Tracks the progress of an ongoing installation.
#[derive(Debug, Clone)]
pub struct InstallProgress {
    /// Index of the currently executing step (0-based).
    pub current_step: usize,
    /// Total number of steps.
    pub total_steps: usize,
    /// Name/description of the current step.
    pub step_name: String,
    /// Overall completion percentage (0-100).
    pub percent: u8,
    /// Accumulated log messages.
    pub log: Vec<String>,
}

impl InstallProgress {
    /// Create a new progress tracker for a plan.
    pub fn new(plan: &InstallPlan) -> Self {
        Self {
            current_step: 0,
            total_steps: plan.steps.len(),
            step_name: String::new(),
            percent: 0,
            log: Vec::new(),
        }
    }

    /// Advance to the next step, updating the percentage and step name.
    pub fn advance(&mut self, step_name: &str) {
        self.current_step = self.current_step.wrapping_add(1);
        self.step_name = step_name.to_string();
        if self.total_steps > 0 {
            // Compute percent, clamped to 100.
            let pct = (self.current_step.saturating_mul(100))
                .checked_div(self.total_steps)
                .unwrap_or(0);
            self.percent = if pct > 100 { 100 } else { pct as u8 };
        }
        self.log
            .push(format!("[{}/{}] {step_name}", self.current_step, self.total_steps));
    }

    /// Record a log message.
    pub fn log_message(&mut self, msg: &str) {
        self.log.push(msg.to_string());
    }
}

// ============================================================================
// Sample config generation
// ============================================================================

/// Generate a sample YAML configuration string suitable for use as a starting
/// template.  This can be parsed back via `InstallConfig::from_yaml`.
pub fn generate_sample_config() -> String {
    r#"# OurOS Installer Configuration
# ================================
# Edit this file to customize your installation.

hostname: my-ouros-pc
locale: en_US.UTF-8
timezone: America/New_York
keyboard_layout: us

# Disk configuration
disk:
  target: /dev/sda
  scheme: gpt
  wipe: true
  partitions:
    - label: EFI
      size: 512M
      filesystem: fat32
      mount_point: /boot/efi
      flags:
        - efi
        - boot
    - label: swap
      size: 4G
      filesystem: swap
      flags:
        - swap
    - label: root
      size: remaining
      filesystem: ext4
      mount_point: /

# User accounts
users:
  - username: admin
    display_name: System Administrator
    password_hash: "$6$rounds=10000$salt$hashedpasswordhere"
    groups:
      - wheel
      - audio
      - video
    admin: true
    auto_login: false
    shell: /bin/sh

# Additional packages
packages:
  - firefox
  - vim
  - git

# Network configuration
network:
  mode: dhcp
  dns:
    - 1.1.1.1
    - 8.8.8.8

# Services to enable at boot
services:
  - networking
  - sshd

# Post-install commands
post_install:
  - echo "Installation complete"

auto_reboot: true
"#
    .to_string()
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a byte count into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TIB {
        format!("{} TiB", bytes / TIB)
    } else if bytes >= GIB {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- YAML parser: scalars -----------------------------------------------

    #[test]
    fn yaml_parse_string_plain() {
        let val = YamlParser::parse("name: hello").unwrap();
        assert_eq!(val.get("name").unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn yaml_parse_string_double_quoted() {
        let val = YamlParser::parse("name: \"hello world\"").unwrap();
        assert_eq!(val.get("name").unwrap().as_str(), Some("hello world"));
    }

    #[test]
    fn yaml_parse_string_single_quoted() {
        let val = YamlParser::parse("name: 'hello'").unwrap();
        assert_eq!(val.get("name").unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn yaml_parse_integer() {
        let val = YamlParser::parse("count: 42").unwrap();
        assert_eq!(val.get("count").unwrap().as_int(), Some(42));
    }

    #[test]
    fn yaml_parse_negative_integer() {
        let val = YamlParser::parse("offset: -10").unwrap();
        assert_eq!(val.get("offset").unwrap().as_int(), Some(-10));
    }

    #[test]
    fn yaml_parse_float() {
        let val = YamlParser::parse("ratio: 3.14").unwrap();
        let f = val.get("ratio").unwrap().as_float().unwrap();
        assert!((f - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn yaml_parse_boolean_true_variants() {
        for word in &["true", "True", "TRUE", "yes", "Yes", "on"] {
            let yaml = format!("flag: {word}");
            let val = YamlParser::parse(&yaml).unwrap();
            assert_eq!(
                val.get("flag").unwrap().as_bool(),
                Some(true),
                "failed for '{word}'"
            );
        }
    }

    #[test]
    fn yaml_parse_boolean_false_variants() {
        for word in &["false", "False", "FALSE", "no", "No", "off"] {
            let yaml = format!("flag: {word}");
            let val = YamlParser::parse(&yaml).unwrap();
            assert_eq!(
                val.get("flag").unwrap().as_bool(),
                Some(false),
                "failed for '{word}'"
            );
        }
    }

    #[test]
    fn yaml_parse_null_variants() {
        for word in &["null", "~", "Null", "NULL"] {
            let yaml = format!("val: {word}");
            let val = YamlParser::parse(&yaml).unwrap();
            assert!(val.get("val").unwrap().is_null(), "failed for '{word}'");
        }
    }

    #[test]
    fn yaml_parse_empty_value_is_null() {
        let val = YamlParser::parse("key:").unwrap();
        assert!(val.get("key").unwrap().is_null());
    }

    // -- YAML parser: nested maps and lists ---------------------------------

    #[test]
    fn yaml_parse_nested_map() {
        let yaml = "outer:\n  inner: value\n  count: 5";
        let val = YamlParser::parse(yaml).unwrap();
        let outer = val.get("outer").unwrap();
        assert_eq!(outer.get("inner").unwrap().as_str(), Some("value"));
        assert_eq!(outer.get("count").unwrap().as_int(), Some(5));
    }

    #[test]
    fn yaml_parse_list_of_strings() {
        let yaml = "items:\n  - alpha\n  - beta\n  - gamma";
        let val = YamlParser::parse(yaml).unwrap();
        let items = val.get("items").unwrap().as_list().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_str(), Some("alpha"));
        assert_eq!(items[1].as_str(), Some("beta"));
        assert_eq!(items[2].as_str(), Some("gamma"));
    }

    #[test]
    fn yaml_parse_list_of_maps() {
        let yaml = "users:\n  - name: alice\n    age: 30\n  - name: bob\n    age: 25";
        let val = YamlParser::parse(yaml).unwrap();
        let users = val.get("users").unwrap().as_list().unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].get("name").unwrap().as_str(), Some("alice"));
        assert_eq!(users[0].get("age").unwrap().as_int(), Some(30));
        assert_eq!(users[1].get("name").unwrap().as_str(), Some("bob"));
    }

    #[test]
    fn yaml_parse_deeply_nested() {
        let yaml = "a:\n  b:\n    c: deep";
        let val = YamlParser::parse(yaml).unwrap();
        let c = val
            .get("a")
            .unwrap()
            .get("b")
            .unwrap()
            .get("c")
            .unwrap();
        assert_eq!(c.as_str(), Some("deep"));
    }

    // -- YAML parser: comments ----------------------------------------------

    #[test]
    fn yaml_parse_inline_comment() {
        let yaml = "host: mypc # this is the hostname";
        let val = YamlParser::parse(yaml).unwrap();
        assert_eq!(val.get("host").unwrap().as_str(), Some("mypc"));
    }

    #[test]
    fn yaml_parse_comment_lines_skipped() {
        let yaml = "# comment\nkey: value\n# another comment\nkey2: value2";
        let val = YamlParser::parse(yaml).unwrap();
        assert_eq!(val.get("key").unwrap().as_str(), Some("value"));
        assert_eq!(val.get("key2").unwrap().as_str(), Some("value2"));
    }

    // -- YAML parser: multi-line strings ------------------------------------

    #[test]
    fn yaml_parse_literal_block() {
        let yaml = "desc: |\n  line one\n  line two\n  line three";
        let val = YamlParser::parse(yaml).unwrap();
        let desc = val.get("desc").unwrap().as_str().unwrap();
        assert_eq!(desc, "line one\nline two\nline three\n");
    }

    #[test]
    fn yaml_parse_folded_block() {
        let yaml = "desc: >\n  this is\n  a folded\n  string";
        let val = YamlParser::parse(yaml).unwrap();
        let desc = val.get("desc").unwrap().as_str().unwrap();
        assert_eq!(desc, "this is a folded string\n");
    }

    #[test]
    fn yaml_parse_literal_block_strip() {
        let yaml = "desc: |-\n  line one\n  line two";
        let val = YamlParser::parse(yaml).unwrap();
        let desc = val.get("desc").unwrap().as_str().unwrap();
        assert_eq!(desc, "line one\nline two");
    }

    // -- YAML parser: error cases -------------------------------------------

    #[test]
    fn yaml_parse_empty_input_returns_null() {
        let val = YamlParser::parse("").unwrap();
        assert!(val.is_null());
    }

    #[test]
    fn yaml_parse_only_comments_returns_null() {
        let val = YamlParser::parse("# just a comment\n# another").unwrap();
        assert!(val.is_null());
    }

    // -- Config parsing from YAML -------------------------------------------

    #[test]
    fn config_parse_full_yaml() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        assert_eq!(config.hostname, "my-ouros-pc");
        assert_eq!(config.locale, "en_US.UTF-8");
        assert_eq!(config.timezone, "America/New_York");
        assert_eq!(config.keyboard_layout, "us");
        assert!(config.auto_reboot);
        assert_eq!(config.disk.target, "/dev/sda");
        assert_eq!(config.disk.scheme, PartitionScheme::Gpt);
        assert!(config.disk.wipe);
        assert_eq!(config.disk.partitions.len(), 3);
        assert_eq!(config.users.len(), 1);
        assert_eq!(config.users[0].username, "admin");
        assert_eq!(config.packages.len(), 3);
        assert_eq!(config.services.len(), 2);
    }

    #[test]
    fn config_parse_missing_hostname() {
        let yaml = "locale: en_US.UTF-8\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: test\n    password_hash: hash";
        let result = InstallConfig::from_yaml(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn config_defaults_applied() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\n      mount_point: /\nusers:\n  - username: test\n    password_hash: hash";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.locale, "en_US.UTF-8");
        assert_eq!(config.timezone, "UTC");
        assert_eq!(config.keyboard_layout, "us");
        assert!(!config.auto_reboot);
    }

    // -- Config validation --------------------------------------------------

    #[test]
    fn config_validate_valid() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validate_missing_root_partition() {
        let yaml = r#"hostname: test
locale: en_US.UTF-8
timezone: America/Chicago
disk:
  target: /dev/sda
  scheme: gpt
  partitions:
    - label: EFI
      size: 512M
      filesystem: fat32
      mount_point: /boot/efi
      flags:
        - efi
users:
  - username: admin
    password_hash: "$6$hash"
"#;
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::MissingField(f) if f.contains("/"))
        }));
    }

    #[test]
    fn config_validate_no_efi_partition() {
        let yaml = r#"hostname: test
locale: en_US.UTF-8
timezone: America/Chicago
disk:
  target: /dev/sda
  scheme: gpt
  partitions:
    - label: root
      size: remaining
      filesystem: ext4
      mount_point: /
users:
  - username: admin
    password_hash: "$6$hash"
"#;
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::MissingField(f) if f.contains("EFI"))
        }));
    }

    #[test]
    fn config_validate_no_users() {
        // Build a config manually with empty users to bypass parse requirement.
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "en_US.UTF-8".to_string(),
            timezone: "UTC".to_string(),
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![
                    PartitionConfig {
                        label: "EFI".to_string(),
                        size: PartitionSize::Fixed(512 * 1024 * 1024),
                        filesystem: FsType::Fat32,
                        mount_point: Some("/boot/efi".to_string()),
                        flags: vec![PartitionFlag::Efi],
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: PartitionSize::Remaining,
                        filesystem: FsType::Ext4,
                        mount_point: Some("/".to_string()),
                        flags: vec![],
                    },
                ],
                wipe: true,
            },
            users: vec![],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::MissingField(f) if f == "users")
        }));
    }

    #[test]
    fn config_validate_no_password_hash() {
        let yaml = r#"hostname: test
locale: en_US.UTF-8
timezone: America/Chicago
disk:
  target: /dev/sda
  scheme: gpt
  partitions:
    - label: EFI
      size: 512M
      filesystem: fat32
      mount_point: /boot/efi
      flags:
        - efi
    - label: root
      size: remaining
      filesystem: ext4
      mount_point: /
users:
  - username: admin
"#;
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::InvalidValue { field, .. } if field.contains("password_hash"))
        }));
    }

    #[test]
    fn config_validate_bad_timezone() {
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "en_US.UTF-8".to_string(),
            timezone: "Eastern".to_string(), // bad — no slash
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![
                    PartitionConfig {
                        label: "EFI".to_string(),
                        size: PartitionSize::Fixed(512 * 1024 * 1024),
                        filesystem: FsType::Fat32,
                        mount_point: Some("/boot/efi".to_string()),
                        flags: vec![PartitionFlag::Efi],
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: PartitionSize::Remaining,
                        filesystem: FsType::Ext4,
                        mount_point: Some("/".to_string()),
                        flags: vec![],
                    },
                ],
                wipe: true,
            },
            users: vec![UserConfig {
                username: "admin".to_string(),
                display_name: None,
                password_hash: Some("hash".to_string()),
                groups: vec![],
                admin: false,
                auto_login: false,
                shell: None,
            }],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::InvalidValue { field, .. } if field == "timezone")
        }));
    }

    #[test]
    fn config_validate_bad_locale() {
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "english".to_string(), // bad — no underscore or dot
            timezone: "UTC".to_string(),
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![
                    PartitionConfig {
                        label: "EFI".to_string(),
                        size: PartitionSize::Fixed(512 * 1024 * 1024),
                        filesystem: FsType::Fat32,
                        mount_point: Some("/boot/efi".to_string()),
                        flags: vec![PartitionFlag::Efi],
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: PartitionSize::Remaining,
                        filesystem: FsType::Ext4,
                        mount_point: Some("/".to_string()),
                        flags: vec![],
                    },
                ],
                wipe: true,
            },
            users: vec![UserConfig {
                username: "admin".to_string(),
                display_name: None,
                password_hash: Some("hash".to_string()),
                groups: vec![],
                admin: false,
                auto_login: false,
                shell: None,
            }],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::InvalidValue { field, .. } if field == "locale")
        }));
    }

    #[test]
    fn config_validate_partition_percentages_exceed_100() {
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "en_US.UTF-8".to_string(),
            timezone: "UTC".to_string(),
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![
                    PartitionConfig {
                        label: "EFI".to_string(),
                        size: PartitionSize::Percentage(60),
                        filesystem: FsType::Fat32,
                        mount_point: Some("/boot/efi".to_string()),
                        flags: vec![PartitionFlag::Efi],
                    },
                    PartitionConfig {
                        label: "root".to_string(),
                        size: PartitionSize::Percentage(50),
                        filesystem: FsType::Ext4,
                        mount_point: Some("/".to_string()),
                        flags: vec![],
                    },
                ],
                wipe: true,
            },
            users: vec![UserConfig {
                username: "admin".to_string(),
                display_name: None,
                password_hash: Some("hash".to_string()),
                groups: vec![],
                admin: false,
                auto_login: false,
                shell: None,
            }],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| {
            matches!(e, ConfigError::InvalidValue { field, .. } if field == "disk.partitions")
        }));
    }

    // -- Partition size parsing ---------------------------------------------

    #[test]
    fn partition_size_fixed_megabytes() {
        let yaml = "label: test\nsize: 512M\nfilesystem: ext4";
        let val = YamlParser::parse(yaml).unwrap();
        let size = InstallConfig::parse_partition_size(&val, 0).unwrap();
        assert_eq!(size, PartitionSize::Fixed(512 * 1024 * 1024));
    }

    #[test]
    fn partition_size_fixed_gigabytes() {
        let yaml = "label: test\nsize: 4G\nfilesystem: ext4";
        let val = YamlParser::parse(yaml).unwrap();
        let size = InstallConfig::parse_partition_size(&val, 0).unwrap();
        assert_eq!(size, PartitionSize::Fixed(4 * 1024 * 1024 * 1024));
    }

    #[test]
    fn partition_size_percentage() {
        let yaml = "label: test\nsize: \"50%\"\nfilesystem: ext4";
        let val = YamlParser::parse(yaml).unwrap();
        let size = InstallConfig::parse_partition_size(&val, 0).unwrap();
        assert_eq!(size, PartitionSize::Percentage(50));
    }

    #[test]
    fn partition_size_remaining() {
        let yaml = "label: test\nsize: remaining\nfilesystem: ext4";
        let val = YamlParser::parse(yaml).unwrap();
        let size = InstallConfig::parse_partition_size(&val, 0).unwrap();
        assert_eq!(size, PartitionSize::Remaining);
    }

    // -- Install plan -------------------------------------------------------

    #[test]
    fn plan_from_config_step_ordering() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        let plan = InstallPlan::from_config(&config);

        // Verify step ordering: wipe -> create -> format -> mount -> copy ->
        // config -> user -> packages -> services -> post_install -> bootloader
        // -> unmount -> reboot.
        assert!(plan.steps.len() >= 10, "too few steps: {}", plan.steps.len());

        // First step should be WipeDisk (since wipe: true).
        assert!(matches!(&plan.steps[0], InstallStep::WipeDisk { .. }));

        // Find the CopyBaseSystem step — it must come after all mount steps.
        let copy_idx = plan
            .steps
            .iter()
            .position(|s| matches!(s, InstallStep::CopyBaseSystem))
            .expect("CopyBaseSystem step missing");
        let last_mount_idx = plan
            .steps
            .iter()
            .rposition(|s| matches!(s, InstallStep::MountPartition { .. }))
            .expect("no mount steps");
        assert!(copy_idx > last_mount_idx, "CopyBaseSystem must come after mounts");

        // Bootloader must come after CopyBaseSystem.
        let boot_idx = plan
            .steps
            .iter()
            .position(|s| matches!(s, InstallStep::InstallBootloader { .. }))
            .expect("InstallBootloader step missing");
        assert!(boot_idx > copy_idx, "bootloader must come after base system copy");

        // Unmount must come after bootloader.
        let unmount_idx = plan
            .steps
            .iter()
            .position(|s| matches!(s, InstallStep::Unmount))
            .expect("Unmount step missing");
        assert!(unmount_idx > boot_idx, "unmount must come after bootloader");

        // Reboot must be last (since auto_reboot is true).
        assert!(
            matches!(plan.steps.last(), Some(InstallStep::Reboot)),
            "reboot should be last step"
        );
    }

    #[test]
    fn plan_no_wipe_when_false() {
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "en_US.UTF-8".to_string(),
            timezone: "UTC".to_string(),
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: PartitionSize::Remaining,
                    filesystem: FsType::Ext4,
                    mount_point: Some("/".to_string()),
                    flags: vec![],
                }],
                wipe: false,
            },
            users: vec![],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let plan = InstallPlan::from_config(&config);
        assert!(!plan.steps.iter().any(|s| matches!(s, InstallStep::WipeDisk { .. })));
    }

    #[test]
    fn plan_no_reboot_when_false() {
        let config = InstallConfig {
            hostname: "test".to_string(),
            locale: "en_US.UTF-8".to_string(),
            timezone: "UTC".to_string(),
            keyboard_layout: "us".to_string(),
            disk: DiskConfig {
                target: "/dev/sda".to_string(),
                scheme: PartitionScheme::Gpt,
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: PartitionSize::Remaining,
                    filesystem: FsType::Ext4,
                    mount_point: Some("/".to_string()),
                    flags: vec![],
                }],
                wipe: false,
            },
            users: vec![],
            packages: vec![],
            network: NetworkConfig {
                mode: NetworkMode::Dhcp,
                dns: vec![],
                wifi: None,
            },
            services: vec![],
            post_install: vec![],
            auto_reboot: false,
        };
        let plan = InstallPlan::from_config(&config);
        assert!(!plan.steps.iter().any(|s| matches!(s, InstallStep::Reboot)));
    }

    #[test]
    fn plan_describe_produces_output() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        let plan = InstallPlan::from_config(&config);
        let desc = plan.describe();
        assert!(desc.contains("Installation Plan"));
        assert!(desc.contains("Total:"));
        assert!(desc.contains("Wipe disk"));
        assert!(desc.contains("Copy base system"));
    }

    // -- Sample config round-trip -------------------------------------------

    #[test]
    fn sample_config_round_trip() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        assert!(config.validate().is_ok());
        // Verify key fields survived the round trip.
        assert_eq!(config.hostname, "my-ouros-pc");
        assert_eq!(config.disk.partitions.len(), 3);
        assert_eq!(config.users.len(), 1);
        assert_eq!(config.packages, vec!["firefox", "vim", "git"]);
    }

    // -- Network config variants --------------------------------------------

    #[test]
    fn network_config_dhcp() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: test\n    password_hash: hash\nnetwork:\n  mode: dhcp";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.network.mode, NetworkMode::Dhcp);
    }

    #[test]
    fn network_config_static() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: test\n    password_hash: hash\nnetwork:\n  mode: static\n  address: 192.168.1.100/24\n  gateway: 192.168.1.1";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        match &config.network.mode {
            NetworkMode::Static(s) => {
                assert_eq!(s.address, "192.168.1.100/24");
                assert_eq!(s.gateway, "192.168.1.1");
            }
            _ => panic!("expected static network mode"),
        }
    }

    #[test]
    fn network_config_wifi() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: test\n    password_hash: hash\nnetwork:\n  mode: dhcp\n  wifi:\n    ssid: MyNetwork\n    password: secret123\n    security: wpa2";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let wifi = config.network.wifi.as_ref().unwrap();
        assert_eq!(wifi.ssid, "MyNetwork");
        assert_eq!(wifi.password, "secret123");
        assert_eq!(wifi.security, WifiSecurity::Wpa2);
    }

    #[test]
    fn network_default_when_absent() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: test\n    password_hash: hash";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.network.mode, NetworkMode::Dhcp);
        assert!(config.network.dns.is_empty());
        assert!(config.network.wifi.is_none());
    }

    // -- User config --------------------------------------------------------

    #[test]
    fn user_config_full() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: admin\n    display_name: Admin User\n    password_hash: hash123\n    groups:\n      - wheel\n      - audio\n    admin: true\n    auto_login: true\n    shell: /bin/bash";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let user = &config.users[0];
        assert_eq!(user.username, "admin");
        assert_eq!(user.display_name.as_deref(), Some("Admin User"));
        assert_eq!(user.password_hash.as_deref(), Some("hash123"));
        assert_eq!(user.groups, vec!["wheel", "audio"]);
        assert!(user.admin);
        assert!(user.auto_login);
        assert_eq!(user.shell.as_deref(), Some("/bin/bash"));
    }

    #[test]
    fn user_config_minimal() {
        let yaml = "hostname: test\ndisk:\n  target: /dev/sda\n  partitions:\n    - label: root\n      size: remaining\n      filesystem: ext4\nusers:\n  - username: basic\n    password_hash: hash";
        let config = InstallConfig::from_yaml(yaml).unwrap();
        let user = &config.users[0];
        assert_eq!(user.username, "basic");
        assert!(user.display_name.is_none());
        assert!(user.groups.is_empty());
        assert!(!user.admin);
        assert!(!user.auto_login);
        assert!(user.shell.is_none());
    }

    // -- Progress tracker ---------------------------------------------------

    #[test]
    fn progress_advance() {
        let yaml = generate_sample_config();
        let config = InstallConfig::from_yaml(&yaml).unwrap();
        let plan = InstallPlan::from_config(&config);
        let mut progress = InstallProgress::new(&plan);
        assert_eq!(progress.current_step, 0);
        assert_eq!(progress.percent, 0);

        progress.advance("Wiping disk");
        assert_eq!(progress.current_step, 1);
        assert!(progress.percent > 0);
        assert_eq!(progress.log.len(), 1);

        progress.log_message("Extra info");
        assert_eq!(progress.log.len(), 2);
    }

    // -- Utility: format_bytes ----------------------------------------------

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2 GiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024 * 1024), "3 TiB");
    }
}
