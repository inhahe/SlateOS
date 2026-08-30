//! Language, region, and locale settings panel for the desktop shell.
//!
//! Configures system language, date/time formats, number formats,
//! currency display, measurement units, and first day of week.
//!
//! # Colour
//!
//! Every colour comes from the [`Palette`] the caller resolved; this module
//! names no value of its own. Five judgements decide which role each site
//! takes, and each is pinned by a test because none of them is recoverable
//! from the code alone.
//!
//! 1. **The accent marks position, and position only.** Three *axes* here have
//!    a "this is the one in force" mark, and each takes `p.accent`: which tab
//!    is open, which language is the system language (marked twice — a bar
//!    beside the row and the row's own name, which is one mark drawn in two
//!    places), and which currency is the default. Nothing else does. Note
//!    this module cannot borrow the taskbar's "exactly one thing carries the
//!    accent" test: the axes are independent, so more than one accented mark
//!    is legitimately on screen at once. The check therefore has to be a
//!    per-site table plus a per-tab count — three on the Language tab, one on
//!    Formats (its tab and nothing else), two on Region.
//! 2. **The "Partial" badge is a *property of the data*, not a position.** It
//!    reports that a translation is incomplete, which is true regardless of
//!    what the user has selected or what accent they chose, so it keeps
//!    `p.yellow` and must never follow the accent.
//! 3. **Ink on a colour this module filled is derived, never named.** The
//!    active tab's label is `readable_on(p.accent)` and the badge's is
//!    `readable_on(p.yellow)`. Both are provably derived rather than frozen,
//!    because the two modes disagree about them.
//! 4. **Headings are a two-rung hierarchy.** A section heading is
//!    `p.lavender` at 15pt Bold — the convention already established by
//!    `datetime_settings` and `notification_settings` — and a sub-heading
//!    inside a section is `p.subtext1` at 13pt Bold. Lavender here is
//!    structure, not decoration, and specifically is *not* the accent: a
//!    heading does not move when the selection does.
//! 5. **An absent value is dimmer than a present one.** The search box's
//!    placeholder is `p.overlay0` and text the user actually typed is
//!    `p.text`, so "Search languages…" cannot be mistaken for a query.

use appearance::{Palette, readable_on};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

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
        Language::new(
            "ja-JP",
            "Japanese (Japan)",
            "\u{65e5}\u{672c}\u{8a9e}",
            true,
        ),
        Language::new("ko-KR", "Korean (Korea)", "\u{d55c}\u{ad6d}\u{c5b4}", true),
        Language::new(
            "zh-CN",
            "Chinese (Simplified)",
            "\u{7b80}\u{4f53}\u{4e2d}\u{6587}",
            true,
        ),
        Language::new(
            "zh-TW",
            "Chinese (Traditional)",
            "\u{7e41}\u{9ad4}\u{4e2d}\u{6587}",
            true,
        ),
        Language::new(
            "ru-RU",
            "Russian (Russia)",
            "\u{0420}\u{0443}\u{0441}\u{0441}\u{043a}\u{0438}\u{0439}",
            true,
        ),
        Language::new("pl-PL", "Polish (Poland)", "Polski", false),
        Language::new("sv-SE", "Swedish (Sweden)", "Svenska", false),
        Language::new("da-DK", "Danish (Denmark)", "Dansk", false),
        Language::new("fi-FI", "Finnish (Finland)", "Suomi", false),
        Language::new("nb-NO", "Norwegian (Norway)", "Norsk", false),
        Language::new("tr-TR", "Turkish (Turkey)", "T\u{00fc}rk\u{00e7}e", false),
        Language::new(
            "ar-SA",
            "Arabic (Saudi Arabia)",
            "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}",
            false,
        ),
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
        self.available_languages
            .iter()
            .find(|l| l.tag == self.language)
    }

    /// Get the currency format for the current currency code.
    pub fn current_currency(&self) -> Option<&CurrencyFormat> {
        self.available_currencies
            .iter()
            .find(|c| c.code == self.currency_code)
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
        self.available_languages
            .iter()
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
    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Language & Region".into(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tabs
        let tabs = [
            LanguageTab::Language,
            LanguageTab::Formats,
            LanguageTab::Region,
        ];
        let tab_y = 60.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = text::padded_width_any_weight(tab.label(), 10.0, 13.0);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 32.0,
                color: if active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active {
                    readable_on(p.accent)
                } else {
                    p.subtext0
                },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tw - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            LanguageTab::Language => self.render_language_tab(&mut cmds, p, 24.0, cy, cw),
            LanguageTab::Formats => self.render_formats_tab(&mut cmds, p, 24.0, cy, cw),
            LanguageTab::Region => self.render_region_tab(&mut cmds, p, 24.0, cy, cw),
        }

        cmds
    }

    fn render_language_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        // Current language
        if let Some(lang) = self.settings.current_language() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 50.0,
                color: p.surface1,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: format!("Current: {}", lang.display_name),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 28.0,
                text: format!("{} ({})", lang.native_name, lang.tag),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 60.0;
        }

        // Search box
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: p.surface0,
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
            color: if self.language_search.is_empty() {
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
            overflow: TextOverflow::Ellipsis,
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
                color: if is_selected { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });

            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy + 4.0,
                    width: 4.0,
                    height: 32.0,
                    color: p.accent,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 4.0,
                text: lang.display_name.clone(),
                font_size: 13.0,
                color: if is_current { p.accent } else { p.text },
                font_weight: if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width * 0.6),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 22.0,
                text: lang.native_name.clone(),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
                overflow: TextOverflow::Ellipsis,
            });

            // Completeness badge
            if !lang.complete {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 70.0,
                    y: cy + 12.0,
                    width: 56.0,
                    height: 18.0,
                    color: p.yellow,
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 64.0,
                    y: cy + 14.0,
                    text: "Partial".into(),
                    font_size: 10.0,
                    color: readable_on(p.yellow),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(48.0),
                    overflow: TextOverflow::Ellipsis,
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
            color: p.overlay0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_formats_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        // Date format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Date Format".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Format",
            self.settings.date_format.label(),
        );
        cy += 24.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Example",
            self.settings.date_example(),
        );
        cy += 36.0;

        // Time format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Time Format".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Format",
            self.settings.time_format.label(),
        );
        cy += 24.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Example",
            self.settings.time_example(),
        );
        cy += 36.0;

        // First day of week
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Calendar".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "First Day",
            self.settings.first_day.label(),
        );
        cy += 36.0;

        // Number format
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Number Format".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Decimal",
            self.settings.decimal_separator.label(),
        );
        cy += 24.0;
        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Example",
            self.settings.number_example(),
        );
        let _ = cy;
    }

    fn render_region_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        // Measurement
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Measurement".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "System",
            self.settings.measurement.label(),
        );
        cy += 36.0;

        // Temperature
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Temperature".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Unit",
            self.settings.temperature.label(),
        );
        cy += 36.0;

        // Currency
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Currency".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_label_value(
            cmds,
            p,
            x,
            cy,
            width,
            "Currency",
            &self.settings.currency_code,
        );
        cy += 24.0;

        let currency_example = self.settings.currency_example();
        self.render_label_value(cmds, p, x, cy, width, "Example", &currency_example);
        cy += 36.0;

        // Available currencies list (first 6)
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Available Currencies".into(),
            font_size: 13.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 22.0;

        for cur in self.settings.available_currencies.iter().take(6) {
            let is_current = cur.code == self.settings.currency_code;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 28.0,
                color: if is_current { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: format!(
                    "{} {} ({})",
                    cur.symbol,
                    cur.code,
                    cur.format_value(1234.56)
                ),
                font_size: 12.0,
                color: if is_current { p.accent } else { p.text },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 32.0;
        }
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.45,
            y,
            text: value.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

impl Default for LanguageSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // The colour tests locate a command by an exact size, y or font size — a
    // literal the renderer writes and the test reads back with no arithmetic
    // in between. Exact equality is the assertion meant; a tolerance would let
    // a 15pt section heading pass as a 13pt sub-heading, which is precisely
    // the distinction being pinned.
    #![allow(clippy::float_cmp)]

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
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_formats_tab() {
        let mut ui = LanguageSettingsUI::new();
        ui.set_tab(LanguageTab::Formats);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_region_tab() {
        let mut ui = LanguageSettingsUI::new();
        ui.set_tab(LanguageTab::Region);
        let cmds = ui.render(&accented(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // ---- Palette conversion ----
    //
    // See the module docs for the five judgements these pin.

    use guitk::color::Color;

    /// A palette whose accent is in no role, so "took the accent" and "took a
    /// role that happens to be the accent" cannot be confused.
    ///
    /// The stock accent *is* `blue`, which makes every accent assertion in
    /// this module pass against a site that named `p.blue` — the module-26
    /// trap. Nothing here may use `Palette::for_mode` directly.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        assert_eq!(
            p.roles()
                .iter()
                .filter(|(name, c)| *name != "accent" && *c == p.accent)
                .count(),
            0,
            "the probe accent collides with another role, so an assertion \
             that a site is *not* the accent could pass for the wrong reason"
        );
        p
    }

    /// A language list built here rather than taken from `default_languages`.
    ///
    /// The defaults cannot reach the "Partial" badge at all: every incomplete
    /// language sits at index 12 or beyond and the list renders `.take(12)`.
    /// A fixture that used them would leave the badge — and therefore its
    /// derived ink — untested while looking complete.
    fn three_languages() -> Vec<Language> {
        vec![
            Language::new("en-US", "English (United States)", "English", true),
            Language::new("pl-PL", "Polish (Poland)", "Polski", false),
            Language::new("de-DE", "German (Germany)", "Deutsch", true),
        ]
    }

    /// Every discriminator this module branches on, switched on at once.
    ///
    /// Current language is row 0, the *selected* row is row 2 (so "selected"
    /// and "current" cannot be confused for one another), row 1 is incomplete
    /// and so carries the badge, and the search box holds typed text rather
    /// than its placeholder.
    fn full_ui() -> LanguageSettingsUI {
        let mut ui = LanguageSettingsUI::new();
        ui.settings.available_languages = three_languages();
        ui.settings.language = "en-US".to_string();
        ui.selected_language_index = Some(2);
        // A query matching all three, so the list keeps every row: "n" is in
        // "English", "Poland" and "German". A query that filtered one out
        // would silently drop whichever branch that row carried — which is
        // how the first version of this fixture lost both the current-language
        // marker and the third row at once.
        ui.language_search = "n".to_string();
        ui
    }

    /// Every colour `cmds` puts on the screen.
    fn all_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the `Text` command whose content is exactly `text`.
    fn text_color(cmds: &[RenderCommand], want: &str) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == want => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "expected exactly one command drawing {want:?}, found {}",
            hits.len()
        );
        hits[0]
    }

    /// Every `FillRect` of exactly `w` x `h`, in draw order.
    fn fills_sized(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The three tab-header fills, selected by the strip's y.
    fn tab_strip(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { y, color, .. } if *y == 60.0 => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Render all three tabs, because one render only ever draws one of them.
    fn every_tab(p: &Palette) -> Vec<(LanguageTab, Vec<RenderCommand>)> {
        [
            LanguageTab::Language,
            LanguageTab::Formats,
            LanguageTab::Region,
        ]
        .into_iter()
        .map(|tab| {
            let mut u = full_ui();
            u.set_tab(tab);
            (tab, u.render(p, 600.0, 800.0))
        })
        .collect()
    }

    /// The whole point: no site draws a colour the palette cannot account for.
    ///
    /// Run in both modes and over all three tabs — a leftover Mocha constant
    /// is invisible in the dark render and names itself in the light one.
    #[test]
    fn every_colour_this_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for (tab, cmds) in every_tab(&p) {
                crate::palette_check::assert_drawn_from(
                    &p,
                    &cmds,
                    &[readable_on(p.accent), readable_on(p.yellow)],
                    &format!("language_settings {tab:?}"),
                );
            }
        }
    }

    /// A sweep is only as wide as the render it is handed.
    ///
    /// Each of these is a branch the fixture must keep reaching; if an edit
    /// makes one unreachable, this fails loudly instead of the sweep quietly
    /// checking less.
    #[test]
    fn the_fixture_reaches_every_branch_this_module_has() {
        let p = accented(false);
        let by_tab = every_tab(&p);
        let lang = &by_tab[0].1;

        // Both tab states: one active fill, two inactive.
        // Tab widths are derived from their label text, so the strip is
        // selected by its y. Not by height: the current-language marker bar
        // is also 32 tall, and matching on that alone counted four "tabs".
        let strip = tab_strip(lang);
        assert_eq!(strip.len(), 3, "three tabs are drawn");
        assert_eq!(
            strip.iter().filter(|c| **c == p.accent).count(),
            1,
            "exactly one tab is active"
        );

        // The current-language card, the search box, three list rows.
        assert_eq!(fills_sized(lang, 552.0, 50.0).len(), 1, "current card");
        assert_eq!(fills_sized(lang, 552.0, 30.0).len(), 1, "search box");
        assert_eq!(fills_sized(lang, 552.0, 40.0).len(), 3, "three list rows");
        // The current-language marker bar and the incomplete badge.
        assert_eq!(fills_sized(lang, 4.0, 32.0).len(), 1, "current marker");
        assert_eq!(fills_sized(lang, 56.0, 18.0).len(), 1, "partial badge");
        assert_eq!(text_color(lang, "Partial"), readable_on(p.yellow));
        // Typed search text, not the placeholder.
        assert!(
            lang.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "n"
            )),
            "the fixture's search box shows its placeholder, so the typed-text \
             branch is never rendered"
        );

        // The other two tabs draw their headings and their label/value rows.
        for (tab, cmds) in &by_tab[1..] {
            assert!(
                cmds.iter().any(|c| matches!(
                    c,
                    RenderCommand::Text { font_size, color, .. }
                        if *font_size == 15.0 && *color == p.lavender
                )),
                "{tab:?} draws no section heading"
            );
        }
        assert!(
            by_tab[2]
                .1
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { font_size, color, .. }
                    if *font_size == 13.0 && *color == p.subtext1)),
            "the Region tab draws no sub-heading"
        );
    }

    /// Every site, named one at a time, in the role this module claims for it.
    ///
    /// The sweep above proves only *membership*: a panel painted `surface2`
    /// instead of `base` is still "from the palette", and a card that swapped
    /// rungs with a list row still draws two legal colours. n source sites
    /// need n assertions, so this is that table. Every command is selected by
    /// a size, a y or a literal string the renderer writes — never by the
    /// colour under test, which would make the expectation a restatement of
    /// the code and unable to fail.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let by_tab = every_tab(&p);
            let lang = &by_tab[0].1;

            assert_eq!(fills_sized(lang, 600.0, 800.0), vec![p.base], "panel");
            assert_eq!(text_color(lang, "Language & Region"), p.text, "title");

            // The current-language card sits one rung above the list rows it
            // summarises, so it is `surface1` while an ordinary row is
            // `surface0`.
            assert_eq!(fills_sized(lang, 552.0, 50.0), vec![p.surface1], "card");
            assert_eq!(
                text_color(lang, "Current: English (United States)"),
                p.text,
                "card title"
            );
            assert_eq!(
                text_color(lang, "English (en-US)"),
                p.subtext0,
                "the card's native name is secondary to the card's title"
            );

            assert_eq!(
                fills_sized(lang, 552.0, 30.0),
                vec![p.surface0],
                "search box"
            );

            // Indexed by position, not counted by set: the multiset
            // {surface0, surface0, surface1} is the same whichever row is
            // raised, so a permutation of the ladder would pass a count.
            // Row 2 is the selected one; row 0 is merely the *current* one,
            // which is marked by hue rather than by rung.
            assert_eq!(
                fills_sized(lang, 552.0, 40.0),
                vec![p.surface0, p.surface0, p.surface1],
                "only the selected row is raised, and it is the third"
            );
            assert_eq!(
                text_color(lang, "Polish (Poland)"),
                p.text,
                "a row that is neither current nor selected"
            );
            assert_eq!(text_color(lang, "Polski"), p.subtext0, "row native name");
            assert_eq!(
                text_color(lang, "3 languages available"),
                p.overlay0,
                "the count line is the dimmest thing on the tab"
            );

            // The Region tab: both heading rungs, a label/value pair, and the
            // currency rows, whose current entry is raised the same way.
            let region = &by_tab[2].1;
            assert_eq!(text_color(region, "Measurement"), p.lavender, "heading");
            assert_eq!(
                text_color(region, "Available Currencies"),
                p.subtext1,
                "the sub-heading rung, below the 15pt lavender one"
            );
            assert_eq!(text_color(region, "System"), p.subtext0, "label");
            assert_eq!(
                text_color(region, "Metric (kg, km, \u{00b0}C)"),
                p.text,
                "value"
            );
            let rows = fills_sized(region, 552.0, 28.0);
            assert_eq!(rows.len(), 6, "six currency rows");
            assert_eq!(
                rows[0], p.surface1,
                "USD is the default currency, so its row is the raised one"
            );
            assert!(
                rows[1..].iter().all(|c| *c == p.surface0),
                "every other currency row sits on the base rung"
            );
        }
    }

    /// Judgement 1: three axes mark position, and they are the only three.
    ///
    /// This module cannot use the taskbar's "exactly one thing carries the
    /// accent" check — a selected tab, a current language and a current
    /// currency are three independent axes and are legitimately accented at
    /// the same time. So the accent is counted per *tab* instead, and each
    /// tab's count is different, which is what stops the three counts from
    /// being one weak assertion repeated.
    #[test]
    fn only_the_three_position_marks_carry_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let by_tab = every_tab(&p);
            let lang = &by_tab[0].1;

            // The active tab's fill, the current row's marker bar, and the
            // current row's label.
            assert_eq!(fills_sized(lang, 4.0, 32.0), vec![p.accent], "marker bar");
            assert_eq!(
                text_color(lang, "English (United States)"),
                p.accent,
                "the current language's name marks which one is in force"
            );
            let accented_count = all_colors(lang).iter().filter(|c| **c == p.accent).count();
            assert_eq!(
                accented_count, 3,
                "the Language tab should accent exactly the active tab, the \
                 current row's marker and the current row's label — found \
                 {accented_count}"
            );

            // The Formats tab has no position mark at all beyond its tab.
            let formats = &by_tab[1].1;
            assert_eq!(
                all_colors(formats)
                    .iter()
                    .filter(|c| **c == p.accent)
                    .count(),
                1,
                "nothing on the Formats tab is a selection"
            );

            // The Region tab has one: the default currency's row. Checked by
            // its text as well as by the count, because a count alone cannot
            // tell "the right row is accented" from "some other row is".
            let region = &by_tab[2].1;
            assert_eq!(
                text_color(region, "$ USD ($1234.56)"),
                p.accent,
                "the default currency's row marks which one prices are shown in"
            );
            let on_region = all_colors(region)
                .iter()
                .filter(|c| **c == p.accent)
                .count();
            assert_eq!(
                on_region, 2,
                "the Region tab should accent exactly its own tab and the \
                 current currency's row — found {on_region}"
            );
        }
    }

    /// Judgement 2: an incomplete translation is a fact about the data.
    ///
    /// It is true whatever the user has selected and whatever accent they
    /// chose, so it must not move with either.
    #[test]
    fn the_partial_badge_is_a_property_of_the_language_not_a_selection() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_ui().render(&p, 600.0, 800.0);
            let badge = fills_sized(&cmds, 56.0, 18.0);
            assert_eq!(badge, vec![p.yellow]);
            assert_ne!(
                badge[0], p.accent,
                "a translation's completeness would mean something different \
                 on every machine if it followed the accent"
            );
        }
    }

    /// Judgement 3: ink on a fill this module chose is computed from it.
    ///
    /// Proved by the two modes disagreeing. Mocha yellow is pale and wants
    /// near-black; Latte yellow is deep and wants near-white. A module that
    /// froze the badge's ink to either endpoint fails one of the two.
    #[test]
    fn the_badge_ink_is_computed_from_the_badge_it_sits_on() {
        let inks: Vec<Color> = [false, true]
            .into_iter()
            .map(|light| {
                let p = accented(light);
                let cmds = full_ui().render(&p, 600.0, 800.0);
                let badge = fills_sized(&cmds, 56.0, 18.0)[0];
                let ink = text_color(&cmds, "Partial");
                assert_eq!(ink, readable_on(badge));
                ink
            })
            .collect();
        assert_ne!(
            inks[0], inks[1],
            "the two modes' yellows sit on opposite sides of the legibility \
             threshold, so equal inks mean the value was frozen"
        );
    }

    /// The active tab's label is derived from the accent, not named beside it.
    ///
    /// Swept across the accent's whole range rather than checked at one
    /// value: `readable_on` has to answer *both* endpoints somewhere in that
    /// sweep, which a frozen constant cannot do.
    #[test]
    fn the_active_tabs_label_is_computed_from_the_accent_under_it() {
        let mut seen = std::collections::BTreeSet::new();
        for v in (0..=0xF0).step_by(0x10) {
            let mut p = Palette::for_mode(false);
            p.accent = Color::rgba(v, v, v, 255);
            let cmds = full_ui().render(&p, 600.0, 800.0);
            let ink = text_color(&cmds, "Language");
            assert_eq!(ink, readable_on(p.accent));
            seen.insert((ink.r, ink.g, ink.b));
        }
        assert_eq!(
            seen.len(),
            2,
            "sweeping the accent from black to near-white must drive the \
             label through both readable_on endpoints; {} observed means it \
             is frozen",
            seen.len()
        );
    }

    /// Judgement 4: headings are structure, so they do not move with selection.
    #[test]
    fn headings_keep_their_own_rung_under_every_accent() {
        for light in [false, true] {
            let p = accented(light);
            let by_tab = every_tab(&p);
            for (tab, cmds) in &by_tab[1..] {
                let headings: Vec<Color> = cmds
                    .iter()
                    .filter_map(|c| match c {
                        RenderCommand::Text {
                            font_size, color, ..
                        } if *font_size == 15.0 => Some(*color),
                        _ => None,
                    })
                    .collect();
                assert!(!headings.is_empty(), "{tab:?} has no headings");
                for h in headings {
                    assert_eq!(h, p.lavender, "{tab:?} heading left its rung");
                    assert_ne!(h, p.accent, "a heading is not a selection");
                }
            }
        }
    }

    /// Judgement 5: the placeholder is dimmer than a real query.
    ///
    /// Run in **both** modes, and that is not decoration. This test first ran
    /// dark only, and dark is the palette these constants were converted
    /// *from* — `p.text` in Mocha is exactly the `0xCDD6F4` that used to be
    /// written here, so `assert_eq!(query_ink, p.text)` compared a frozen
    /// constant against itself and could not fail. Harness defect Rx39 froze
    /// the query's ink and this test, the one named for that very site, missed
    /// it; only the light render caught it. Every colour assertion in this
    /// module therefore runs both modes or sweeps the accent.
    #[test]
    fn an_empty_search_box_is_dimmer_than_one_with_a_query_in_it() {
        for light in [false, true] {
            let p = accented(light);
            let mut ui = full_ui();
            ui.language_search = String::new();
            let empty = ui.render(&p, 600.0, 800.0);
            assert_eq!(text_color(&empty, "Search languages..."), p.overlay0);

            let typed = full_ui().render(&p, 600.0, 800.0);
            assert_eq!(text_color(&typed, "n"), p.text);
            assert_ne!(
                p.overlay0, p.text,
                "if these were equal a user could not tell a placeholder from \
                 a query they typed"
            );
        }
    }

    /// The panel is not the same picture in both modes.
    ///
    /// A module that ignored its palette entirely would still pass every
    /// role table above, because those compare against whatever `p` says.
    /// This is the check that the palette was read at all.
    #[test]
    fn the_render_is_not_the_same_in_both_modes() {
        for tab in [
            LanguageTab::Language,
            LanguageTab::Formats,
            LanguageTab::Region,
        ] {
            let mut ui = full_ui();
            ui.set_tab(tab);
            let dark = all_colors(&ui.render(&accented(false), 600.0, 800.0));
            let light = all_colors(&ui.render(&accented(true), 600.0, 800.0));
            assert_ne!(
                dark, light,
                "{tab:?} draws identically in both modes, so it is not \
                 reading the palette it was handed"
            );
        }
    }

    /// None of the eleven deleted constants is still on screen.
    ///
    /// Named separately from the sweep because the sweep allows a Mocha value
    /// in the *dark* render by construction; this asks the sharper question.
    #[test]
    fn none_of_the_eleven_deleted_constants_is_still_drawn() {
        // The Mocha values this module used to hold, minus `crust`, which is
        // also a `readable_on` endpoint and so is legitimately drawn.
        const DELETED: [u32; 10] = [
            0x001E_1E2E, // base
            0x0031_3244, // surface0
            0x0045_475A, // surface1
            0x00CD_D6F4, // text
            0x00A6_ADC8, // subtext0
            0x00BA_C2DE, // subtext1
            0x0089_B4FA, // blue
            0x00F9_E2AF, // yellow
            0x00B4_BEFE, // lavender
            0x006C_7086, // overlay0
        ];
        let p = accented(true);
        for (tab, cmds) in every_tab(&p) {
            for c in all_colors(&cmds) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                assert!(
                    !DELETED.contains(&rgb),
                    "{tab:?} still draws Mocha #{rgb:06X} in a light render"
                );
            }
        }
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(LanguageTab::Language.label(), "Language");
        assert_eq!(LanguageTab::Formats.label(), "Formats");
        assert_eq!(LanguageTab::Region.label(), "Region");
    }
}
