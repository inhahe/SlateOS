//! Language, region, and locale settings panel for the desktop shell.
//!
//! Configures system language, date/time formats, number formats,
//! currency display, measurement units, and first day of week.

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
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Language
// ============================================================================

/// A system language with display and native names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Language {
    /// BCP 47 language tag (e.g. "en-US").
    pub tag: String,
    /// English display name (e.g. "English (United States)").
    pub display_name: String,
    /// Native name (e.g. "English").
    pub native_name: String,
    /// Whether this language has full translation coverage.
    pub complete: bool,
}

impl Language {
    pub fn new(
        tag: impl Into<String>,
        display_name: impl Into<String>,
        native_name: impl Into<String>,
        complete: bool,
    ) -> Self {
        Self {
            tag: tag.into(),
            display_name: display_name.into(),
            native_name: native_name.into(),
            complete,
        }
    }
}

/// Default available languages.
pub fn default_languages() -> Vec<Language> {
    vec![
        Language::new("en-US", "English (United States)", "English", true),
        Language::new("en-GB", "English (United Kingdom)", "English", true),
        Language::new("de-DE", "German (Germany)", "Deutsch", true),
        Language::new("fr-FR", "French (France)", "Fran\u{00e7}ais", true),
        Language::new("es-ES", "Spanish (Spain)", "Espa\u{00f1}ol", true),
        Language::new("it-IT", "Italian (Italy)", "Italiano", true),
        Language::new("pt-BR", "Portuguese (Brazil)", "Portugu\u{00ea}s", true),
        Language::new("nl-NL", "Dutch (Netherlands)", "Nederlands", true),
        Language::new("ja-JP", "Japanese (Japan)", "\u{65e5}\u{672c}\u{8a9e}", true),
        Language::new("ko-KR", "Korean (Korea)", "\u{d55c}\u{ad6d}\u{c5b4}", true),
        Language::new("zh-CN", "Chinese (Simplified)", "\u{7b80}\u{4f53}\u{4e2d}\u{6587}", true),
        Language::new("zh-TW", "Chinese (Traditional)", "\u{7e41}\u{9ad4}\u{4e2d}\u{6587}", true),
        Language::new("ru-RU", "Russian (Russia)", "\u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}", true),
        Language::new("pl-PL", "Polish (Poland)", "Polski", false),
        Language::new("sv-SE", "Swedish (Sweden)", "Svenska", false),
        Language::new("da-DK", "Danish (Denmark)", "Dansk", false),
        Language::new("fi-FI", "Finnish (Finland)", "Suomi", false),
        Language::new("nb-NO", "Norwegian (Norway)", "Norsk", false),
        Language::new("tr-TR", "Turkish (Turkey)", "T\u{00fc}rk\u{00e7}e", false),
        Language::new("ar-SA", "Arabic (Saudi Arabia)", "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}", false),
    ]
}

// ============================================================================
// Date/time format
// ============================================================================

/// Date format style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFormat {
    /// MM/DD/YYYY (US).
    Mdy,
    /// DD/MM/YYYY (most of world).
    Dmy,
    /// YYYY-MM-DD (ISO 8601).
    Ymd,
    /// DD.MM.YYYY (German/Swiss).
    DmyDot,
}

impl DateFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Mdy => "MM/DD/YYYY",
            Self::Dmy => "DD/MM/YYYY",
            Self::Ymd => "YYYY-MM-DD",
            Self::DmyDot => "DD.MM.YYYY",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::Mdy => "05/18/2026",
            Self::Dmy => "18/05/2026",
            Self::Ymd => "2026-05-18",
            Self::DmyDot => "18.05.2026",
        }
    }
}

/// Time format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeFormat {
    /// 12-hour (1:30 PM).
    TwelveHour,
    /// 24-hour (13:30).
    TwentyFourHour,
}

impl TimeFormat {
    fn label(self) -> &'static str {
        match self {
            Self::TwelveHour => "12-hour (1:30 PM)",
            Self::TwentyFourHour => "24-hour (13:30)",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::TwelveHour => "2:45 PM",
            Self::TwentyFourHour => "14:45",
        }
    }
}

/// First day of the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstDayOfWeek {
    Sunday,
    Monday,
    Saturday,
}

impl FirstDayOfWeek {
    fn label(self) -> &'static str {
        match self {
            Self::Sunday => "Sunday",
            Self::Monday => "Monday",
            Self::Saturday => "Saturday",
        }
    }
}

// ============================================================================
// Number format
// ============================================================================

/// Decimal separator style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimalSeparator {
    /// Period (1,234.56).
    Period,
    /// Comma (1.234,56).
    Comma,
}

impl DecimalSeparator {
    fn label(self) -> &'static str {
        match self {
            Self::Period => ". (period)",
            Self::Comma => ", (comma)",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::Period => "1,234.56",
            Self::Comma => "1.234,56",
        }
    }
}

/// Measurement system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementSystem {
    Metric,
    Imperial,
}

impl MeasurementSystem {
    fn label(self) -> &'static str {
        match self {
            Self::Metric => "Metric (kg, km, \u{00b0}C)",
            Self::Imperial => "Imperial (lb, mi, \u{00b0}F)",
        }
    }
}

/// Temperature unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl TemperatureUnit {
    fn label(self) -> &'static str {
        match self {
            Self::Celsius => "\u{00b0}C (Celsius)",
            Self::Fahrenheit => "\u{00b0}F (Fahrenheit)",
            Self::Kelvin => "K (Kelvin)",
        }
    }
}

// ============================================================================
// Currency
// ============================================================================

/// Currency display format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrencyFormat {
    /// Currency code (e.g. "USD").
    pub code: String,
    /// Symbol (e.g. "$").
    pub symbol: String,
    /// Whether symbol comes before the amount.
    pub symbol_before: bool,
    /// Decimal places (typically 2, but JPY/KRW use 0).
    pub decimal_places: u8,
}

impl CurrencyFormat {
    pub fn new(
        code: impl Into<String>,
        symbol: impl Into<String>,
        symbol_before: bool,
        decimal_places: u8,
    ) -> Self {
        Self {
            code: code.into(),
            symbol: symbol.into(),
            symbol_before,
            decimal_places,
        }
    }

    /// Format a value using this currency.
    pub fn format_value(&self, value: f64) -> String {
        let formatted = if self.decimal_places == 0 {
            format!("{}", value as i64)
        } else {
            format!("{:.prec$}", value, prec = self.decimal_places as usize)
        };
        if self.symbol_before {
            format!("{}{}", self.symbol, formatted)
        } else {
            format!("{} {}", formatted, self.symbol)
        }
    }
}

/// Default currencies.
pub fn default_currencies() -> Vec<CurrencyFormat> {
    vec![
        CurrencyFormat::new("USD", "$", true, 2),
        CurrencyFormat::new("EUR", "\u{20ac}", true, 2),
        CurrencyFormat::new("GBP", "\u{00a3}", true, 2),
        CurrencyFormat::new("JPY", "\u{00a5}", true, 0),
        CurrencyFormat::new("CNY", "\u{00a5}", true, 2),
        CurrencyFormat::new("KRW", "\u{20a9}", true, 0),
        CurrencyFormat::new("INR", "\u{20b9}", true, 2),
        CurrencyFormat::new("BRL", "R$", true, 2),
        CurrencyFormat::new("CAD", "C$", true, 2),
        CurrencyFormat::new("AUD", "A$", true, 2),
        CurrencyFormat::new("CHF", "CHF", false, 2),
        CurrencyFormat::new("SEK", "kr", false, 2),
    ]
}

// ============================================================================
// Locale settings aggregate
// ============================================================================

/// All regional/locale settings.
#[derive(Clone, Debug)]
pub struct LocaleSettings {
    /// Primary system language tag.
    pub language: String,
    /// Fallback language tag (used when primary translation is missing).
    pub fallback_language: String,
    /// Date display format.
    pub date_format: DateFormat,
    /// Time display format.
    pub time_format: TimeFormat,
    /// First day of the week.
    pub first_day: FirstDayOfWeek,
    /// Decimal separator.
    pub decimal_separator: DecimalSeparator,
    /// Measurement system.
    pub measurement: MeasurementSystem,
    /// Temperature display unit.
    pub temperature: TemperatureUnit,
    /// Currency code for default currency display.
    pub currency_code: String,
    /// Available languages installed on the system.
    pub available_languages: Vec<Language>,
    /// Available currency formats.
    pub available_currencies: Vec<CurrencyFormat>,
}

impl Default for LocaleSettings {
    fn default() -> Self {
        Self {
            language: "en-US".to_string(),
            fallback_language: "en-US".to_string(),
            date_format: DateFormat::Mdy,
            time_format: TimeFormat::TwelveHour,
            first_day: FirstDayOfWeek::Sunday,
            decimal_separator: DecimalSeparator::Period,
            measurement: MeasurementSystem::Metric,
            temperature: TemperatureUnit::Celsius,
            currency_code: "USD".to_string(),
            available_languages: default_languages(),
            available_currencies: default_currencies(),
        }
    }
}

impl LocaleSettings {
    /// Get the current primary language info.
    pub fn current_language(&self) -> Option<&Language> {
        self.available_languages.iter().find(|l| l.tag == self.language)
    }

    /// Get the currency format for the current currency code.
    pub fn current_currency(&self) -> Option<&CurrencyFormat> {
        self.available_currencies.iter().find(|c| c.code == self.currency_code)
    }

    /// Set the primary language (validates against available list).
    pub fn set_language(&mut self, tag: &str) -> bool {
        if self.available_languages.iter().any(|l| l.tag == tag) {
            self.language = tag.to_string();
            true
        } else {
            false
        }
    }

    /// Set the currency code (validates against available list).
    pub fn set_currency(&mut self, code: &str) -> bool {
        if self.available_currencies.iter().any(|c| c.code == code) {
            self.currency_code = code.to_string();
            true
        } else {
            false
        }
    }

    /// Format a date example string using the current format.
    pub fn date_example(&self) -> &str {
        self.date_format.example()
    }

    /// Format a time example string using the current format.
    pub fn time_example(&self) -> &str {
        self.time_format.example()
    }

    /// Format a number example string using the current decimal separator.
    pub fn number_example(&self) -> &str {
        self.decimal_separator.example()
    }

    /// Format a currency example value.
    pub fn currency_example(&self) -> String {
        match self.current_currency() {
            Some(c) => c.format_value(1234.56),
            None => "1234.56".to_string(),
        }
    }

    /// Search available languages by name or tag.
    pub fn search_languages(&self, query: &str) -> Vec<&Language> {
        let q = query.to_lowercase();
        self.available_languages.iter()
            .filter(|l| {
                l.tag.to_lowercase().contains(&q)
                    || l.display_name.to_lowercase().contains(&q)
                    || l.native_name.to_lowercase().contains(&q)
            })
            .collect()
    }
}

// ============================================================================
// UI: Language settings panel
// ============================================================================

/// Active tab in the language settings UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageTab {
    /// Language selection.
    Language,
    /// Date, time, numbers.
    Formats,
    /// Region-specific settings (measurement, temperature, currency).
    Region,
}

impl LanguageTab {
    fn label(self) -> &'static str {
        match self {
            Self::Language => "Language",
            Self::Formats => "Formats",
            Self::Region => "Region",
        }
    }
}

/// Language settings UI state.
pub struct LanguageSettingsUI {
    /// Active tab.
    pub active_tab: LanguageTab,
    /// The underlying settings.
    pub settings: LocaleSettings,
    /// Language search text.
    pub language_search: String,
    /// Currently highlighted language index in the filtered list.
    pub selected_language_index: Option<usize>,
}

impl LanguageSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: LanguageTab::Language,
            settings: LocaleSettings::default(),
            language_search: String::new(),
            selected_language_index: None,
        }
    }

    /// Set active tab.
    pub fn set_tab(&mut self, tab: LanguageTab) {
        self.active_tab = tab;
    }

    /// Get filtered languages based on search.
    fn filtered_languages(&self) -> Vec<&Language> {
        if self.language_search.is_empty() {
            self.settings.available_languages.iter().collect()
        } else {
            self.settings.search_languages(&self.language_search)
        }
    }

    /// Render the language settings panel.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Language & Region".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
        });

        // Tabs
        let tabs = [LanguageTab::Language, LanguageTab::Formats, LanguageTab::Region];
        let tab_y = 60.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = tab.label().len() as f32 * 8.0 + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tw - 20.0),
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            LanguageTab::Language => self.render_language_tab(&mut cmds, 24.0, cy, cw),
            LanguageTab::Formats => self.render_formats_tab(&mut cmds, 24.0, cy, cw),
            LanguageTab::Region => self.render_region_tab(&mut cmds, 24.0, cy, cw),
        }

        cmds
    }

    fn render_language_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Current language
        if let Some(lang) = self.settings.current_language() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 50.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: format!("Current: {}", lang.display_name),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 28.0,
                text: format!("{} ({})", lang.native_name, lang.tag),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });
            cy += 60.0;
        }

        // Search box
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.language_search.is_empty() {
            "Search languages...".to_string()
        } else {
            self.language_search.clone()
        };
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: cy + 7.0,
            text: search_text,
            font_size: 13.0,
            color: if self.language_search.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
        });
        cy += 40.0;

        // Language list
        let filtered = self.filtered_languages();
        for (i, lang) in filtered.iter().enumerate().take(12) {
            let is_selected = self.selected_language_index == Some(i);
            let is_current = lang.tag == self.settings.language;

            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 40.0,
                color: if is_selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });

            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy + 4.0,
                    width: 4.0,
                    height: 32.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 4.0,
                text: lang.display_name.clone(),
                font_size: 13.0,
                color: if is_current { BLUE } else { TEXT },
                font_weight: if is_current { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(width * 0.6),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 22.0,
                text: lang.native_name.clone(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
            });

            // Completeness badge
            if !lang.complete {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 70.0,
                    y: cy + 12.0,
                    width: 56.0,
                    height: 18.0,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 64.0,
                    y: cy + 14.0,
                    text: "Partial".into(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(48.0),
                });
            }

            cy += 44.0;
        }

        // Count
        cmds.push(RenderCommand::Text {
            x,
            y: cy + 4.0,
            text: format!("{} languages available", filtered.len()),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
    }

    fn render_formats_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Date format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Date Format".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Format", self.settings.date_format.label());
        cy += 24.0;
        self.render_label_value(cmds, x, cy, width, "Example", self.settings.date_example());
        cy += 36.0;

        // Time format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Time Format".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Format", self.settings.time_format.label());
        cy += 24.0;
        self.render_label_value(cmds, x, cy, width, "Example", self.settings.time_example());
        cy += 36.0;

        // First day of week
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Calendar".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "First Day", self.settings.first_day.label());
        cy += 36.0;

        // Number format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Number Format".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Decimal", self.settings.decimal_separator.label());
        cy += 24.0;
        self.render_label_value(cmds, x, cy, width, "Example", self.settings.number_example());
        let _ = cy;
    }

    fn render_region_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Measurement
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Measurement".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "System", self.settings.measurement.label());
        cy += 36.0;

        // Temperature
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Temperature".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Unit", self.settings.temperature.label());
        cy += 36.0;

        // Currency
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Currency".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Currency", &self.settings.currency_code);
        cy += 24.0;

        let currency_example = self.settings.currency_example();
        self.render_label_value(cmds, x, cy, width, "Example", &currency_example);
        cy += 36.0;

        // Available currencies list (first 6)
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Available Currencies".into(),
            font_size: 13.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 22.0;

        for cur in self.settings.available_currencies.iter().take(6) {
            let is_current = cur.code == self.settings.currency_code;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 28.0,
                color: if is_current { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: format!("{} {} ({})", cur.symbol, cur.code, cur.format_value(1234.56)),
                font_size: 12.0,
                color: if is_current { BLUE } else { TEXT },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
            });
            cy += 32.0;
        }
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.45,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Language ----

    #[test]
    fn test_default_languages_count() {
        let langs = default_languages();
        assert_eq!(langs.len(), 20);
    }

    #[test]
    fn test_language_completeness() {
        let langs = default_languages();
        let complete = langs.iter().filter(|l| l.complete).count();
        assert!(complete > 10);
    }

    // ---- DateFormat ----

    #[test]
    fn test_date_format_labels() {
        assert_eq!(DateFormat::Mdy.label(), "MM/DD/YYYY");
        assert_eq!(DateFormat::Ymd.label(), "YYYY-MM-DD");
    }

    #[test]
    fn test_date_format_examples() {
        assert!(DateFormat::Mdy.example().contains('/'));
        assert!(DateFormat::Ymd.example().contains('-'));
        assert!(DateFormat::DmyDot.example().contains('.'));
    }

    // ---- TimeFormat ----

    #[test]
    fn test_time_format_labels() {
        assert!(TimeFormat::TwelveHour.label().contains("12"));
        assert!(TimeFormat::TwentyFourHour.label().contains("24"));
    }

    #[test]
    fn test_time_format_examples() {
        assert!(TimeFormat::TwelveHour.example().contains("PM"));
        assert!(!TimeFormat::TwentyFourHour.example().contains("PM"));
    }

    // ---- FirstDayOfWeek ----

    #[test]
    fn test_first_day_labels() {
        assert_eq!(FirstDayOfWeek::Sunday.label(), "Sunday");
        assert_eq!(FirstDayOfWeek::Monday.label(), "Monday");
    }

    // ---- DecimalSeparator ----

    #[test]
    fn test_decimal_separator_examples() {
        assert!(DecimalSeparator::Period.example().contains('.'));
        assert!(DecimalSeparator::Comma.example().contains(','));
    }

    // ---- MeasurementSystem ----

    #[test]
    fn test_measurement_labels() {
        assert!(MeasurementSystem::Metric.label().contains("kg"));
        assert!(MeasurementSystem::Imperial.label().contains("lb"));
    }

    // ---- TemperatureUnit ----

    #[test]
    fn test_temperature_labels() {
        assert!(TemperatureUnit::Celsius.label().contains('C'));
        assert!(TemperatureUnit::Fahrenheit.label().contains('F'));
        assert!(TemperatureUnit::Kelvin.label().contains('K'));
    }

    // ---- CurrencyFormat ----

    #[test]
    fn test_currency_format_before() {
        let usd = CurrencyFormat::new("USD", "$", true, 2);
        assert_eq!(usd.format_value(1234.56), "$1234.56");
    }

    #[test]
    fn test_currency_format_after() {
        let chf = CurrencyFormat::new("CHF", "CHF", false, 2);
        assert_eq!(chf.format_value(1234.56), "1234.56 CHF");
    }

    #[test]
    fn test_currency_format_zero_decimals() {
        let jpy = CurrencyFormat::new("JPY", "\u{00a5}", true, 0);
        assert_eq!(jpy.format_value(1234.0), "\u{00a5}1234");
    }

    #[test]
    fn test_default_currencies_count() {
        assert_eq!(default_currencies().len(), 12);
    }

    // ---- LocaleSettings ----

    #[test]
    fn test_locale_defaults() {
        let s = LocaleSettings::default();
        assert_eq!(s.language, "en-US");
        assert_eq!(s.date_format, DateFormat::Mdy);
        assert_eq!(s.time_format, TimeFormat::TwelveHour);
    }

    #[test]
    fn test_current_language() {
        let s = LocaleSettings::default();
        let lang = s.current_language().unwrap();
        assert_eq!(lang.tag, "en-US");
    }

    #[test]
    fn test_current_currency() {
        let s = LocaleSettings::default();
        let cur = s.current_currency().unwrap();
        assert_eq!(cur.code, "USD");
    }

    #[test]
    fn test_set_language_valid() {
        let mut s = LocaleSettings::default();
        assert!(s.set_language("de-DE"));
        assert_eq!(s.language, "de-DE");
    }

    #[test]
    fn test_set_language_invalid() {
        let mut s = LocaleSettings::default();
        assert!(!s.set_language("xx-XX"));
        assert_eq!(s.language, "en-US");
    }

    #[test]
    fn test_set_currency_valid() {
        let mut s = LocaleSettings::default();
        assert!(s.set_currency("EUR"));
        assert_eq!(s.currency_code, "EUR");
    }

    #[test]
    fn test_set_currency_invalid() {
        let mut s = LocaleSettings::default();
        assert!(!s.set_currency("ZZZ"));
        assert_eq!(s.currency_code, "USD");
    }

    #[test]
    fn test_date_example() {
        let s = LocaleSettings::default();
        assert!(!s.date_example().is_empty());
    }

    #[test]
    fn test_currency_example() {
        let s = LocaleSettings::default();
        let ex = s.currency_example();
        assert!(ex.contains('$'));
    }

    #[test]
    fn test_search_languages() {
        let s = LocaleSettings::default();
        let results = s.search_languages("deutsch");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tag, "de-DE");
    }

    #[test]
    fn test_search_languages_by_tag() {
        let s = LocaleSettings::default();
        let results = s.search_languages("en-");
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_search_languages_empty() {
        let s = LocaleSettings::default();
        let results = s.search_languages("xyzzy");
        assert!(results.is_empty());
    }

    // ---- LanguageSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = LanguageSettingsUI::new();
        assert_eq!(ui.active_tab, LanguageTab::Language);
        assert!(ui.language_search.is_empty());
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = LanguageSettingsUI::new();
        ui.set_tab(LanguageTab::Region);
        assert_eq!(ui.active_tab, LanguageTab::Region);
    }

    #[test]
    fn test_ui_filtered_all() {
        let ui = LanguageSettingsUI::new();
        let filtered = ui.filtered_languages();
        assert_eq!(filtered.len(), 20);
    }

    #[test]
    fn test_ui_filtered_search() {
        let mut ui = LanguageSettingsUI::new();
        ui.language_search = "spanish".to_string();
        let filtered = ui.filtered_languages();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_ui_render_language_tab() {
        let ui = LanguageSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_formats_tab() {
        let mut ui = LanguageSettingsUI::new();
        ui.set_tab(LanguageTab::Formats);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_region_tab() {
        let mut ui = LanguageSettingsUI::new();
        ui.set_tab(LanguageTab::Region);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(LanguageTab::Language.label(), "Language");
        assert_eq!(LanguageTab::Formats.label(), "Formats");
        assert_eq!(LanguageTab::Region.label(), "Region");
    }
}
