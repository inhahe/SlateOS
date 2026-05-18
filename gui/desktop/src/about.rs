//! About dialog and system branding module.
//!
//! Provides the "About This Computer" dialog showing:
//! - OS branding with logo area and version
//! - Hardware information (CPU, memory, GPU, display)
//! - Software information (kernel version, architecture, hostname, uptime)
//! - Open-source license viewer
//!
//! Uses a tabbed layout with four tabs: Overview, Hardware, Software, Licenses.
//! System information is gathered into a `SystemInfo` struct and can be
//! serialized/deserialized in a simple key=value text format for persistence.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_PEACH: Color = Color::from_hex(0xFAB387);

// ============================================================================
// System information
// ============================================================================

/// Collected system information for the About dialog.
#[derive(Clone, Debug)]
pub struct SystemInfo {
    /// OS name (e.g., "OurOS").
    pub os_name: String,
    /// OS version string (e.g., "0.1.0-dev").
    pub os_version: String,
    /// Kernel version string.
    pub kernel_version: String,
    /// Build date string (e.g., "2026-05-18").
    pub build_date: String,
    /// CPU architecture (e.g., "x86_64").
    pub architecture: String,
    /// Machine hostname.
    pub hostname: String,
    /// CPU model name (e.g., "Intel Core i9-13900K").
    pub cpu_model: String,
    /// Number of logical CPU cores.
    pub cpu_cores: u32,
    /// Total physical RAM in megabytes.
    pub total_memory_mb: u64,
    /// GPU model name (e.g., "NVIDIA RTX 4090").
    pub gpu_model: String,
    /// Display resolution string (e.g., "1920x1080").
    pub display_resolution: String,
    /// System uptime in seconds.
    pub uptime_seconds: u64,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            os_name: "OurOS".to_string(),
            os_version: "0.1.0-dev".to_string(),
            kernel_version: "0.1.0".to_string(),
            build_date: "2026-05-18".to_string(),
            architecture: "x86_64".to_string(),
            hostname: "ouros".to_string(),
            cpu_model: "Unknown CPU".to_string(),
            cpu_cores: 1,
            total_memory_mb: 0,
            gpu_model: "Unknown GPU".to_string(),
            display_resolution: "1920x1080".to_string(),
            uptime_seconds: 0,
        }
    }
}

impl SystemInfo {
    /// Serialize to key=value text format for persistence.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# System Information\n");
        out.push_str(&format!("os_name={}\n", self.os_name));
        out.push_str(&format!("os_version={}\n", self.os_version));
        out.push_str(&format!("kernel_version={}\n", self.kernel_version));
        out.push_str(&format!("build_date={}\n", self.build_date));
        out.push_str(&format!("architecture={}\n", self.architecture));
        out.push_str(&format!("hostname={}\n", self.hostname));
        out.push_str(&format!("cpu_model={}\n", self.cpu_model));
        out.push_str(&format!("cpu_cores={}\n", self.cpu_cores));
        out.push_str(&format!("total_memory_mb={}\n", self.total_memory_mb));
        out.push_str(&format!("gpu_model={}\n", self.gpu_model));
        out.push_str(&format!("display_resolution={}\n", self.display_resolution));
        out.push_str(&format!("uptime_seconds={}\n", self.uptime_seconds));
        out
    }

    /// Parse from key=value text format.
    pub fn from_text(text: &str) -> Self {
        let mut info = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "os_name" => info.os_name = val.to_string(),
                    "os_version" => info.os_version = val.to_string(),
                    "kernel_version" => info.kernel_version = val.to_string(),
                    "build_date" => info.build_date = val.to_string(),
                    "architecture" => info.architecture = val.to_string(),
                    "hostname" => info.hostname = val.to_string(),
                    "cpu_model" => info.cpu_model = val.to_string(),
                    "cpu_cores" => {
                        if let Ok(v) = val.parse::<u32>() {
                            info.cpu_cores = v;
                        }
                    }
                    "total_memory_mb" => {
                        if let Ok(v) = val.parse::<u64>() {
                            info.total_memory_mb = v;
                        }
                    }
                    "gpu_model" => info.gpu_model = val.to_string(),
                    "display_resolution" => info.display_resolution = val.to_string(),
                    "uptime_seconds" => {
                        if let Ok(v) = val.parse::<u64>() {
                            info.uptime_seconds = v;
                        }
                    }
                    _ => {} // Ignore unknown keys for forward compatibility.
                }
            }
        }
        info
    }
}

// ============================================================================
// License information
// ============================================================================

/// A single open-source license entry.
#[derive(Clone, Debug)]
pub struct LicenseInfo {
    /// Component or library name.
    pub name: String,
    /// Full license text.
    pub text: String,
}

impl LicenseInfo {
    /// Create a new license entry.
    pub fn new(name: &str, text: &str) -> Self {
        Self {
            name: name.to_string(),
            text: text.to_string(),
        }
    }
}

// ============================================================================
// About tab enum
// ============================================================================

/// Tabs available in the About dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboutTab {
    /// OS branding, version, and description.
    Overview,
    /// CPU, memory, GPU, display hardware details.
    Hardware,
    /// Kernel version, architecture, hostname, uptime.
    Software,
    /// Open-source license listing.
    Licenses,
}

impl AboutTab {
    /// All tab variants in display order.
    pub const ALL: &'static [Self] = &[
        Self::Overview,
        Self::Hardware,
        Self::Software,
        Self::Licenses,
    ];

    /// Human-readable tab label.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Hardware => "Hardware",
            Self::Software => "Software",
            Self::Licenses => "Licenses",
        }
    }
}

// ============================================================================
// About dialog
// ============================================================================

/// The "About This Computer" dialog state and renderer.
#[derive(Clone, Debug)]
pub struct AboutDialog {
    /// Whether the dialog is currently visible.
    pub visible: bool,
    /// System information to display.
    pub system_info: SystemInfo,
    /// Bundled license entries.
    pub licenses: Vec<LicenseInfo>,
    /// Currently active tab.
    pub active_tab: AboutTab,
    /// Scroll offset for the Licenses tab (pixels scrolled).
    pub scroll_offset: f32,
}

impl Default for AboutDialog {
    fn default() -> Self {
        Self {
            visible: false,
            system_info: SystemInfo::default(),
            licenses: Vec::new(),
            active_tab: AboutTab::Overview,
            scroll_offset: 0.0,
        }
    }
}

impl AboutDialog {
    /// Create a new About dialog with the given system info and licenses.
    pub fn new(system_info: SystemInfo, licenses: Vec<LicenseInfo>) -> Self {
        Self {
            visible: false,
            system_info,
            licenses,
            active_tab: AboutTab::Overview,
            scroll_offset: 0.0,
        }
    }

    /// Show the dialog (reset to Overview tab and zero scroll).
    pub fn show(&mut self) {
        self.visible = true;
        self.active_tab = AboutTab::Overview;
        self.scroll_offset = 0.0;
    }

    /// Hide the dialog.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Switch to a specific tab, resetting scroll offset.
    pub fn set_tab(&mut self, tab: AboutTab) {
        self.active_tab = tab;
        self.scroll_offset = 0.0;
    }

    /// Scroll the license tab by a delta (positive = scroll down).
    pub fn scroll(&mut self, delta: f32) {
        self.scroll_offset = (self.scroll_offset + delta).max(0.0);
    }

    /// Render the dialog, producing a list of render commands.
    /// The dialog is positioned at (`x`, `y`) with the given `width`/`height`.
    /// Returns an empty Vec if the dialog is not visible.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(64);

        // Dialog background with rounded corners.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: COL_BASE,
            corner_radii: CornerRadii::all(12.0),
        });

        // Subtle border.
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color: COL_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title bar.
        cmds.push(RenderCommand::Text {
            x: x + 20.0,
            y: y + 16.0,
            text: "About This Computer".to_string(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 40.0),
        });

        // Tab bar.
        let tab_y = y + 50.0;
        for (i, tab) in AboutTab::ALL.iter().enumerate() {
            let tab_x = x + 20.0 + i as f32 * 110.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: 100.0,
                    height: 28.0,
                    color: COL_SURFACE1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: tab_y + 6.0,
                text: tab.display_name().to_string(),
                font_size: 12.0,
                color: if is_active { COL_BLUE } else { COL_SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(90.0),
            });
        }

        // Content area.
        let content_y = tab_y + 40.0;
        let content_h = height - (content_y - y) - 20.0;
        let content_w = width - 40.0;

        match self.active_tab {
            AboutTab::Overview => {
                self.render_overview(&mut cmds, x + 20.0, content_y, content_w, content_h);
            }
            AboutTab::Hardware => {
                self.render_hardware(&mut cmds, x + 20.0, content_y, content_w, content_h);
            }
            AboutTab::Software => {
                self.render_software(&mut cmds, x + 20.0, content_y, content_w, content_h);
            }
            AboutTab::Licenses => {
                self.render_licenses(&mut cmds, x + 20.0, content_y, content_w, content_h);
            }
        }

        cmds
    }

    // ----------------------------------------------------------------
    // Tab renderers
    // ----------------------------------------------------------------

    fn render_overview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let info = &self.system_info;

        // Logo area — large colored rectangle with OS name.
        let logo_w = width.min(280.0);
        let logo_h: f32 = 100.0;
        let logo_x = x + (width - logo_w) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: logo_x,
            y,
            width: logo_w,
            height: logo_h,
            color: COL_BLUE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: logo_x + logo_w / 2.0 - 50.0,
            y: y + logo_h / 2.0 - 16.0,
            text: info.os_name.clone(),
            font_size: 32.0,
            color: COL_MANTLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(logo_w - 20.0),
        });

        // Version and build date below the logo.
        let mut row_y = y + logo_h + 20.0;

        cmds.push(RenderCommand::Text {
            x: logo_x,
            y: row_y,
            text: format!("Version {}", info.os_version),
            font_size: 14.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(logo_w),
        });
        row_y += 24.0;

        cmds.push(RenderCommand::Text {
            x: logo_x,
            y: row_y,
            text: format!("Built on {}", info.build_date),
            font_size: 12.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(logo_w),
        });
        row_y += 30.0;

        // Short description.
        cmds.push(RenderCommand::Text {
            x: logo_x,
            y: row_y,
            text: "A modern, capability-based microkernel operating system.".to_string(),
            font_size: 12.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(logo_w),
        });
    }

    fn render_hardware(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let info = &self.system_info;

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Hardware".to_string(),
            font_size: 14.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let mut row_y = y + 30.0;

        render_property_row(cmds, x, row_y, width, "CPU", &info.cpu_model);
        row_y += 28.0;

        render_property_row(
            cmds,
            x,
            row_y,
            width,
            "CPU Cores",
            &info.cpu_cores.to_string(),
        );
        row_y += 28.0;

        render_property_row(
            cmds,
            x,
            row_y,
            width,
            "Memory",
            &format_memory(info.total_memory_mb),
        );
        row_y += 28.0;

        render_property_row(cmds, x, row_y, width, "GPU", &info.gpu_model);
        row_y += 28.0;

        render_property_row(cmds, x, row_y, width, "Display", &info.display_resolution);
    }

    fn render_software(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let info = &self.system_info;

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Software".to_string(),
            font_size: 14.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let mut row_y = y + 30.0;

        render_property_row(cmds, x, row_y, width, "Kernel", &info.kernel_version);
        row_y += 28.0;

        render_property_row(cmds, x, row_y, width, "Architecture", &info.architecture);
        row_y += 28.0;

        render_property_row(cmds, x, row_y, width, "Hostname", &info.hostname);
        row_y += 28.0;

        render_property_row(
            cmds,
            x,
            row_y,
            width,
            "Uptime",
            &format_uptime(info.uptime_seconds),
        );
    }

    fn render_licenses(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Open Source Licenses".to_string(),
            font_size: 14.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        if self.licenses.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y: y + 30.0,
                text: "No licenses to display.".to_string(),
                font_size: 12.0,
                color: COL_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return;
        }

        // Clip the license list to the content area.
        cmds.push(RenderCommand::PushClip {
            x,
            y: y + 30.0,
            width,
            height: height - 30.0,
        });

        let mut item_y = y + 30.0 - self.scroll_offset;
        let visible_top = y + 30.0;
        let visible_bottom = y + height;

        for license in &self.licenses {
            // Estimate height: name row + text lines (rough: 16px per 80-char line).
            let text_lines = (license.text.len() as f32 / 80.0).ceil().max(1.0) as u32;
            let item_h = 28.0 + text_lines as f32 * 16.0 + 16.0;

            // Only render if overlapping the visible area.
            if item_y + item_h >= visible_top && item_y <= visible_bottom {
                // License name header.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: item_y,
                    width,
                    height: 24.0,
                    color: COL_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: item_y + 4.0,
                    text: license.name.clone(),
                    font_size: 12.0,
                    color: COL_LAVENDER,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width - 16.0),
                });

                // License body text.
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: item_y + 28.0,
                    text: license.text.clone(),
                    font_size: 11.0,
                    color: COL_SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 16.0),
                });
            }

            item_y += item_h;
        }

        cmds.push(RenderCommand::PopClip);
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Format an uptime duration in seconds as a human-readable string.
///
/// Examples:
/// - 0 seconds -> "0m"
/// - 90 seconds -> "1m"
/// - 3600 seconds -> "1h 0m"
/// - 90000 seconds -> "1d 1h 0m"
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Format a memory amount in megabytes as a human-readable GiB string.
///
/// Examples:
/// - 0 -> "0 MiB"
/// - 512 -> "512 MiB"
/// - 1024 -> "1.0 GiB"
/// - 16384 -> "16.0 GiB"
/// - 32768 -> "32.0 GiB"
pub fn format_memory(mb: u64) -> String {
    if mb < 1024 {
        format!("{} MiB", mb)
    } else {
        let gib = mb as f64 / 1024.0;
        format!("{:.1} GiB", gib)
    }
}

/// Render a labeled property row (label on the left, value on the right).
fn render_property_row(
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
        text: label.to_string(),
        font_size: 12.0,
        color: COL_SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width * 0.4),
    });
    cmds.push(RenderCommand::Text {
        x: x + width * 0.45,
        y,
        text: value.to_string(),
        font_size: 12.0,
        color: COL_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width * 0.55),
    });
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- format_uptime tests ----

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_format_uptime_minutes_only() {
        assert_eq!(format_uptime(90), "1m");
    }

    #[test]
    fn test_format_uptime_one_hour() {
        assert_eq!(format_uptime(3600), "1h 0m");
    }

    #[test]
    fn test_format_uptime_hours_and_minutes() {
        assert_eq!(format_uptime(5400), "1h 30m");
    }

    #[test]
    fn test_format_uptime_days() {
        // 2d 5h 30m = 2*86400 + 5*3600 + 30*60 = 172800 + 18000 + 1800 = 192600
        assert_eq!(format_uptime(192600), "2d 5h 30m");
    }

    #[test]
    fn test_format_uptime_one_day_exact() {
        assert_eq!(format_uptime(86400), "1d 0h 0m");
    }

    #[test]
    fn test_format_uptime_under_one_minute() {
        assert_eq!(format_uptime(59), "0m");
    }

    #[test]
    fn test_format_uptime_large_value() {
        // 365 days
        let secs = 365 * 86400;
        let result = format_uptime(secs);
        assert!(result.starts_with("365d"));
    }

    // ---- format_memory tests ----

    #[test]
    fn test_format_memory_zero() {
        assert_eq!(format_memory(0), "0 MiB");
    }

    #[test]
    fn test_format_memory_small() {
        assert_eq!(format_memory(512), "512 MiB");
    }

    #[test]
    fn test_format_memory_one_gib() {
        assert_eq!(format_memory(1024), "1.0 GiB");
    }

    #[test]
    fn test_format_memory_16_gib() {
        assert_eq!(format_memory(16384), "16.0 GiB");
    }

    #[test]
    fn test_format_memory_32_gib() {
        assert_eq!(format_memory(32768), "32.0 GiB");
    }

    #[test]
    fn test_format_memory_fractional() {
        assert_eq!(format_memory(1536), "1.5 GiB");
    }

    #[test]
    fn test_format_memory_boundary() {
        assert_eq!(format_memory(1023), "1023 MiB");
    }

    // ---- SystemInfo tests ----

    #[test]
    fn test_system_info_default() {
        let info = SystemInfo::default();
        assert_eq!(info.os_name, "OurOS");
        assert_eq!(info.architecture, "x86_64");
        assert_eq!(info.cpu_cores, 1);
        assert_eq!(info.total_memory_mb, 0);
    }

    #[test]
    fn test_system_info_to_text_contains_fields() {
        let info = SystemInfo {
            os_name: "TestOS".to_string(),
            os_version: "1.2.3".to_string(),
            cpu_cores: 16,
            total_memory_mb: 32768,
            ..SystemInfo::default()
        };
        let text = info.to_text();
        assert!(text.contains("os_name=TestOS"));
        assert!(text.contains("os_version=1.2.3"));
        assert!(text.contains("cpu_cores=16"));
        assert!(text.contains("total_memory_mb=32768"));
    }

    #[test]
    fn test_system_info_from_text_roundtrip() {
        let original = SystemInfo {
            os_name: "MyOS".to_string(),
            os_version: "2.0.0".to_string(),
            kernel_version: "2.0.0-rc1".to_string(),
            build_date: "2026-01-15".to_string(),
            architecture: "aarch64".to_string(),
            hostname: "myhost".to_string(),
            cpu_model: "ARM Cortex-A76".to_string(),
            cpu_cores: 8,
            total_memory_mb: 8192,
            gpu_model: "Mali-G76".to_string(),
            display_resolution: "2560x1440".to_string(),
            uptime_seconds: 12345,
        };
        let text = original.to_text();
        let parsed = SystemInfo::from_text(&text);
        assert_eq!(parsed.os_name, original.os_name);
        assert_eq!(parsed.os_version, original.os_version);
        assert_eq!(parsed.kernel_version, original.kernel_version);
        assert_eq!(parsed.build_date, original.build_date);
        assert_eq!(parsed.architecture, original.architecture);
        assert_eq!(parsed.hostname, original.hostname);
        assert_eq!(parsed.cpu_model, original.cpu_model);
        assert_eq!(parsed.cpu_cores, original.cpu_cores);
        assert_eq!(parsed.total_memory_mb, original.total_memory_mb);
        assert_eq!(parsed.gpu_model, original.gpu_model);
        assert_eq!(parsed.display_resolution, original.display_resolution);
        assert_eq!(parsed.uptime_seconds, original.uptime_seconds);
    }

    #[test]
    fn test_system_info_from_text_ignores_comments() {
        let text = "# A comment\nos_name=IgnoredCommentOS\n# Another\n";
        let info = SystemInfo::from_text(text);
        assert_eq!(info.os_name, "IgnoredCommentOS");
    }

    #[test]
    fn test_system_info_from_text_ignores_unknown_keys() {
        let text = "os_name=TestOS\nfuture_field=whatever\ncpu_cores=4\n";
        let info = SystemInfo::from_text(text);
        assert_eq!(info.os_name, "TestOS");
        assert_eq!(info.cpu_cores, 4);
    }

    #[test]
    fn test_system_info_from_text_bad_numeric_keeps_default() {
        let text = "cpu_cores=notanumber\ntotal_memory_mb=also_bad\n";
        let info = SystemInfo::from_text(text);
        assert_eq!(info.cpu_cores, 1); // default
        assert_eq!(info.total_memory_mb, 0); // default
    }

    #[test]
    fn test_system_info_from_text_empty() {
        let info = SystemInfo::from_text("");
        assert_eq!(info.os_name, "OurOS"); // all defaults
    }

    // ---- LicenseInfo tests ----

    #[test]
    fn test_license_info_creation() {
        let lic = LicenseInfo::new("MIT", "Permission is hereby granted...");
        assert_eq!(lic.name, "MIT");
        assert!(lic.text.starts_with("Permission"));
    }

    // ---- AboutTab tests ----

    #[test]
    fn test_about_tab_display_names() {
        assert_eq!(AboutTab::Overview.display_name(), "Overview");
        assert_eq!(AboutTab::Hardware.display_name(), "Hardware");
        assert_eq!(AboutTab::Software.display_name(), "Software");
        assert_eq!(AboutTab::Licenses.display_name(), "Licenses");
    }

    #[test]
    fn test_about_tab_all_has_four() {
        assert_eq!(AboutTab::ALL.len(), 4);
    }

    #[test]
    fn test_about_tab_all_names_nonempty() {
        for tab in AboutTab::ALL {
            assert!(!tab.display_name().is_empty());
        }
    }

    // ---- AboutDialog state tests ----

    #[test]
    fn test_about_dialog_default_hidden() {
        let dlg = AboutDialog::default();
        assert!(!dlg.visible);
        assert_eq!(dlg.active_tab, AboutTab::Overview);
        assert_eq!(dlg.scroll_offset, 0.0);
    }

    #[test]
    fn test_about_dialog_show() {
        let mut dlg = AboutDialog::default();
        dlg.active_tab = AboutTab::Licenses;
        dlg.scroll_offset = 100.0;
        dlg.show();
        assert!(dlg.visible);
        assert_eq!(dlg.active_tab, AboutTab::Overview);
        assert_eq!(dlg.scroll_offset, 0.0);
    }

    #[test]
    fn test_about_dialog_hide() {
        let mut dlg = AboutDialog::default();
        dlg.show();
        assert!(dlg.visible);
        dlg.hide();
        assert!(!dlg.visible);
    }

    #[test]
    fn test_about_dialog_toggle() {
        let mut dlg = AboutDialog::default();
        assert!(!dlg.visible);
        dlg.toggle();
        assert!(dlg.visible);
        dlg.toggle();
        assert!(!dlg.visible);
    }

    #[test]
    fn test_about_dialog_set_tab() {
        let mut dlg = AboutDialog::default();
        dlg.scroll_offset = 50.0;
        dlg.set_tab(AboutTab::Hardware);
        assert_eq!(dlg.active_tab, AboutTab::Hardware);
        assert_eq!(dlg.scroll_offset, 0.0);
    }

    #[test]
    fn test_about_dialog_scroll() {
        let mut dlg = AboutDialog::default();
        dlg.scroll(50.0);
        assert_eq!(dlg.scroll_offset, 50.0);
        dlg.scroll(30.0);
        assert_eq!(dlg.scroll_offset, 80.0);
    }

    #[test]
    fn test_about_dialog_scroll_clamps_negative() {
        let mut dlg = AboutDialog::default();
        dlg.scroll(-100.0);
        assert_eq!(dlg.scroll_offset, 0.0);
    }

    #[test]
    fn test_about_dialog_new() {
        let info = SystemInfo {
            os_name: "TestOS".to_string(),
            ..SystemInfo::default()
        };
        let licenses = vec![LicenseInfo::new("MIT", "text")];
        let dlg = AboutDialog::new(info, licenses);
        assert_eq!(dlg.system_info.os_name, "TestOS");
        assert_eq!(dlg.licenses.len(), 1);
        assert!(!dlg.visible);
    }

    // ---- Render tests ----

    #[test]
    fn test_render_hidden_returns_empty() {
        let dlg = AboutDialog::default();
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_overview_tab() {
        let mut dlg = AboutDialog::default();
        dlg.show();
        dlg.set_tab(AboutTab::Overview);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
        // Should have background, border, title, tabs, and overview content.
        assert!(cmds.len() >= 10);
    }

    #[test]
    fn test_render_hardware_tab() {
        let mut dlg = AboutDialog {
            system_info: SystemInfo {
                cpu_model: "Test CPU".to_string(),
                total_memory_mb: 16384,
                ..SystemInfo::default()
            },
            ..AboutDialog::default()
        };
        dlg.show();
        dlg.set_tab(AboutTab::Hardware);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_software_tab() {
        let mut dlg = AboutDialog {
            system_info: SystemInfo {
                kernel_version: "1.0.0".to_string(),
                uptime_seconds: 90000,
                ..SystemInfo::default()
            },
            ..AboutDialog::default()
        };
        dlg.show();
        dlg.set_tab(AboutTab::Software);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_licenses_tab_empty() {
        let mut dlg = AboutDialog::default();
        dlg.show();
        dlg.set_tab(AboutTab::Licenses);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_licenses_tab_with_entries() {
        let mut dlg = AboutDialog {
            licenses: vec![
                LicenseInfo::new("MIT", "MIT license text here"),
                LicenseInfo::new("Apache-2.0", "Apache license text here"),
            ],
            ..AboutDialog::default()
        };
        dlg.show();
        dlg.set_tab(AboutTab::Licenses);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
        // Should include clip, license headers, license text, and pop clip.
        assert!(cmds.len() >= 12);
    }

    #[test]
    fn test_render_at_offset_position() {
        let mut dlg = AboutDialog::default();
        dlg.show();
        let cmds = dlg.render(100.0, 200.0, 500.0, 400.0);
        // First command should be the background FillRect at (100, 200).
        if let Some(RenderCommand::FillRect { x, y, .. }) = cmds.first() {
            assert!((*x - 100.0).abs() < 0.01);
            assert!((*y - 200.0).abs() < 0.01);
        } else {
            panic!("Expected FillRect as first render command");
        }
    }

    #[test]
    fn test_render_licenses_scroll_offset() {
        let mut dlg = AboutDialog {
            licenses: vec![
                LicenseInfo::new("Lib1", "License 1 text"),
                LicenseInfo::new("Lib2", "License 2 text"),
                LicenseInfo::new("Lib3", "License 3 text"),
            ],
            ..AboutDialog::default()
        };
        dlg.show();
        dlg.set_tab(AboutTab::Licenses);
        dlg.scroll(50.0);
        let cmds = dlg.render(0.0, 0.0, 500.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // ---- render_property_row tests ----

    #[test]
    fn test_render_property_row_produces_two_texts() {
        let mut cmds = Vec::new();
        render_property_row(&mut cmds, 0.0, 0.0, 400.0, "Label", "Value");
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_render_property_row_label_content() {
        let mut cmds = Vec::new();
        render_property_row(&mut cmds, 10.0, 20.0, 400.0, "CPU", "i9-13900K");
        if let RenderCommand::Text { text, .. } = &cmds[0] {
            assert_eq!(text, "CPU");
        } else {
            panic!("Expected Text command for label");
        }
        if let RenderCommand::Text { text, .. } = &cmds[1] {
            assert_eq!(text, "i9-13900K");
        } else {
            panic!("Expected Text command for value");
        }
    }

    // ---- Themed color constant tests ----

    #[test]
    fn test_theme_colors_opaque() {
        // All theme constants should be fully opaque.
        let colors = [
            COL_BASE, COL_SURFACE0, COL_SURFACE1, COL_TEXT, COL_SUBTEXT0,
            COL_BLUE, COL_LAVENDER, COL_OVERLAY0, COL_MANTLE, COL_GREEN,
            COL_PEACH,
        ];
        for c in &colors {
            assert_eq!(c.a, 255, "Theme color should be opaque: {:?}", c);
        }
    }

    #[test]
    fn test_mocha_base_rgb() {
        assert_eq!(COL_BASE.r, 0x1E);
        assert_eq!(COL_BASE.g, 0x1E);
        assert_eq!(COL_BASE.b, 0x2E);
    }

    #[test]
    fn test_mocha_blue_rgb() {
        assert_eq!(COL_BLUE.r, 0x89);
        assert_eq!(COL_BLUE.g, 0xB4);
        assert_eq!(COL_BLUE.b, 0xFA);
    }
}
