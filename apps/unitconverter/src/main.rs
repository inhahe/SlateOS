//! OurOS Unit Converter
//!
//! Comprehensive unit conversion application with:
//! - 12 unit categories (length, weight, temperature, volume, area, speed,
//!   time, digital storage, pressure, energy, frequency, angle)
//! - Conversions through a base unit per category (special affine handling
//!   for temperature)
//! - Category sidebar with icons
//! - Two input fields with unit dropdowns and a swap button
//! - Conversion formula display
//! - History of the last 20 conversions
//! - Favorites/bookmarks for commonly used conversions
//!
//! Uses the guitk library with Catppuccin Mocha theme.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::layout::FlexDirection;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;
use guitk::widget::{Widget, WidgetTree};

use std::collections::VecDeque;
use std::fmt::Write as FmtWrite;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const OVERLAY1: Color = Color::from_hex(0x7F849C);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const SKY: Color = Color::from_hex(0x89DCFE);
    pub const PINK: Color = Color::from_hex(0xF5C2E7);
    pub const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
    pub const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
}

// ============================================================================
// Unit categories
// ============================================================================

/// All supported unit categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Length,
    Weight,
    Temperature,
    Volume,
    Area,
    Speed,
    Time,
    DigitalStorage,
    Pressure,
    Energy,
    Frequency,
    Angle,
}

impl Category {
    /// All categories in display order.
    pub const ALL: &[Category] = &[
        Category::Length,
        Category::Weight,
        Category::Temperature,
        Category::Volume,
        Category::Area,
        Category::Speed,
        Category::Time,
        Category::DigitalStorage,
        Category::Pressure,
        Category::Energy,
        Category::Frequency,
        Category::Angle,
    ];

    /// Display name for the category.
    pub fn name(self) -> &'static str {
        match self {
            Category::Length => "Length",
            Category::Weight => "Weight / Mass",
            Category::Temperature => "Temperature",
            Category::Volume => "Volume",
            Category::Area => "Area",
            Category::Speed => "Speed",
            Category::Time => "Time",
            Category::DigitalStorage => "Digital Storage",
            Category::Pressure => "Pressure",
            Category::Energy => "Energy",
            Category::Frequency => "Frequency",
            Category::Angle => "Angle",
        }
    }

    /// Icon/symbol for the category sidebar.
    pub fn icon(self) -> &'static str {
        match self {
            Category::Length => "L",
            Category::Weight => "W",
            Category::Temperature => "T",
            Category::Volume => "V",
            Category::Area => "A",
            Category::Speed => "S",
            Category::Time => "Tm",
            Category::DigitalStorage => "DS",
            Category::Pressure => "P",
            Category::Energy => "E",
            Category::Frequency => "Hz",
            Category::Angle => "An",
        }
    }

    /// Color accent for this category.
    pub fn accent(self) -> Color {
        match self {
            Category::Length => theme::BLUE,
            Category::Weight => theme::GREEN,
            Category::Temperature => theme::RED,
            Category::Volume => theme::TEAL,
            Category::Area => theme::YELLOW,
            Category::Speed => theme::PEACH,
            Category::Time => theme::MAUVE,
            Category::DigitalStorage => theme::SKY,
            Category::Pressure => theme::FLAMINGO,
            Category::Energy => theme::PINK,
            Category::Frequency => theme::LAVENDER,
            Category::Angle => theme::ROSEWATER,
        }
    }
}

// ============================================================================
// Unit definitions
// ============================================================================

/// A single unit within a category.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitDef {
    /// Short symbol (e.g. "km", "lb").
    pub symbol: &'static str,
    /// Full display name (e.g. "Kilometer", "Pound").
    pub name: &'static str,
    /// Conversion factor to the base unit (multiply by this to get base).
    /// For temperature, this is the scale factor.
    pub factor: f64,
    /// Offset for affine conversions (temperature).
    /// value_in_base = value * factor + offset
    pub offset: f64,
}

impl UnitDef {
    /// Create a linear unit (most units).
    const fn linear(symbol: &'static str, name: &'static str, factor: f64) -> Self {
        Self {
            symbol,
            name,
            factor,
            offset: 0.0,
        }
    }

    /// Create an affine unit (temperature).
    const fn affine(symbol: &'static str, name: &'static str, factor: f64, offset: f64) -> Self {
        Self {
            symbol,
            name,
            factor,
            offset,
        }
    }
}

/// Get all units for a given category.
pub fn units_for_category(cat: Category) -> &'static [UnitDef] {
    match cat {
        Category::Length => LENGTH_UNITS,
        Category::Weight => WEIGHT_UNITS,
        Category::Temperature => TEMPERATURE_UNITS,
        Category::Volume => VOLUME_UNITS,
        Category::Area => AREA_UNITS,
        Category::Speed => SPEED_UNITS,
        Category::Time => TIME_UNITS,
        Category::DigitalStorage => DIGITAL_UNITS,
        Category::Pressure => PRESSURE_UNITS,
        Category::Energy => ENERGY_UNITS,
        Category::Frequency => FREQUENCY_UNITS,
        Category::Angle => ANGLE_UNITS,
    }
}

// -- Length (base: meters) --
const LENGTH_UNITS: &[UnitDef] = &[
    UnitDef::linear("mm", "Millimeter", 0.001),
    UnitDef::linear("cm", "Centimeter", 0.01),
    UnitDef::linear("m", "Meter", 1.0),
    UnitDef::linear("km", "Kilometer", 1000.0),
    UnitDef::linear("in", "Inch", 0.0254),
    UnitDef::linear("ft", "Foot", 0.3048),
    UnitDef::linear("yd", "Yard", 0.9144),
    UnitDef::linear("mi", "Mile", 1609.344),
    UnitDef::linear("nmi", "Nautical Mile", 1852.0),
];

// -- Weight/Mass (base: kilograms) --
const WEIGHT_UNITS: &[UnitDef] = &[
    UnitDef::linear("mg", "Milligram", 0.000_001),
    UnitDef::linear("g", "Gram", 0.001),
    UnitDef::linear("kg", "Kilogram", 1.0),
    UnitDef::linear("t", "Metric Ton", 1000.0),
    UnitDef::linear("oz", "Ounce", 0.028_349_523_125),
    UnitDef::linear("lb", "Pound", 0.453_592_37),
    UnitDef::linear("st", "Stone", 6.350_293_18),
    UnitDef::linear("ton", "US Ton", 907.184_74),
];

// -- Temperature (base: Kelvin) --
// value_in_kelvin = value * factor + offset
const TEMPERATURE_UNITS: &[UnitDef] = &[
    UnitDef::affine("\u{00B0}C", "Celsius", 1.0, 273.15),
    UnitDef::affine("\u{00B0}F", "Fahrenheit", 5.0 / 9.0, 255.372_222_222_222_2),
    UnitDef::affine("K", "Kelvin", 1.0, 0.0),
    UnitDef::affine("\u{00B0}R", "Rankine", 5.0 / 9.0, 0.0),
];

// -- Volume (base: liters) --
const VOLUME_UNITS: &[UnitDef] = &[
    UnitDef::linear("mL", "Milliliter", 0.001),
    UnitDef::linear("L", "Liter", 1.0),
    UnitDef::linear("gal", "Gallon (US)", 3.785_411_784),
    UnitDef::linear("gal UK", "Gallon (UK)", 4.546_09),
    UnitDef::linear("qt", "Quart", 0.946_352_946),
    UnitDef::linear("pt", "Pint", 0.473_176_473),
    UnitDef::linear("cup", "Cup", 0.236_588_236_5),
    UnitDef::linear("fl oz", "Fluid Ounce", 0.029_573_529_562_5),
    UnitDef::linear("tbsp", "Tablespoon", 0.014_786_764_781_25),
    UnitDef::linear("tsp", "Teaspoon", 0.004_928_921_593_75),
];

// -- Area (base: square meters) --
const AREA_UNITS: &[UnitDef] = &[
    UnitDef::linear("mm\u{00B2}", "Square Millimeter", 0.000_001),
    UnitDef::linear("cm\u{00B2}", "Square Centimeter", 0.000_1),
    UnitDef::linear("m\u{00B2}", "Square Meter", 1.0),
    UnitDef::linear("km\u{00B2}", "Square Kilometer", 1_000_000.0),
    UnitDef::linear("ha", "Hectare", 10_000.0),
    UnitDef::linear("ac", "Acre", 4046.856_422_4),
    UnitDef::linear("ft\u{00B2}", "Square Foot", 0.092_903_04),
    UnitDef::linear("in\u{00B2}", "Square Inch", 0.000_645_16),
    UnitDef::linear("mi\u{00B2}", "Square Mile", 2_589_988.110_336),
];

// -- Speed (base: meters per second) --
const SPEED_UNITS: &[UnitDef] = &[
    UnitDef::linear("m/s", "Meters per Second", 1.0),
    UnitDef::linear("km/h", "Kilometers per Hour", 1.0 / 3.6),
    UnitDef::linear("mph", "Miles per Hour", 0.447_04),
    UnitDef::linear("kn", "Knots", 0.514_444_444),
    UnitDef::linear("ft/s", "Feet per Second", 0.3048),
    // Mach at sea level, standard atmosphere (340.29 m/s)
    UnitDef::linear("Mach", "Mach", 340.29),
];

// -- Time (base: seconds) --
const TIME_UNITS: &[UnitDef] = &[
    UnitDef::linear("ms", "Millisecond", 0.001),
    UnitDef::linear("s", "Second", 1.0),
    UnitDef::linear("min", "Minute", 60.0),
    UnitDef::linear("h", "Hour", 3600.0),
    UnitDef::linear("day", "Day", 86_400.0),
    UnitDef::linear("wk", "Week", 604_800.0),
    // Average month = 30.4375 days (365.25 / 12)
    UnitDef::linear("mo", "Month", 2_629_800.0),
    // Julian year = 365.25 days
    UnitDef::linear("yr", "Year", 31_557_600.0),
];

// -- Digital Storage (base: bytes) --
const DIGITAL_UNITS: &[UnitDef] = &[
    UnitDef::linear("bit", "Bit", 0.125),
    UnitDef::linear("B", "Byte", 1.0),
    UnitDef::linear("KB", "Kilobyte", 1_000.0),
    UnitDef::linear("MB", "Megabyte", 1_000_000.0),
    UnitDef::linear("GB", "Gigabyte", 1_000_000_000.0),
    UnitDef::linear("TB", "Terabyte", 1_000_000_000_000.0),
    UnitDef::linear("PB", "Petabyte", 1_000_000_000_000_000.0),
    UnitDef::linear("KiB", "Kibibyte", 1_024.0),
    UnitDef::linear("MiB", "Mebibyte", 1_048_576.0),
    UnitDef::linear("GiB", "Gibibyte", 1_073_741_824.0),
    UnitDef::linear("TiB", "Tebibyte", 1_099_511_627_776.0),
];

// -- Pressure (base: pascals) --
const PRESSURE_UNITS: &[UnitDef] = &[
    UnitDef::linear("Pa", "Pascal", 1.0),
    UnitDef::linear("kPa", "Kilopascal", 1_000.0),
    UnitDef::linear("MPa", "Megapascal", 1_000_000.0),
    UnitDef::linear("bar", "Bar", 100_000.0),
    UnitDef::linear("atm", "Atmosphere", 101_325.0),
    UnitDef::linear("psi", "Pounds per Sq Inch", 6_894.757_293_168),
    UnitDef::linear("mmHg", "Millimeters of Mercury", 133.322_387_415),
    UnitDef::linear("inHg", "Inches of Mercury", 3_386.389),
];

// -- Energy (base: joules) --
const ENERGY_UNITS: &[UnitDef] = &[
    UnitDef::linear("J", "Joule", 1.0),
    UnitDef::linear("kJ", "Kilojoule", 1_000.0),
    UnitDef::linear("cal", "Calorie", 4.184),
    UnitDef::linear("kcal", "Kilocalorie", 4_184.0),
    UnitDef::linear("Wh", "Watt-hour", 3_600.0),
    UnitDef::linear("kWh", "Kilowatt-hour", 3_600_000.0),
    UnitDef::linear("BTU", "British Thermal Unit", 1_055.06),
    UnitDef::linear("eV", "Electronvolt", 1.602_176_634e-19),
];

// -- Frequency (base: hertz) --
const FREQUENCY_UNITS: &[UnitDef] = &[
    UnitDef::linear("Hz", "Hertz", 1.0),
    UnitDef::linear("kHz", "Kilohertz", 1_000.0),
    UnitDef::linear("MHz", "Megahertz", 1_000_000.0),
    UnitDef::linear("GHz", "Gigahertz", 1_000_000_000.0),
    // rpm = revolutions per minute = 1/60 Hz
    UnitDef::linear("rpm", "Revolutions/Minute", 1.0 / 60.0),
];

// -- Angle (base: radians) --
const ANGLE_UNITS: &[UnitDef] = &[
    UnitDef::linear("deg", "Degree", std::f64::consts::PI / 180.0),
    UnitDef::linear("rad", "Radian", 1.0),
    // 1 gradian = pi/200 radians
    UnitDef::linear("grad", "Gradian", std::f64::consts::PI / 200.0),
    // 1 arcminute = 1/60 degree
    UnitDef::linear("arcmin", "Arcminute", std::f64::consts::PI / 10_800.0),
    // 1 arcsecond = 1/3600 degree
    UnitDef::linear("arcsec", "Arcsecond", std::f64::consts::PI / 648_000.0),
    // 1 turn = 2*pi radians
    UnitDef::linear("turn", "Turn", std::f64::consts::TAU),
];

// ============================================================================
// Conversion engine
// ============================================================================

/// Convert a value from one unit to another within the same category.
///
/// For linear units:  result = value * (from_factor / to_factor)
/// For affine units:  base = value * from_factor + from_offset
///                    result = (base - to_offset) / to_factor
pub fn convert(value: f64, from: &UnitDef, to: &UnitDef) -> f64 {
    // Convert to base unit.
    let base = value * from.factor + from.offset;
    // Convert from base unit to target.
    if to.factor == 0.0 {
        return f64::NAN;
    }
    (base - to.offset) / to.factor
}

/// Produce a human-readable formula string describing the conversion.
pub fn formula_text(from: &UnitDef, to: &UnitDef) -> String {
    // Both offsets zero means linear relationship.
    if from.offset == 0.0 && to.offset == 0.0 {
        if to.factor == 0.0 {
            return String::from("undefined (division by zero)");
        }
        let ratio = from.factor / to.factor;
        let mut buf = String::new();
        let _ = write!(
            buf,
            "1 {} = {} {}",
            from.symbol,
            format_number(ratio),
            to.symbol
        );
        buf
    } else {
        // Affine (temperature-like).
        // base = val * from.factor + from.offset
        // result = (base - to.offset) / to.factor
        let mut buf = String::new();
        let _ = write!(
            buf,
            "{to} = ({from} * {sf} + {so} - {to_off}) / {tf}",
            to = to.symbol,
            from = from.symbol,
            sf = format_number(from.factor),
            so = format_number(from.offset),
            to_off = format_number(to.offset),
            tf = format_number(to.factor),
        );
        buf
    }
}

/// Format a number for display: use fixed notation for reasonable magnitudes,
/// scientific notation for very large/small values, and trim trailing zeros.
pub fn format_number(val: f64) -> String {
    if val.is_nan() {
        return String::from("NaN");
    }
    if val.is_infinite() {
        return if val > 0.0 {
            String::from("Inf")
        } else {
            String::from("-Inf")
        };
    }
    if val == 0.0 {
        return String::from("0");
    }

    let abs = val.abs();

    if !(1e-6..1e15).contains(&abs) {
        // Scientific notation.
        let mut buf = String::new();
        let _ = write!(buf, "{val:.6e}");
        buf
    } else if abs == (abs as u64) as f64 && abs < 1e15 {
        // Integer-valued.
        let mut buf = String::new();
        let _ = write!(buf, "{}", val as i64);
        buf
    } else {
        // Fixed notation, up to 10 decimal places, trim trailing zeros.
        let mut buf = String::new();
        let _ = write!(buf, "{val:.10}");
        // Trim trailing zeros after decimal point.
        if buf.contains('.') {
            let trimmed = buf.trim_end_matches('0');
            let trimmed = trimmed.trim_end_matches('.');
            return trimmed.to_string();
        }
        buf
    }
}

// ============================================================================
// History & Favorites
// ============================================================================

/// A record of a completed conversion.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub category: Category,
    pub from_value: f64,
    pub from_symbol: &'static str,
    pub to_value: f64,
    pub to_symbol: &'static str,
}

impl HistoryEntry {
    /// Format as a display string.
    pub fn display(&self) -> String {
        let mut buf = String::new();
        let _ = write!(
            buf,
            "{} {} = {} {}",
            format_number(self.from_value),
            self.from_symbol,
            format_number(self.to_value),
            self.to_symbol,
        );
        buf
    }
}

/// A bookmarked (favorite) conversion pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Favorite {
    pub category: Category,
    pub from_idx: usize,
    pub to_idx: usize,
}

impl Favorite {
    /// Display label for the favorite.
    pub fn label(&self) -> String {
        let units = units_for_category(self.category);
        let from_sym = units.get(self.from_idx).map_or("?", |u| u.symbol);
        let to_sym = units.get(self.to_idx).map_or("?", |u| u.symbol);
        let mut buf = String::new();
        let _ = write!(buf, "{}: {} -> {}", self.category.name(), from_sym, to_sym);
        buf
    }
}

// ============================================================================
// Application state
// ============================================================================

const MAX_HISTORY: usize = 20;
const SIDEBAR_WIDTH: f32 = 180.0;
const HISTORY_PANEL_WIDTH: f32 = 260.0;
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// Main application state.
pub struct UnitConverterApp {
    /// Currently selected category.
    pub selected_category: Category,
    /// Index of the "from" unit in the current category's unit list.
    pub from_unit_idx: usize,
    /// Index of the "to" unit in the current category's unit list.
    pub to_unit_idx: usize,
    /// Text in the "from" input field.
    pub from_input: String,
    /// Cursor position in the from input.
    pub from_cursor: usize,
    /// Text in the "to" (result) display.
    pub to_display: String,
    /// Conversion history (most recent first).
    pub history: VecDeque<HistoryEntry>,
    /// Favorite conversion pairs.
    pub favorites: Vec<Favorite>,
    /// Whether the "from" dropdown is open.
    pub from_dropdown_open: bool,
    /// Whether the "to" dropdown is open.
    pub to_dropdown_open: bool,
    /// Scroll offset for history.
    pub history_scroll: f32,
    /// Whether the favorites panel is visible.
    pub show_favorites: bool,
    /// Which input is focused: true = from, false = to (for future bidirectional).
    pub from_focused: bool,
}

impl UnitConverterApp {
    /// Create a new application with default state.
    pub fn new() -> Self {
        Self {
            selected_category: Category::Length,
            from_unit_idx: 2, // meters
            to_unit_idx: 3,   // kilometers
            from_input: String::from("1"),
            from_cursor: 1,
            to_display: String::new(),
            history: VecDeque::new(),
            favorites: Vec::new(),
            from_dropdown_open: false,
            to_dropdown_open: false,
            history_scroll: 0.0,
            show_favorites: false,
            from_focused: true,
        }
    }

    /// Perform the conversion based on current state and update display.
    pub fn do_convert(&mut self) {
        let units = units_for_category(self.selected_category);
        let from = match units.get(self.from_unit_idx) {
            Some(u) => u,
            None => return,
        };
        let to = match units.get(self.to_unit_idx) {
            Some(u) => u,
            None => return,
        };

        let val: f64 = match self.from_input.parse() {
            Ok(v) => v,
            Err(_) => {
                self.to_display = String::from("Invalid input");
                return;
            }
        };

        let result = convert(val, from, to);
        self.to_display = format_number(result);

        // Add to history.
        let entry = HistoryEntry {
            category: self.selected_category,
            from_value: val,
            from_symbol: from.symbol,
            to_value: result,
            to_symbol: to.symbol,
        };
        self.history.push_front(entry);
        if self.history.len() > MAX_HISTORY {
            self.history.pop_back();
        }
    }

    /// Swap from and to units.
    pub fn swap_units(&mut self) {
        std::mem::swap(&mut self.from_unit_idx, &mut self.to_unit_idx);

        // If there was a valid result, use it as the new input.
        if let Ok(val) = self.to_display.parse::<f64>() {
            self.from_input = format_number(val);
            self.from_cursor = self.from_input.len();
        }

        self.do_convert();
    }

    /// Select a category, resetting unit indices to defaults.
    pub fn select_category(&mut self, cat: Category) {
        if self.selected_category == cat {
            return;
        }
        self.selected_category = cat;
        self.from_unit_idx = 0;
        let units = units_for_category(cat);
        self.to_unit_idx = if units.len() > 1 { 1 } else { 0 };
        self.from_dropdown_open = false;
        self.to_dropdown_open = false;
        self.do_convert();
    }

    /// Toggle whether the current from/to pair is a favorite.
    pub fn toggle_favorite(&mut self) {
        let fav = Favorite {
            category: self.selected_category,
            from_idx: self.from_unit_idx,
            to_idx: self.to_unit_idx,
        };
        if let Some(pos) = self.favorites.iter().position(|f| f == &fav) {
            self.favorites.remove(pos);
        } else {
            self.favorites.push(fav);
        }
    }

    /// Check if the current from/to pair is a favorite.
    pub fn is_current_favorite(&self) -> bool {
        let fav = Favorite {
            category: self.selected_category,
            from_idx: self.from_unit_idx,
            to_idx: self.to_unit_idx,
        };
        self.favorites.contains(&fav)
    }

    /// Apply a favorite: switch to its category and unit pair.
    pub fn apply_favorite(&mut self, idx: usize) {
        if let Some(fav) = self.favorites.get(idx).cloned() {
            self.selected_category = fav.category;
            self.from_unit_idx = fav.from_idx;
            self.to_unit_idx = fav.to_idx;
            self.from_dropdown_open = false;
            self.to_dropdown_open = false;
            self.do_convert();
        }
    }

    /// Get the formula text for the current unit pair.
    pub fn current_formula(&self) -> String {
        let units = units_for_category(self.selected_category);
        let from = match units.get(self.from_unit_idx) {
            Some(u) => u,
            None => return String::new(),
        };
        let to = match units.get(self.to_unit_idx) {
            Some(u) => u,
            None => return String::new(),
        };
        formula_text(from, to)
    }

    /// Handle a keyboard event. Returns true if the event was consumed.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if !key.pressed {
            return false;
        }

        // Close dropdowns on Escape.
        if key.key == Key::Escape {
            if self.from_dropdown_open || self.to_dropdown_open {
                self.from_dropdown_open = false;
                self.to_dropdown_open = false;
                return true;
            }
            return false;
        }

        // Tab to switch focus.
        if key.key == Key::Tab {
            self.from_focused = !self.from_focused;
            return true;
        }

        // Ctrl+S to swap.
        if key.key == Key::S && key.modifiers.ctrl {
            self.swap_units();
            return true;
        }

        // Ctrl+F to toggle favorite.
        if key.key == Key::F && key.modifiers.ctrl {
            self.toggle_favorite();
            return true;
        }

        // Handle text input for the from field when focused.
        if self.from_focused && !self.from_dropdown_open && !self.to_dropdown_open {
            match key.key {
                Key::Backspace => {
                    if self.from_cursor > 0 {
                        self.from_cursor -= 1;
                        if self.from_cursor < self.from_input.len() {
                            self.from_input.remove(self.from_cursor);
                        }
                        self.do_convert();
                        return true;
                    }
                }
                Key::Delete => {
                    if self.from_cursor < self.from_input.len() {
                        self.from_input.remove(self.from_cursor);
                        self.do_convert();
                        return true;
                    }
                }
                Key::Left => {
                    if self.from_cursor > 0 {
                        self.from_cursor -= 1;
                    }
                    return true;
                }
                Key::Right => {
                    if self.from_cursor < self.from_input.len() {
                        self.from_cursor += 1;
                    }
                    return true;
                }
                Key::Home => {
                    self.from_cursor = 0;
                    return true;
                }
                Key::End => {
                    self.from_cursor = self.from_input.len();
                    return true;
                }
                Key::Enter => {
                    self.do_convert();
                    return true;
                }
                _ => {
                    // Type a character.
                    if let Some(ch) = key.text
                        && (ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == 'e' || ch == 'E') {
                            self.from_input.insert(self.from_cursor, ch);
                            self.from_cursor += 1;
                            self.do_convert();
                            return true;
                        }
                }
            }
        }

        false
    }

    /// Handle a mouse click at the given position. Returns true if consumed.
    pub fn handle_click(&mut self, x: f32, y: f32) -> bool {
        // Close any open dropdown if clicking outside it.
        let consumed_dropdown = self.handle_dropdown_click(x, y);
        if consumed_dropdown {
            return true;
        }

        // Sidebar category clicks.
        if x < SIDEBAR_WIDTH {
            let item_height: f32 = 44.0;
            let header_height: f32 = 56.0;
            if y >= header_height {
                let idx = ((y - header_height) / item_height) as usize;
                if idx < Category::ALL.len() {
                    self.select_category(Category::ALL[idx]);
                    return true;
                }
            }
            return false;
        }

        // Main area (between sidebar and history panel).
        let main_left = SIDEBAR_WIDTH;
        let main_right = WINDOW_WIDTH - HISTORY_PANEL_WIDTH;

        if x >= main_left && x < main_right {
            // Swap button: centered horizontally, at y ~ 200.
            let swap_cx = (main_left + main_right) / 2.0;
            let swap_cy: f32 = 230.0;
            let swap_r: f32 = 18.0;
            let dx = x - swap_cx;
            let dy = y - swap_cy;
            if dx * dx + dy * dy <= swap_r * swap_r {
                self.swap_units();
                return true;
            }

            // Favorite star button: right of the swap area.
            let star_cx = main_right - 40.0;
            let star_cy: f32 = 80.0;
            if (x - star_cx).abs() < 20.0 && (y - star_cy).abs() < 20.0 {
                self.toggle_favorite();
                return true;
            }

            // "From" dropdown toggle.
            let dd_from_x = main_left + 20.0;
            let dd_from_y: f32 = 150.0;
            let dd_from_w: f32 = 180.0;
            let dd_from_h: f32 = 32.0;
            if x >= dd_from_x
                && x <= dd_from_x + dd_from_w
                && y >= dd_from_y
                && y <= dd_from_y + dd_from_h
            {
                self.from_dropdown_open = !self.from_dropdown_open;
                self.to_dropdown_open = false;
                return true;
            }

            // "To" dropdown toggle.
            let dd_to_x = main_right - 20.0 - 180.0;
            let dd_to_y: f32 = 150.0;
            let dd_to_w: f32 = 180.0;
            let dd_to_h: f32 = 32.0;
            if x >= dd_to_x && x <= dd_to_x + dd_to_w && y >= dd_to_y && y <= dd_to_y + dd_to_h {
                self.to_dropdown_open = !self.to_dropdown_open;
                self.from_dropdown_open = false;
                return true;
            }

            // Click on from input field to focus.
            let input_x = main_left + 20.0;
            let input_y: f32 = 100.0;
            let input_w: f32 = 180.0;
            let input_h: f32 = 36.0;
            if x >= input_x && x <= input_x + input_w && y >= input_y && y <= input_y + input_h {
                self.from_focused = true;
                return true;
            }

            // Show favorites toggle.
            let fav_btn_x = main_left + 20.0;
            let fav_btn_y: f32 = 370.0;
            let fav_btn_w: f32 = 120.0;
            let fav_btn_h: f32 = 30.0;
            if x >= fav_btn_x
                && x <= fav_btn_x + fav_btn_w
                && y >= fav_btn_y
                && y <= fav_btn_y + fav_btn_h
            {
                self.show_favorites = !self.show_favorites;
                return true;
            }

            // Favorite items click.
            if self.show_favorites {
                let fav_start_y: f32 = 410.0;
                let fav_item_h: f32 = 28.0;
                if x >= main_left + 20.0 && y >= fav_start_y {
                    let idx = ((y - fav_start_y) / fav_item_h) as usize;
                    if idx < self.favorites.len() {
                        self.apply_favorite(idx);
                        return true;
                    }
                }
            }
        }

        // History panel clicks (right side).
        if x >= WINDOW_WIDTH - HISTORY_PANEL_WIDTH {
            // Could add clear-history button, etc.
            return false;
        }

        false
    }

    /// Handle clicks within open dropdowns.
    fn handle_dropdown_click(&mut self, x: f32, y: f32) -> bool {
        let main_left = SIDEBAR_WIDTH;
        let main_right = WINDOW_WIDTH - HISTORY_PANEL_WIDTH;
        let units = units_for_category(self.selected_category);

        if self.from_dropdown_open {
            let dd_x = main_left + 20.0;
            let dd_y: f32 = 182.0;
            let dd_w: f32 = 180.0;
            let item_h: f32 = 28.0;
            let dd_h = units.len() as f32 * item_h;

            if x >= dd_x && x <= dd_x + dd_w && y >= dd_y && y <= dd_y + dd_h {
                let idx = ((y - dd_y) / item_h) as usize;
                if idx < units.len() {
                    self.from_unit_idx = idx;
                    self.from_dropdown_open = false;
                    self.do_convert();
                    return true;
                }
            }
            // Click outside closes.
            self.from_dropdown_open = false;
            return true;
        }

        if self.to_dropdown_open {
            let dd_x = main_right - 20.0 - 180.0;
            let dd_y: f32 = 182.0;
            let dd_w: f32 = 180.0;
            let item_h: f32 = 28.0;
            let dd_h = units.len() as f32 * item_h;

            if x >= dd_x && x <= dd_x + dd_w && y >= dd_y && y <= dd_y + dd_h {
                let idx = ((y - dd_y) / item_h) as usize;
                if idx < units.len() {
                    self.to_unit_idx = idx;
                    self.to_dropdown_open = false;
                    self.do_convert();
                    return true;
                }
            }
            // Click outside closes.
            self.to_dropdown_open = false;
            return true;
        }

        false
    }

    /// Handle a full event (mouse, key, etc.). Returns EventResult.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) => {
                if self.handle_key(key) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::Mouse(mouse) => match &mouse.kind {
                MouseEventKind::Press(MouseButton::Left) => {
                    if self.handle_click(mouse.x, mouse.y) {
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                MouseEventKind::Scroll { dy, .. } => {
                    // Scroll history panel.
                    if mouse.x >= WINDOW_WIDTH - HISTORY_PANEL_WIDTH {
                        self.history_scroll = (self.history_scroll - dy).max(0.0);
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                _ => EventResult::Ignored,
            },
            _ => EventResult::Ignored,
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full application into a render tree.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Full-window background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: theme::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_sidebar(&mut tree);
        self.render_main_area(&mut tree);
        self.render_history_panel(&mut tree);

        // Render dropdowns on top of everything.
        if self.from_dropdown_open {
            self.render_dropdown(&mut tree, true);
        }
        if self.to_dropdown_open {
            self.render_dropdown(&mut tree, false);
        }

        tree
    }

    /// Render the category sidebar.
    fn render_sidebar(&self, tree: &mut RenderTree) {
        // Sidebar background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: SIDEBAR_WIDTH,
            height: WINDOW_HEIGHT,
            color: theme::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header.
        tree.push(RenderCommand::Text {
            x: 16.0,
            y: 18.0,
            text: String::from("Unit Converter"),
            color: theme::BLUE,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 24.0),
        });

        // Separator under header.
        tree.push(RenderCommand::Line {
            x1: 12.0,
            y1: 48.0,
            x2: SIDEBAR_WIDTH - 12.0,
            y2: 48.0,
            color: theme::SURFACE1,
            width: 1.0,
        });

        // Category items.
        let item_height: f32 = 44.0;
        let start_y: f32 = 56.0;

        for (i, cat) in Category::ALL.iter().enumerate() {
            let y = start_y + i as f32 * item_height;
            let is_selected = *cat == self.selected_category;

            // Highlight selected item.
            if is_selected {
                tree.push(RenderCommand::FillRect {
                    x: 4.0,
                    y,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: item_height - 4.0,
                    color: theme::SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Accent bar on the left.
                tree.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: y + 6.0,
                    width: 3.0,
                    height: item_height - 16.0,
                    color: cat.accent(),
                    corner_radii: CornerRadii::all(1.5),
                });
            }

            // Icon circle.
            let icon_x: f32 = 18.0;
            let icon_y = y + 8.0;
            let icon_size: f32 = 26.0;
            let icon_color = if is_selected {
                cat.accent()
            } else {
                theme::SURFACE1
            };
            tree.push(RenderCommand::FillRect {
                x: icon_x,
                y: icon_y,
                width: icon_size,
                height: icon_size,
                color: icon_color,
                corner_radii: CornerRadii::all(13.0),
            });

            // Icon text.
            let icon_text_color = if is_selected {
                theme::MANTLE
            } else {
                theme::SUBTEXT0
            };
            tree.push(RenderCommand::Text {
                x: icon_x + 4.0,
                y: icon_y + 6.0,
                text: String::from(cat.icon()),
                color: icon_text_color,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(icon_size - 4.0),
            });

            // Category name.
            let text_color = if is_selected {
                theme::TEXT
            } else {
                theme::SUBTEXT0
            };
            tree.push(RenderCommand::Text {
                x: 52.0,
                y: y + 14.0,
                text: String::from(cat.name()),
                color: text_color,
                font_size: 13.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 60.0),
            });
        }
    }

    /// Render the main conversion area.
    fn render_main_area(&self, tree: &mut RenderTree) {
        let main_left = SIDEBAR_WIDTH;
        let main_right = WINDOW_WIDTH - HISTORY_PANEL_WIDTH;
        let main_width = main_right - main_left;

        // Separator line.
        tree.push(RenderCommand::Line {
            x1: main_left,
            y1: 0.0,
            x2: main_left,
            y2: WINDOW_HEIGHT,
            color: theme::SURFACE0,
            width: 1.0,
        });

        let units = units_for_category(self.selected_category);
        let from_unit = units.get(self.from_unit_idx);
        let to_unit = units.get(self.to_unit_idx);

        // Category title with accent color.
        let accent = self.selected_category.accent();
        tree.push(RenderCommand::Text {
            x: main_left + 24.0,
            y: 20.0,
            text: String::from(self.selected_category.name()),
            color: accent,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(main_width - 80.0),
        });

        // Favorite star.
        let star_text = if self.is_current_favorite() {
            "\u{2605}" // filled star
        } else {
            "\u{2606}" // empty star
        };
        let star_color = if self.is_current_favorite() {
            theme::YELLOW
        } else {
            theme::OVERLAY0
        };
        tree.push(RenderCommand::Text {
            x: main_right - 44.0,
            y: 20.0,
            text: String::from(star_text),
            color: star_color,
            font_size: 22.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // --- From section ---
        tree.push(RenderCommand::Text {
            x: main_left + 24.0,
            y: 70.0,
            text: String::from("From"),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // From input field.
        let input_x = main_left + 20.0;
        let input_y: f32 = 88.0;
        let input_w: f32 = 180.0;
        let input_h: f32 = 36.0;
        self.render_input_field(
            tree,
            input_x,
            input_y,
            input_w,
            input_h,
            &self.from_input,
            true,
        );

        // From unit dropdown button.
        let dd_x = main_left + 20.0;
        let dd_y: f32 = 134.0;
        let dd_w: f32 = 180.0;
        let dd_h: f32 = 32.0;
        let from_label = from_unit.map_or("Select", |u| u.name);
        self.render_dropdown_button(
            tree,
            dd_x,
            dd_y,
            dd_w,
            dd_h,
            from_label,
            self.from_dropdown_open,
        );

        // --- Swap button ---
        let swap_cx = (main_left + main_right) / 2.0;
        let swap_cy: f32 = 115.0;
        let swap_r: f32 = 18.0;
        tree.push(RenderCommand::FillRect {
            x: swap_cx - swap_r,
            y: swap_cy - swap_r,
            width: swap_r * 2.0,
            height: swap_r * 2.0,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(swap_r),
        });
        tree.push(RenderCommand::Text {
            x: swap_cx - 7.0,
            y: swap_cy - 8.0,
            text: String::from("\u{21C4}"), // left-right arrows
            color: theme::BLUE,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // --- To section ---
        let to_section_x = main_right - 20.0 - 180.0;
        tree.push(RenderCommand::Text {
            x: to_section_x,
            y: 70.0,
            text: String::from("To"),
            color: theme::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // To result display.
        let result_text = if self.to_display.is_empty() {
            "..."
        } else {
            &self.to_display
        };
        self.render_input_field(
            tree,
            to_section_x,
            input_y,
            input_w,
            input_h,
            result_text,
            false,
        );

        // To unit dropdown button.
        let to_label = to_unit.map_or("Select", |u| u.name);
        self.render_dropdown_button(
            tree,
            to_section_x,
            dd_y,
            dd_w,
            dd_h,
            to_label,
            self.to_dropdown_open,
        );

        // --- Large result display ---
        let result_y: f32 = 190.0;
        tree.push(RenderCommand::FillRect {
            x: main_left + 16.0,
            y: result_y,
            width: main_width - 32.0,
            height: 80.0,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(10.0),
        });

        // Shadow hint.
        tree.push(RenderCommand::BoxShadow {
            x: main_left + 16.0,
            y: result_y,
            width: main_width - 32.0,
            height: 80.0,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 40),
            corner_radii: CornerRadii::all(10.0),
        });

        let from_sym = from_unit.map_or("?", |u| u.symbol);
        let to_sym = to_unit.map_or("?", |u| u.symbol);

        let mut result_line = String::new();
        let _ = write!(result_line, "{} {}", self.from_input, from_sym);
        tree.push(RenderCommand::Text {
            x: main_left + 32.0,
            y: result_y + 16.0,
            text: result_line,
            color: theme::SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(main_width - 64.0),
        });

        let mut big_result = String::new();
        let _ = write!(big_result, "= {} {}", result_text, to_sym);
        tree.push(RenderCommand::Text {
            x: main_left + 32.0,
            y: result_y + 40.0,
            text: big_result,
            color: theme::TEXT,
            font_size: 22.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(main_width - 64.0),
        });

        // --- Formula display ---
        let formula_y: f32 = 286.0;
        tree.push(RenderCommand::Text {
            x: main_left + 24.0,
            y: formula_y,
            text: String::from("Formula"),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        tree.push(RenderCommand::FillRect {
            x: main_left + 16.0,
            y: formula_y + 18.0,
            width: main_width - 32.0,
            height: 32.0,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::Text {
            x: main_left + 28.0,
            y: formula_y + 26.0,
            text: self.current_formula(),
            color: theme::LAVENDER,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(main_width - 56.0),
        });

        // --- Favorites toggle button ---
        let fav_btn_y: f32 = 356.0;
        let fav_btn_w: f32 = 120.0;
        let fav_btn_h: f32 = 28.0;
        tree.push(RenderCommand::FillRect {
            x: main_left + 20.0,
            y: fav_btn_y,
            width: fav_btn_w,
            height: fav_btn_h,
            color: if self.show_favorites {
                theme::SURFACE1
            } else {
                theme::SURFACE0
            },
            corner_radii: CornerRadii::all(6.0),
        });
        tree.push(RenderCommand::Text {
            x: main_left + 30.0,
            y: fav_btn_y + 7.0,
            text: if self.show_favorites {
                String::from("\u{2605} Hide Favorites")
            } else {
                String::from("\u{2606} Favorites")
            },
            color: theme::YELLOW,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(fav_btn_w - 20.0),
        });

        // --- Favorites list ---
        if self.show_favorites {
            let fav_start_y: f32 = 394.0;
            let fav_item_h: f32 = 28.0;

            if self.favorites.is_empty() {
                tree.push(RenderCommand::Text {
                    x: main_left + 28.0,
                    y: fav_start_y + 6.0,
                    text: String::from("No favorites yet. Press Ctrl+F to add."),
                    color: theme::OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(main_width - 56.0),
                });
            } else {
                for (i, fav) in self.favorites.iter().enumerate() {
                    let fy = fav_start_y + i as f32 * fav_item_h;
                    if fy + fav_item_h > WINDOW_HEIGHT {
                        break;
                    }

                    tree.push(RenderCommand::FillRect {
                        x: main_left + 20.0,
                        y: fy,
                        width: main_width - 40.0,
                        height: fav_item_h - 4.0,
                        color: theme::SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });

                    tree.push(RenderCommand::Text {
                        x: main_left + 30.0,
                        y: fy + 6.0,
                        text: fav.label(),
                        color: theme::TEXT,
                        font_size: 12.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(main_width - 60.0),
                    });
                }
            }
        }

        // --- Keyboard shortcuts hint ---
        let hint_y = WINDOW_HEIGHT - 32.0;
        tree.push(RenderCommand::Text {
            x: main_left + 20.0,
            y: hint_y,
            text: String::from("Ctrl+S: Swap | Ctrl+F: Favorite | Tab: Switch focus"),
            color: theme::OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(main_width - 40.0),
        });
    }

    /// Render an input field (or read-only result field).
    fn render_input_field(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        focused: bool,
    ) {
        let border_color = if focused {
            theme::BLUE
        } else {
            theme::SURFACE1
        };

        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: border_color,
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(8.0),
        });

        let text_color = if text == "..." || text == "Invalid input" {
            theme::OVERLAY0
        } else {
            theme::TEXT
        };

        tree.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 10.0,
            text: String::from(text),
            color: text_color,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 20.0),
        });

        // Cursor for focused input.
        if focused && self.from_focused {
            let cursor_x = x + 10.0 + self.from_cursor as f32 * 8.4;
            tree.push(RenderCommand::Line {
                x1: cursor_x,
                y1: y + 8.0,
                x2: cursor_x,
                y2: y + h - 8.0,
                color: theme::BLUE,
                width: 1.5,
            });
        }
    }

    /// Render a dropdown button (the closed selector).
    fn render_dropdown_button(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        is_open: bool,
    ) {
        let bg = if is_open {
            theme::SURFACE1
        } else {
            theme::SURFACE0
        };

        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: theme::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 8.0,
            text: String::from(label),
            color: theme::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 30.0),
        });

        // Dropdown arrow.
        let arrow = if is_open { "\u{25B2}" } else { "\u{25BC}" };
        tree.push(RenderCommand::Text {
            x: x + w - 20.0,
            y: y + 8.0,
            text: String::from(arrow),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render an open dropdown list overlay.
    fn render_dropdown(&self, tree: &mut RenderTree, is_from: bool) {
        let main_left = SIDEBAR_WIDTH;
        let main_right = WINDOW_WIDTH - HISTORY_PANEL_WIDTH;

        let dd_x = if is_from {
            main_left + 20.0
        } else {
            main_right - 20.0 - 180.0
        };
        let dd_y: f32 = 166.0;
        let dd_w: f32 = 180.0;
        let item_h: f32 = 28.0;

        let units = units_for_category(self.selected_category);
        let dd_h = units.len() as f32 * item_h + 8.0;
        let selected_idx = if is_from {
            self.from_unit_idx
        } else {
            self.to_unit_idx
        };

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x: dd_x,
            y: dd_y,
            width: dd_w,
            height: dd_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(8.0),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x: dd_x,
            y: dd_y,
            width: dd_w,
            height: dd_h,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x: dd_x,
            y: dd_y,
            width: dd_w,
            height: dd_h,
            color: theme::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Items.
        for (i, unit) in units.iter().enumerate() {
            let iy = dd_y + 4.0 + i as f32 * item_h;
            let is_sel = i == selected_idx;

            if is_sel {
                tree.push(RenderCommand::FillRect {
                    x: dd_x + 4.0,
                    y: iy,
                    width: dd_w - 8.0,
                    height: item_h - 2.0,
                    color: theme::BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let text_color = if is_sel { theme::MANTLE } else { theme::TEXT };

            // Symbol.
            tree.push(RenderCommand::Text {
                x: dd_x + 12.0,
                y: iy + 6.0,
                text: String::from(unit.symbol),
                color: text_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
            });

            // Full name.
            tree.push(RenderCommand::Text {
                x: dd_x + 56.0,
                y: iy + 6.0,
                text: String::from(unit.name),
                color: if is_sel {
                    theme::MANTLE
                } else {
                    theme::SUBTEXT0
                },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dd_w - 68.0),
            });
        }
    }

    /// Render the history/recent conversions panel on the right.
    fn render_history_panel(&self, tree: &mut RenderTree) {
        let panel_x = WINDOW_WIDTH - HISTORY_PANEL_WIDTH;

        // Separator.
        tree.push(RenderCommand::Line {
            x1: panel_x,
            y1: 0.0,
            x2: panel_x,
            y2: WINDOW_HEIGHT,
            color: theme::SURFACE0,
            width: 1.0,
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x: panel_x,
            y: 0.0,
            width: HISTORY_PANEL_WIDTH,
            height: WINDOW_HEIGHT,
            color: theme::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header.
        tree.push(RenderCommand::Text {
            x: panel_x + 16.0,
            y: 18.0,
            text: String::from("Recent Conversions"),
            color: theme::PEACH,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(HISTORY_PANEL_WIDTH - 32.0),
        });

        // Separator.
        tree.push(RenderCommand::Line {
            x1: panel_x + 12.0,
            y1: 42.0,
            x2: panel_x + HISTORY_PANEL_WIDTH - 12.0,
            y2: 42.0,
            color: theme::SURFACE1,
            width: 1.0,
        });

        // History entries.
        if self.history.is_empty() {
            tree.push(RenderCommand::Text {
                x: panel_x + 16.0,
                y: 56.0,
                text: String::from("No conversions yet."),
                color: theme::OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(HISTORY_PANEL_WIDTH - 32.0),
            });
            tree.push(RenderCommand::Text {
                x: panel_x + 16.0,
                y: 74.0,
                text: String::from("Type a value and it will"),
                color: theme::OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(HISTORY_PANEL_WIDTH - 32.0),
            });
            tree.push(RenderCommand::Text {
                x: panel_x + 16.0,
                y: 90.0,
                text: String::from("appear here."),
                color: theme::OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(HISTORY_PANEL_WIDTH - 32.0),
            });
        } else {
            let item_h: f32 = 52.0;
            let start_y: f32 = 50.0;
            let scroll = self.history_scroll;

            // Clip to panel area.
            tree.push(RenderCommand::PushClip {
                x: panel_x,
                y: start_y,
                width: HISTORY_PANEL_WIDTH,
                height: WINDOW_HEIGHT - start_y,
            });

            for (i, entry) in self.history.iter().enumerate() {
                let ey = start_y + i as f32 * item_h - scroll;

                if ey + item_h < start_y || ey > WINDOW_HEIGHT {
                    continue;
                }

                // Entry background.
                tree.push(RenderCommand::FillRect {
                    x: panel_x + 8.0,
                    y: ey + 2.0,
                    width: HISTORY_PANEL_WIDTH - 16.0,
                    height: item_h - 6.0,
                    color: theme::SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Category badge.
                let cat_color = entry.category.accent();
                tree.push(RenderCommand::FillRect {
                    x: panel_x + 14.0,
                    y: ey + 8.0,
                    width: 4.0,
                    height: item_h - 18.0,
                    color: cat_color,
                    corner_radii: CornerRadii::all(2.0),
                });

                // Conversion text.
                tree.push(RenderCommand::Text {
                    x: panel_x + 26.0,
                    y: ey + 10.0,
                    text: entry.display(),
                    color: theme::TEXT,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(HISTORY_PANEL_WIDTH - 50.0),
                });

                // Category label.
                tree.push(RenderCommand::Text {
                    x: panel_x + 26.0,
                    y: ey + 28.0,
                    text: String::from(entry.category.name()),
                    color: theme::OVERLAY0,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(HISTORY_PANEL_WIDTH - 50.0),
                });
            }

            tree.push(RenderCommand::PopClip);
        }
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let mut app = UnitConverterApp::new();
    // Perform initial conversion so display is populated.
    app.do_convert();

    // Build widget tree.
    let root = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_background(theme::BASE);
    let mut widget_tree = WidgetTree::new(root, WINDOW_WIDTH, WINDOW_HEIGHT);
    widget_tree.layout();

    // Render initial frame.
    let _frame = app.render();

    // In a real OS environment, this would enter an event loop:
    //   loop {
    //       let event = wait_for_event();
    //       app.handle_event(&event);
    //       let frame = app.render();
    //       compositor_submit(frame);
    //   }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    // -- Conversion engine tests --

    #[test]
    fn test_meters_to_kilometers() {
        let units = units_for_category(Category::Length);
        let m = &units[2]; // m
        let km = &units[3]; // km
        let result = convert(1000.0, m, km);
        assert!((result - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_kilometers_to_meters() {
        let units = units_for_category(Category::Length);
        let m = &units[2];
        let km = &units[3];
        let result = convert(1.0, km, m);
        assert!((result - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn test_inches_to_centimeters() {
        let units = units_for_category(Category::Length);
        let inch = &units[4]; // in
        let cm = &units[1]; // cm
        let result = convert(1.0, inch, cm);
        assert!((result - 2.54).abs() < 1e-9);
    }

    #[test]
    fn test_miles_to_km() {
        let units = units_for_category(Category::Length);
        let mi = &units[7]; // mi
        let km = &units[3]; // km
        let result = convert(1.0, mi, km);
        assert!((result - 1.609344).abs() < 1e-6);
    }

    #[test]
    fn test_feet_to_meters() {
        let units = units_for_category(Category::Length);
        let ft = &units[5]; // ft
        let m = &units[2]; // m
        let result = convert(1.0, ft, m);
        assert!((result - 0.3048).abs() < 1e-9);
    }

    #[test]
    fn test_yard_to_feet() {
        let units = units_for_category(Category::Length);
        let yd = &units[6];
        let ft = &units[5];
        let result = convert(1.0, yd, ft);
        assert!((result - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_nautical_miles_to_km() {
        let units = units_for_category(Category::Length);
        let nmi = &units[8];
        let km = &units[3];
        let result = convert(1.0, nmi, km);
        assert!((result - 1.852).abs() < 1e-6);
    }

    #[test]
    fn test_kg_to_pounds() {
        let units = units_for_category(Category::Weight);
        let kg = &units[2]; // kg
        let lb = &units[5]; // lb
        let result = convert(1.0, kg, lb);
        assert!((result - 2.204_622_6).abs() < 1e-4);
    }

    #[test]
    fn test_ounces_to_grams() {
        let units = units_for_category(Category::Weight);
        let oz = &units[4]; // oz
        let g = &units[1]; // g
        let result = convert(1.0, oz, g);
        assert!((result - 28.349_523_125).abs() < 1e-6);
    }

    #[test]
    fn test_stone_to_kg() {
        let units = units_for_category(Category::Weight);
        let st = &units[6];
        let kg = &units[2];
        let result = convert(1.0, st, kg);
        assert!((result - 6.350_293_18).abs() < 1e-6);
    }

    #[test]
    fn test_metric_ton_to_kg() {
        let units = units_for_category(Category::Weight);
        let t = &units[3]; // metric ton
        let kg = &units[2];
        let result = convert(1.0, t, kg);
        assert!((result - 1000.0).abs() < 1e-9);
    }

    // -- Temperature tests (affine) --

    #[test]
    fn test_celsius_to_fahrenheit() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let f = &units[1];
        let result = convert(100.0, c, f);
        assert!((result - 212.0).abs() < 0.01);
    }

    #[test]
    fn test_fahrenheit_to_celsius() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let f = &units[1];
        let result = convert(32.0, f, c);
        assert!(result.abs() < 0.01);
    }

    #[test]
    fn test_celsius_to_kelvin() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let k = &units[2];
        let result = convert(0.0, c, k);
        assert!((result - 273.15).abs() < 0.01);
    }

    #[test]
    fn test_kelvin_to_celsius() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let k = &units[2];
        let result = convert(373.15, k, c);
        assert!((result - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_fahrenheit_to_kelvin() {
        let units = units_for_category(Category::Temperature);
        let f = &units[1];
        let k = &units[2];
        // 32F = 273.15K
        let result = convert(32.0, f, k);
        assert!((result - 273.15).abs() < 0.1);
    }

    #[test]
    fn test_rankine_to_kelvin() {
        let units = units_for_category(Category::Temperature);
        let r = &units[3];
        let k = &units[2];
        // 491.67 R = 273.15 K
        let result = convert(491.67, r, k);
        assert!((result - 273.15).abs() < 0.1);
    }

    #[test]
    fn test_celsius_to_rankine() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let r = &units[3];
        // 0C = 273.15K = 491.67R
        let result = convert(0.0, c, r);
        assert!((result - 491.67).abs() < 0.1);
    }

    // -- Volume tests --

    #[test]
    fn test_liters_to_gallons_us() {
        let units = units_for_category(Category::Volume);
        let l = &units[1]; // L
        let gal = &units[2]; // gal US
        let result = convert(1.0, l, gal);
        assert!((result - 0.264_172).abs() < 1e-3);
    }

    #[test]
    fn test_cup_to_ml() {
        let units = units_for_category(Category::Volume);
        let cup = &units[6];
        let ml = &units[0];
        let result = convert(1.0, cup, ml);
        assert!((result - 236.588).abs() < 1.0);
    }

    #[test]
    fn test_tablespoon_to_teaspoon() {
        let units = units_for_category(Category::Volume);
        let tbsp = &units[8];
        let tsp = &units[9];
        let result = convert(1.0, tbsp, tsp);
        assert!((result - 3.0).abs() < 0.01);
    }

    // -- Area tests --

    #[test]
    fn test_hectare_to_acres() {
        let units = units_for_category(Category::Area);
        let ha = &units[4];
        let ac = &units[5];
        let result = convert(1.0, ha, ac);
        assert!((result - 2.471_054).abs() < 1e-3);
    }

    #[test]
    fn test_sqkm_to_sqmi() {
        let units = units_for_category(Category::Area);
        let sqkm = &units[3]; // km^2
        let sqmi = &units[8]; // mi^2
        let result = convert(1.0, sqkm, sqmi);
        assert!((result - 0.386_102).abs() < 1e-3);
    }

    #[test]
    fn test_sqft_to_sqm() {
        let units = units_for_category(Category::Area);
        let sqft = &units[6]; // ft^2
        let sqm = &units[2]; // m^2
        let result = convert(1.0, sqft, sqm);
        assert!((result - 0.092_903_04).abs() < 1e-6);
    }

    // -- Speed tests --

    #[test]
    fn test_kmh_to_mph() {
        let units = units_for_category(Category::Speed);
        let kmh = &units[1]; // km/h
        let mph = &units[2]; // mph
        let result = convert(100.0, kmh, mph);
        assert!((result - 62.137).abs() < 0.1);
    }

    #[test]
    fn test_mach_to_ms() {
        let units = units_for_category(Category::Speed);
        let mach = &units[5]; // Mach
        let ms = &units[0]; // m/s
        let result = convert(1.0, mach, ms);
        assert!((result - 340.29).abs() < 0.1);
    }

    #[test]
    fn test_knots_to_kmh() {
        let units = units_for_category(Category::Speed);
        let kn = &units[3];
        let kmh = &units[1];
        let result = convert(1.0, kn, kmh);
        assert!((result - 1.852).abs() < 0.01);
    }

    // -- Time tests --

    #[test]
    fn test_hours_to_minutes() {
        let units = units_for_category(Category::Time);
        let h = &units[3]; // h
        let min = &units[2]; // min
        let result = convert(1.0, h, min);
        assert!((result - 60.0).abs() < 1e-9);
    }

    #[test]
    fn test_days_to_hours() {
        let units = units_for_category(Category::Time);
        let day = &units[4];
        let h = &units[3];
        let result = convert(1.0, day, h);
        assert!((result - 24.0).abs() < 1e-9);
    }

    #[test]
    fn test_weeks_to_days() {
        let units = units_for_category(Category::Time);
        let wk = &units[5];
        let day = &units[4];
        let result = convert(1.0, wk, day);
        assert!((result - 7.0).abs() < 1e-9);
    }

    #[test]
    fn test_year_to_days() {
        let units = units_for_category(Category::Time);
        let yr = &units[7];
        let day = &units[4];
        let result = convert(1.0, yr, day);
        // Julian year = 365.25 days
        assert!((result - 365.25).abs() < 0.01);
    }

    // -- Digital storage tests --

    #[test]
    fn test_gb_to_mb() {
        let units = units_for_category(Category::DigitalStorage);
        let gb = &units[4]; // GB
        let mb = &units[3]; // MB
        let result = convert(1.0, gb, mb);
        assert!((result - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_gib_to_mib() {
        let units = units_for_category(Category::DigitalStorage);
        let gib = &units[9]; // GiB
        let mib = &units[8]; // MiB
        let result = convert(1.0, gib, mib);
        assert!((result - 1024.0).abs() < 1e-6);
    }

    #[test]
    fn test_byte_to_bits() {
        let units = units_for_category(Category::DigitalStorage);
        let byte = &units[1]; // B
        let bit = &units[0]; // bit
        let result = convert(1.0, byte, bit);
        assert!((result - 8.0).abs() < 1e-9);
    }

    #[test]
    fn test_tb_to_gb() {
        let units = units_for_category(Category::DigitalStorage);
        let tb = &units[5]; // TB
        let gb = &units[4]; // GB
        let result = convert(1.0, tb, gb);
        assert!((result - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_kib_to_bytes() {
        let units = units_for_category(Category::DigitalStorage);
        let kib = &units[7]; // KiB
        let b = &units[1]; // B
        let result = convert(1.0, kib, b);
        assert!((result - 1024.0).abs() < 1e-9);
    }

    // -- Pressure tests --

    #[test]
    fn test_atm_to_psi() {
        let units = units_for_category(Category::Pressure);
        let atm = &units[4]; // atm
        let psi = &units[5]; // psi
        let result = convert(1.0, atm, psi);
        assert!((result - 14.696).abs() < 0.01);
    }

    #[test]
    fn test_bar_to_kpa() {
        let units = units_for_category(Category::Pressure);
        let bar = &units[3]; // bar
        let kpa = &units[1]; // kPa
        let result = convert(1.0, bar, kpa);
        assert!((result - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_mmhg_to_inhg() {
        let units = units_for_category(Category::Pressure);
        let mmhg = &units[6]; // mmHg
        let inhg = &units[7]; // inHg
        let result = convert(25.4, mmhg, inhg);
        assert!((result - 1.0).abs() < 0.01);
    }

    // -- Energy tests --

    #[test]
    fn test_kwh_to_joules() {
        let units = units_for_category(Category::Energy);
        let kwh = &units[5]; // kWh
        let j = &units[0]; // J
        let result = convert(1.0, kwh, j);
        assert!((result - 3_600_000.0).abs() < 1.0);
    }

    #[test]
    fn test_calorie_to_joules() {
        let units = units_for_category(Category::Energy);
        let cal = &units[2]; // cal
        let j = &units[0]; // J
        let result = convert(1.0, cal, j);
        assert!((result - 4.184).abs() < 1e-9);
    }

    #[test]
    fn test_kcal_to_cal() {
        let units = units_for_category(Category::Energy);
        let kcal = &units[3]; // kcal
        let cal = &units[2]; // cal
        let result = convert(1.0, kcal, cal);
        assert!((result - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_btu_to_kj() {
        let units = units_for_category(Category::Energy);
        let btu = &units[6]; // BTU
        let kj = &units[1]; // kJ
        let result = convert(1.0, btu, kj);
        assert!((result - 1.055_06).abs() < 1e-3);
    }

    // -- Frequency tests --

    #[test]
    fn test_mhz_to_khz() {
        let units = units_for_category(Category::Frequency);
        let mhz = &units[2]; // MHz
        let khz = &units[1]; // kHz
        let result = convert(1.0, mhz, khz);
        assert!((result - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_ghz_to_mhz() {
        let units = units_for_category(Category::Frequency);
        let ghz = &units[3]; // GHz
        let mhz = &units[2]; // MHz
        let result = convert(1.0, ghz, mhz);
        assert!((result - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_rpm_to_hz() {
        let units = units_for_category(Category::Frequency);
        let rpm = &units[4]; // rpm
        let hz = &units[0]; // Hz
        let result = convert(60.0, rpm, hz);
        assert!((result - 1.0).abs() < 1e-9);
    }

    // -- Angle tests --

    #[test]
    fn test_degrees_to_radians() {
        let units = units_for_category(Category::Angle);
        let deg = &units[0]; // deg
        let rad = &units[1]; // rad
        let result = convert(180.0, deg, rad);
        assert!((result - std::f64::consts::PI).abs() < 1e-9);
    }

    #[test]
    fn test_radians_to_degrees() {
        let units = units_for_category(Category::Angle);
        let deg = &units[0];
        let rad = &units[1];
        let result = convert(std::f64::consts::PI, rad, deg);
        assert!((result - 180.0).abs() < 1e-9);
    }

    #[test]
    fn test_turn_to_degrees() {
        let units = units_for_category(Category::Angle);
        let turn = &units[5]; // turn
        let deg = &units[0]; // deg
        let result = convert(1.0, turn, deg);
        assert!((result - 360.0).abs() < 1e-6);
    }

    #[test]
    fn test_gradians_to_degrees() {
        let units = units_for_category(Category::Angle);
        let grad = &units[2]; // grad
        let deg = &units[0]; // deg
        let result = convert(200.0, grad, deg);
        assert!((result - 180.0).abs() < 1e-6);
    }

    #[test]
    fn test_arcminute_to_degrees() {
        let units = units_for_category(Category::Angle);
        let arcmin = &units[3]; // arcmin
        let deg = &units[0]; // deg
        let result = convert(60.0, arcmin, deg);
        assert!((result - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_arcsecond_to_arcminute() {
        let units = units_for_category(Category::Angle);
        let arcsec = &units[4]; // arcsec
        let arcmin = &units[3]; // arcmin
        let result = convert(60.0, arcsec, arcmin);
        assert!((result - 1.0).abs() < 1e-9);
    }

    // -- Edge case tests --

    #[test]
    fn test_zero_conversion() {
        let units = units_for_category(Category::Length);
        let m = &units[2];
        let km = &units[3];
        let result = convert(0.0, m, km);
        assert!((result - 0.0).abs() < 1e-15);
    }

    #[test]
    fn test_negative_conversion() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0]; // Celsius
        let f = &units[1]; // Fahrenheit
        // -40 C = -40 F
        let result = convert(-40.0, c, f);
        assert!((result - (-40.0)).abs() < 0.01);
    }

    #[test]
    fn test_very_large_value() {
        let units = units_for_category(Category::DigitalStorage);
        let pb = &units[6]; // PB
        let b = &units[1]; // Byte
        let result = convert(1.0, pb, b);
        assert!((result - 1e15).abs() < 1e6);
    }

    #[test]
    fn test_very_small_value() {
        let units = units_for_category(Category::Length);
        let mm = &units[0]; // mm
        let km = &units[3]; // km
        let result = convert(0.001, mm, km);
        assert!((result - 1e-9).abs() < 1e-15);
    }

    #[test]
    fn test_same_unit_conversion() {
        let units = units_for_category(Category::Length);
        let m = &units[2];
        let result = convert(42.0, m, m);
        assert!((result - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_identity_roundtrip() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let f = &units[1];
        let original = 37.0;
        let intermediate = convert(original, c, f);
        let roundtrip = convert(intermediate, f, c);
        assert!((roundtrip - original).abs() < 1e-9);
    }

    // -- format_number tests --

    #[test]
    fn test_format_zero() {
        assert_eq!(format_number(0.0), "0");
    }

    #[test]
    fn test_format_integer() {
        assert_eq!(format_number(42.0), "42");
    }

    #[test]
    fn test_format_decimal() {
        // 3.25 — exactly representable, dodges clippy::approx_constant.
        let s = format_number(3.25);
        assert!(s.starts_with("3.25"));
    }

    #[test]
    fn test_format_nan() {
        assert_eq!(format_number(f64::NAN), "NaN");
    }

    #[test]
    fn test_format_infinity() {
        assert_eq!(format_number(f64::INFINITY), "Inf");
        assert_eq!(format_number(f64::NEG_INFINITY), "-Inf");
    }

    #[test]
    fn test_format_scientific() {
        let s = format_number(1.602e-19);
        assert!(s.contains('e'));
    }

    // -- History tests --

    #[test]
    fn test_history_starts_empty() {
        let app = UnitConverterApp::new();
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_history_records_conversion() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("1000");
        app.do_convert();
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_history_max_20() {
        let mut app = UnitConverterApp::new();
        for i in 0..25 {
            app.from_input = format!("{}", i + 1);
            app.do_convert();
        }
        assert_eq!(app.history.len(), MAX_HISTORY);
    }

    #[test]
    fn test_history_most_recent_first() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("100");
        app.do_convert();
        app.from_input = String::from("200");
        app.do_convert();
        // Most recent should be from_value = 200.
        assert!((app.history[0].from_value - 200.0).abs() < 1e-9);
    }

    // -- Favorites tests --

    #[test]
    fn test_favorites_starts_empty() {
        let app = UnitConverterApp::new();
        assert!(app.favorites.is_empty());
    }

    #[test]
    fn test_toggle_favorite_adds() {
        let mut app = UnitConverterApp::new();
        app.toggle_favorite();
        assert_eq!(app.favorites.len(), 1);
        assert!(app.is_current_favorite());
    }

    #[test]
    fn test_toggle_favorite_removes() {
        let mut app = UnitConverterApp::new();
        app.toggle_favorite();
        assert!(app.is_current_favorite());
        app.toggle_favorite();
        assert!(!app.is_current_favorite());
        assert!(app.favorites.is_empty());
    }

    #[test]
    fn test_apply_favorite() {
        let mut app = UnitConverterApp::new();
        // Add a favorite for Length: m -> km.
        app.toggle_favorite();

        // Switch to weight.
        app.select_category(Category::Weight);
        assert_eq!(app.selected_category, Category::Weight);

        // Apply favorite 0 (Length: m -> km).
        app.apply_favorite(0);
        assert_eq!(app.selected_category, Category::Length);
    }

    #[test]
    fn test_favorite_label() {
        let fav = Favorite {
            category: Category::Length,
            from_idx: 2,
            to_idx: 3,
        };
        let label = fav.label();
        assert!(label.contains("Length"));
        assert!(label.contains("m"));
        assert!(label.contains("km"));
    }

    // -- Swap tests --

    #[test]
    fn test_swap_units() {
        let mut app = UnitConverterApp::new();
        let orig_from = app.from_unit_idx;
        let orig_to = app.to_unit_idx;
        app.from_input = String::from("1");
        app.do_convert();
        app.swap_units();
        assert_eq!(app.from_unit_idx, orig_to);
        assert_eq!(app.to_unit_idx, orig_from);
    }

    // -- Category selection tests --

    #[test]
    fn test_select_category_resets_indices() {
        let mut app = UnitConverterApp::new();
        app.from_unit_idx = 5;
        app.to_unit_idx = 7;
        app.select_category(Category::Temperature);
        assert_eq!(app.from_unit_idx, 0);
        assert_eq!(app.to_unit_idx, 1);
        assert_eq!(app.selected_category, Category::Temperature);
    }

    #[test]
    fn test_select_same_category_noop() {
        let mut app = UnitConverterApp::new();
        app.from_unit_idx = 5;
        app.select_category(Category::Length); // already Length
        assert_eq!(app.from_unit_idx, 5); // should not reset
    }

    // -- App state tests --

    #[test]
    fn test_new_app_default_state() {
        let app = UnitConverterApp::new();
        assert_eq!(app.selected_category, Category::Length);
        assert_eq!(app.from_unit_idx, 2);
        assert_eq!(app.to_unit_idx, 3);
        assert_eq!(app.from_input, "1");
        assert!(app.history.is_empty());
        assert!(app.favorites.is_empty());
        assert!(!app.from_dropdown_open);
        assert!(!app.to_dropdown_open);
    }

    #[test]
    fn test_invalid_input_shows_error() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("abc");
        app.do_convert();
        assert_eq!(app.to_display, "Invalid input");
    }

    #[test]
    fn test_do_convert_updates_display() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("1000");
        app.from_unit_idx = 2; // m
        app.to_unit_idx = 3; // km
        app.do_convert();
        assert_eq!(app.to_display, "1");
    }

    // -- Render tests --

    #[test]
    fn test_render_produces_commands() {
        let mut app = UnitConverterApp::new();
        app.do_convert();
        let frame = app.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_render_with_history() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("100");
        app.do_convert();
        app.from_input = String::from("200");
        app.do_convert();
        let frame = app.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_render_with_dropdown_open() {
        let mut app = UnitConverterApp::new();
        app.do_convert();
        app.from_dropdown_open = true;
        let frame = app.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_render_with_favorites_visible() {
        let mut app = UnitConverterApp::new();
        app.do_convert();
        app.toggle_favorite();
        app.show_favorites = true;
        let frame = app.render();
        assert!(!frame.is_empty());
    }

    // -- Event handling tests --

    #[test]
    fn test_escape_closes_dropdown() {
        let mut app = UnitConverterApp::new();
        app.from_dropdown_open = true;
        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let consumed = app.handle_key(&key);
        assert!(consumed);
        assert!(!app.from_dropdown_open);
    }

    #[test]
    fn test_tab_switches_focus() {
        let mut app = UnitConverterApp::new();
        assert!(app.from_focused);
        let key = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&key);
        assert!(!app.from_focused);
    }

    #[test]
    fn test_ctrl_s_swaps() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("1");
        app.do_convert();
        let orig_from = app.from_unit_idx;
        let orig_to = app.to_unit_idx;
        let key = KeyEvent {
            key: Key::S,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        app.handle_key(&key);
        assert_eq!(app.from_unit_idx, orig_to);
        assert_eq!(app.to_unit_idx, orig_from);
    }

    #[test]
    fn test_ctrl_f_toggles_favorite() {
        let mut app = UnitConverterApp::new();
        assert!(!app.is_current_favorite());
        let key = KeyEvent {
            key: Key::F,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        app.handle_key(&key);
        assert!(app.is_current_favorite());
    }

    #[test]
    fn test_typing_digit_updates_input() {
        let mut app = UnitConverterApp::new();
        app.from_input.clear();
        app.from_cursor = 0;
        let key = KeyEvent {
            key: Key::Num5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('5'),
        };
        app.handle_key(&key);
        assert_eq!(app.from_input, "5");
    }

    #[test]
    fn test_backspace_deletes_char() {
        let mut app = UnitConverterApp::new();
        app.from_input = String::from("123");
        app.from_cursor = 3;
        let key = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&key);
        assert_eq!(app.from_input, "12");
        assert_eq!(app.from_cursor, 2);
    }

    // -- HistoryEntry display test --

    #[test]
    fn test_history_entry_display() {
        let entry = HistoryEntry {
            category: Category::Length,
            from_value: 1000.0,
            from_symbol: "m",
            to_value: 1.0,
            to_symbol: "km",
        };
        let display = entry.display();
        assert!(display.contains("1000"));
        assert!(display.contains("m"));
        assert!(display.contains("km"));
    }

    // -- formula_text tests --

    #[test]
    fn test_formula_linear() {
        let units = units_for_category(Category::Length);
        let m = &units[2];
        let km = &units[3];
        let formula = formula_text(m, km);
        assert!(formula.contains("m"));
        assert!(formula.contains("km"));
    }

    #[test]
    fn test_formula_temperature() {
        let units = units_for_category(Category::Temperature);
        let c = &units[0];
        let f = &units[1];
        let formula = formula_text(c, f);
        // Temperature formula should be affine, not just a ratio.
        assert!(formula.contains(c.symbol));
        assert!(formula.contains(f.symbol));
    }

    // -- Category enum tests --

    #[test]
    fn test_all_categories_count() {
        assert_eq!(Category::ALL.len(), 12);
    }

    #[test]
    fn test_each_category_has_units() {
        for cat in Category::ALL {
            let units = units_for_category(*cat);
            assert!(
                units.len() >= 2,
                "Category {:?} should have at least 2 units",
                cat
            );
        }
    }

    #[test]
    fn test_category_names_nonempty() {
        for cat in Category::ALL {
            assert!(!cat.name().is_empty());
        }
    }

    #[test]
    fn test_category_icons_nonempty() {
        for cat in Category::ALL {
            assert!(!cat.icon().is_empty());
        }
    }

    // -- Additional cross-category conversion tests --

    #[test]
    fn test_gallon_uk_to_liters() {
        let units = units_for_category(Category::Volume);
        let gal_uk = &units[3]; // gal UK
        let l = &units[1]; // L
        let result = convert(1.0, gal_uk, l);
        assert!((result - 4.546_09).abs() < 1e-3);
    }

    #[test]
    fn test_us_ton_to_kg() {
        let units = units_for_category(Category::Weight);
        let us_ton = &units[7]; // US ton
        let kg = &units[2]; // kg
        let result = convert(1.0, us_ton, kg);
        assert!((result - 907.184_74).abs() < 0.01);
    }

    #[test]
    fn test_ev_to_joules() {
        let units = units_for_category(Category::Energy);
        let ev = &units[7]; // eV
        let j = &units[0]; // J
        let result = convert(1.0, ev, j);
        assert!((result - 1.602_176_634e-19).abs() < 1e-28);
    }

    #[test]
    fn test_mpa_to_psi() {
        let units = units_for_category(Category::Pressure);
        let mpa = &units[2]; // MPa
        let psi = &units[5]; // psi
        let result = convert(1.0, mpa, psi);
        assert!((result - 145.038).abs() < 0.1);
    }

    #[test]
    fn test_wh_to_joules() {
        let units = units_for_category(Category::Energy);
        let wh = &units[4]; // Wh
        let j = &units[0]; // J
        let result = convert(1.0, wh, j);
        assert!((result - 3600.0).abs() < 1e-6);
    }

    #[test]
    fn test_ms_to_seconds() {
        let units = units_for_category(Category::Time);
        let ms = &units[0]; // ms
        let s = &units[1]; // s
        let result = convert(1000.0, ms, s);
        assert!((result - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_sqin_to_sqcm() {
        let units = units_for_category(Category::Area);
        let sqin = &units[7]; // in^2
        let sqcm = &units[1]; // cm^2
        let result = convert(1.0, sqin, sqcm);
        assert!((result - 6.4516).abs() < 1e-3);
    }

    #[test]
    fn test_fts_to_ms() {
        let units = units_for_category(Category::Speed);
        let fts = &units[4]; // ft/s
        let ms = &units[0]; // m/s
        let result = convert(1.0, fts, ms);
        assert!((result - 0.3048).abs() < 1e-6);
    }

    #[test]
    fn test_tib_to_gib() {
        let units = units_for_category(Category::DigitalStorage);
        let tib = &units[10]; // TiB
        let gib = &units[9]; // GiB
        let result = convert(1.0, tib, gib);
        assert!((result - 1024.0).abs() < 1e-6);
    }

    #[test]
    fn test_fl_oz_to_ml() {
        let units = units_for_category(Category::Volume);
        let floz = &units[7]; // fl oz
        let ml = &units[0]; // mL
        let result = convert(1.0, floz, ml);
        assert!((result - 29.573_529).abs() < 0.01);
    }

    #[test]
    fn test_pint_to_cups() {
        let units = units_for_category(Category::Volume);
        let pt = &units[5]; // pt
        let cup = &units[6]; // cup
        let result = convert(1.0, pt, cup);
        assert!((result - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_quart_to_pint() {
        let units = units_for_category(Category::Volume);
        let qt = &units[4]; // qt
        let pt = &units[5]; // pt
        let result = convert(1.0, qt, pt);
        assert!((result - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_mg_to_grams() {
        let units = units_for_category(Category::Weight);
        let mg = &units[0]; // mg
        let g = &units[1]; // g
        let result = convert(1000.0, mg, g);
        assert!((result - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_mm_to_inches() {
        let units = units_for_category(Category::Length);
        let mm = &units[0]; // mm
        let inch = &units[4]; // in
        let result = convert(25.4, mm, inch);
        assert!((result - 1.0).abs() < 1e-6);
    }
}
