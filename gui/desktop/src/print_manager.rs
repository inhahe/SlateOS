//! Print Manager
//!
//! Desktop print management infrastructure:
//!
//! - Printer discovery and listing
//! - Print queue management (view/cancel/pause/resume jobs)
//! - Default printer selection
//! - Print dialog (page range, copies, orientation, quality)
//! - Printer properties (paper size, color/mono, duplex)
//! - Print spooler status
//! - Print history / job log

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Printer types
// ============================================================================

/// Connection type for a printer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrinterConnection {
    Usb,
    Network,
    Bluetooth,
    Virtual,
}

/// Printer capabilities.
#[derive(Clone, Debug)]
pub struct PrinterCapabilities {
    pub color: bool,
    pub duplex: bool,
    pub max_dpi: u32,
    pub paper_sizes: Vec<PaperSize>,
    pub supports_borderless: bool,
    pub max_copies: u32,
    pub stapling: bool,
    pub collation: bool,
}

impl PrinterCapabilities {
    pub fn basic() -> Self {
        Self {
            color: false,
            duplex: false,
            max_dpi: 600,
            paper_sizes: vec![PaperSize::A4, PaperSize::Letter],
            supports_borderless: false,
            max_copies: 99,
            stapling: false,
            collation: true,
        }
    }

    pub fn full_color() -> Self {
        Self {
            color: true,
            duplex: true,
            max_dpi: 2400,
            paper_sizes: vec![
                PaperSize::A4, PaperSize::Letter, PaperSize::Legal,
                PaperSize::A3, PaperSize::A5, PaperSize::Envelope,
            ],
            supports_borderless: true,
            max_copies: 999,
            stapling: true,
            collation: true,
        }
    }
}

/// Standard paper sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperSize {
    A3,
    A4,
    A5,
    Letter,
    Legal,
    Tabloid,
    Envelope,
    Custom,
}

impl PaperSize {
    pub fn label(&self) -> &str {
        match self {
            Self::A3 => "A3 (297 x 420 mm)",
            Self::A4 => "A4 (210 x 297 mm)",
            Self::A5 => "A5 (148 x 210 mm)",
            Self::Letter => "Letter (8.5 x 11 in)",
            Self::Legal => "Legal (8.5 x 14 in)",
            Self::Tabloid => "Tabloid (11 x 17 in)",
            Self::Envelope => "Envelope (#10)",
            Self::Custom => "Custom",
        }
    }

    /// Width in mm.
    pub fn width_mm(&self) -> f32 {
        match self {
            Self::A3 => 297.0,
            Self::A4 => 210.0,
            Self::A5 => 148.0,
            Self::Letter => 215.9,
            Self::Legal => 215.9,
            Self::Tabloid => 279.4,
            Self::Envelope => 104.8,
            Self::Custom => 210.0,
        }
    }

    /// Height in mm.
    pub fn height_mm(&self) -> f32 {
        match self {
            Self::A3 => 420.0,
            Self::A4 => 297.0,
            Self::A5 => 210.0,
            Self::Letter => 279.4,
            Self::Legal => 355.6,
            Self::Tabloid => 431.8,
            Self::Envelope => 241.3,
            Self::Custom => 297.0,
        }
    }
}

/// Page orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Print quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintQuality {
    Draft,
    Normal,
    High,
    Best,
}

impl PrintQuality {
    pub fn label(&self) -> &str {
        match self {
            Self::Draft => "Draft (fast)",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Best => "Best (slow)",
        }
    }
}

/// Color mode for printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Grayscale,
    MonoBlack,
}

// ============================================================================
// Printer device
// ============================================================================

/// A detected printer device.
#[derive(Clone, Debug)]
pub struct Printer {
    pub id: u32,
    pub name: String,
    pub model: String,
    pub connection: PrinterConnection,
    pub capabilities: PrinterCapabilities,
    pub online: bool,
    pub is_default: bool,
    /// Number of jobs in queue.
    pub queue_count: u32,
    /// Ink/toner levels (0-100, None if unknown).
    pub ink_level: Option<u8>,
}

impl Printer {
    pub fn status_label(&self) -> &str {
        if !self.online {
            "Offline"
        } else if self.queue_count > 0 {
            "Printing"
        } else {
            "Ready"
        }
    }

    pub fn status_color(&self) -> Color {
        if !self.online {
            MOCHA_RED
        } else if self.queue_count > 0 {
            MOCHA_YELLOW
        } else {
            MOCHA_GREEN
        }
    }
}

// ============================================================================
// Print job
// ============================================================================

/// State of a print job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Printing,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn label(&self) -> &str {
        match self {
            Self::Queued => "Queued",
            Self::Printing => "Printing",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Queued => MOCHA_BLUE,
            Self::Printing => MOCHA_PEACH,
            Self::Paused => MOCHA_YELLOW,
            Self::Completed => MOCHA_GREEN,
            Self::Failed => MOCHA_RED,
            Self::Cancelled => MOCHA_OVERLAY0,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Print job settings.
#[derive(Clone, Debug)]
pub struct PrintSettings {
    pub copies: u32,
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub quality: PrintQuality,
    pub color_mode: ColorMode,
    pub duplex: bool,
    pub collate: bool,
    /// Page range: None = all pages.
    pub page_range: Option<(u32, u32)>,
    pub scale_percent: u32,
}

impl PrintSettings {
    pub fn default_settings() -> Self {
        Self {
            copies: 1,
            paper_size: PaperSize::A4,
            orientation: Orientation::Portrait,
            quality: PrintQuality::Normal,
            color_mode: ColorMode::Color,
            duplex: false,
            collate: true,
            page_range: None,
            scale_percent: 100,
        }
    }

    /// Validate settings against printer capabilities.
    pub fn validate(&self, caps: &PrinterCapabilities) -> Vec<String> {
        let mut errors = Vec::new();
        if self.copies == 0 || self.copies > caps.max_copies {
            errors.push(format!("Copies must be 1-{}", caps.max_copies));
        }
        if self.duplex && !caps.duplex {
            errors.push("Printer does not support duplex".to_string());
        }
        if self.color_mode == ColorMode::Color && !caps.color {
            errors.push("Printer does not support color".to_string());
        }
        if !caps.paper_sizes.contains(&self.paper_size) && self.paper_size != PaperSize::Custom {
            errors.push("Paper size not supported".to_string());
        }
        if let Some((start, end)) = self.page_range
            && (start == 0 || end < start) {
                errors.push("Invalid page range".to_string());
            }
        if self.scale_percent == 0 || self.scale_percent > 400 {
            errors.push("Scale must be 1-400%".to_string());
        }
        errors
    }
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

/// A print job in the queue.
#[derive(Clone, Debug)]
pub struct PrintJob {
    pub id: u32,
    pub document_name: String,
    pub printer_id: u32,
    pub state: JobState,
    pub settings: PrintSettings,
    pub total_pages: u32,
    pub pages_printed: u32,
    pub submitted_at: u64,
    pub completed_at: Option<u64>,
    pub size_bytes: u64,
    pub owner: String,
}

impl PrintJob {
    /// Progress as percentage (0-100).
    pub fn progress_pct(&self) -> u32 {
        if self.total_pages == 0 { return 0; }
        ((self.pages_printed as u64 * 100) / self.total_pages as u64) as u32
    }

    /// Size display.
    pub fn size_display(&self) -> String {
        if self.size_bytes < 1024 {
            format!("{} B", self.size_bytes)
        } else if self.size_bytes < 1024 * 1024 {
            format!("{:.1} KB", self.size_bytes as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size_bytes as f64 / (1024.0 * 1024.0))
        }
    }
}

// ============================================================================
// Print Manager
// ============================================================================

/// Maximum printers.
const MAX_PRINTERS: usize = 32;
/// Maximum jobs in history.
const MAX_JOBS: usize = 200;

/// Manages printers, jobs, and print dialog.
pub struct PrintManager {
    pub printers: Vec<Printer>,
    pub jobs: Vec<PrintJob>,
    pub default_printer_id: Option<u32>,
    next_printer_id: u32,
    next_job_id: u32,
    /// Whether the spooler is running.
    pub spooler_running: bool,
}

impl PrintManager {
    pub fn new() -> Self {
        let mut mgr = Self {
            printers: Vec::new(),
            jobs: Vec::new(),
            default_printer_id: None,
            next_printer_id: 1,
            next_job_id: 1,
            spooler_running: true,
        };
        mgr.add_default_printers();
        mgr
    }

    fn add_default_printers(&mut self) {
        // PDF printer (virtual).
        let pdf = Printer {
            id: self.alloc_printer_id(),
            name: "Print to PDF".to_string(),
            model: "Virtual PDF Printer".to_string(),
            connection: PrinterConnection::Virtual,
            capabilities: PrinterCapabilities::full_color(),
            online: true,
            is_default: true,
            queue_count: 0,
            ink_level: None,
        };
        self.default_printer_id = Some(pdf.id);
        self.printers.push(pdf);
    }

    fn alloc_printer_id(&mut self) -> u32 {
        let id = self.next_printer_id;
        self.next_printer_id = self.next_printer_id.saturating_add(1);
        id
    }

    fn alloc_job_id(&mut self) -> u32 {
        let id = self.next_job_id;
        self.next_job_id = self.next_job_id.saturating_add(1);
        id
    }

    /// Add a new printer. Returns printer ID or None if full.
    pub fn add_printer(&mut self, mut printer: Printer) -> Option<u32> {
        if self.printers.len() >= MAX_PRINTERS {
            return None;
        }
        let id = self.alloc_printer_id();
        printer.id = id;
        if self.printers.is_empty() {
            printer.is_default = true;
            self.default_printer_id = Some(id);
        }
        self.printers.push(printer);
        Some(id)
    }

    /// Remove a printer by ID.
    pub fn remove_printer(&mut self, id: u32) -> bool {
        let before = self.printers.len();
        self.printers.retain(|p| p.id != id);
        if self.printers.len() < before {
            if self.default_printer_id == Some(id) {
                self.default_printer_id = self.printers.first().map(|p| p.id);
                if let Some(def_id) = self.default_printer_id
                    && let Some(p) = self.printers.iter_mut().find(|p| p.id == def_id) {
                        p.is_default = true;
                    }
            }
            true
        } else {
            false
        }
    }

    /// Set the default printer.
    pub fn set_default(&mut self, id: u32) -> bool {
        if self.printers.iter().any(|p| p.id == id) {
            for p in &mut self.printers {
                p.is_default = p.id == id;
            }
            self.default_printer_id = Some(id);
            true
        } else {
            false
        }
    }

    /// Get the default printer.
    pub fn default_printer(&self) -> Option<&Printer> {
        self.default_printer_id.and_then(|id| self.printers.iter().find(|p| p.id == id))
    }

    /// Submit a print job. Returns job ID.
    pub fn submit_job(
        &mut self,
        document_name: &str,
        printer_id: u32,
        settings: PrintSettings,
        total_pages: u32,
        size_bytes: u64,
        owner: &str,
        timestamp: u64,
    ) -> Option<u32> {
        if !self.spooler_running {
            return None;
        }
        if !self.printers.iter().any(|p| p.id == printer_id && p.online) {
            return None;
        }
        let id = self.alloc_job_id();
        let job = PrintJob {
            id,
            document_name: document_name.to_string(),
            printer_id,
            state: JobState::Queued,
            settings,
            total_pages,
            pages_printed: 0,
            submitted_at: timestamp,
            completed_at: None,
            size_bytes,
            owner: owner.to_string(),
        };
        if self.jobs.len() >= MAX_JOBS {
            // Remove oldest terminal job.
            if let Some(pos) = self.jobs.iter().position(|j| j.state.is_terminal()) {
                self.jobs.remove(pos);
            }
        }
        // Update printer queue count.
        if let Some(p) = self.printers.iter_mut().find(|p| p.id == printer_id) {
            p.queue_count = p.queue_count.saturating_add(1);
        }
        self.jobs.push(job);
        Some(id)
    }

    /// Cancel a job by ID.
    pub fn cancel_job(&mut self, job_id: u32) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            if job.state.is_terminal() {
                return false;
            }
            let printer_id = job.printer_id;
            job.state = JobState::Cancelled;
            if let Some(p) = self.printers.iter_mut().find(|p| p.id == printer_id) {
                p.queue_count = p.queue_count.saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// Pause a job.
    pub fn pause_job(&mut self, job_id: u32) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id)
            && (job.state == JobState::Printing || job.state == JobState::Queued) {
                job.state = JobState::Paused;
                return true;
            }
        false
    }

    /// Resume a paused job.
    pub fn resume_job(&mut self, job_id: u32) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id)
            && job.state == JobState::Paused {
                job.state = JobState::Queued;
                return true;
            }
        false
    }

    /// Advance a printing job (simulate printing a page).
    pub fn advance_job(&mut self, job_id: u32) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            if job.state == JobState::Queued {
                job.state = JobState::Printing;
            }
            if job.state == JobState::Printing {
                job.pages_printed = job.pages_printed.saturating_add(1);
                if job.pages_printed >= job.total_pages {
                    job.state = JobState::Completed;
                    // Update printer queue.
                    let printer_id = job.printer_id;
                    if let Some(p) = self.printers.iter_mut().find(|p| p.id == printer_id) {
                        p.queue_count = p.queue_count.saturating_sub(1);
                    }
                }
                return true;
            }
        }
        false
    }

    /// Get all active (non-terminal) jobs.
    pub fn active_jobs(&self) -> Vec<&PrintJob> {
        self.jobs.iter().filter(|j| !j.state.is_terminal()).collect()
    }

    /// Get all jobs for a specific printer.
    pub fn jobs_for_printer(&self, printer_id: u32) -> Vec<&PrintJob> {
        self.jobs.iter().filter(|j| j.printer_id == printer_id).collect()
    }

    /// Purge completed/cancelled/failed jobs from history.
    pub fn purge_terminal_jobs(&mut self) -> usize {
        let before = self.jobs.len();
        self.jobs.retain(|j| !j.state.is_terminal());
        before - self.jobs.len()
    }

    /// Total pages printed across all completed jobs.
    pub fn total_pages_printed(&self) -> u64 {
        self.jobs.iter()
            .filter(|j| j.state == JobState::Completed)
            .map(|j| j.pages_printed as u64)
            .sum()
    }

    /// Toggle spooler on/off.
    pub fn set_spooler(&mut self, running: bool) {
        self.spooler_running = running;
    }
}

impl Default for PrintManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Print dialog state
// ============================================================================

/// Print dialog for configuring and submitting a print job.
pub struct PrintDialog {
    pub visible: bool,
    pub selected_printer_idx: usize,
    pub settings: PrintSettings,
    pub document_name: String,
    pub total_pages: u32,
    pub validation_errors: Vec<String>,
}

impl PrintDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected_printer_idx: 0,
            settings: PrintSettings::default_settings(),
            document_name: String::new(),
            total_pages: 0,
            validation_errors: Vec::new(),
        }
    }

    /// Open the dialog for a document.
    pub fn open(&mut self, document_name: &str, total_pages: u32) {
        self.visible = true;
        self.document_name = document_name.to_string();
        self.total_pages = total_pages;
        self.settings = PrintSettings::default_settings();
        self.validation_errors.clear();
    }

    /// Close the dialog.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Validate against a printer's capabilities.
    pub fn validate(&mut self, caps: &PrinterCapabilities) {
        self.validation_errors = self.settings.validate(caps);
    }

    pub fn is_valid(&self) -> bool {
        self.validation_errors.is_empty()
    }

    /// Render the print dialog.
    pub fn render(&self, printers: &[Printer], x: f32, y: f32, w: f32, h: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        if !self.visible {
            return cmds;
        }

        // Overlay background.
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: Color::rgba(0, 0, 0, 128),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog box.
        let dw = 500.0;
        let dh = 400.0;
        let dx = x + (w - dw) / 2.0;
        let dy = y + (h - dh) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: dx, y: dy, width: dw, height: dh,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0, y: dy + 16.0,
            text: "Print".to_string(),
            font_size: 16.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Document name.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0, y: dy + 40.0,
            text: format!("Document: {}", self.document_name),
            font_size: 12.0, color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Printer selector.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0, y: dy + 68.0,
            text: "Printer:".to_string(),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        let printer_name = printers.get(self.selected_printer_idx)
            .map(|p| p.name.as_str())
            .unwrap_or("None");
        cmds.push(RenderCommand::FillRect {
            x: dx + 100.0, y: dy + 62.0, width: 280.0, height: 24.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + 108.0, y: dy + 66.0,
            text: printer_name.to_string(),
            font_size: 12.0, color: MOCHA_BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Settings rows.
        let mut cy = dy + 100.0;
        let settings_rows = [
            ("Copies:", format!("{}", self.settings.copies)),
            ("Paper:", self.settings.paper_size.label().to_string()),
            ("Orientation:", if self.settings.orientation == Orientation::Portrait { "Portrait" } else { "Landscape" }.to_string()),
            ("Quality:", self.settings.quality.label().to_string()),
            ("Color:", match self.settings.color_mode { ColorMode::Color => "Color", ColorMode::Grayscale => "Grayscale", ColorMode::MonoBlack => "Black & White" }.to_string()),
            ("Duplex:", if self.settings.duplex { "On" } else { "Off" }.to_string()),
            ("Pages:", self.settings.page_range.map(|(s, e)| format!("{}-{}", s, e)).unwrap_or_else(|| "All".to_string())),
        ];
        for (label, value) in &settings_rows {
            cmds.push(RenderCommand::Text {
                x: dx + 20.0, y: cy,
                text: label.to_string(),
                font_size: 12.0, color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: dx + 120.0, y: cy,
                text: value.clone(),
                font_size: 12.0, color: MOCHA_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 24.0;
        }

        // Validation errors.
        for err in &self.validation_errors {
            cmds.push(RenderCommand::Text {
                x: dx + 20.0, y: cy,
                text: err.clone(),
                font_size: 11.0, color: MOCHA_RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 18.0;
        }

        // Buttons.
        let btn_y = dy + dh - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: dx + dw - 180.0, y: btn_y, width: 70.0, height: 28.0,
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dw - 166.0, y: btn_y + 7.0,
            text: "Print".to_string(),
            font_size: 12.0, color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: dx + dw - 100.0, y: btn_y, width: 80.0, height: 28.0,
            color: MOCHA_SURFACE1,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dw - 84.0, y: btn_y + 7.0,
            text: "Cancel".to_string(),
            font_size: 12.0, color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

impl Default for PrintDialog {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- PaperSize ---
    #[test]
    fn test_paper_size_labels() {
        assert!(PaperSize::A4.label().contains("210"));
        assert!(PaperSize::Letter.label().contains("8.5"));
    }

    #[test]
    fn test_paper_size_dimensions() {
        assert_eq!(PaperSize::A4.width_mm(), 210.0);
        assert_eq!(PaperSize::A4.height_mm(), 297.0);
    }

    // --- PrinterCapabilities ---
    #[test]
    fn test_basic_caps() {
        let caps = PrinterCapabilities::basic();
        assert!(!caps.color);
        assert!(!caps.duplex);
        assert_eq!(caps.max_dpi, 600);
    }

    #[test]
    fn test_full_color_caps() {
        let caps = PrinterCapabilities::full_color();
        assert!(caps.color);
        assert!(caps.duplex);
        assert!(caps.paper_sizes.len() >= 4);
    }

    // --- PrintSettings ---
    #[test]
    fn test_settings_defaults() {
        let s = PrintSettings::default_settings();
        assert_eq!(s.copies, 1);
        assert_eq!(s.paper_size, PaperSize::A4);
        assert_eq!(s.orientation, Orientation::Portrait);
    }

    #[test]
    fn test_validate_valid() {
        let s = PrintSettings::default_settings();
        let caps = PrinterCapabilities::full_color();
        assert!(s.validate(&caps).is_empty());
    }

    #[test]
    fn test_validate_too_many_copies() {
        let mut s = PrintSettings::default_settings();
        s.copies = 0;
        let caps = PrinterCapabilities::basic();
        let errs = s.validate(&caps);
        assert!(!errs.is_empty());
    }

    #[test]
    fn test_validate_duplex_unsupported() {
        let mut s = PrintSettings::default_settings();
        s.duplex = true;
        let caps = PrinterCapabilities::basic();
        let errs = s.validate(&caps);
        assert!(errs.iter().any(|e| e.contains("duplex")));
    }

    #[test]
    fn test_validate_color_unsupported() {
        let mut s = PrintSettings::default_settings();
        s.color_mode = ColorMode::Color;
        let caps = PrinterCapabilities::basic();
        let errs = s.validate(&caps);
        assert!(errs.iter().any(|e| e.contains("color")));
    }

    #[test]
    fn test_validate_invalid_page_range() {
        let mut s = PrintSettings::default_settings();
        s.page_range = Some((5, 3));
        let caps = PrinterCapabilities::full_color();
        let errs = s.validate(&caps);
        assert!(errs.iter().any(|e| e.contains("page range")));
    }

    #[test]
    fn test_validate_zero_scale() {
        let mut s = PrintSettings::default_settings();
        s.scale_percent = 0;
        let caps = PrinterCapabilities::full_color();
        let errs = s.validate(&caps);
        assert!(errs.iter().any(|e| e.contains("Scale")));
    }

    // --- Printer ---
    #[test]
    fn test_printer_status() {
        let mut p = Printer {
            id: 1, name: "Test".to_string(), model: "Test".to_string(),
            connection: PrinterConnection::Usb, capabilities: PrinterCapabilities::basic(),
            online: true, is_default: false, queue_count: 0, ink_level: Some(80),
        };
        assert_eq!(p.status_label(), "Ready");
        p.queue_count = 3;
        assert_eq!(p.status_label(), "Printing");
        p.online = false;
        assert_eq!(p.status_label(), "Offline");
    }

    // --- JobState ---
    #[test]
    fn test_job_state_terminal() {
        assert!(!JobState::Queued.is_terminal());
        assert!(!JobState::Printing.is_terminal());
        assert!(!JobState::Paused.is_terminal());
        assert!(JobState::Completed.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
    }

    // --- PrintJob ---
    #[test]
    fn test_job_progress() {
        let mut job = PrintJob {
            id: 1, document_name: "test.pdf".to_string(), printer_id: 1,
            state: JobState::Printing, settings: PrintSettings::default_settings(),
            total_pages: 10, pages_printed: 5, submitted_at: 0,
            completed_at: None, size_bytes: 50000, owner: "user".to_string(),
        };
        assert_eq!(job.progress_pct(), 50);
        job.pages_printed = 0;
        assert_eq!(job.progress_pct(), 0);
    }

    #[test]
    fn test_job_size_display() {
        let job = PrintJob {
            id: 1, document_name: "test".to_string(), printer_id: 1,
            state: JobState::Queued, settings: PrintSettings::default_settings(),
            total_pages: 1, pages_printed: 0, submitted_at: 0,
            completed_at: None, size_bytes: 2048, owner: "user".to_string(),
        };
        assert_eq!(job.size_display(), "2.0 KB");
    }

    // --- PrintManager ---
    #[test]
    fn test_manager_new() {
        let mgr = PrintManager::new();
        assert_eq!(mgr.printers.len(), 1); // Default PDF printer
        assert!(mgr.default_printer_id.is_some());
    }

    #[test]
    fn test_add_printer() {
        let mut mgr = PrintManager::new();
        let p = Printer {
            id: 0, name: "HP".to_string(), model: "LaserJet".to_string(),
            connection: PrinterConnection::Network, capabilities: PrinterCapabilities::basic(),
            online: true, is_default: false, queue_count: 0, ink_level: None,
        };
        let id = mgr.add_printer(p);
        assert!(id.is_some());
        assert_eq!(mgr.printers.len(), 2);
    }

    #[test]
    fn test_remove_printer() {
        let mut mgr = PrintManager::new();
        let id = mgr.printers[0].id;
        assert!(mgr.remove_printer(id));
        assert!(mgr.printers.is_empty());
    }

    #[test]
    fn test_set_default() {
        let mut mgr = PrintManager::new();
        let p = Printer {
            id: 0, name: "HP".to_string(), model: "LJ".to_string(),
            connection: PrinterConnection::Usb, capabilities: PrinterCapabilities::basic(),
            online: true, is_default: false, queue_count: 0, ink_level: None,
        };
        let id = mgr.add_printer(p).unwrap();
        assert!(mgr.set_default(id));
        assert_eq!(mgr.default_printer_id, Some(id));
    }

    #[test]
    fn test_submit_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 10, 5000, "user", 1000);
        assert!(jid.is_some());
        assert_eq!(mgr.jobs.len(), 1);
        assert_eq!(mgr.printers[0].queue_count, 1);
    }

    #[test]
    fn test_submit_job_offline_printer() {
        let mut mgr = PrintManager::new();
        mgr.printers[0].online = false;
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 1, 100, "u", 0);
        assert!(jid.is_none());
    }

    #[test]
    fn test_submit_job_spooler_off() {
        let mut mgr = PrintManager::new();
        mgr.set_spooler(false);
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 1, 100, "u", 0);
        assert!(jid.is_none());
    }

    #[test]
    fn test_cancel_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 10, 5000, "u", 0).unwrap();
        assert!(mgr.cancel_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Cancelled);
    }

    #[test]
    fn test_cancel_completed_job_fails() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 1, 100, "u", 0).unwrap();
        mgr.advance_job(jid); // complete
        assert!(!mgr.cancel_job(jid));
    }

    #[test]
    fn test_pause_resume_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 5, 100, "u", 0).unwrap();
        assert!(mgr.pause_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Paused);
        assert!(mgr.resume_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Queued);
    }

    #[test]
    fn test_advance_job_to_completion() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job("doc.pdf", pid, PrintSettings::default_settings(), 3, 100, "u", 0).unwrap();
        mgr.advance_job(jid); // page 1
        mgr.advance_job(jid); // page 2
        mgr.advance_job(jid); // page 3 → completed
        assert_eq!(mgr.jobs[0].state, JobState::Completed);
        assert_eq!(mgr.printers[0].queue_count, 0);
    }

    #[test]
    fn test_active_jobs() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        mgr.submit_job("a.pdf", pid, PrintSettings::default_settings(), 5, 100, "u", 0);
        mgr.submit_job("b.pdf", pid, PrintSettings::default_settings(), 1, 100, "u", 0);
        let jid2 = mgr.jobs[1].id;
        mgr.advance_job(jid2); // complete b.pdf
        assert_eq!(mgr.active_jobs().len(), 1);
    }

    #[test]
    fn test_purge_terminal() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        mgr.submit_job("a.pdf", pid, PrintSettings::default_settings(), 1, 100, "u", 0);
        mgr.submit_job("b.pdf", pid, PrintSettings::default_settings(), 5, 100, "u", 0);
        let jid1 = mgr.jobs[0].id;
        mgr.advance_job(jid1);
        let purged = mgr.purge_terminal_jobs();
        assert_eq!(purged, 1);
        assert_eq!(mgr.jobs.len(), 1);
    }

    #[test]
    fn test_total_pages_printed() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        mgr.submit_job("a.pdf", pid, PrintSettings::default_settings(), 3, 100, "u", 0);
        mgr.submit_job("b.pdf", pid, PrintSettings::default_settings(), 2, 100, "u", 0);
        let jid1 = mgr.jobs[0].id;
        let jid2 = mgr.jobs[1].id;
        for _ in 0..3 { mgr.advance_job(jid1); }
        for _ in 0..2 { mgr.advance_job(jid2); }
        assert_eq!(mgr.total_pages_printed(), 5);
    }

    // --- PrintDialog ---
    #[test]
    fn test_dialog_open_close() {
        let mut dlg = PrintDialog::new();
        assert!(!dlg.visible);
        dlg.open("test.pdf", 10);
        assert!(dlg.visible);
        assert_eq!(dlg.document_name, "test.pdf");
        dlg.close();
        assert!(!dlg.visible);
    }

    #[test]
    fn test_dialog_validate() {
        let mut dlg = PrintDialog::new();
        dlg.settings.duplex = true;
        let caps = PrinterCapabilities::basic();
        dlg.validate(&caps);
        assert!(!dlg.is_valid());
    }

    #[test]
    fn test_dialog_render_hidden() {
        let dlg = PrintDialog::new();
        let cmds = dlg.render(&[], 0.0, 0.0, 800.0, 600.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_dialog_render_visible() {
        let mut dlg = PrintDialog::new();
        dlg.open("doc.pdf", 5);
        let cmds = dlg.render(&[], 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_default_trait_impls() {
        let _ = PrintSettings::default();
        let _ = PrintManager::default();
        let _ = PrintDialog::default();
    }

    // --- PrintQuality ---
    #[test]
    fn test_quality_labels() {
        assert!(PrintQuality::Draft.label().contains("fast"));
        assert!(PrintQuality::Best.label().contains("slow"));
    }

    // --- OutputFormat extensions ---
    #[test]
    fn test_job_state_labels() {
        assert_eq!(JobState::Queued.label(), "Queued");
        assert_eq!(JobState::Printing.label(), "Printing");
    }
}
