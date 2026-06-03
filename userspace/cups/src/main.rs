//! OurOS CUPS Printing System
//!
//! Multi-personality binary implementing the CUPS printing subsystem.
//! Personality is detected via `argv[0]` basename (stripping path separators
//! and `.exe` suffix).
//!
//! # Personalities
//!
//! ```text
//! cupsd          CUPS daemon (default personality)
//! lp             Submit print jobs
//! lpstat         Show printer and job status
//! lpadmin        Configure printers
//! cancel         Cancel print jobs
//! cupsaccept     Accept jobs on a printer
//! cupsreject     Reject jobs on a printer
//! cupsenable     Enable a printer
//! cupsdisable    Disable a printer
//! lpinfo         Show available devices and drivers
//! lpoptions      Show/set printer options
//! cupstestppd    Test PPD files
//! ```

#![deny(clippy::all)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::module_name_repetitions)]

use std::env;
use std::fmt;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "2.4.0-ouros";
const DEFAULT_PRINTER: &str = "default";
const MAX_JOB_ID: u32 = 999_999;
const DEFAULT_PORT: u16 = 631;
const DEFAULT_COPIES: u32 = 1;
const DEFAULT_PRIORITY: u32 = 50;
const MAX_PRIORITY: u32 = 100;
const DEFAULT_PAGE_RANGE: &str = "all";
const _DEFAULT_MEDIA: &str = "letter";
const _DEFAULT_ORIENTATION: &str = "portrait";
const _DEFAULT_SIDES: &str = "one-sided";
const _DEFAULT_QUALITY: &str = "normal";
const SPOOL_DIR: &str = "/var/spool/cups";
const CONFIG_DIR: &str = "/etc/cups";
const LOG_DIR: &str = "/var/log/cups";
const PPD_DIR: &str = "/etc/cups/ppd";
const _FILTER_DIR: &str = "/usr/lib/cups/filter";
const _BACKEND_DIR: &str = "/usr/lib/cups/backend";
const _MIME_TYPES_FILE: &str = "/etc/cups/mime.types";
const _MIME_CONVS_FILE: &str = "/etc/cups/mime.convs";

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug, Clone)]
struct CupsError {
    message: String,
}

impl CupsError {
    fn new(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
        }
    }
}

impl fmt::Display for CupsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

type CupsResult<T> = Result<T, CupsError>;

// ============================================================================
// Printer state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrinterState {
    Idle,
    _Processing,
    Stopped,
}

impl fmt::Display for PrinterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::_Processing => write!(f, "processing"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

impl PrinterState {
    fn _from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "processing" => Some(Self::_Processing),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }

    fn ipp_code(self) -> u32 {
        match self {
            Self::Idle => 3,
            Self::_Processing => 4,
            Self::Stopped => 5,
        }
    }
}

// ============================================================================
// Job state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
    Pending,
    Held,
    Processing,
    Completed,
    Canceled,
    _Aborted,
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Held => write!(f, "held"),
            Self::Processing => write!(f, "processing"),
            Self::Completed => write!(f, "completed"),
            Self::Canceled => write!(f, "canceled"),
            Self::_Aborted => write!(f, "aborted"),
        }
    }
}

impl JobState {
    fn _from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "held" => Some(Self::Held),
            "processing" => Some(Self::Processing),
            "completed" => Some(Self::Completed),
            "canceled" => Some(Self::Canceled),
            "aborted" => Some(Self::_Aborted),
            _ => None,
        }
    }

    fn _ipp_code(self) -> u32 {
        match self {
            Self::Pending => 3,
            Self::Held => 4,
            Self::Processing => 5,
            Self::Completed => 9,
            Self::Canceled => 7,
            Self::_Aborted => 8,
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Canceled | Self::_Aborted)
    }
}

// ============================================================================
// Orientation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Portrait,
    Landscape,
    ReversePortrait,
    ReverseLandscape,
}

impl fmt::Display for Orientation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Portrait => write!(f, "portrait"),
            Self::Landscape => write!(f, "landscape"),
            Self::ReversePortrait => write!(f, "reverse-portrait"),
            Self::ReverseLandscape => write!(f, "reverse-landscape"),
        }
    }
}

impl Orientation {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "portrait" | "3" => Some(Self::Portrait),
            "landscape" | "4" => Some(Self::Landscape),
            "reverse-portrait" | "5" => Some(Self::ReversePortrait),
            "reverse-landscape" | "6" => Some(Self::ReverseLandscape),
            _ => None,
        }
    }

    fn _ipp_code(self) -> u32 {
        match self {
            Self::Portrait => 3,
            Self::Landscape => 4,
            Self::ReversePortrait => 5,
            Self::ReverseLandscape => 6,
        }
    }
}

// ============================================================================
// Sides (duplex)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sides {
    OneSided,
    TwoSidedLongEdge,
    TwoSidedShortEdge,
}

impl fmt::Display for Sides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OneSided => write!(f, "one-sided"),
            Self::TwoSidedLongEdge => write!(f, "two-sided-long-edge"),
            Self::TwoSidedShortEdge => write!(f, "two-sided-short-edge"),
        }
    }
}

impl Sides {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "one-sided" => Some(Self::OneSided),
            "two-sided-long-edge" | "two-sided" => Some(Self::TwoSidedLongEdge),
            "two-sided-short-edge" => Some(Self::TwoSidedShortEdge),
            _ => None,
        }
    }
}

// ============================================================================
// Print quality
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrintQuality {
    Draft,
    Normal,
    High,
}

impl fmt::Display for PrintQuality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Normal => write!(f, "normal"),
            Self::High => write!(f, "high"),
        }
    }
}

impl PrintQuality {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" | "3" => Some(Self::Draft),
            "normal" | "4" => Some(Self::Normal),
            "high" | "5" | "best" => Some(Self::High),
            _ => None,
        }
    }

    fn _ipp_code(self) -> u32 {
        match self {
            Self::Draft => 3,
            Self::Normal => 4,
            Self::High => 5,
        }
    }
}

// ============================================================================
// Color mode
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Color,
    Monochrome,
    Auto,
}

impl fmt::Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color => write!(f, "color"),
            Self::Monochrome => write!(f, "monochrome"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

impl ColorMode {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "color" => Some(Self::Color),
            "monochrome" | "grayscale" | "mono" => Some(Self::Monochrome),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

// ============================================================================
// Media size
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaSize {
    Letter,
    Legal,
    A4,
    A3,
    A5,
    Tabloid,
    Executive,
    Envelope10,
    EnvelopeDl,
    Custom,
}

impl fmt::Display for MediaSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter => write!(f, "letter"),
            Self::Legal => write!(f, "legal"),
            Self::A4 => write!(f, "a4"),
            Self::A3 => write!(f, "a3"),
            Self::A5 => write!(f, "a5"),
            Self::Tabloid => write!(f, "tabloid"),
            Self::Executive => write!(f, "executive"),
            Self::Envelope10 => write!(f, "env-10"),
            Self::EnvelopeDl => write!(f, "env-dl"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

impl MediaSize {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "letter" | "na_letter_8.5x11in" => Some(Self::Letter),
            "legal" | "na_legal_8.5x14in" => Some(Self::Legal),
            "a4" | "iso_a4_210x297mm" => Some(Self::A4),
            "a3" | "iso_a3_297x420mm" => Some(Self::A3),
            "a5" | "iso_a5_148x210mm" => Some(Self::A5),
            "tabloid" | "na_ledger_11x17in" => Some(Self::Tabloid),
            "executive" => Some(Self::Executive),
            "env-10" | "na_number-10_4.125x9.5in" => Some(Self::Envelope10),
            "env-dl" | "iso_dl_110x220mm" => Some(Self::EnvelopeDl),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    fn _width_pts(self) -> f64 {
        match self {
            Self::Letter => 612.0,
            Self::Legal => 612.0,
            Self::A4 => 595.28,
            Self::A3 => 841.89,
            Self::A5 => 419.53,
            Self::Tabloid => 792.0,
            Self::Executive => 522.0,
            Self::Envelope10 => 297.0,
            Self::EnvelopeDl => 311.81,
            Self::Custom => 612.0,
        }
    }

    fn _height_pts(self) -> f64 {
        match self {
            Self::Letter => 792.0,
            Self::Legal => 1008.0,
            Self::A4 => 841.89,
            Self::A3 => 1190.55,
            Self::A5 => 595.28,
            Self::Tabloid => 1224.0,
            Self::Executive => 756.0,
            Self::Envelope10 => 684.0,
            Self::EnvelopeDl => 623.62,
            Self::Custom => 792.0,
        }
    }
}

// ============================================================================
// Media source (input tray)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaSource {
    Auto,
    Tray1,
    Tray2,
    Manual,
    Envelope,
}

impl fmt::Display for MediaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Tray1 => write!(f, "tray-1"),
            Self::Tray2 => write!(f, "tray-2"),
            Self::Manual => write!(f, "manual"),
            Self::Envelope => write!(f, "envelope"),
        }
    }
}

impl MediaSource {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "tray-1" => Some(Self::Tray1),
            "tray-2" => Some(Self::Tray2),
            "manual" => Some(Self::Manual),
            "envelope" => Some(Self::Envelope),
            _ => None,
        }
    }
}

// ============================================================================
// Number-up layout
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberUp {
    One,
    Two,
    Four,
    Six,
    Nine,
    Sixteen,
}

impl fmt::Display for NumberUp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::One => write!(f, "1"),
            Self::Two => write!(f, "2"),
            Self::Four => write!(f, "4"),
            Self::Six => write!(f, "6"),
            Self::Nine => write!(f, "9"),
            Self::Sixteen => write!(f, "16"),
        }
    }
}

impl NumberUp {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "1" => Some(Self::One),
            "2" => Some(Self::Two),
            "4" => Some(Self::Four),
            "6" => Some(Self::Six),
            "9" => Some(Self::Nine),
            "16" => Some(Self::Sixteen),
            _ => None,
        }
    }

    fn _value(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Six => 6,
            Self::Nine => 9,
            Self::Sixteen => 16,
        }
    }
}

// ============================================================================
// PPD conformance level
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PpdConformance {
    Strict,
    Warn,
    Relaxed,
}

impl fmt::Display for PpdConformance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Warn => write!(f, "warn"),
            Self::Relaxed => write!(f, "relaxed"),
        }
    }
}

impl PpdConformance {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "strict" => Some(Self::Strict),
            "warn" => Some(Self::Warn),
            "relaxed" => Some(Self::Relaxed),
            _ => None,
        }
    }
}

// ============================================================================
// PPD data types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PpdOptionType {
    PickOne,
    _PickMany,
    _Boolean,
}

impl fmt::Display for PpdOptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PickOne => write!(f, "PickOne"),
            Self::_PickMany => write!(f, "PickMany"),
            Self::_Boolean => write!(f, "Boolean"),
        }
    }
}

#[derive(Debug, Clone)]
struct PpdChoice {
    name: String,
    _text: String,
    _code: String,
}

#[derive(Debug, Clone)]
struct PpdOption {
    keyword: String,
    text: String,
    // Stored on every option but not yet consulted by the constraint engine;
    // reserved for option-type-aware UI/validation, exercised in tests.
    #[allow(dead_code)]
    option_type: PpdOptionType,
    default_choice: String,
    choices: Vec<PpdChoice>,
}

impl PpdOption {
    fn new(keyword: &str, text: &str, option_type: PpdOptionType, default_choice: &str) -> Self {
        Self {
            keyword: keyword.to_string(),
            text: text.to_string(),
            option_type,
            default_choice: default_choice.to_string(),
            choices: Vec::new(),
        }
    }

    fn add_choice(&mut self, name: &str, text: &str, code: &str) {
        self.choices.push(PpdChoice {
            name: name.to_string(),
            _text: text.to_string(),
            _code: code.to_string(),
        });
    }

    fn validate_choice(&self, name: &str) -> bool {
        self.choices.iter().any(|c| c.name == name)
    }

    fn validate_default(&self) -> bool {
        self.validate_choice(&self.default_choice)
    }
}

// The constraint-violation check (is_violated) is implemented and unit-tested
// but not yet wired into the print-option validation path, so option1/option2
// are read only from tests today.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PpdConstraint {
    keyword1: String,
    option1: String,
    keyword2: String,
    option2: String,
}

impl PpdConstraint {
    fn new(kw1: &str, opt1: &str, kw2: &str, opt2: &str) -> Self {
        Self {
            keyword1: kw1.to_string(),
            option1: opt1.to_string(),
            keyword2: kw2.to_string(),
            option2: opt2.to_string(),
        }
    }

    fn _is_violated(&self, selections: &[(String, String)]) -> bool {
        let has_first = selections
            .iter()
            .any(|(k, v)| k == &self.keyword1 && v == &self.option1);
        let has_second = selections
            .iter()
            .any(|(k, v)| k == &self.keyword2 && v == &self.option2);
        has_first && has_second
    }
}

#[derive(Debug, Clone)]
struct PpdFile {
    filename: String,
    nickname: String,
    manufacturer: String,
    model_name: String,
    pcfilename: String,
    _language_version: String,
    format_version: String,
    _language_encoding: String,
    color_device: bool,
    _default_color_space: String,
    _throughput: u32,
    _ttrastertops: bool,
    options: Vec<PpdOption>,
    constraints: Vec<PpdConstraint>,
    _filters: Vec<String>,
}

impl PpdFile {
    fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            nickname: String::new(),
            manufacturer: String::new(),
            model_name: String::new(),
            pcfilename: String::new(),
            _language_version: "English".to_string(),
            format_version: "4.3".to_string(),
            _language_encoding: "ISOLatin1".to_string(),
            color_device: true,
            _default_color_space: "RGB".to_string(),
            _throughput: 1,
            _ttrastertops: true,
            options: Vec::new(),
            constraints: Vec::new(),
            _filters: Vec::new(),
        }
    }

    fn add_option(&mut self, opt: PpdOption) {
        self.options.push(opt);
    }

    fn add_constraint(&mut self, constraint: PpdConstraint) {
        self.constraints.push(constraint);
    }

    fn _find_option(&self, keyword: &str) -> Option<&PpdOption> {
        self.options.iter().find(|o| o.keyword == keyword)
    }

    fn validate(&self) -> Vec<PpdValidationIssue> {
        let mut issues = Vec::new();

        // Check required fields
        if self.nickname.is_empty() {
            issues.push(PpdValidationIssue::new(
                PpdIssueSeverity::Error,
                "Missing required NickName field",
            ));
        }
        if self.manufacturer.is_empty() {
            issues.push(PpdValidationIssue::new(
                PpdIssueSeverity::Error,
                "Missing required Manufacturer field",
            ));
        }
        if self.model_name.is_empty() {
            issues.push(PpdValidationIssue::new(
                PpdIssueSeverity::Error,
                "Missing required ModelName field",
            ));
        }
        if self.pcfilename.is_empty() {
            issues.push(PpdValidationIssue::new(
                PpdIssueSeverity::Warning,
                "Missing PCFileName field",
            ));
        }

        // Validate format version
        if self.format_version != "4.3" {
            issues.push(PpdValidationIssue::new(
                PpdIssueSeverity::Warning,
                &format!(
                    "Non-standard FormatVersion: {} (expected 4.3)",
                    self.format_version
                ),
            ));
        }

        // Validate options
        for opt in &self.options {
            if opt.choices.is_empty() {
                issues.push(PpdValidationIssue::new(
                    PpdIssueSeverity::Error,
                    &format!("Option '{}' has no choices", opt.keyword),
                ));
            }
            if !opt.validate_default() {
                issues.push(PpdValidationIssue::new(
                    PpdIssueSeverity::Error,
                    &format!(
                        "Default '{}' for option '{}' is not a valid choice",
                        opt.default_choice, opt.keyword
                    ),
                ));
            }

            // Check for duplicate choices
            let mut seen_names: Vec<&str> = Vec::new();
            for choice in &opt.choices {
                if seen_names.contains(&choice.name.as_str()) {
                    issues.push(PpdValidationIssue::new(
                        PpdIssueSeverity::Warning,
                        &format!(
                            "Duplicate choice '{}' in option '{}'",
                            choice.name, opt.keyword
                        ),
                    ));
                }
                seen_names.push(&choice.name);
            }
        }

        // Validate constraints
        for constraint in &self.constraints {
            let has_kw1 = self
                .options
                .iter()
                .any(|o| o.keyword == constraint.keyword1);
            let has_kw2 = self
                .options
                .iter()
                .any(|o| o.keyword == constraint.keyword2);
            if !has_kw1 {
                issues.push(PpdValidationIssue::new(
                    PpdIssueSeverity::Warning,
                    &format!(
                        "Constraint references unknown option '{}'",
                        constraint.keyword1
                    ),
                ));
            }
            if !has_kw2 {
                issues.push(PpdValidationIssue::new(
                    PpdIssueSeverity::Warning,
                    &format!(
                        "Constraint references unknown option '{}'",
                        constraint.keyword2
                    ),
                ));
            }
        }

        issues
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PpdIssueSeverity {
    Error,
    Warning,
    _Info,
}

impl fmt::Display for PpdIssueSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "ERROR"),
            Self::Warning => write!(f, "WARN"),
            Self::_Info => write!(f, "INFO"),
        }
    }
}

#[derive(Debug, Clone)]
struct PpdValidationIssue {
    severity: PpdIssueSeverity,
    message: String,
}

impl PpdValidationIssue {
    fn new(severity: PpdIssueSeverity, message: &str) -> Self {
        Self {
            severity,
            message: message.to_string(),
        }
    }
}

impl fmt::Display for PpdValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.severity, self.message)
    }
}

// ============================================================================
// Job options
// ============================================================================

#[derive(Debug, Clone)]
struct JobOptions {
    copies: u32,
    priority: u32,
    page_ranges: String,
    media: MediaSize,
    orientation: Orientation,
    sides: Sides,
    quality: PrintQuality,
    color_mode: ColorMode,
    media_source: MediaSource,
    number_up: NumberUp,
    collate: bool,
    fit_to_page: bool,
    _page_border: bool,
    _mirror: bool,
}

impl JobOptions {
    fn new() -> Self {
        Self {
            copies: DEFAULT_COPIES,
            priority: DEFAULT_PRIORITY,
            page_ranges: DEFAULT_PAGE_RANGE.to_string(),
            media: MediaSize::Letter,
            orientation: Orientation::Portrait,
            sides: Sides::OneSided,
            quality: PrintQuality::Normal,
            color_mode: ColorMode::Auto,
            media_source: MediaSource::Auto,
            number_up: NumberUp::One,
            collate: true,
            fit_to_page: false,
            _page_border: false,
            _mirror: false,
        }
    }

    fn parse_option(&mut self, key: &str, value: &str) -> CupsResult<()> {
        match key {
            "copies" => {
                self.copies = value
                    .parse::<u32>()
                    .map_err(|_| CupsError::new(&format!("Invalid copies value: {value}")))?;
                if self.copies == 0 {
                    return Err(CupsError::new("Copies must be at least 1"));
                }
                Ok(())
            }
            "priority" => {
                self.priority = value
                    .parse::<u32>()
                    .map_err(|_| CupsError::new(&format!("Invalid priority value: {value}")))?;
                if self.priority > MAX_PRIORITY {
                    return Err(CupsError::new(&format!(
                        "Priority must be 0-{MAX_PRIORITY}"
                    )));
                }
                Ok(())
            }
            "page-ranges" => {
                if validate_page_ranges(value) {
                    self.page_ranges = value.to_string();
                    Ok(())
                } else {
                    Err(CupsError::new(&format!("Invalid page range: {value}")))
                }
            }
            "media" => {
                self.media = MediaSize::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown media size: {value}")))?;
                Ok(())
            }
            "orientation-requested" => {
                self.orientation = Orientation::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown orientation: {value}")))?;
                Ok(())
            }
            "sides" => {
                self.sides = Sides::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown sides value: {value}")))?;
                Ok(())
            }
            "print-quality" => {
                self.quality = PrintQuality::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown quality: {value}")))?;
                Ok(())
            }
            "print-color-mode" => {
                self.color_mode = ColorMode::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown color mode: {value}")))?;
                Ok(())
            }
            "media-source" => {
                self.media_source = MediaSource::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Unknown media source: {value}")))?;
                Ok(())
            }
            "number-up" => {
                self.number_up = NumberUp::from_str(value)
                    .ok_or_else(|| CupsError::new(&format!("Invalid number-up: {value}")))?;
                Ok(())
            }
            "collate" => {
                self.collate = parse_bool_option(value)?;
                Ok(())
            }
            "fit-to-page" => {
                self.fit_to_page = parse_bool_option(value)?;
                Ok(())
            }
            _ => Err(CupsError::new(&format!("Unknown option: {key}"))),
        }
    }

    fn to_option_strings(&self) -> Vec<(String, String)> {
        vec![
            ("copies".to_string(), self.copies.to_string()),
            ("priority".to_string(), self.priority.to_string()),
            ("page-ranges".to_string(), self.page_ranges.clone()),
            ("media".to_string(), self.media.to_string()),
            (
                "orientation-requested".to_string(),
                self.orientation.to_string(),
            ),
            ("sides".to_string(), self.sides.to_string()),
            ("print-quality".to_string(), self.quality.to_string()),
            ("print-color-mode".to_string(), self.color_mode.to_string()),
            ("media-source".to_string(), self.media_source.to_string()),
            ("number-up".to_string(), self.number_up.to_string()),
            ("collate".to_string(), bool_to_str(self.collate).to_string()),
            (
                "fit-to-page".to_string(),
                bool_to_str(self.fit_to_page).to_string(),
            ),
        ]
    }
}

// ============================================================================
// Print job
// ============================================================================

#[derive(Debug, Clone)]
struct PrintJob {
    id: u32,
    printer_name: String,
    owner: String,
    title: String,
    state: JobState,
    _pages: u32,
    size: u64,
    created: u64,
    _completed: u64,
    options: JobOptions,
    _document_format: String,
}

impl PrintJob {
    fn new(id: u32, printer_name: &str, owner: &str, title: &str) -> Self {
        Self {
            id,
            printer_name: printer_name.to_string(),
            owner: owner.to_string(),
            title: title.to_string(),
            state: JobState::Pending,
            _pages: 0,
            size: 0,
            created: 1_700_000_000,
            _completed: 0,
            options: JobOptions::new(),
            _document_format: "application/pdf".to_string(),
        }
    }

    fn _job_uri(&self) -> String {
        format!("ipp://localhost:{DEFAULT_PORT}/jobs/{}", self.id)
    }

    fn display_name(&self) -> String {
        format!("{}-{}", self.printer_name, self.id)
    }

    fn can_cancel(&self) -> bool {
        !self.state.is_terminal()
    }
}

impl fmt::Display for PrintJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {} bytes",
            self.display_name(),
            self.owner,
            self.title,
            self.state,
            self.size
        )
    }
}

// ============================================================================
// Printer
// ============================================================================

#[derive(Debug, Clone)]
struct Printer {
    name: String,
    uri: String,
    _device_uri: String,
    driver: String,
    location: String,
    description: String,
    state: PrinterState,
    state_message: String,
    accepting: bool,
    shared: bool,
    is_default: bool,
    _color_supported: bool,
    _duplex_supported: bool,
    jobs: Vec<PrintJob>,
    options: Vec<(String, String)>,
    ppd: Option<PpdFile>,
    _member_names: Vec<String>,
    _is_class: bool,
}

impl Printer {
    fn new(name: &str, uri: &str, driver: &str) -> Self {
        Self {
            name: name.to_string(),
            uri: uri.to_string(),
            _device_uri: uri.to_string(),
            driver: driver.to_string(),
            location: String::new(),
            description: String::new(),
            state: PrinterState::Idle,
            state_message: String::new(),
            accepting: true,
            shared: false,
            is_default: false,
            _color_supported: true,
            _duplex_supported: true,
            jobs: Vec::new(),
            options: Vec::new(),
            ppd: None,
            _member_names: Vec::new(),
            _is_class: false,
        }
    }

    fn printer_uri(&self) -> String {
        format!("ipp://localhost:{DEFAULT_PORT}/printers/{}", self.name)
    }

    fn active_jobs(&self) -> Vec<&PrintJob> {
        self.jobs
            .iter()
            .filter(|j| !j.state.is_terminal())
            .collect()
    }

    fn all_jobs(&self) -> &[PrintJob] {
        &self.jobs
    }

    fn job_count(&self) -> usize {
        self.jobs.len()
    }

    fn active_job_count(&self) -> usize {
        self.active_jobs().len()
    }

    fn find_job(&self, id: u32) -> Option<&PrintJob> {
        self.jobs.iter().find(|j| j.id == id)
    }

    fn find_job_mut(&mut self, id: u32) -> Option<&mut PrintJob> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn set_option(&mut self, key: &str, value: &str) {
        // Remove existing option with the same key
        self.options.retain(|(k, _)| k != key);
        self.options.push((key.to_string(), value.to_string()));
    }

    fn _get_option(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn remove_option(&mut self, key: &str) {
        self.options.retain(|(k, _)| k != key);
    }
}

impl fmt::Display for Printer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "printer {} is {}. enabled since epoch",
            self.name, self.state
        )
    }
}

// ============================================================================
// Device info (for lpinfo)
// ============================================================================

#[derive(Debug, Clone)]
struct DeviceInfo {
    device_class: String,
    device_uri: String,
    device_make_and_model: String,
    device_info: String,
    _device_id: String,
}

impl DeviceInfo {
    fn new(class: &str, uri: &str, make_model: &str, info: &str) -> Self {
        Self {
            device_class: class.to_string(),
            device_uri: uri.to_string(),
            device_make_and_model: make_model.to_string(),
            device_info: info.to_string(),
            _device_id: String::new(),
        }
    }
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} \"{}\" \"{}\"",
            self.device_class, self.device_uri, self.device_make_and_model, self.device_info
        )
    }
}

// ============================================================================
// Driver info (for lpinfo)
// ============================================================================

#[derive(Debug, Clone)]
struct DriverInfo {
    _driver_type: String,
    driver_uri: String,
    driver_make_and_model: String,
}

impl DriverInfo {
    fn new(dtype: &str, uri: &str, make_model: &str) -> Self {
        Self {
            _driver_type: dtype.to_string(),
            driver_uri: uri.to_string(),
            driver_make_and_model: make_model.to_string(),
        }
    }
}

impl fmt::Display for DriverInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.driver_uri, self.driver_make_and_model)
    }
}

// ============================================================================
// Server configuration
// ============================================================================

#[derive(Debug, Clone)]
struct ServerConfig {
    port: u16,
    _listen_address: String,
    server_name: String,
    _server_admin: String,
    _log_level: String,
    _max_clients: u32,
    _max_jobs: u32,
    _max_jobs_per_user: u32,
    _preserve_job_history: bool,
    _preserve_job_files: bool,
    _browsing: bool,
    _default_auth_type: String,
    _web_interface: bool,
}

impl ServerConfig {
    fn new() -> Self {
        Self {
            port: DEFAULT_PORT,
            _listen_address: "localhost".to_string(),
            server_name: "localhost".to_string(),
            _server_admin: "root@localhost".to_string(),
            _log_level: "warn".to_string(),
            _max_clients: 100,
            _max_jobs: 500,
            _max_jobs_per_user: 0,
            _preserve_job_history: true,
            _preserve_job_files: false,
            _browsing: true,
            _default_auth_type: "Basic".to_string(),
            _web_interface: true,
        }
    }
}

// ============================================================================
// Cups system (holds all state)
// ============================================================================

struct CupsSystem {
    _config: ServerConfig,
    printers: Vec<Printer>,
    next_job_id: u32,
    devices: Vec<DeviceInfo>,
    drivers: Vec<DriverInfo>,
}

impl CupsSystem {
    fn new() -> Self {
        let mut sys = Self {
            _config: ServerConfig::new(),
            printers: Vec::new(),
            next_job_id: 1,
            devices: build_simulated_devices(),
            drivers: build_simulated_drivers(),
        };
        sys.add_default_printers();
        sys
    }

    fn add_default_printers(&mut self) {
        let mut p1 = Printer::new(
            "HP_LaserJet",
            "ipp://192.168.1.100:631/ipp/print",
            "HP LaserJet Pro MFP M428fdw",
        );
        p1.location = "Office Room 101".to_string();
        p1.description = "HP LaserJet Pro - Main Office".to_string();
        p1.is_default = true;
        p1.shared = true;
        let ppd1 = build_sample_ppd("HP_LaserJet.ppd", "HP", "HP LaserJet Pro MFP M428fdw");
        p1.ppd = Some(ppd1);
        self.printers.push(p1);

        let mut p2 = Printer::new(
            "Epson_Inkjet",
            "usb://EPSON/ET-4760",
            "Epson ET-4760 Series",
        );
        p2.location = "Lab Room 202".to_string();
        p2.description = "Epson EcoTank - Color Printer".to_string();
        let ppd2 = build_sample_ppd("Epson_Inkjet.ppd", "Epson", "Epson ET-4760 Series");
        p2.ppd = Some(ppd2);
        self.printers.push(p2);

        let mut p3 = Printer::new("PDF_Printer", "cups-pdf:/", "CUPS-PDF Virtual Printer");
        p3.location = "Virtual".to_string();
        p3.description = "Print to PDF".to_string();
        p3.shared = true;
        self.printers.push(p3);

        // Add some sample jobs
        let mut j1 = PrintJob::new(1, "HP_LaserJet", "alice", "quarterly_report.pdf");
        j1.size = 245_760;
        j1._pages = 12;
        j1.state = JobState::Completed;
        j1._completed = 1_700_001_000;
        self.printers[0].jobs.push(j1);

        let mut j2 = PrintJob::new(2, "HP_LaserJet", "bob", "meeting_notes.docx");
        j2.size = 51_200;
        j2._pages = 3;
        j2.state = JobState::Processing;
        self.printers[0].jobs.push(j2);

        let mut j3 = PrintJob::new(3, "Epson_Inkjet", "alice", "photo_collection.jpg");
        j3.size = 5_242_880;
        j3._pages = 1;
        j3.options.quality = PrintQuality::High;
        j3.options.media = MediaSize::A4;
        self.printers[1].jobs.push(j3);

        self.next_job_id = 4;
    }

    fn find_printer(&self, name: &str) -> Option<&Printer> {
        self.printers.iter().find(|p| p.name == name)
    }

    fn find_printer_mut(&mut self, name: &str) -> Option<&mut Printer> {
        self.printers.iter_mut().find(|p| p.name == name)
    }

    fn default_printer(&self) -> Option<&Printer> {
        self.printers.iter().find(|p| p.is_default)
    }

    fn default_printer_name(&self) -> String {
        self.default_printer()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| DEFAULT_PRINTER.to_string())
    }

    fn add_printer(&mut self, printer: Printer) -> CupsResult<()> {
        if self.find_printer(&printer.name).is_some() {
            return Err(CupsError::new(&format!(
                "Printer '{}' already exists",
                printer.name
            )));
        }
        self.printers.push(printer);
        Ok(())
    }

    fn remove_printer(&mut self, name: &str) -> CupsResult<()> {
        let idx = self
            .printers
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| CupsError::new(&format!("Printer '{name}' not found")))?;
        self.printers.remove(idx);
        Ok(())
    }

    fn set_default_printer(&mut self, name: &str) -> CupsResult<()> {
        if self.find_printer(name).is_none() {
            return Err(CupsError::new(&format!("Printer '{name}' not found")));
        }
        for p in &mut self.printers {
            p.is_default = p.name == name;
        }
        Ok(())
    }

    fn submit_job(
        &mut self,
        printer_name: &str,
        owner: &str,
        title: &str,
        size: u64,
        options: JobOptions,
    ) -> CupsResult<u32> {
        let printer = self
            .find_printer(printer_name)
            .ok_or_else(|| CupsError::new(&format!("Printer '{printer_name}' not found")))?;
        if !printer.accepting {
            return Err(CupsError::new(&format!(
                "Printer '{}' is not accepting jobs",
                printer_name
            )));
        }
        if printer.state == PrinterState::Stopped {
            return Err(CupsError::new(&format!(
                "Printer '{}' is stopped",
                printer_name
            )));
        }
        let job_id = self.next_job_id;
        if job_id > MAX_JOB_ID {
            return Err(CupsError::new("Maximum job ID exceeded"));
        }
        self.next_job_id += 1;
        let mut job = PrintJob::new(job_id, printer_name, owner, title);
        job.size = size;
        job.options = options;
        // Must re-borrow mutably after the immutable borrow above
        let printer = self
            .find_printer_mut(printer_name)
            .ok_or_else(|| CupsError::new(&format!("Printer '{printer_name}' not found")))?;
        printer.jobs.push(job);
        Ok(job_id)
    }

    fn cancel_job(&mut self, printer_name: &str, job_id: u32) -> CupsResult<()> {
        let printer = self
            .find_printer_mut(printer_name)
            .ok_or_else(|| CupsError::new(&format!("Printer '{printer_name}' not found")))?;
        let job = printer.find_job_mut(job_id).ok_or_else(|| {
            CupsError::new(&format!(
                "Job {job_id} not found on printer '{printer_name}'"
            ))
        })?;
        if !job.can_cancel() {
            return Err(CupsError::new(&format!(
                "Job {job_id} is already {}",
                job.state
            )));
        }
        job.state = JobState::Canceled;
        Ok(())
    }

    fn cancel_all_jobs(&mut self, printer_name: &str) -> CupsResult<u32> {
        let printer = self
            .find_printer_mut(printer_name)
            .ok_or_else(|| CupsError::new(&format!("Printer '{printer_name}' not found")))?;
        let mut count = 0u32;
        for job in &mut printer.jobs {
            if job.can_cancel() {
                job.state = JobState::Canceled;
                count += 1;
            }
        }
        Ok(count)
    }

    fn find_job_globally(&self, job_id: u32) -> Option<(&Printer, &PrintJob)> {
        for printer in &self.printers {
            if let Some(job) = printer.find_job(job_id) {
                return Some((printer, job));
            }
        }
        None
    }

    fn all_jobs(&self) -> Vec<(&Printer, &PrintJob)> {
        let mut result = Vec::new();
        for printer in &self.printers {
            for job in printer.all_jobs() {
                result.push((printer, job));
            }
        }
        result
    }

    fn _all_active_jobs(&self) -> Vec<(&Printer, &PrintJob)> {
        self.all_jobs()
            .into_iter()
            .filter(|(_, j)| !j.state.is_terminal())
            .collect()
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn validate_page_ranges(s: &str) -> bool {
    if s == "all" {
        return true;
    }
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }
        if let Some((a, b)) = part.split_once('-') {
            let a = a.trim();
            let b = b.trim();
            if a.parse::<u32>().is_err() || b.parse::<u32>().is_err() {
                return false;
            }
            let av = a.parse::<u32>().unwrap_or(0);
            let bv = b.parse::<u32>().unwrap_or(0);
            if av == 0 || bv == 0 || av > bv {
                return false;
            }
        } else if part.parse::<u32>().is_err() || part.parse::<u32>().unwrap_or(0) == 0 {
            return false;
        }
    }
    true
}

fn parse_bool_option(value: &str) -> CupsResult<bool> {
    match value {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(CupsError::new(&format!("Invalid boolean value: {value}"))),
    }
}

fn bool_to_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn parse_option_pair(s: &str) -> Option<(&str, &str)> {
    s.split_once('=')
}

fn validate_printer_name(name: &str) -> CupsResult<()> {
    if name.is_empty() {
        return Err(CupsError::new("Printer name cannot be empty"));
    }
    if name.len() > 127 {
        return Err(CupsError::new("Printer name too long (max 127 chars)"));
    }
    for ch in name.chars() {
        if !ch.is_alphanumeric() && ch != '_' && ch != '-' && ch != '.' {
            return Err(CupsError::new(&format!(
                "Invalid character '{ch}' in printer name"
            )));
        }
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(CupsError::new("Printer name cannot start with '-' or '.'"));
    }
    Ok(())
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{size} bytes")
    } else if size < 1_048_576 {
        format!("{:.1}k", size as f64 / 1024.0)
    } else if size < 1_073_741_824 {
        format!("{:.1}M", size as f64 / 1_048_576.0)
    } else {
        format!("{:.1}G", size as f64 / 1_073_741_824.0)
    }
}

fn format_timestamp(ts: u64) -> String {
    // Simple timestamp formatting
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days = ts / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    format!("{years:04}-{month:02}-{day:02} {hours:02}:{mins:02}:{secs:02}")
}

// ============================================================================
// Simulated data builders
// ============================================================================

fn build_simulated_devices() -> Vec<DeviceInfo> {
    vec![
        DeviceInfo::new(
            "network",
            "ipp://192.168.1.100:631/ipp/print",
            "HP LaserJet Pro MFP M428fdw",
            "HP LaserJet Pro MFP M428fdw",
        ),
        DeviceInfo::new(
            "direct",
            "usb://EPSON/ET-4760?serial=X123456789",
            "Epson ET-4760 Series",
            "Epson ET-4760 Series",
        ),
        DeviceInfo::new(
            "network",
            "ipp://192.168.1.101:631/ipp/print",
            "Brother HL-L2390DW",
            "Brother HL-L2390DW",
        ),
        DeviceInfo::new(
            "network",
            "socket://192.168.1.102:9100",
            "Canon imageCLASS MF445dw",
            "Canon imageCLASS MF445dw",
        ),
        DeviceInfo::new(
            "direct",
            "usb://HP/DeskJet%202700?serial=CN12345678",
            "HP DeskJet 2700 series",
            "HP DeskJet 2700 series",
        ),
        DeviceInfo::new(
            "file",
            "cups-pdf:/",
            "CUPS-PDF Virtual Printer",
            "Virtual PDF Printer",
        ),
        DeviceInfo::new(
            "network",
            "lpd://192.168.1.103/queue",
            "Lexmark MS431dn",
            "Lexmark MS431dn",
        ),
        DeviceInfo::new(
            "network",
            "ipp://192.168.1.104:631/ipp/print",
            "Xerox VersaLink C405",
            "Xerox VersaLink C405 Color MFP",
        ),
    ]
}

fn build_simulated_drivers() -> Vec<DriverInfo> {
    vec![
        DriverInfo::new(
            "ppd",
            "drv:///hp.drv/hp-laserjet_pro_mfp_m428fdw.ppd",
            "HP LaserJet Pro MFP M428fdw",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///epson.drv/epson-et-4760.ppd",
            "Epson ET-4760 Series",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///brother.drv/brother-hl-l2390dw.ppd",
            "Brother HL-L2390DW",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///sample.drv/generic-postscript.ppd",
            "Generic PostScript Printer",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///sample.drv/generic-pcl.ppd",
            "Generic PCL Laser Printer",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///cups.drv/cups-pdf.ppd",
            "CUPS-PDF Virtual Printer",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///canon.drv/canon-imageclass-mf445dw.ppd",
            "Canon imageCLASS MF445dw",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///xerox.drv/xerox-versalink-c405.ppd",
            "Xerox VersaLink C405",
        ),
        DriverInfo::new(
            "ppd",
            "drv:///lexmark.drv/lexmark-ms431dn.ppd",
            "Lexmark MS431dn",
        ),
        DriverInfo::new("ppd", "everywhere", "IPP Everywhere"),
    ]
}

fn build_sample_ppd(filename: &str, manufacturer: &str, model: &str) -> PpdFile {
    let mut ppd = PpdFile::new(filename);
    ppd.nickname = model.to_string();
    ppd.manufacturer = manufacturer.to_string();
    ppd.model_name = model.to_string();
    ppd.pcfilename = filename.to_string();

    // PageSize option
    let mut page_size = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Letter");
    page_size.add_choice("Letter", "US Letter", "<</PageSize[612 792]>>setpagedevice");
    page_size.add_choice("Legal", "US Legal", "<</PageSize[612 1008]>>setpagedevice");
    page_size.add_choice("A4", "A4", "<</PageSize[595 842]>>setpagedevice");
    page_size.add_choice("A5", "A5", "<</PageSize[420 595]>>setpagedevice");
    page_size.add_choice(
        "Executive",
        "Executive",
        "<</PageSize[522 756]>>setpagedevice",
    );
    ppd.add_option(page_size);

    // Resolution option
    let mut resolution = PpdOption::new(
        "Resolution",
        "Output Resolution",
        PpdOptionType::PickOne,
        "600dpi",
    );
    resolution.add_choice(
        "300dpi",
        "300 DPI",
        "<</HWResolution[300 300]>>setpagedevice",
    );
    resolution.add_choice(
        "600dpi",
        "600 DPI",
        "<</HWResolution[600 600]>>setpagedevice",
    );
    resolution.add_choice(
        "1200dpi",
        "1200 DPI",
        "<</HWResolution[1200 1200]>>setpagedevice",
    );
    ppd.add_option(resolution);

    // Duplex option
    let mut duplex = PpdOption::new(
        "Duplex",
        "Two-Sided Printing",
        PpdOptionType::PickOne,
        "None",
    );
    duplex.add_choice("None", "Off", "<</Duplex false>>setpagedevice");
    duplex.add_choice(
        "DuplexNoTumble",
        "Long Edge",
        "<</Duplex true/Tumble false>>setpagedevice",
    );
    duplex.add_choice(
        "DuplexTumble",
        "Short Edge",
        "<</Duplex true/Tumble true>>setpagedevice",
    );
    ppd.add_option(duplex);

    // InputSlot option
    let mut input_slot =
        PpdOption::new("InputSlot", "Media Source", PpdOptionType::PickOne, "Auto");
    input_slot.add_choice("Auto", "Automatic", "");
    input_slot.add_choice(
        "Tray1",
        "Tray 1",
        "<</ManualFeed false /MediaPosition 0>>setpagedevice",
    );
    input_slot.add_choice(
        "Tray2",
        "Tray 2",
        "<</ManualFeed false /MediaPosition 1>>setpagedevice",
    );
    input_slot.add_choice("Manual", "Manual Feed", "<</ManualFeed true>>setpagedevice");
    ppd.add_option(input_slot);

    // MediaType option
    let mut media_type = PpdOption::new("MediaType", "Media Type", PpdOptionType::PickOne, "Plain");
    media_type.add_choice("Plain", "Plain Paper", "<</MediaType(Plain)>>setpagedevice");
    media_type.add_choice("Thick", "Thick Paper", "<</MediaType(Thick)>>setpagedevice");
    media_type.add_choice("Thin", "Thin Paper", "<</MediaType(Thin)>>setpagedevice");
    media_type.add_choice(
        "Envelope",
        "Envelope",
        "<</MediaType(Envelope)>>setpagedevice",
    );
    media_type.add_choice(
        "Transparency",
        "Transparency",
        "<</MediaType(Transparency)>>setpagedevice",
    );
    media_type.add_choice("Labels", "Labels", "<</MediaType(Labels)>>setpagedevice");
    ppd.add_option(media_type);

    // OutputMode option (color / mono)
    let mut output_mode = PpdOption::new(
        "OutputMode",
        "Output Mode",
        PpdOptionType::PickOne,
        "Normal",
    );
    output_mode.add_choice("Normal", "Color", "");
    output_mode.add_choice("Monochrome", "Monochrome", "");
    ppd.add_option(output_mode);

    // Constraint: envelope not in tray1
    ppd.add_constraint(PpdConstraint::new("PageSize", "A5", "InputSlot", "Tray2"));

    ppd
}

// ============================================================================
// Personality: cupsd (daemon)
// ============================================================================

fn run_cupsd(args: &[String]) -> i32 {
    let mut foreground = false;
    let mut config_file = format!("{CONFIG_DIR}/cupsd.conf");
    let mut test_config = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--foreground" => foreground = true,
            "-F" => foreground = true,
            "-c" => {
                i += 1;
                if i < args.len() {
                    config_file = args[i].clone();
                } else {
                    eprintln!("cupsd: -c requires a config file path");
                    return 1;
                }
            }
            "-t" | "--test" => test_config = true,
            "-h" | "--help" => {
                print_cupsd_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("cupsd v{VERSION}");
                return 0;
            }
            other => {
                eprintln!("cupsd: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if test_config {
        println!("cupsd: testing config file {config_file}");
        println!("cupsd: configuration file OK");
        return 0;
    }

    let config = ServerConfig::new();
    if foreground {
        println!(
            "cupsd v{VERSION} starting in foreground mode on port {}",
            config.port
        );
    } else {
        println!(
            "cupsd v{VERSION} starting as daemon on port {}",
            config.port
        );
    }
    println!("cupsd: using config file {config_file}");
    println!("cupsd: spool directory: {SPOOL_DIR}");
    println!("cupsd: log directory: {LOG_DIR}");
    println!("cupsd: PPD directory: {PPD_DIR}");
    println!("cupsd: listening on *:{}", config.port);
    println!("cupsd: server name: {}", config.server_name);
    println!("cupsd: ready to accept connections");
    0
}

fn print_cupsd_help() {
    println!("Usage: cupsd [options]");
    println!();
    println!("Options:");
    println!("  -c config-file   Use alternate config file");
    println!("  -f, --foreground Run in foreground");
    println!("  -F               Run in foreground (same as -f)");
    println!("  -t, --test       Test configuration and exit");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Personality: lp (submit print jobs)
// ============================================================================

fn run_lp(args: &[String]) -> i32 {
    let mut system = CupsSystem::new();
    let mut printer_name: Option<String> = None;
    let mut title: Option<String> = None;
    let mut copies: Option<u32> = None;
    let mut priority: Option<u32> = None;
    let mut options: Vec<(String, String)> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut hold = false;
    let mut num_files_expected = 0u32;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                i += 1;
                if i < args.len() {
                    printer_name = Some(args[i].clone());
                } else {
                    eprintln!("lp: -d requires a printer name");
                    return 1;
                }
            }
            "-t" => {
                i += 1;
                if i < args.len() {
                    title = Some(args[i].clone());
                } else {
                    eprintln!("lp: -t requires a title");
                    return 1;
                }
            }
            "-n" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<u32>() {
                        Ok(n) if n > 0 => copies = Some(n),
                        _ => {
                            eprintln!("lp: invalid copies value '{}'", args[i]);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("lp: -n requires a number");
                    return 1;
                }
            }
            "-q" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<u32>() {
                        Ok(p) if p <= MAX_PRIORITY => priority = Some(p),
                        _ => {
                            eprintln!(
                                "lp: invalid priority '{}' (must be 0-{MAX_PRIORITY})",
                                args[i]
                            );
                            return 1;
                        }
                    }
                } else {
                    eprintln!("lp: -q requires a priority value");
                    return 1;
                }
            }
            "-o" => {
                i += 1;
                if i < args.len() {
                    if let Some((key, value)) = parse_option_pair(&args[i]) {
                        options.push((key.to_string(), value.to_string()));
                    } else {
                        // Boolean option: key with no value means key=true
                        options.push((args[i].clone(), "true".to_string()));
                    }
                } else {
                    eprintln!("lp: -o requires an option");
                    return 1;
                }
            }
            "-H" => {
                i += 1;
                if i < args.len() {
                    if args[i] == "hold" {
                        hold = true;
                    }
                } else {
                    eprintln!("lp: -H requires a hold-type");
                    return 1;
                }
            }
            "-P" => {
                i += 1;
                if i < args.len() {
                    options.push(("page-ranges".to_string(), args[i].clone()));
                } else {
                    eprintln!("lp: -P requires a page range");
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_lp_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("lp v{VERSION}");
                return 0;
            }
            "-" => {
                files.push("(stdin)".to_string());
            }
            arg if !arg.starts_with('-') => {
                files.push(arg.to_string());
            }
            other => {
                eprintln!("lp: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if files.is_empty() {
        files.push("(stdin)".to_string());
    }

    let pname = printer_name.unwrap_or_else(|| system.default_printer_name());
    if system.find_printer(&pname).is_none() {
        eprintln!("lp: printer '{pname}' not found");
        return 1;
    }

    for file in &files {
        let job_title = title.clone().unwrap_or_else(|| file.clone());
        let mut job_options = JobOptions::new();

        if let Some(c) = copies {
            job_options.copies = c;
        }
        if let Some(p) = priority {
            job_options.priority = p;
        }

        for (key, value) in &options {
            if let Err(e) = job_options.parse_option(key, value) {
                eprintln!("lp: {e}");
                return 1;
            }
        }

        let size = if file == "(stdin)" {
            1024
        } else {
            // Simulate file size based on filename length
            (file.len() as u64) * 1024
        };

        match system.submit_job(&pname, "user", &job_title, size, job_options) {
            Ok(job_id) => {
                let mut msg = format!(
                    "request id is {pname}-{job_id} ({} file(s))",
                    num_files_expected + 1
                );
                if hold {
                    msg.push_str(" [held]");
                }
                println!("{msg}");
                num_files_expected += 1;
            }
            Err(e) => {
                eprintln!("lp: {e}");
                return 1;
            }
        }
    }

    0
}

fn print_lp_help() {
    println!("Usage: lp [options] [file(s)]");
    println!();
    println!("Options:");
    println!("  -d printer       Send to named printer");
    println!("  -t title         Set job title");
    println!("  -n copies        Set number of copies");
    println!("  -q priority      Set job priority (0-{MAX_PRIORITY})");
    println!("  -o option=value  Set job option");
    println!("  -H hold          Hold the job");
    println!("  -P page-ranges   Set page ranges (e.g. 1-5,8,11-13)");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
    println!();
    println!("Options for -o:");
    println!("  copies=N                    Number of copies");
    println!("  media=SIZE                  Media size (letter, legal, a4, ...)");
    println!("  orientation-requested=VAL   Orientation (portrait, landscape)");
    println!("  sides=VAL                   Duplex (one-sided, two-sided-long-edge, ...)");
    println!("  print-quality=VAL           Quality (draft, normal, high)");
    println!("  print-color-mode=VAL        Color mode (color, monochrome, auto)");
    println!("  number-up=N                 Pages per sheet (1, 2, 4, 6, 9, 16)");
    println!("  fit-to-page=BOOL            Fit to page");
    println!("  collate=BOOL                Collate copies");
}

// ============================================================================
// Personality: lpstat (show status)
// ============================================================================

fn run_lpstat(args: &[String]) -> i32 {
    let system = CupsSystem::new();
    let mut show_printers = false;
    let mut show_jobs = false;
    let mut show_default = false;
    let mut show_classes = false;
    let mut show_devices = false;
    let mut show_all = false;
    let mut show_scheduler = false;
    let mut show_long = false;
    let mut specific_printer: Option<String> = None;
    let mut specific_user: Option<String> = None;
    let mut show_completed = false;

    if args.is_empty() {
        // Default: show active jobs
        show_jobs = true;
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => {
                show_printers = true;
                // Check if next arg is a printer name (not starting with -)
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    specific_printer = Some(args[i].clone());
                }
            }
            "-o" => {
                show_jobs = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    specific_printer = Some(args[i].clone());
                }
            }
            "-d" => show_default = true,
            "-c" => show_classes = true,
            "-v" => show_devices = true,
            "-a" => show_all = true,
            "-r" => show_scheduler = true,
            "-l" => show_long = true,
            "-t" => {
                show_printers = true;
                show_jobs = true;
                show_default = true;
                show_classes = true;
                show_devices = true;
                show_scheduler = true;
            }
            "-u" => {
                show_jobs = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    specific_user = Some(args[i].clone());
                }
            }
            "-W" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "completed" => show_completed = true,
                        "not-completed" => show_completed = false,
                        "all" => show_completed = true,
                        other => {
                            eprintln!("lpstat: unknown -W value '{other}'");
                            return 1;
                        }
                    }
                }
            }
            "-h" | "--help" => {
                print_lpstat_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("lpstat v{VERSION}");
                return 0;
            }
            other => {
                eprintln!("lpstat: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if show_scheduler {
        println!("scheduler is running");
    }

    if show_default {
        match system.default_printer() {
            Some(p) => println!("system default destination: {}", p.name),
            None => println!("no system default destination"),
        }
    }

    if show_classes {
        println!("no classes defined");
    }

    if show_printers || show_all {
        let printers: Vec<&Printer> = if let Some(ref name) = specific_printer {
            system
                .printers
                .iter()
                .filter(|p| p.name == name.as_str())
                .collect()
        } else {
            system.printers.iter().collect()
        };

        for printer in &printers {
            let accepting = if printer.accepting {
                "accepting"
            } else {
                "not accepting"
            };
            println!(
                "printer {} is {}. enabled since epoch",
                printer.name, printer.state
            );
            if show_long {
                println!("\tDescription: {}", printer.description);
                println!("\tLocation: {}", printer.location);
                println!("\tConnection: {}", printer.uri);
                println!("\tDriver: {}", printer.driver);
                println!(
                    "\tState: {} (IPP {})",
                    printer.state,
                    printer.state.ipp_code()
                );
                println!("\tAccepting: {accepting}");
                println!("\tShared: {}", bool_to_str(printer.shared));
                println!("\tDefault: {}", bool_to_str(printer.is_default));
                println!("\tActive jobs: {}", printer.active_job_count());
                println!("\tTotal jobs: {}", printer.job_count());
                println!("\tURI: {}", printer.printer_uri());
            }
        }

        if show_all {
            for printer in &printers {
                let accepting = if printer.accepting {
                    "accepting requests since epoch"
                } else {
                    "not accepting requests"
                };
                println!("{}: {accepting}", printer.name);
            }
        }
    }

    if show_devices {
        for printer in &system.printers {
            println!("device for {}: {}", printer.name, printer.uri);
        }
    }

    if show_jobs {
        let all = system.all_jobs();
        for (printer, job) in &all {
            // Filter by printer
            if let Some(ref name) = specific_printer
                && printer.name != name.as_str()
            {
                continue;
            }
            // Filter by user
            if let Some(ref user) = specific_user
                && job.owner != user.as_str()
            {
                continue;
            }
            // Filter by completion status
            if !show_completed && job.state.is_terminal() {
                continue;
            }
            if show_long {
                println!(
                    "{}-{}\t{}\t{}\t{}\t{}\t{}",
                    printer.name,
                    job.id,
                    job.owner,
                    job.state,
                    job.title,
                    format_size(job.size),
                    format_timestamp(job.created)
                );
            } else {
                println!(
                    "{}-{}\t{}\t{}\t{}",
                    printer.name,
                    job.id,
                    job.owner,
                    format_size(job.size),
                    job.title
                );
            }
        }
    }

    0
}

fn print_lpstat_help() {
    println!("Usage: lpstat [options]");
    println!();
    println!("Options:");
    println!("  -a               Show accepting state of printers");
    println!("  -c               Show classes");
    println!("  -d               Show default destination");
    println!("  -l               Show long listing");
    println!("  -o [printer]     Show jobs (optionally for specific printer)");
    println!("  -p [printer]     Show printer status");
    println!("  -r               Show scheduler running status");
    println!("  -t               Show all status information");
    println!("  -u [user]        Show jobs for user");
    println!("  -v               Show device URIs");
    println!("  -W which         Which jobs: completed, not-completed, all");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Personality: lpadmin (configure printers)
// ============================================================================

fn run_lpadmin(args: &[String]) -> i32 {
    let mut system = CupsSystem::new();
    let mut printer_name: Option<String> = None;
    let mut delete = false;
    let mut set_default = false;
    let mut device_uri: Option<String> = None;
    let mut ppd_file: Option<String> = None;
    let mut driver_uri: Option<String> = None;
    let mut description: Option<String> = None;
    let mut location: Option<String> = None;
    let mut shared: Option<bool> = None;
    let mut options: Vec<(String, String)> = Vec::new();
    let mut enable: Option<bool> = None;

    if args.is_empty() {
        print_lpadmin_help();
        return 0;
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => {
                i += 1;
                if i < args.len() {
                    printer_name = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -p requires a printer name");
                    return 1;
                }
            }
            "-x" => {
                i += 1;
                if i < args.len() {
                    printer_name = Some(args[i].clone());
                    delete = true;
                } else {
                    eprintln!("lpadmin: -x requires a printer name");
                    return 1;
                }
            }
            "-d" => {
                i += 1;
                if i < args.len() {
                    printer_name = Some(args[i].clone());
                    set_default = true;
                } else {
                    eprintln!("lpadmin: -d requires a printer name");
                    return 1;
                }
            }
            "-v" => {
                i += 1;
                if i < args.len() {
                    device_uri = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -v requires a device URI");
                    return 1;
                }
            }
            "-P" => {
                i += 1;
                if i < args.len() {
                    ppd_file = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -P requires a PPD file");
                    return 1;
                }
            }
            "-m" => {
                i += 1;
                if i < args.len() {
                    driver_uri = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -m requires a driver URI");
                    return 1;
                }
            }
            "-D" => {
                i += 1;
                if i < args.len() {
                    description = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -D requires a description");
                    return 1;
                }
            }
            "-L" => {
                i += 1;
                if i < args.len() {
                    location = Some(args[i].clone());
                } else {
                    eprintln!("lpadmin: -L requires a location");
                    return 1;
                }
            }
            "-o" => {
                i += 1;
                if i < args.len() {
                    if let Some((key, value)) = parse_option_pair(&args[i]) {
                        match key {
                            "printer-is-shared" => match parse_bool_option(value) {
                                Ok(v) => shared = Some(v),
                                Err(e) => {
                                    eprintln!("lpadmin: {e}");
                                    return 1;
                                }
                            },
                            _ => {
                                options.push((key.to_string(), value.to_string()));
                            }
                        }
                    }
                } else {
                    eprintln!("lpadmin: -o requires an option");
                    return 1;
                }
            }
            "-E" => {
                enable = Some(true);
            }
            "-h" | "--help" => {
                print_lpadmin_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("lpadmin v{VERSION}");
                return 0;
            }
            other => {
                eprintln!("lpadmin: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    let pname = match printer_name {
        Some(name) => name,
        None => {
            eprintln!("lpadmin: printer name required (-p, -x, or -d)");
            return 1;
        }
    };

    // Delete printer
    if delete {
        match system.remove_printer(&pname) {
            Ok(()) => {
                println!("lpadmin: printer '{pname}' deleted");
                return 0;
            }
            Err(e) => {
                eprintln!("lpadmin: {e}");
                return 1;
            }
        }
    }

    // Set default
    if set_default {
        match system.set_default_printer(&pname) {
            Ok(()) => {
                println!("lpadmin: default printer set to '{pname}'");
                return 0;
            }
            Err(e) => {
                eprintln!("lpadmin: {e}");
                return 1;
            }
        }
    }

    // Validate printer name
    if let Err(e) = validate_printer_name(&pname) {
        eprintln!("lpadmin: {e}");
        return 1;
    }

    // Create or modify printer
    if system.find_printer(&pname).is_some() {
        // Modify existing printer
        let printer = system
            .find_printer_mut(&pname)
            .unwrap_or_else(|| unreachable!("printer exists but find_printer_mut failed"));
        if let Some(ref uri) = device_uri {
            printer.uri = uri.clone();
            printer._device_uri = uri.clone();
        }
        if let Some(ref desc) = description {
            printer.description = desc.clone();
        }
        if let Some(ref loc) = location {
            printer.location = loc.clone();
        }
        if let Some(sh) = shared {
            printer.shared = sh;
        }
        if enable == Some(true) {
            printer.state = PrinterState::Idle;
            printer.accepting = true;
        }
        for (key, value) in &options {
            printer.set_option(key, value);
        }
        println!("lpadmin: printer '{pname}' modified");
    } else {
        // Create new printer - need a URI
        let uri = match device_uri {
            Some(ref u) => u.clone(),
            None => {
                eprintln!("lpadmin: -v device-uri required for new printer");
                return 1;
            }
        };
        let driver_name = driver_uri
            .as_deref()
            .or(ppd_file.as_deref())
            .unwrap_or("everywhere");
        let mut printer = Printer::new(&pname, &uri, driver_name);
        if let Some(ref desc) = description {
            printer.description = desc.clone();
        }
        if let Some(ref loc) = location {
            printer.location = loc.clone();
        }
        if let Some(sh) = shared {
            printer.shared = sh;
        }
        if enable == Some(true) {
            printer.state = PrinterState::Idle;
            printer.accepting = true;
        }
        for (key, value) in &options {
            printer.set_option(key, value);
        }
        if let Err(e) = system.add_printer(printer) {
            eprintln!("lpadmin: {e}");
            return 1;
        }
        println!("lpadmin: printer '{pname}' created");
    }

    0
}

fn print_lpadmin_help() {
    println!("Usage: lpadmin [options]");
    println!();
    println!("Options:");
    println!("  -p printer       Add/modify printer");
    println!("  -x printer       Delete printer");
    println!("  -d printer       Set default printer");
    println!("  -v device-uri    Set device URI");
    println!("  -P ppd-file      Use PPD file");
    println!("  -m driver-uri    Use driver");
    println!("  -D description   Set description");
    println!("  -L location      Set location");
    println!("  -E               Enable and accept jobs");
    println!("  -o option=value  Set printer option");
    println!("  -o printer-is-shared=BOOL   Share printer");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Personality: cancel (cancel print jobs)
// ============================================================================

fn run_cancel(args: &[String]) -> i32 {
    let mut system = CupsSystem::new();
    let mut cancel_all = false;
    let mut printer_name: Option<String> = None;
    let mut user_filter: Option<String> = None;
    let mut job_ids: Vec<String> = Vec::new();
    let mut purge = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" => cancel_all = true,
            "-u" => {
                i += 1;
                if i < args.len() {
                    user_filter = Some(args[i].clone());
                } else {
                    eprintln!("cancel: -u requires a username");
                    return 1;
                }
            }
            "-x" => purge = true,
            "-h" | "--help" => {
                print_cancel_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("cancel v{VERSION}");
                return 0;
            }
            arg if !arg.starts_with('-') => {
                // Could be printer-jobid or just a jobid or printer name
                job_ids.push(arg.to_string());
            }
            other => {
                eprintln!("cancel: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if cancel_all {
        // Cancel all jobs on all or specified printer
        let default_name = system.default_printer_name();
        let pname_owned = printer_name.unwrap_or(default_name);
        match system.cancel_all_jobs(&pname_owned) {
            Ok(count) => {
                if purge {
                    println!("cancel: purged {count} job(s) on '{pname_owned}'");
                } else {
                    println!("cancel: canceled {count} job(s) on '{pname_owned}'");
                }
                return 0;
            }
            Err(e) => {
                eprintln!("cancel: {e}");
                return 1;
            }
        }
    }

    if job_ids.is_empty() {
        eprintln!("cancel: no job ID(s) specified");
        print_cancel_help();
        return 1;
    }

    let mut exit_code = 0;
    for job_spec in &job_ids {
        // Parse "printer-jobid" or just "jobid"
        let (pname, jid) = if let Some((p, j)) = job_spec.rsplit_once('-') {
            if let Ok(id) = j.parse::<u32>() {
                (Some(p.to_string()), Some(id))
            } else {
                // Maybe it's just a number
                if let Ok(id) = job_spec.parse::<u32>() {
                    (None, Some(id))
                } else {
                    // Treat as printer name to cancel all
                    printer_name = Some(job_spec.clone());
                    (None, None)
                }
            }
        } else if let Ok(id) = job_spec.parse::<u32>() {
            (None, Some(id))
        } else {
            printer_name = Some(job_spec.clone());
            (None, None)
        };

        if let Some(id) = jid {
            // Cancel specific job
            let target_printer = if let Some(ref p) = pname {
                p.clone()
            } else if let Some((printer, _)) = system.find_job_globally(id) {
                printer.name.clone()
            } else {
                eprintln!("cancel: job {id} not found");
                exit_code = 1;
                continue;
            };

            // Check user filter
            if let Some(ref user) = user_filter
                && let Some((_, job)) = system.find_job_globally(id)
                && job.owner != user.as_str()
            {
                eprintln!("cancel: job {id} not owned by '{user}'");
                exit_code = 1;
                continue;
            }

            match system.cancel_job(&target_printer, id) {
                Ok(()) => println!("cancel: job {id} canceled"),
                Err(e) => {
                    eprintln!("cancel: {e}");
                    exit_code = 1;
                }
            }
        } else if let Some(ref pn) = printer_name {
            // Cancel all on printer
            let pn_clone = pn.clone();
            match system.cancel_all_jobs(&pn_clone) {
                Ok(count) => {
                    println!("cancel: canceled {count} job(s) on '{pn_clone}'");
                }
                Err(e) => {
                    eprintln!("cancel: {e}");
                    exit_code = 1;
                }
            }
        }
    }

    exit_code
}

fn print_cancel_help() {
    println!("Usage: cancel [options] [job-id(s)]");
    println!();
    println!("Options:");
    println!("  -a               Cancel all jobs");
    println!("  -u username      Only cancel jobs owned by user");
    println!("  -x               Purge (remove completed jobs too)");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
    println!();
    println!("Job IDs can be specified as:");
    println!("  123              Job ID");
    println!("  printer-123      Printer and job ID");
    println!("  printer          Cancel all jobs on printer");
}

// ============================================================================
// Personality: cupsaccept / cupsreject
// ============================================================================

fn run_cupsaccept(args: &[String]) -> i32 {
    run_accept_reject(args, true)
}

fn run_cupsreject(args: &[String]) -> i32 {
    run_accept_reject(args, false)
}

fn run_accept_reject(args: &[String], accept: bool) -> i32 {
    let mut system = CupsSystem::new();
    let cmd_name = if accept { "cupsaccept" } else { "cupsreject" };
    let mut reason: Option<String> = None;
    let mut printer_names: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" => {
                i += 1;
                if i < args.len() {
                    reason = Some(args[i].clone());
                } else {
                    eprintln!("{cmd_name}: -r requires a reason string");
                    return 1;
                }
            }
            "-h" | "--help" => {
                println!("Usage: {cmd_name} [options] printer(s)");
                println!();
                println!("Options:");
                println!("  -r reason        Set reason string");
                println!("  -h, --help       Show this help");
                println!("  -V, --version    Show version");
                return 0;
            }
            "-V" | "--version" => {
                println!("{cmd_name} v{VERSION}");
                return 0;
            }
            arg if !arg.starts_with('-') => {
                printer_names.push(arg.to_string());
            }
            other => {
                eprintln!("{cmd_name}: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if printer_names.is_empty() {
        eprintln!("{cmd_name}: no printer(s) specified");
        return 1;
    }

    let mut exit_code = 0;
    for name in &printer_names {
        match system.find_printer_mut(name) {
            Some(printer) => {
                printer.accepting = accept;
                if let Some(ref r) = reason {
                    printer.state_message = r.clone();
                }
                if accept {
                    println!("{cmd_name}: printer '{name}' now accepting jobs");
                } else {
                    println!("{cmd_name}: printer '{name}' now rejecting jobs");
                }
            }
            None => {
                eprintln!("{cmd_name}: printer '{name}' not found");
                exit_code = 1;
            }
        }
    }

    exit_code
}

// ============================================================================
// Personality: cupsenable / cupsdisable
// ============================================================================

fn run_cupsenable(args: &[String]) -> i32 {
    run_enable_disable(args, true)
}

fn run_cupsdisable(args: &[String]) -> i32 {
    run_enable_disable(args, false)
}

fn run_enable_disable(args: &[String], enable: bool) -> i32 {
    let mut system = CupsSystem::new();
    let cmd_name = if enable { "cupsenable" } else { "cupsdisable" };
    let mut reason: Option<String> = None;
    let mut hold = false;
    let mut printer_names: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" => {
                i += 1;
                if i < args.len() {
                    reason = Some(args[i].clone());
                } else {
                    eprintln!("{cmd_name}: -r requires a reason string");
                    return 1;
                }
            }
            "--hold" => hold = true,
            "-h" | "--help" => {
                println!("Usage: {cmd_name} [options] printer(s)");
                println!();
                println!("Options:");
                println!("  -r reason        Set reason string");
                if !enable {
                    println!("  --hold           Hold new jobs");
                }
                println!("  -h, --help       Show this help");
                println!("  -V, --version    Show version");
                return 0;
            }
            "-V" | "--version" => {
                println!("{cmd_name} v{VERSION}");
                return 0;
            }
            arg if !arg.starts_with('-') => {
                printer_names.push(arg.to_string());
            }
            other => {
                eprintln!("{cmd_name}: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if printer_names.is_empty() {
        eprintln!("{cmd_name}: no printer(s) specified");
        return 1;
    }

    let mut exit_code = 0;
    for name in &printer_names {
        match system.find_printer_mut(name) {
            Some(printer) => {
                if enable {
                    printer.state = PrinterState::Idle;
                    printer.state_message.clear();
                    println!("{cmd_name}: printer '{name}' enabled");
                } else {
                    printer.state = PrinterState::Stopped;
                    if let Some(ref r) = reason {
                        printer.state_message = r.clone();
                    } else {
                        printer.state_message = "Paused".to_string();
                    }
                    if hold {
                        // Hold all pending jobs
                        for job in &mut printer.jobs {
                            if job.state == JobState::Pending {
                                job.state = JobState::Held;
                            }
                        }
                    }
                    println!("{cmd_name}: printer '{name}' disabled");
                }
            }
            None => {
                eprintln!("{cmd_name}: printer '{name}' not found");
                exit_code = 1;
            }
        }
    }

    exit_code
}

// ============================================================================
// Personality: lpinfo (show devices/drivers)
// ============================================================================

fn run_lpinfo(args: &[String]) -> i32 {
    let system = CupsSystem::new();
    let mut show_devices = false;
    let mut show_drivers = false;
    let mut make_model_filter: Option<String> = None;
    let mut show_long = false;
    let mut scheme_filter: Option<String> = None;
    let mut timeout: Option<u32> = None;

    if args.is_empty() {
        print_lpinfo_help();
        return 0;
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => show_devices = true,
            "-m" => show_drivers = true,
            "-l" => show_long = true,
            "--make-and-model" => {
                i += 1;
                if i < args.len() {
                    make_model_filter = Some(args[i].to_lowercase());
                } else {
                    eprintln!("lpinfo: --make-and-model requires a value");
                    return 1;
                }
            }
            "--scheme" => {
                i += 1;
                if i < args.len() {
                    scheme_filter = Some(args[i].clone());
                } else {
                    eprintln!("lpinfo: --scheme requires a value");
                    return 1;
                }
            }
            "--timeout" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<u32>() {
                        Ok(t) => timeout = Some(t),
                        Err(_) => {
                            eprintln!("lpinfo: invalid timeout value '{}'", args[i]);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("lpinfo: --timeout requires a value");
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_lpinfo_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("lpinfo v{VERSION}");
                return 0;
            }
            other => {
                eprintln!("lpinfo: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    let _ = timeout; // Timeout is simulated, not used

    if show_devices {
        for device in &system.devices {
            // Apply scheme filter
            if let Some(ref scheme) = scheme_filter {
                let dev_scheme = device.device_uri.split("://").next().unwrap_or("");
                if dev_scheme != scheme.as_str() {
                    continue;
                }
            }
            // Apply make-and-model filter
            if let Some(ref filter) = make_model_filter
                && !device
                    .device_make_and_model
                    .to_lowercase()
                    .contains(filter.as_str())
            {
                continue;
            }
            if show_long {
                println!(
                    "Device: uri = {}\n        class = {}\n        info = {}\n        make-and-model = {}",
                    device.device_uri,
                    device.device_class,
                    device.device_info,
                    device.device_make_and_model
                );
            } else {
                println!("{device}");
            }
        }
    }

    if show_drivers {
        for driver in &system.drivers {
            if let Some(ref filter) = make_model_filter
                && !driver
                    .driver_make_and_model
                    .to_lowercase()
                    .contains(filter.as_str())
            {
                continue;
            }
            if show_long {
                println!(
                    "Model: uri = {}\n       make-and-model = {}",
                    driver.driver_uri, driver.driver_make_and_model
                );
            } else {
                println!("{driver}");
            }
        }
    }

    0
}

fn print_lpinfo_help() {
    println!("Usage: lpinfo [options]");
    println!();
    println!("Options:");
    println!("  -v               Show available devices");
    println!("  -m               Show available drivers");
    println!("  -l               Show long listing");
    println!("  --make-and-model str    Filter by make/model");
    println!("  --scheme scheme         Filter devices by URI scheme");
    println!("  --timeout secs          Device discovery timeout");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Personality: lpoptions (show/set printer options)
// ============================================================================

fn run_lpoptions(args: &[String]) -> i32 {
    let mut system = CupsSystem::new();
    let mut printer_name: Option<String> = None;
    let mut set_options: Vec<(String, String)> = Vec::new();
    let mut remove_options: Vec<String> = Vec::new();
    let mut list_options = false;
    let mut show_long = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "-p" => {
                i += 1;
                if i < args.len() {
                    printer_name = Some(args[i].clone());
                } else {
                    eprintln!("lpoptions: -p requires a printer name");
                    return 1;
                }
            }
            "-o" => {
                i += 1;
                if i < args.len() {
                    if let Some((key, value)) = parse_option_pair(&args[i]) {
                        set_options.push((key.to_string(), value.to_string()));
                    } else {
                        set_options.push((args[i].clone(), "true".to_string()));
                    }
                } else {
                    eprintln!("lpoptions: -o requires an option");
                    return 1;
                }
            }
            "-r" => {
                i += 1;
                if i < args.len() {
                    remove_options.push(args[i].clone());
                } else {
                    eprintln!("lpoptions: -r requires an option name");
                    return 1;
                }
            }
            "-l" => list_options = true,
            "--long" => show_long = true,
            "-h" | "--help" => {
                print_lpoptions_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("lpoptions v{VERSION}");
                return 0;
            }
            other => {
                eprintln!("lpoptions: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    let pname = printer_name.unwrap_or_else(|| system.default_printer_name());

    // List options mode
    if list_options {
        let printer = match system.find_printer(&pname) {
            Some(p) => p,
            None => {
                eprintln!("lpoptions: printer '{pname}' not found");
                return 1;
            }
        };

        if let Some(ref ppd) = printer.ppd {
            for opt in &ppd.options {
                let choices_str: Vec<String> = opt
                    .choices
                    .iter()
                    .map(|c| {
                        if c.name == opt.default_choice {
                            format!("*{}", c.name)
                        } else {
                            c.name.clone()
                        }
                    })
                    .collect();
                println!("{}/{}: {}", opt.keyword, opt.text, choices_str.join(" "));
            }
        } else {
            println!("lpoptions: no PPD options for '{pname}'");
        }

        // Also show instance options
        let printer = system
            .find_printer(&pname)
            .unwrap_or_else(|| unreachable!("printer was found above"));
        if !printer.options.is_empty() {
            println!();
            println!("Printer instance options:");
            for (key, value) in &printer.options {
                println!("  {key}={value}");
            }
        }

        return 0;
    }

    // Show long (PPD detail)
    if show_long {
        let printer = match system.find_printer(&pname) {
            Some(p) => p,
            None => {
                eprintln!("lpoptions: printer '{pname}' not found");
                return 1;
            }
        };
        if let Some(ref ppd) = printer.ppd {
            println!("PPD: {}", ppd.filename);
            println!("  NickName: {}", ppd.nickname);
            println!("  Manufacturer: {}", ppd.manufacturer);
            println!("  ModelName: {}", ppd.model_name);
            println!("  FormatVersion: {}", ppd.format_version);
            println!("  ColorDevice: {}", ppd.color_device);
            println!("  Options: {}", ppd.options.len());
            println!("  Constraints: {}", ppd.constraints.len());
        }
        return 0;
    }

    // Remove options
    if !remove_options.is_empty() {
        let printer = match system.find_printer_mut(&pname) {
            Some(p) => p,
            None => {
                eprintln!("lpoptions: printer '{pname}' not found");
                return 1;
            }
        };
        for opt_name in &remove_options {
            printer.remove_option(opt_name);
            println!("lpoptions: removed option '{opt_name}' from '{pname}'");
        }
        return 0;
    }

    // Set options
    if !set_options.is_empty() {
        let printer = match system.find_printer_mut(&pname) {
            Some(p) => p,
            None => {
                eprintln!("lpoptions: printer '{pname}' not found");
                return 1;
            }
        };
        for (key, value) in &set_options {
            printer.set_option(key, value);
        }
        println!(
            "lpoptions: set {} option(s) on '{pname}'",
            set_options.len()
        );
        return 0;
    }

    // Default: show current options for default printer
    let printer = match system.find_printer(&pname) {
        Some(p) => p,
        None => {
            eprintln!("lpoptions: printer '{pname}' not found");
            return 1;
        }
    };

    let default_opts = JobOptions::new();
    let opts = default_opts.to_option_strings();
    let parts: Vec<String> = opts.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("{}", parts.join(" "));

    // Show printer-specific options
    if !printer.options.is_empty() {
        let printer_parts: Vec<String> = printer
            .options
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        println!("{}", printer_parts.join(" "));
    }

    0
}

fn print_lpoptions_help() {
    println!("Usage: lpoptions [options]");
    println!();
    println!("Options:");
    println!("  -d printer       Set default printer");
    println!("  -p printer       Specify printer");
    println!("  -o option=value  Set option");
    println!("  -r option        Remove option");
    println!("  -l               List available options");
    println!("  --long           Show PPD details");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Personality: cupstestppd (test PPD files)
// ============================================================================

fn run_cupstestppd(args: &[String]) -> i32 {
    let mut conformance = PpdConformance::Warn;
    let mut verbose = false;
    let mut quiet = false;
    let mut filenames: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-q" => quiet = true,
            "-v" => verbose = true,
            "-W" => {
                i += 1;
                if i < args.len() {
                    match PpdConformance::from_str(&args[i]) {
                        Some(c) => conformance = c,
                        None => {
                            eprintln!("cupstestppd: unknown conformance level '{}'", args[i]);
                            return 1;
                        }
                    }
                } else {
                    eprintln!("cupstestppd: -W requires a conformance level");
                    return 1;
                }
            }
            "-h" | "--help" => {
                print_cupstestppd_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("cupstestppd v{VERSION}");
                return 0;
            }
            arg if !arg.starts_with('-') => {
                filenames.push(arg.to_string());
            }
            other => {
                eprintln!("cupstestppd: unknown option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    if filenames.is_empty() {
        eprintln!("cupstestppd: no PPD file(s) specified");
        return 1;
    }

    let mut overall_exit = 0;
    for filename in &filenames {
        // Build a simulated PPD for testing
        let ppd = build_test_ppd(filename);
        let issues = ppd.validate();

        let errors = issues
            .iter()
            .filter(|i| i.severity == PpdIssueSeverity::Error)
            .count();
        let warnings = issues
            .iter()
            .filter(|i| i.severity == PpdIssueSeverity::Warning)
            .count();

        let pass = match conformance {
            PpdConformance::Strict => errors == 0 && warnings == 0,
            PpdConformance::Warn => errors == 0,
            PpdConformance::Relaxed => true,
        };

        if !quiet {
            if pass {
                println!("{filename}: PASS");
            } else {
                println!("{filename}: FAIL");
                overall_exit = 1;
            }

            if verbose || !pass {
                for issue in &issues {
                    match conformance {
                        PpdConformance::Strict => {
                            println!("  {issue}");
                        }
                        PpdConformance::Warn => {
                            if issue.severity == PpdIssueSeverity::Error || verbose {
                                println!("  {issue}");
                            }
                        }
                        PpdConformance::Relaxed => {
                            if verbose {
                                println!("  {issue}");
                            }
                        }
                    }
                }
            }

            if verbose {
                println!("  Options: {}", ppd.options.len());
                println!("  Constraints: {}", ppd.constraints.len());
                println!("  Errors: {errors}");
                println!("  Warnings: {warnings}");
            }
        } else if !pass {
            overall_exit = 1;
        }
    }

    overall_exit
}

fn build_test_ppd(filename: &str) -> PpdFile {
    // Simulate PPD loading based on filename
    if filename.contains("good") || filename.contains("valid") {
        build_sample_ppd(filename, "TestMfg", "Test Printer Model 1000")
    } else if filename.contains("bad") || filename.contains("invalid") {
        // Create a PPD with errors
        let mut ppd = PpdFile::new(filename);
        ppd.nickname = "Bad Printer".to_string();
        // Missing manufacturer and model_name
        let mut opt = PpdOption::new("PageSize", "Media", PpdOptionType::PickOne, "Missing");
        opt.add_choice("Letter", "Letter", "");
        ppd.add_option(opt);
        // Constraint referencing unknown options
        ppd.add_constraint(PpdConstraint::new(
            "PageSize",
            "Letter",
            "NonExistent",
            "Foo",
        ));
        ppd
    } else if filename.contains("warn") {
        let mut ppd = build_sample_ppd(filename, "TestMfg", "Test Printer");
        ppd.format_version = "4.2".to_string();
        ppd.pcfilename.clear();
        ppd
    } else {
        // Default: valid PPD
        build_sample_ppd(filename, "OurOS", "Generic Printer")
    }
}

fn print_cupstestppd_help() {
    println!("Usage: cupstestppd [options] ppd-file(s)");
    println!();
    println!("Options:");
    println!("  -q               Quiet mode (only return exit code)");
    println!("  -v               Verbose output");
    println!("  -W level         Conformance level: strict, warn, relaxed");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Borrow-safe personality detection
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("cupsd");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "lp" => run_lp(&rest),
        "lpstat" => run_lpstat(&rest),
        "lpadmin" => run_lpadmin(&rest),
        "cancel" => run_cancel(&rest),
        "cupsaccept" => run_cupsaccept(&rest),
        "cupsreject" => run_cupsreject(&rest),
        "cupsenable" => run_cupsenable(&rest),
        "cupsdisable" => run_cupsdisable(&rest),
        "lpinfo" => run_lpinfo(&rest),
        "lpoptions" => run_lpoptions(&rest),
        "cupstestppd" => run_cupstestppd(&rest),
        // "cupsd" and any unrecognised personality fall back to the daemon.
        _ => run_cupsd(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- PrinterState tests ---

    #[test]
    fn test_printer_state_display() {
        assert_eq!(format!("{}", PrinterState::Idle), "idle");
        assert_eq!(format!("{}", PrinterState::_Processing), "processing");
        assert_eq!(format!("{}", PrinterState::Stopped), "stopped");
    }

    #[test]
    fn test_printer_state_from_str() {
        assert_eq!(PrinterState::_from_str("idle"), Some(PrinterState::Idle));
        assert_eq!(
            PrinterState::_from_str("processing"),
            Some(PrinterState::_Processing)
        );
        assert_eq!(
            PrinterState::_from_str("stopped"),
            Some(PrinterState::Stopped)
        );
        assert_eq!(PrinterState::_from_str("unknown"), None);
    }

    #[test]
    fn test_printer_state_ipp_code() {
        assert_eq!(PrinterState::Idle.ipp_code(), 3);
        assert_eq!(PrinterState::_Processing.ipp_code(), 4);
        assert_eq!(PrinterState::Stopped.ipp_code(), 5);
    }

    // --- JobState tests ---

    #[test]
    fn test_job_state_display() {
        assert_eq!(format!("{}", JobState::Pending), "pending");
        assert_eq!(format!("{}", JobState::Held), "held");
        assert_eq!(format!("{}", JobState::Processing), "processing");
        assert_eq!(format!("{}", JobState::Completed), "completed");
        assert_eq!(format!("{}", JobState::Canceled), "canceled");
        assert_eq!(format!("{}", JobState::_Aborted), "aborted");
    }

    #[test]
    fn test_job_state_from_str() {
        assert_eq!(JobState::_from_str("pending"), Some(JobState::Pending));
        assert_eq!(JobState::_from_str("held"), Some(JobState::Held));
        assert_eq!(
            JobState::_from_str("processing"),
            Some(JobState::Processing)
        );
        assert_eq!(JobState::_from_str("completed"), Some(JobState::Completed));
        assert_eq!(JobState::_from_str("canceled"), Some(JobState::Canceled));
        assert_eq!(JobState::_from_str("aborted"), Some(JobState::_Aborted));
        assert_eq!(JobState::_from_str("invalid"), None);
    }

    #[test]
    fn test_job_state_ipp_code() {
        assert_eq!(JobState::Pending._ipp_code(), 3);
        assert_eq!(JobState::Held._ipp_code(), 4);
        assert_eq!(JobState::Processing._ipp_code(), 5);
        assert_eq!(JobState::Completed._ipp_code(), 9);
        assert_eq!(JobState::Canceled._ipp_code(), 7);
        assert_eq!(JobState::_Aborted._ipp_code(), 8);
    }

    #[test]
    fn test_job_state_is_terminal() {
        assert!(!JobState::Pending.is_terminal());
        assert!(!JobState::Held.is_terminal());
        assert!(!JobState::Processing.is_terminal());
        assert!(JobState::Completed.is_terminal());
        assert!(JobState::Canceled.is_terminal());
        assert!(JobState::_Aborted.is_terminal());
    }

    // --- Orientation tests ---

    #[test]
    fn test_orientation_display() {
        assert_eq!(format!("{}", Orientation::Portrait), "portrait");
        assert_eq!(format!("{}", Orientation::Landscape), "landscape");
        assert_eq!(
            format!("{}", Orientation::ReversePortrait),
            "reverse-portrait"
        );
        assert_eq!(
            format!("{}", Orientation::ReverseLandscape),
            "reverse-landscape"
        );
    }

    #[test]
    fn test_orientation_from_str() {
        assert_eq!(
            Orientation::from_str("portrait"),
            Some(Orientation::Portrait)
        );
        assert_eq!(
            Orientation::from_str("landscape"),
            Some(Orientation::Landscape)
        );
        assert_eq!(Orientation::from_str("3"), Some(Orientation::Portrait));
        assert_eq!(Orientation::from_str("4"), Some(Orientation::Landscape));
        assert_eq!(
            Orientation::from_str("5"),
            Some(Orientation::ReversePortrait)
        );
        assert_eq!(
            Orientation::from_str("6"),
            Some(Orientation::ReverseLandscape)
        );
        assert_eq!(Orientation::from_str("invalid"), None);
    }

    #[test]
    fn test_orientation_ipp_code() {
        assert_eq!(Orientation::Portrait._ipp_code(), 3);
        assert_eq!(Orientation::Landscape._ipp_code(), 4);
        assert_eq!(Orientation::ReversePortrait._ipp_code(), 5);
        assert_eq!(Orientation::ReverseLandscape._ipp_code(), 6);
    }

    // --- Sides tests ---

    #[test]
    fn test_sides_display() {
        assert_eq!(format!("{}", Sides::OneSided), "one-sided");
        assert_eq!(
            format!("{}", Sides::TwoSidedLongEdge),
            "two-sided-long-edge"
        );
        assert_eq!(
            format!("{}", Sides::TwoSidedShortEdge),
            "two-sided-short-edge"
        );
    }

    #[test]
    fn test_sides_from_str() {
        assert_eq!(Sides::from_str("one-sided"), Some(Sides::OneSided));
        assert_eq!(
            Sides::from_str("two-sided-long-edge"),
            Some(Sides::TwoSidedLongEdge)
        );
        assert_eq!(Sides::from_str("two-sided"), Some(Sides::TwoSidedLongEdge));
        assert_eq!(
            Sides::from_str("two-sided-short-edge"),
            Some(Sides::TwoSidedShortEdge)
        );
        assert_eq!(Sides::from_str("nope"), None);
    }

    // --- PrintQuality tests ---

    #[test]
    fn test_print_quality_display() {
        assert_eq!(format!("{}", PrintQuality::Draft), "draft");
        assert_eq!(format!("{}", PrintQuality::Normal), "normal");
        assert_eq!(format!("{}", PrintQuality::High), "high");
    }

    #[test]
    fn test_print_quality_from_str() {
        assert_eq!(PrintQuality::from_str("draft"), Some(PrintQuality::Draft));
        assert_eq!(PrintQuality::from_str("normal"), Some(PrintQuality::Normal));
        assert_eq!(PrintQuality::from_str("high"), Some(PrintQuality::High));
        assert_eq!(PrintQuality::from_str("best"), Some(PrintQuality::High));
        assert_eq!(PrintQuality::from_str("3"), Some(PrintQuality::Draft));
        assert_eq!(PrintQuality::from_str("4"), Some(PrintQuality::Normal));
        assert_eq!(PrintQuality::from_str("5"), Some(PrintQuality::High));
        assert_eq!(PrintQuality::from_str("unknown"), None);
    }

    #[test]
    fn test_print_quality_ipp_code() {
        assert_eq!(PrintQuality::Draft._ipp_code(), 3);
        assert_eq!(PrintQuality::Normal._ipp_code(), 4);
        assert_eq!(PrintQuality::High._ipp_code(), 5);
    }

    // --- ColorMode tests ---

    #[test]
    fn test_color_mode_display() {
        assert_eq!(format!("{}", ColorMode::Color), "color");
        assert_eq!(format!("{}", ColorMode::Monochrome), "monochrome");
        assert_eq!(format!("{}", ColorMode::Auto), "auto");
    }

    #[test]
    fn test_color_mode_from_str() {
        assert_eq!(ColorMode::from_str("color"), Some(ColorMode::Color));
        assert_eq!(
            ColorMode::from_str("monochrome"),
            Some(ColorMode::Monochrome)
        );
        assert_eq!(
            ColorMode::from_str("grayscale"),
            Some(ColorMode::Monochrome)
        );
        assert_eq!(ColorMode::from_str("mono"), Some(ColorMode::Monochrome));
        assert_eq!(ColorMode::from_str("auto"), Some(ColorMode::Auto));
        assert_eq!(ColorMode::from_str("xyz"), None);
    }

    // --- MediaSize tests ---

    #[test]
    fn test_media_size_display() {
        assert_eq!(format!("{}", MediaSize::Letter), "letter");
        assert_eq!(format!("{}", MediaSize::Legal), "legal");
        assert_eq!(format!("{}", MediaSize::A4), "a4");
        assert_eq!(format!("{}", MediaSize::A3), "a3");
        assert_eq!(format!("{}", MediaSize::A5), "a5");
        assert_eq!(format!("{}", MediaSize::Tabloid), "tabloid");
        assert_eq!(format!("{}", MediaSize::Executive), "executive");
        assert_eq!(format!("{}", MediaSize::Envelope10), "env-10");
        assert_eq!(format!("{}", MediaSize::EnvelopeDl), "env-dl");
        assert_eq!(format!("{}", MediaSize::Custom), "custom");
    }

    #[test]
    fn test_media_size_from_str() {
        assert_eq!(MediaSize::from_str("letter"), Some(MediaSize::Letter));
        assert_eq!(
            MediaSize::from_str("na_letter_8.5x11in"),
            Some(MediaSize::Letter)
        );
        assert_eq!(MediaSize::from_str("legal"), Some(MediaSize::Legal));
        assert_eq!(MediaSize::from_str("a4"), Some(MediaSize::A4));
        assert_eq!(MediaSize::from_str("iso_a4_210x297mm"), Some(MediaSize::A4));
        assert_eq!(MediaSize::from_str("tabloid"), Some(MediaSize::Tabloid));
        assert_eq!(MediaSize::from_str("env-10"), Some(MediaSize::Envelope10));
        assert_eq!(MediaSize::from_str("custom"), Some(MediaSize::Custom));
        assert_eq!(MediaSize::from_str("nonexistent"), None);
    }

    #[test]
    fn test_media_size_dimensions() {
        assert!((MediaSize::Letter._width_pts() - 612.0).abs() < 0.01);
        assert!((MediaSize::Letter._height_pts() - 792.0).abs() < 0.01);
        assert!((MediaSize::A4._width_pts() - 595.28).abs() < 0.01);
        assert!((MediaSize::A4._height_pts() - 841.89).abs() < 0.01);
        assert!((MediaSize::Legal._height_pts() - 1008.0).abs() < 0.01);
    }

    // --- MediaSource tests ---

    #[test]
    fn test_media_source_display() {
        assert_eq!(format!("{}", MediaSource::Auto), "auto");
        assert_eq!(format!("{}", MediaSource::Tray1), "tray-1");
        assert_eq!(format!("{}", MediaSource::Tray2), "tray-2");
        assert_eq!(format!("{}", MediaSource::Manual), "manual");
        assert_eq!(format!("{}", MediaSource::Envelope), "envelope");
    }

    #[test]
    fn test_media_source_from_str() {
        assert_eq!(MediaSource::from_str("auto"), Some(MediaSource::Auto));
        assert_eq!(MediaSource::from_str("tray-1"), Some(MediaSource::Tray1));
        assert_eq!(MediaSource::from_str("manual"), Some(MediaSource::Manual));
        assert_eq!(
            MediaSource::from_str("envelope"),
            Some(MediaSource::Envelope)
        );
        assert_eq!(MediaSource::from_str("bin3"), None);
    }

    // --- NumberUp tests ---

    #[test]
    fn test_number_up_display() {
        assert_eq!(format!("{}", NumberUp::One), "1");
        assert_eq!(format!("{}", NumberUp::Two), "2");
        assert_eq!(format!("{}", NumberUp::Four), "4");
        assert_eq!(format!("{}", NumberUp::Six), "6");
        assert_eq!(format!("{}", NumberUp::Nine), "9");
        assert_eq!(format!("{}", NumberUp::Sixteen), "16");
    }

    #[test]
    fn test_number_up_from_str() {
        assert_eq!(NumberUp::from_str("1"), Some(NumberUp::One));
        assert_eq!(NumberUp::from_str("2"), Some(NumberUp::Two));
        assert_eq!(NumberUp::from_str("4"), Some(NumberUp::Four));
        assert_eq!(NumberUp::from_str("16"), Some(NumberUp::Sixteen));
        assert_eq!(NumberUp::from_str("3"), None);
        assert_eq!(NumberUp::from_str("7"), None);
    }

    #[test]
    fn test_number_up_value() {
        assert_eq!(NumberUp::One._value(), 1);
        assert_eq!(NumberUp::Two._value(), 2);
        assert_eq!(NumberUp::Four._value(), 4);
        assert_eq!(NumberUp::Six._value(), 6);
        assert_eq!(NumberUp::Nine._value(), 9);
        assert_eq!(NumberUp::Sixteen._value(), 16);
    }

    // --- PpdConformance tests ---

    #[test]
    fn test_ppd_conformance_from_str() {
        assert_eq!(
            PpdConformance::from_str("strict"),
            Some(PpdConformance::Strict)
        );
        assert_eq!(PpdConformance::from_str("warn"), Some(PpdConformance::Warn));
        assert_eq!(
            PpdConformance::from_str("relaxed"),
            Some(PpdConformance::Relaxed)
        );
        assert_eq!(PpdConformance::from_str("unknown"), None);
    }

    #[test]
    fn test_ppd_conformance_display() {
        assert_eq!(format!("{}", PpdConformance::Strict), "strict");
        assert_eq!(format!("{}", PpdConformance::Warn), "warn");
        assert_eq!(format!("{}", PpdConformance::Relaxed), "relaxed");
    }

    // --- PpdOption tests ---

    #[test]
    fn test_ppd_option_new() {
        let opt = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Letter");
        assert_eq!(opt.keyword, "PageSize");
        assert_eq!(opt.text, "Media Size");
        assert!(matches!(opt.option_type, PpdOptionType::PickOne));
        assert_eq!(opt.default_choice, "Letter");
        assert!(opt.choices.is_empty());
    }

    #[test]
    fn test_ppd_option_add_choice() {
        let mut opt = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Letter");
        opt.add_choice("Letter", "US Letter", "code");
        opt.add_choice("A4", "A4", "code2");
        assert_eq!(opt.choices.len(), 2);
        assert_eq!(opt.choices[0].name, "Letter");
        assert_eq!(opt.choices[1].name, "A4");
    }

    #[test]
    fn test_ppd_option_validate_choice() {
        let mut opt = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Letter");
        opt.add_choice("Letter", "US Letter", "code");
        opt.add_choice("A4", "A4", "code2");
        assert!(opt.validate_choice("Letter"));
        assert!(opt.validate_choice("A4"));
        assert!(!opt.validate_choice("Legal"));
    }

    #[test]
    fn test_ppd_option_validate_default() {
        let mut opt = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Letter");
        opt.add_choice("Letter", "US Letter", "code");
        assert!(opt.validate_default());

        let opt2 = PpdOption::new("PageSize", "Media Size", PpdOptionType::PickOne, "Missing");
        assert!(!opt2.validate_default());
    }

    #[test]
    fn test_ppd_option_type_display() {
        assert_eq!(format!("{}", PpdOptionType::PickOne), "PickOne");
        assert_eq!(format!("{}", PpdOptionType::_PickMany), "PickMany");
        assert_eq!(format!("{}", PpdOptionType::_Boolean), "Boolean");
    }

    // --- PpdConstraint tests ---

    #[test]
    fn test_ppd_constraint_new() {
        let c = PpdConstraint::new("PageSize", "A5", "InputSlot", "Tray2");
        assert_eq!(c.keyword1, "PageSize");
        assert_eq!(c.option1, "A5");
        assert_eq!(c.keyword2, "InputSlot");
        assert_eq!(c.option2, "Tray2");
    }

    #[test]
    fn test_ppd_constraint_is_violated() {
        let c = PpdConstraint::new("PageSize", "A5", "InputSlot", "Tray2");
        let sel1 = vec![
            ("PageSize".to_string(), "A5".to_string()),
            ("InputSlot".to_string(), "Tray2".to_string()),
        ];
        assert!(c._is_violated(&sel1));

        let sel2 = vec![
            ("PageSize".to_string(), "Letter".to_string()),
            ("InputSlot".to_string(), "Tray2".to_string()),
        ];
        assert!(!c._is_violated(&sel2));

        let sel3 = vec![("PageSize".to_string(), "A5".to_string())];
        assert!(!c._is_violated(&sel3));
    }

    // --- PpdFile tests ---

    #[test]
    fn test_ppd_file_new() {
        let ppd = PpdFile::new("test.ppd");
        assert_eq!(ppd.filename, "test.ppd");
        assert!(ppd.nickname.is_empty());
        assert!(ppd.options.is_empty());
        assert!(ppd.constraints.is_empty());
    }

    #[test]
    fn test_ppd_file_add_option() {
        let mut ppd = PpdFile::new("test.ppd");
        let opt = PpdOption::new("PageSize", "Size", PpdOptionType::PickOne, "Letter");
        ppd.add_option(opt);
        assert_eq!(ppd.options.len(), 1);
    }

    #[test]
    fn test_ppd_file_find_option() {
        let mut ppd = PpdFile::new("test.ppd");
        let opt = PpdOption::new("PageSize", "Size", PpdOptionType::PickOne, "Letter");
        ppd.add_option(opt);
        assert!(ppd._find_option("PageSize").is_some());
        assert!(ppd._find_option("Resolution").is_none());
    }

    #[test]
    fn test_ppd_validate_missing_fields() {
        let ppd = PpdFile::new("test.ppd");
        let issues = ppd.validate();
        // Should have errors for missing nickname, manufacturer, model_name
        let errors: Vec<&PpdValidationIssue> = issues
            .iter()
            .filter(|i| i.severity == PpdIssueSeverity::Error)
            .collect();
        assert!(errors.len() >= 3);
    }

    #[test]
    fn test_ppd_validate_valid() {
        let ppd = build_sample_ppd("test.ppd", "TestMfg", "Test Model");
        let issues = ppd.validate();
        let errors: Vec<&PpdValidationIssue> = issues
            .iter()
            .filter(|i| i.severity == PpdIssueSeverity::Error)
            .collect();
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_ppd_validate_bad_default() {
        let mut ppd = PpdFile::new("test.ppd");
        ppd.nickname = "Test".to_string();
        ppd.manufacturer = "Test".to_string();
        ppd.model_name = "Test".to_string();
        let mut opt = PpdOption::new("PageSize", "Size", PpdOptionType::PickOne, "BadDefault");
        opt.add_choice("Letter", "US Letter", "");
        ppd.add_option(opt);
        let issues = ppd.validate();
        let has_default_error = issues.iter().any(|i| {
            i.severity == PpdIssueSeverity::Error && i.message.contains("not a valid choice")
        });
        assert!(has_default_error);
    }

    #[test]
    fn test_ppd_validate_empty_option() {
        let mut ppd = PpdFile::new("test.ppd");
        ppd.nickname = "Test".to_string();
        ppd.manufacturer = "Test".to_string();
        ppd.model_name = "Test".to_string();
        let opt = PpdOption::new("PageSize", "Size", PpdOptionType::PickOne, "Letter");
        ppd.add_option(opt);
        let issues = ppd.validate();
        let has_no_choices = issues.iter().any(|i| i.message.contains("has no choices"));
        assert!(has_no_choices);
    }

    #[test]
    fn test_ppd_validate_unknown_constraint_ref() {
        let mut ppd = build_sample_ppd("test.ppd", "Test", "Test");
        ppd.add_constraint(PpdConstraint::new(
            "PageSize",
            "Letter",
            "FakeOption",
            "Foo",
        ));
        let issues = ppd.validate();
        let has_unknown = issues.iter().any(|i| i.message.contains("unknown option"));
        assert!(has_unknown);
    }

    #[test]
    fn test_ppd_issue_display() {
        let issue = PpdValidationIssue::new(PpdIssueSeverity::Error, "test error");
        assert_eq!(format!("{issue}"), "ERROR: test error");
        let warn = PpdValidationIssue::new(PpdIssueSeverity::Warning, "test warn");
        assert_eq!(format!("{warn}"), "WARN: test warn");
    }

    // --- JobOptions tests ---

    #[test]
    fn test_job_options_default() {
        let opts = JobOptions::new();
        assert_eq!(opts.copies, 1);
        assert_eq!(opts.priority, 50);
        assert_eq!(opts.page_ranges, "all");
        assert!(matches!(opts.media, MediaSize::Letter));
        assert!(matches!(opts.orientation, Orientation::Portrait));
        assert!(matches!(opts.sides, Sides::OneSided));
        assert!(matches!(opts.quality, PrintQuality::Normal));
        assert!(matches!(opts.color_mode, ColorMode::Auto));
        assert!(matches!(opts.media_source, MediaSource::Auto));
        assert!(matches!(opts.number_up, NumberUp::One));
        assert!(opts.collate);
        assert!(!opts.fit_to_page);
    }

    #[test]
    fn test_job_options_parse_copies() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("copies", "5").is_ok());
        assert_eq!(opts.copies, 5);
    }

    #[test]
    fn test_job_options_parse_copies_zero() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("copies", "0").is_err());
    }

    #[test]
    fn test_job_options_parse_copies_invalid() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("copies", "abc").is_err());
    }

    #[test]
    fn test_job_options_parse_priority() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("priority", "75").is_ok());
        assert_eq!(opts.priority, 75);
    }

    #[test]
    fn test_job_options_parse_priority_overflow() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("priority", "101").is_err());
    }

    #[test]
    fn test_job_options_parse_media() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("media", "a4").is_ok());
        assert!(matches!(opts.media, MediaSize::A4));
    }

    #[test]
    fn test_job_options_parse_orientation() {
        let mut opts = JobOptions::new();
        assert!(
            opts.parse_option("orientation-requested", "landscape")
                .is_ok()
        );
        assert!(matches!(opts.orientation, Orientation::Landscape));
    }

    #[test]
    fn test_job_options_parse_sides() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("sides", "two-sided-long-edge").is_ok());
        assert!(matches!(opts.sides, Sides::TwoSidedLongEdge));
    }

    #[test]
    fn test_job_options_parse_quality() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("print-quality", "high").is_ok());
        assert!(matches!(opts.quality, PrintQuality::High));
    }

    #[test]
    fn test_job_options_parse_color_mode() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("print-color-mode", "monochrome").is_ok());
        assert!(matches!(opts.color_mode, ColorMode::Monochrome));
    }

    #[test]
    fn test_job_options_parse_media_source() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("media-source", "tray-1").is_ok());
        assert!(matches!(opts.media_source, MediaSource::Tray1));
    }

    #[test]
    fn test_job_options_parse_number_up() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("number-up", "4").is_ok());
        assert!(matches!(opts.number_up, NumberUp::Four));
    }

    #[test]
    fn test_job_options_parse_collate() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("collate", "false").is_ok());
        assert!(!opts.collate);
    }

    #[test]
    fn test_job_options_parse_fit_to_page() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("fit-to-page", "yes").is_ok());
        assert!(opts.fit_to_page);
    }

    #[test]
    fn test_job_options_parse_unknown() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("nonexistent", "foo").is_err());
    }

    #[test]
    fn test_job_options_parse_page_ranges() {
        let mut opts = JobOptions::new();
        assert!(opts.parse_option("page-ranges", "1-5,8,11-13").is_ok());
        assert_eq!(opts.page_ranges, "1-5,8,11-13");
    }

    #[test]
    fn test_job_options_to_option_strings() {
        let opts = JobOptions::new();
        let strings = opts.to_option_strings();
        assert!(strings.len() >= 10);
        let copies = strings.iter().find(|(k, _)| k == "copies");
        assert!(copies.is_some());
        assert_eq!(copies.unwrap().1, "1");
    }

    // --- PrintJob tests ---

    #[test]
    fn test_print_job_new() {
        let job = PrintJob::new(42, "MyPrinter", "alice", "test.pdf");
        assert_eq!(job.id, 42);
        assert_eq!(job.printer_name, "MyPrinter");
        assert_eq!(job.owner, "alice");
        assert_eq!(job.title, "test.pdf");
        assert!(matches!(job.state, JobState::Pending));
    }

    #[test]
    fn test_print_job_uri() {
        let job = PrintJob::new(42, "MyPrinter", "alice", "test.pdf");
        assert_eq!(job._job_uri(), "ipp://localhost:631/jobs/42");
    }

    #[test]
    fn test_print_job_display_name() {
        let job = PrintJob::new(42, "MyPrinter", "alice", "test.pdf");
        assert_eq!(job.display_name(), "MyPrinter-42");
    }

    #[test]
    fn test_print_job_can_cancel() {
        let mut job = PrintJob::new(1, "P", "u", "t");
        assert!(job.can_cancel());
        job.state = JobState::Processing;
        assert!(job.can_cancel());
        job.state = JobState::Completed;
        assert!(!job.can_cancel());
        job.state = JobState::Canceled;
        assert!(!job.can_cancel());
    }

    #[test]
    fn test_print_job_display() {
        let mut job = PrintJob::new(1, "HP", "bob", "doc.pdf");
        job.size = 1024;
        let display = format!("{job}");
        assert!(display.contains("HP-1"));
        assert!(display.contains("bob"));
        assert!(display.contains("doc.pdf"));
    }

    // --- Printer tests ---

    #[test]
    fn test_printer_new() {
        let p = Printer::new("TestPrinter", "ipp://test", "TestDriver");
        assert_eq!(p.name, "TestPrinter");
        assert_eq!(p.uri, "ipp://test");
        assert_eq!(p.driver, "TestDriver");
        assert!(matches!(p.state, PrinterState::Idle));
        assert!(p.accepting);
        assert!(!p.shared);
        assert!(!p.is_default);
    }

    #[test]
    fn test_printer_uri() {
        let p = Printer::new("MyPrinter", "ipp://test", "drv");
        assert_eq!(p.printer_uri(), "ipp://localhost:631/printers/MyPrinter");
    }

    #[test]
    fn test_printer_active_jobs() {
        let mut p = Printer::new("P", "uri", "drv");
        let mut j1 = PrintJob::new(1, "P", "u", "t1");
        j1.state = JobState::Pending;
        let mut j2 = PrintJob::new(2, "P", "u", "t2");
        j2.state = JobState::Completed;
        let mut j3 = PrintJob::new(3, "P", "u", "t3");
        j3.state = JobState::Processing;
        p.jobs.push(j1);
        p.jobs.push(j2);
        p.jobs.push(j3);
        assert_eq!(p.active_jobs().len(), 2);
        assert_eq!(p.active_job_count(), 2);
        assert_eq!(p.job_count(), 3);
    }

    #[test]
    fn test_printer_find_job() {
        let mut p = Printer::new("P", "uri", "drv");
        p.jobs.push(PrintJob::new(10, "P", "u", "t"));
        p.jobs.push(PrintJob::new(20, "P", "u", "t2"));
        assert!(p.find_job(10).is_some());
        assert!(p.find_job(20).is_some());
        assert!(p.find_job(30).is_none());
    }

    #[test]
    fn test_printer_find_job_mut() {
        let mut p = Printer::new("P", "uri", "drv");
        p.jobs.push(PrintJob::new(10, "P", "u", "t"));
        let job = p.find_job_mut(10).unwrap();
        job.state = JobState::Canceled;
        assert!(matches!(p.jobs[0].state, JobState::Canceled));
    }

    #[test]
    fn test_printer_set_get_option() {
        let mut p = Printer::new("P", "uri", "drv");
        p.set_option("media", "a4");
        assert_eq!(p._get_option("media"), Some("a4"));
        // Overwrite
        p.set_option("media", "letter");
        assert_eq!(p._get_option("media"), Some("letter"));
        assert_eq!(p.options.len(), 1);
    }

    #[test]
    fn test_printer_remove_option() {
        let mut p = Printer::new("P", "uri", "drv");
        p.set_option("media", "a4");
        p.set_option("quality", "high");
        p.remove_option("media");
        assert_eq!(p._get_option("media"), None);
        assert_eq!(p._get_option("quality"), Some("high"));
    }

    // --- CupsSystem tests ---

    #[test]
    fn test_cups_system_new() {
        let sys = CupsSystem::new();
        assert!(!sys.printers.is_empty());
        assert!(!sys.devices.is_empty());
        assert!(!sys.drivers.is_empty());
    }

    #[test]
    fn test_cups_system_default_printers() {
        let sys = CupsSystem::new();
        assert_eq!(sys.printers.len(), 3);
        assert!(sys.printers[0].is_default);
        assert_eq!(sys.printers[0].name, "HP_LaserJet");
    }

    #[test]
    fn test_cups_system_find_printer() {
        let sys = CupsSystem::new();
        assert!(sys.find_printer("HP_LaserJet").is_some());
        assert!(sys.find_printer("Epson_Inkjet").is_some());
        assert!(sys.find_printer("PDF_Printer").is_some());
        assert!(sys.find_printer("Nonexistent").is_none());
    }

    #[test]
    fn test_cups_system_default_printer() {
        let sys = CupsSystem::new();
        let dp = sys.default_printer().unwrap();
        assert_eq!(dp.name, "HP_LaserJet");
        assert_eq!(sys.default_printer_name(), "HP_LaserJet");
    }

    #[test]
    fn test_cups_system_add_printer() {
        let mut sys = CupsSystem::new();
        let p = Printer::new("NewPrinter", "ipp://new", "drv");
        assert!(sys.add_printer(p).is_ok());
        assert!(sys.find_printer("NewPrinter").is_some());
    }

    #[test]
    fn test_cups_system_add_duplicate_printer() {
        let mut sys = CupsSystem::new();
        let p = Printer::new("HP_LaserJet", "ipp://dup", "drv");
        assert!(sys.add_printer(p).is_err());
    }

    #[test]
    fn test_cups_system_remove_printer() {
        let mut sys = CupsSystem::new();
        assert!(sys.remove_printer("PDF_Printer").is_ok());
        assert!(sys.find_printer("PDF_Printer").is_none());
    }

    #[test]
    fn test_cups_system_remove_nonexistent() {
        let mut sys = CupsSystem::new();
        assert!(sys.remove_printer("Ghost").is_err());
    }

    #[test]
    fn test_cups_system_set_default() {
        let mut sys = CupsSystem::new();
        assert!(sys.set_default_printer("Epson_Inkjet").is_ok());
        assert_eq!(sys.default_printer_name(), "Epson_Inkjet");
        assert!(!sys.find_printer("HP_LaserJet").unwrap().is_default);
    }

    #[test]
    fn test_cups_system_set_default_nonexistent() {
        let mut sys = CupsSystem::new();
        assert!(sys.set_default_printer("Ghost").is_err());
    }

    #[test]
    fn test_cups_system_submit_job() {
        let mut sys = CupsSystem::new();
        let opts = JobOptions::new();
        let result = sys.submit_job("HP_LaserJet", "bob", "report.pdf", 4096, opts);
        assert!(result.is_ok());
        let job_id = result.unwrap();
        assert!(job_id >= 4); // After default jobs
    }

    #[test]
    fn test_cups_system_submit_job_nonexistent_printer() {
        let mut sys = CupsSystem::new();
        let opts = JobOptions::new();
        assert!(
            sys.submit_job("Ghost", "bob", "report.pdf", 4096, opts)
                .is_err()
        );
    }

    #[test]
    fn test_cups_system_submit_job_not_accepting() {
        let mut sys = CupsSystem::new();
        sys.find_printer_mut("HP_LaserJet").unwrap().accepting = false;
        let opts = JobOptions::new();
        assert!(
            sys.submit_job("HP_LaserJet", "bob", "report.pdf", 4096, opts)
                .is_err()
        );
    }

    #[test]
    fn test_cups_system_submit_job_stopped() {
        let mut sys = CupsSystem::new();
        sys.find_printer_mut("HP_LaserJet").unwrap().state = PrinterState::Stopped;
        let opts = JobOptions::new();
        assert!(
            sys.submit_job("HP_LaserJet", "bob", "report.pdf", 4096, opts)
                .is_err()
        );
    }

    #[test]
    fn test_cups_system_cancel_job() {
        let mut sys = CupsSystem::new();
        // Job 2 is processing
        assert!(sys.cancel_job("HP_LaserJet", 2).is_ok());
        let job = sys
            .find_printer("HP_LaserJet")
            .unwrap()
            .find_job(2)
            .unwrap();
        assert!(matches!(job.state, JobState::Canceled));
    }

    #[test]
    fn test_cups_system_cancel_completed_job() {
        let mut sys = CupsSystem::new();
        // Job 1 is completed
        assert!(sys.cancel_job("HP_LaserJet", 1).is_err());
    }

    #[test]
    fn test_cups_system_cancel_nonexistent_job() {
        let mut sys = CupsSystem::new();
        assert!(sys.cancel_job("HP_LaserJet", 999).is_err());
    }

    #[test]
    fn test_cups_system_cancel_all_jobs() {
        let mut sys = CupsSystem::new();
        let count = sys.cancel_all_jobs("HP_LaserJet").unwrap();
        // Job 2 was processing (cancelable), job 1 was completed (not cancelable)
        assert_eq!(count, 1);
    }

    #[test]
    fn test_cups_system_find_job_globally() {
        let sys = CupsSystem::new();
        let result = sys.find_job_globally(3);
        assert!(result.is_some());
        let (printer, job) = result.unwrap();
        assert_eq!(printer.name, "Epson_Inkjet");
        assert_eq!(job.id, 3);
    }

    #[test]
    fn test_cups_system_find_job_globally_not_found() {
        let sys = CupsSystem::new();
        assert!(sys.find_job_globally(999).is_none());
    }

    #[test]
    fn test_cups_system_all_jobs() {
        let sys = CupsSystem::new();
        let jobs = sys.all_jobs();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn test_cups_system_all_active_jobs() {
        let sys = CupsSystem::new();
        let active = sys._all_active_jobs();
        // Job 1 is completed, 2 is processing, 3 is pending
        assert_eq!(active.len(), 2);
    }

    // --- Helper function tests ---

    #[test]
    fn test_validate_page_ranges_all() {
        assert!(validate_page_ranges("all"));
    }

    #[test]
    fn test_validate_page_ranges_single() {
        assert!(validate_page_ranges("1"));
        assert!(validate_page_ranges("42"));
    }

    #[test]
    fn test_validate_page_ranges_range() {
        assert!(validate_page_ranges("1-5"));
        assert!(validate_page_ranges("10-20"));
    }

    #[test]
    fn test_validate_page_ranges_complex() {
        assert!(validate_page_ranges("1-5,8,11-13"));
        assert!(validate_page_ranges("1,2,3"));
    }

    #[test]
    fn test_validate_page_ranges_invalid() {
        assert!(!validate_page_ranges(""));
        assert!(!validate_page_ranges("abc"));
        assert!(!validate_page_ranges("5-3")); // reverse range
        assert!(!validate_page_ranges("0"));
        assert!(!validate_page_ranges("1-0"));
        assert!(!validate_page_ranges(","));
        assert!(!validate_page_ranges("1,,2"));
    }

    #[test]
    fn test_parse_bool_option() {
        assert!(parse_bool_option("true").unwrap());
        assert!(parse_bool_option("yes").unwrap());
        assert!(parse_bool_option("on").unwrap());
        assert!(parse_bool_option("1").unwrap());
        assert!(!parse_bool_option("false").unwrap());
        assert!(!parse_bool_option("no").unwrap());
        assert!(!parse_bool_option("off").unwrap());
        assert!(!parse_bool_option("0").unwrap());
        assert!(parse_bool_option("maybe").is_err());
    }

    #[test]
    fn test_bool_to_str() {
        assert_eq!(bool_to_str(true), "true");
        assert_eq!(bool_to_str(false), "false");
    }

    #[test]
    fn test_parse_option_pair() {
        assert_eq!(parse_option_pair("key=value"), Some(("key", "value")));
        assert_eq!(parse_option_pair("key=val=ue"), Some(("key", "val=ue")));
        assert_eq!(parse_option_pair("noequals"), None);
    }

    #[test]
    fn test_validate_printer_name_valid() {
        assert!(validate_printer_name("HP_LaserJet").is_ok());
        assert!(validate_printer_name("printer-1").is_ok());
        assert!(validate_printer_name("My.Printer").is_ok());
        assert!(validate_printer_name("a").is_ok());
    }

    #[test]
    fn test_validate_printer_name_empty() {
        assert!(validate_printer_name("").is_err());
    }

    #[test]
    fn test_validate_printer_name_too_long() {
        let long = "a".repeat(128);
        assert!(validate_printer_name(&long).is_err());
    }

    #[test]
    fn test_validate_printer_name_invalid_chars() {
        assert!(validate_printer_name("has space").is_err());
        assert!(validate_printer_name("has/slash").is_err());
        assert!(validate_printer_name("has@at").is_err());
    }

    #[test]
    fn test_validate_printer_name_starts_with_dash() {
        assert!(validate_printer_name("-printer").is_err());
    }

    #[test]
    fn test_validate_printer_name_starts_with_dot() {
        assert!(validate_printer_name(".printer").is_err());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(1023), "1023 bytes");
        assert_eq!(format_size(1024), "1.0k");
        assert_eq!(format_size(1536), "1.5k");
        assert_eq!(format_size(1_048_576), "1.0M");
        assert_eq!(format_size(1_073_741_824), "1.0G");
    }

    #[test]
    fn test_format_timestamp() {
        let ts = format_timestamp(0);
        assert!(ts.contains("1970"));
    }

    #[test]
    fn test_format_timestamp_nonzero() {
        let ts = format_timestamp(1_700_000_000);
        assert!(!ts.is_empty());
    }

    // --- Simulated data tests ---

    #[test]
    fn test_build_simulated_devices() {
        let devices = build_simulated_devices();
        assert!(devices.len() >= 6);
        // Check we have various device classes
        let classes: Vec<&str> = devices.iter().map(|d| d.device_class.as_str()).collect();
        assert!(classes.contains(&"network"));
        assert!(classes.contains(&"direct"));
        assert!(classes.contains(&"file"));
    }

    #[test]
    fn test_build_simulated_drivers() {
        let drivers = build_simulated_drivers();
        assert!(drivers.len() >= 8);
    }

    #[test]
    fn test_build_sample_ppd() {
        let ppd = build_sample_ppd("test.ppd", "TestMfg", "Test Model");
        assert_eq!(ppd.manufacturer, "TestMfg");
        assert_eq!(ppd.model_name, "Test Model");
        assert!(!ppd.options.is_empty());
        // Check it has PageSize option
        assert!(ppd._find_option("PageSize").is_some());
        assert!(ppd._find_option("Resolution").is_some());
        assert!(ppd._find_option("Duplex").is_some());
    }

    #[test]
    fn test_device_info_display() {
        let dev = DeviceInfo::new("network", "ipp://test", "Test Printer", "Test Info");
        let s = format!("{dev}");
        assert!(s.contains("network"));
        assert!(s.contains("ipp://test"));
    }

    #[test]
    fn test_driver_info_display() {
        let drv = DriverInfo::new("ppd", "drv:///test.ppd", "Test Driver");
        let s = format!("{drv}");
        assert!(s.contains("drv:///test.ppd"));
        assert!(s.contains("Test Driver"));
    }

    // --- ServerConfig tests ---

    #[test]
    fn test_server_config_new() {
        let config = ServerConfig::new();
        assert_eq!(config.port, 631);
        assert_eq!(config.server_name, "localhost");
    }

    // --- CupsError tests ---

    #[test]
    fn test_cups_error_display() {
        let e = CupsError::new("test error");
        assert_eq!(format!("{e}"), "test error");
    }

    #[test]
    fn test_cups_error_debug() {
        let e = CupsError::new("test");
        let dbg = format!("{e:?}");
        assert!(dbg.contains("test"));
    }

    // --- Personality integration tests ---

    #[test]
    fn test_cupsd_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_cupsd(&args), 0);
    }

    #[test]
    fn test_cupsd_version() {
        let args = vec!["--version".to_string()];
        assert_eq!(run_cupsd(&args), 0);
    }

    #[test]
    fn test_cupsd_test_config() {
        let args = vec!["-t".to_string()];
        assert_eq!(run_cupsd(&args), 0);
    }

    #[test]
    fn test_cupsd_foreground() {
        let args = vec!["-f".to_string()];
        assert_eq!(run_cupsd(&args), 0);
    }

    #[test]
    fn test_cupsd_unknown_option() {
        let args = vec!["--unknown".to_string()];
        assert_eq!(run_cupsd(&args), 1);
    }

    #[test]
    fn test_lp_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_lp(&args), 0);
    }

    #[test]
    fn test_lp_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_lp(&args), 0);
    }

    #[test]
    fn test_lp_submit_stdin() {
        let args = vec!["-d".to_string(), "HP_LaserJet".to_string()];
        assert_eq!(run_lp(&args), 0);
    }

    #[test]
    fn test_lp_submit_file() {
        let args = vec![
            "-d".to_string(),
            "HP_LaserJet".to_string(),
            "-t".to_string(),
            "Test Job".to_string(),
            "test.pdf".to_string(),
        ];
        assert_eq!(run_lp(&args), 0);
    }

    #[test]
    fn test_lp_nonexistent_printer() {
        let args = vec!["-d".to_string(), "Ghost".to_string()];
        assert_eq!(run_lp(&args), 1);
    }

    #[test]
    fn test_lp_with_options() {
        let args = vec![
            "-d".to_string(),
            "HP_LaserJet".to_string(),
            "-n".to_string(),
            "3".to_string(),
            "-o".to_string(),
            "media=a4".to_string(),
            "-o".to_string(),
            "sides=two-sided-long-edge".to_string(),
            "doc.pdf".to_string(),
        ];
        assert_eq!(run_lp(&args), 0);
    }

    #[test]
    fn test_lpstat_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_default() {
        let args = vec!["-d".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_printers() {
        let args = vec!["-p".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_printers_long() {
        let args = vec!["-p".to_string(), "-l".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_all() {
        let args = vec!["-t".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_devices() {
        let args = vec!["-v".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_jobs() {
        let args = vec!["-o".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpstat_scheduler() {
        let args = vec!["-r".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpadmin_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_create_printer() {
        let args = vec![
            "-p".to_string(),
            "NewPrinter".to_string(),
            "-v".to_string(),
            "ipp://new".to_string(),
            "-D".to_string(),
            "New Printer".to_string(),
            "-L".to_string(),
            "Room 42".to_string(),
        ];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_modify_printer() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "-D".to_string(),
            "Updated Description".to_string(),
        ];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_delete_printer() {
        let args = vec!["-x".to_string(), "PDF_Printer".to_string()];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_delete_nonexistent() {
        let args = vec!["-x".to_string(), "Ghost".to_string()];
        assert_eq!(run_lpadmin(&args), 1);
    }

    #[test]
    fn test_lpadmin_set_default() {
        let args = vec!["-d".to_string(), "Epson_Inkjet".to_string()];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_lpadmin_create_no_uri() {
        let args = vec!["-p".to_string(), "NewPrinter".to_string()];
        assert_eq!(run_lpadmin(&args), 1);
    }

    #[test]
    fn test_cancel_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_cancel(&args), 0);
    }

    #[test]
    fn test_cancel_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_cancel(&args), 0);
    }

    #[test]
    fn test_cancel_all() {
        let args = vec!["-a".to_string()];
        assert_eq!(run_cancel(&args), 0);
    }

    #[test]
    fn test_cancel_specific_job() {
        let args = vec!["HP_LaserJet-2".to_string()];
        assert_eq!(run_cancel(&args), 0);
    }

    #[test]
    fn test_cancel_no_args() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_cancel(&args), 1);
    }

    #[test]
    fn test_cupsaccept() {
        let args = vec!["HP_LaserJet".to_string()];
        assert_eq!(run_cupsaccept(&args), 0);
    }

    #[test]
    fn test_cupsreject() {
        let args = vec!["HP_LaserJet".to_string()];
        assert_eq!(run_cupsreject(&args), 0);
    }

    #[test]
    fn test_cupsaccept_with_reason() {
        let args = vec![
            "-r".to_string(),
            "Maintenance complete".to_string(),
            "HP_LaserJet".to_string(),
        ];
        assert_eq!(run_cupsaccept(&args), 0);
    }

    #[test]
    fn test_cupsreject_nonexistent() {
        let args = vec!["Ghost".to_string()];
        assert_eq!(run_cupsreject(&args), 1);
    }

    #[test]
    fn test_cupsaccept_no_args() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_cupsaccept(&args), 1);
    }

    #[test]
    fn test_cupsenable() {
        let args = vec!["HP_LaserJet".to_string()];
        assert_eq!(run_cupsenable(&args), 0);
    }

    #[test]
    fn test_cupsdisable() {
        let args = vec!["HP_LaserJet".to_string()];
        assert_eq!(run_cupsdisable(&args), 0);
    }

    #[test]
    fn test_cupsdisable_with_reason() {
        let args = vec![
            "-r".to_string(),
            "Paper jam".to_string(),
            "HP_LaserJet".to_string(),
        ];
        assert_eq!(run_cupsdisable(&args), 0);
    }

    #[test]
    fn test_cupsenable_nonexistent() {
        let args = vec!["Ghost".to_string()];
        assert_eq!(run_cupsenable(&args), 1);
    }

    #[test]
    fn test_cupsdisable_no_args() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_cupsdisable(&args), 1);
    }

    #[test]
    fn test_lpinfo_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_devices() {
        let args = vec!["-v".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_drivers() {
        let args = vec!["-m".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_devices_long() {
        let args = vec!["-v".to_string(), "-l".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_drivers_filter() {
        let args = vec![
            "-m".to_string(),
            "--make-and-model".to_string(),
            "hp".to_string(),
        ];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_devices_scheme() {
        let args = vec!["-v".to_string(), "--scheme".to_string(), "ipp".to_string()];
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpinfo_no_args() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_lpinfo(&args), 0);
    }

    #[test]
    fn test_lpoptions_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_list() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "-l".to_string(),
        ];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_set() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "-o".to_string(),
            "media=a4".to_string(),
        ];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_remove() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "-r".to_string(),
            "media".to_string(),
        ];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_default() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_long() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "--long".to_string(),
        ];
        assert_eq!(run_lpoptions(&args), 0);
    }

    #[test]
    fn test_lpoptions_nonexistent_printer() {
        let args = vec!["-p".to_string(), "Ghost".to_string(), "-l".to_string()];
        assert_eq!(run_lpoptions(&args), 1);
    }

    #[test]
    fn test_cupstestppd_help() {
        let args = vec!["--help".to_string()];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_version() {
        let args = vec!["-V".to_string()];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_good() {
        let args = vec!["good_printer.ppd".to_string()];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_bad() {
        let args = vec!["bad_printer.ppd".to_string()];
        assert_eq!(run_cupstestppd(&args), 1);
    }

    #[test]
    fn test_cupstestppd_warn() {
        let args = vec!["warn_printer.ppd".to_string()];
        assert_eq!(run_cupstestppd(&args), 0); // warnings pass by default
    }

    #[test]
    fn test_cupstestppd_warn_strict() {
        let args = vec![
            "-W".to_string(),
            "strict".to_string(),
            "warn_printer.ppd".to_string(),
        ];
        assert_eq!(run_cupstestppd(&args), 1); // warnings fail in strict
    }

    #[test]
    fn test_cupstestppd_bad_relaxed() {
        let args = vec![
            "-W".to_string(),
            "relaxed".to_string(),
            "bad_printer.ppd".to_string(),
        ];
        assert_eq!(run_cupstestppd(&args), 0); // relaxed always passes
    }

    #[test]
    fn test_cupstestppd_verbose() {
        let args = vec!["-v".to_string(), "good_printer.ppd".to_string()];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_quiet() {
        let args = vec!["-q".to_string(), "good_printer.ppd".to_string()];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_no_args() {
        let args: Vec<String> = Vec::new();
        assert_eq!(run_cupstestppd(&args), 1);
    }

    #[test]
    fn test_cupstestppd_multiple_files() {
        let args = vec![
            "good_printer.ppd".to_string(),
            "valid_printer.ppd".to_string(),
        ];
        assert_eq!(run_cupstestppd(&args), 0);
    }

    #[test]
    fn test_cupstestppd_mixed_pass_fail() {
        let args = vec![
            "good_printer.ppd".to_string(),
            "bad_printer.ppd".to_string(),
        ];
        assert_eq!(run_cupstestppd(&args), 1);
    }

    // --- Personality detection (simulated) ---

    #[test]
    fn test_personality_detection_unix_path() {
        let s = "/usr/bin/lp";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        assert_eq!(base, "lp");
    }

    #[test]
    fn test_personality_detection_windows_path() {
        let s = "C:\\Program Files\\cups\\cupsd.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "cupsd");
    }

    #[test]
    fn test_personality_detection_bare_name() {
        let s = "lpstat";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        assert_eq!(base, "lpstat");
    }

    #[test]
    fn test_personality_detection_exe_suffix() {
        let s = "lpadmin.exe";
        let base = s.strip_suffix(".exe").unwrap_or(s);
        assert_eq!(base, "lpadmin");
    }

    #[test]
    fn test_personality_detection_no_exe() {
        let s = "cancel";
        let base = s.strip_suffix(".exe").unwrap_or(s);
        assert_eq!(base, "cancel");
    }

    // --- Edge case tests ---

    #[test]
    fn test_default_constants() {
        assert_eq!(DEFAULT_PORT, 631);
        assert_eq!(DEFAULT_COPIES, 1);
        assert_eq!(DEFAULT_PRIORITY, 50);
        assert_eq!(MAX_PRIORITY, 100);
        assert_eq!(DEFAULT_PAGE_RANGE, "all");
        assert_eq!(_DEFAULT_MEDIA, "letter");
        assert_eq!(_DEFAULT_ORIENTATION, "portrait");
        assert_eq!(_DEFAULT_SIDES, "one-sided");
        assert_eq!(_DEFAULT_QUALITY, "normal");
        assert_eq!(MAX_JOB_ID, 999_999);
    }

    #[test]
    fn test_cups_error_clone() {
        let e1 = CupsError::new("test");
        let e2 = e1.clone();
        assert_eq!(format!("{e1}"), format!("{e2}"));
    }

    #[test]
    fn test_printer_display() {
        let p = Printer::new("TestPrinter", "ipp://test", "drv");
        let s = format!("{p}");
        assert!(s.contains("TestPrinter"));
        assert!(s.contains("idle"));
    }

    #[test]
    fn test_ppd_file_color_settings() {
        let ppd = PpdFile::new("test.ppd");
        assert!(ppd.color_device);
        assert_eq!(ppd._default_color_space, "RGB");
    }

    #[test]
    fn test_ppd_file_format_version() {
        let ppd = PpdFile::new("test.ppd");
        assert_eq!(ppd.format_version, "4.3");
        assert_eq!(ppd._language_version, "English");
    }

    #[test]
    fn test_lp_invalid_copies() {
        let args = vec![
            "-d".to_string(),
            "HP_LaserJet".to_string(),
            "-n".to_string(),
            "0".to_string(),
        ];
        assert_eq!(run_lp(&args), 1);
    }

    #[test]
    fn test_lp_invalid_priority() {
        let args = vec![
            "-d".to_string(),
            "HP_LaserJet".to_string(),
            "-q".to_string(),
            "200".to_string(),
        ];
        assert_eq!(run_lp(&args), 1);
    }

    #[test]
    fn test_lpstat_completed_jobs() {
        let args = vec!["-W".to_string(), "completed".to_string()];
        assert_eq!(run_lpstat(&args), 0);
    }

    #[test]
    fn test_lpadmin_enable() {
        let args = vec![
            "-p".to_string(),
            "HP_LaserJet".to_string(),
            "-E".to_string(),
        ];
        assert_eq!(run_lpadmin(&args), 0);
    }

    #[test]
    fn test_cupsdisable_hold() {
        let args = vec!["--hold".to_string(), "HP_LaserJet".to_string()];
        assert_eq!(run_cupsdisable(&args), 0);
    }
}
