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

use appearance::Palette;
use guitk::color::Color;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// Colour
//
// Every colour this module draws is resolved from the live `Palette`. Four
// judgements decide which role each site asks for:
//
//  1. `Printer::status_color` (offline/busy/ready) is a CATEGORY, not a
//     decoration -- red *means* "this printer will not print". Frozen; never
//     follows the accent.
//  2. `JobState::color` is the same, six ways. Note `Queued => p.blue`: blue
//     and the stock accent are the same colour, so only an out-of-palette
//     accent can tell a correct freeze from a wrong accent-follow. The tests
//     use one.
//  3. The "Print" button is the default action -- what Enter does -- which is
//     the accent's job. Its fill is `p.accent` and its label is
//     `p.on_accent()`, derived rather than named, because a pale accent needs
//     dark ink and a deep one needs light. "Cancel" is not the default action
//     and stays `p.surface1` / `p.text`.
//  4. The selected printer's name marks the choice you have made, so it takes
//     the accent; the field it sits in stays `p.surface0`. Position is marked
//     by the label, not by repainting the furniture.

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
                PaperSize::A4,
                PaperSize::Letter,
                PaperSize::Legal,
                PaperSize::A3,
                PaperSize::A5,
                PaperSize::Envelope,
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

    /// The colour this printer reports its readiness in.
    ///
    /// Categorical: red *means* offline. See judgement 1 in the module's
    /// colour notes -- this never follows the accent.
    pub fn status_color(&self, p: &Palette) -> Color {
        if !self.online {
            p.red
        } else if self.queue_count > 0 {
            p.yellow
        } else {
            p.green
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

    /// The colour this job state reports itself in.
    ///
    /// A six-way category scale, so every arm is frozen. `Queued` is
    /// `p.blue` and NOT `p.accent`, which is a distinction the stock accent
    /// cannot show: see judgement 2 in the module's colour notes.
    pub fn color(&self, p: &Palette) -> Color {
        match self {
            Self::Queued => p.blue,
            Self::Printing => p.peach,
            Self::Paused => p.yellow,
            Self::Completed => p.green,
            Self::Failed => p.red,
            Self::Cancelled => p.overlay0,
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
            && (start == 0 || end < start)
        {
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
    /// Progress as percentage (0-100). A job with no page count is at 0%.
    #[must_use]
    pub fn progress_pct(&self) -> u32 {
        ratio::percent_whole(self.pages_printed, self.total_pages).unwrap_or(0)
    }

    /// Size display.
    pub fn size_display(&self) -> String {
        guitk::bytes::iec(self.size_bytes)
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
                    && let Some(p) = self.printers.iter_mut().find(|p| p.id == def_id)
                {
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
        self.default_printer_id
            .and_then(|id| self.printers.iter().find(|p| p.id == id))
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
            && (job.state == JobState::Printing || job.state == JobState::Queued)
        {
            job.state = JobState::Paused;
            return true;
        }
        false
    }

    /// Resume a paused job.
    pub fn resume_job(&mut self, job_id: u32) -> bool {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id)
            && job.state == JobState::Paused
        {
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
        self.jobs
            .iter()
            .filter(|j| !j.state.is_terminal())
            .collect()
    }

    /// Get all jobs for a specific printer.
    pub fn jobs_for_printer(&self, printer_id: u32) -> Vec<&PrintJob> {
        self.jobs
            .iter()
            .filter(|j| j.printer_id == printer_id)
            .collect()
    }

    /// Purge completed/cancelled/failed jobs from history.
    pub fn purge_terminal_jobs(&mut self) -> usize {
        let before = self.jobs.len();
        self.jobs.retain(|j| !j.state.is_terminal());
        // `retain` only shrinks; saturating says so in the expression itself.
        before.saturating_sub(self.jobs.len())
    }

    /// Total pages printed across all completed jobs.
    pub fn total_pages_printed(&self) -> u64 {
        self.jobs
            .iter()
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
    pub fn render(
        &self,
        p: &Palette,
        printers: &[Printer],
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        if !self.visible {
            return cmds;
        }

        // Overlay background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: Color::rgba(0, 0, 0, 128),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog box.
        let dw = 500.0;
        let dh = 400.0;
        let dx = x + (w - dw) / 2.0;
        let dy = y + (h - dh) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: p.base,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 16.0,
            text: "Print".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Document name.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 40.0,
            text: format!("Document: {}", self.document_name),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Printer selector.
        cmds.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + 68.0,
            text: "Printer:".to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        let printer_name = printers
            .get(self.selected_printer_idx)
            .map(|p| p.name.as_str())
            .unwrap_or("None");
        cmds.push(RenderCommand::FillRect {
            x: dx + 100.0,
            y: dy + 62.0,
            width: 280.0,
            height: 24.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + 108.0,
            y: dy + 66.0,
            text: printer_name.to_string(),
            font_size: 12.0,
            color: p.accent,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Settings rows.
        let mut cy = dy + 100.0;
        let settings_rows = [
            ("Copies:", format!("{}", self.settings.copies)),
            ("Paper:", self.settings.paper_size.label().to_string()),
            (
                "Orientation:",
                if self.settings.orientation == Orientation::Portrait {
                    "Portrait"
                } else {
                    "Landscape"
                }
                .to_string(),
            ),
            ("Quality:", self.settings.quality.label().to_string()),
            (
                "Color:",
                match self.settings.color_mode {
                    ColorMode::Color => "Color",
                    ColorMode::Grayscale => "Grayscale",
                    ColorMode::MonoBlack => "Black & White",
                }
                .to_string(),
            ),
            (
                "Duplex:",
                if self.settings.duplex { "On" } else { "Off" }.to_string(),
            ),
            (
                "Pages:",
                self.settings
                    .page_range
                    .map(|(s, e)| format!("{}-{}", s, e))
                    .unwrap_or_else(|| "All".to_string()),
            ),
        ];
        for (label, value) in &settings_rows {
            cmds.push(RenderCommand::Text {
                x: dx + 20.0,
                y: cy,
                text: label.to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: dx + 120.0,
                y: cy,
                text: value.clone(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 24.0;
        }

        // Validation errors.
        for err in &self.validation_errors {
            cmds.push(RenderCommand::Text {
                x: dx + 20.0,
                y: cy,
                text: err.clone(),
                font_size: 11.0,
                color: p.red,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 18.0;
        }

        // Buttons.
        let btn_y = dy + dh - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: dx + dw - 180.0,
            y: btn_y,
            width: 70.0,
            height: 28.0,
            color: p.accent,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dw - 166.0,
            y: btn_y + 7.0,
            text: "Print".to_string(),
            font_size: 12.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.push(RenderCommand::FillRect {
            x: dx + dw - 100.0,
            y: btn_y,
            width: 80.0,
            height: 28.0,
            color: p.surface1,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + dw - 84.0,
            y: btn_y + 7.0,
            text: "Cancel".to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;

    // ================================================================
    // Colour: every site resolves from the live palette
    // ================================================================

    use crate::palette_check::assert_drawn_from;

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    /// An accent that is a member of neither palette.
    ///
    /// The stock accent is blue, and this module has a `Queued => p.blue`
    /// arm, so under the stock accent "correctly frozen to blue" and
    /// "wrongly following the accent" are the same colour. Every table below
    /// renders this instead.
    const OFF_PALETTE: Color = Color::from_hex(0x00FF_8C1A);

    /// The palettes the role tables assert against: both modes, neither
    /// using an accent that any role could be confused with.
    fn table_palettes() -> Vec<(String, Palette)> {
        [false, true]
            .into_iter()
            .map(|light| {
                let mut p = Palette::for_mode(light);
                p.accent = OFF_PALETTE;
                (format!("light={light}"), p)
            })
            .collect()
    }

    fn printers() -> Vec<Printer> {
        vec![Printer {
            id: 1,
            name: "Office Laser".to_string(),
            model: "L100".to_string(),
            connection: PrinterConnection::Network,
            capabilities: PrinterCapabilities::full_color(),
            online: true,
            is_default: true,
            queue_count: 0,
            ink_level: Some(80),
        }]
    }

    /// One fixture per branch of `render` that selects or suppresses a
    /// colour. Geometry-only branches are deliberately absent; branches that
    /// change *what is drawn* are all here.
    fn every_state() -> Vec<(String, PrintDialog)> {
        let mut out = Vec::new();

        // Hidden: draws nothing at all.
        out.push(("hidden".to_string(), PrintDialog::new()));

        // Visible, a printer selected, portrait, no duplex, no errors.
        let mut d = PrintDialog::new();
        d.open("report.pdf", 12);
        out.push(("open: portrait, simplex".to_string(), d));

        // Landscape and duplex flip two settings-row *values*.
        let mut d = PrintDialog::new();
        d.open("report.pdf", 12);
        d.settings.orientation = Orientation::Landscape;
        d.settings.duplex = true;
        out.push(("open: landscape, duplex".to_string(), d));

        // No printer selected -- the field falls back to "None".
        let mut d = PrintDialog::new();
        d.open("report.pdf", 12);
        d.selected_printer_idx = 99;
        out.push(("open: no printer".to_string(), d));

        // Validation errors -- the only site that draws `p.red`.
        let mut d = PrintDialog::new();
        d.open("report.pdf", 12);
        d.validation_errors = vec!["Duplex not supported".to_string()];
        out.push(("open: one error".to_string(), d));

        out
    }

    fn draw(name: &str, p: &Palette) -> Vec<RenderCommand> {
        let (_, d) = every_state()
            .into_iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no fixture named {name}"));
        d.render(p, &printers(), 0.0, 0.0, 800.0, 600.0)
    }

    fn every_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. } | RenderCommand::Text { color, .. } => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    /// The colour of the one text whose content contains `want`.
    ///
    /// Many rendered instances may share a single source site (the settings
    /// rows draw four labels from one expression), so the question is not
    /// "which one" but "do they all agree". If they ever disagree the site
    /// has grown a branch that needs naming, and this fails rather than
    /// silently reporting whichever came first.
    fn text_containing(cmds: &[RenderCommand], want: &str) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text.contains(want) => Some(*color),
                _ => None,
            })
            .collect();
        assert!(!hits.is_empty(), "no text containing {want:?}");
        assert!(
            hits.iter().all(|c| rgb(*c) == rgb(hits[0])),
            "the {} texts containing {want:?} are not all one colour",
            hits.len()
        );
        hits[0]
    }

    /// The colour of the one text that is exactly `want` at exactly `size`.
    ///
    /// Needed because this dialog draws the string "Print" three ways: as the
    /// title, inside the caption "Printer:", and as the default button's
    /// label -- in `p.text`, `p.text` and `p.on_accent()` respectively. A
    /// substring match would silently compare the wrong site, which is how
    /// module 25 nearly asserted a tab label against a window title.
    fn text_exact(cmds: &[RenderCommand], want: &str, size: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size,
                    ..
                } if text == want && *font_size == size => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "expected exactly one {want:?} at {size}pt");
        hits[0]
    }

    fn fill_of_size(cmds: &[RenderCommand], w: f32, h: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "expected exactly one {w}x{h} fill");
        hits[0]
    }

    #[test]
    fn every_colour_this_dialog_draws_comes_from_its_palette() {
        // The membership half: after the conversion no drawn colour may be a
        // value the palette does not contain. A leftover Mocha constant is
        // invisible in Mocha and shows up the moment Latte is rendered.
        for light in [false, true] {
            for accent in [
                appearance::MAUVE,
                appearance::TEAL,
                appearance::SAPPHIRE,
                appearance::PINK,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (what, d) in every_state() {
                    let cmds = d.render(&p, &printers(), 0.0, 0.0, 800.0, 600.0);
                    // `on_accent` is derived from the accent rather than named,
                    // so it is not a palette member and must be declared.
                    assert_drawn_from(
                        &p,
                        &cmds,
                        &[p.on_accent()],
                        &format!("{what} light={light}"),
                    );
                }
            }
        }
    }

    #[test]
    fn the_fixtures_take_every_branch_this_dialog_has() {
        // The sweep above is worth exactly as much as the states it renders.
        // This pins the fixtures to the branches, so deleting a fixture fails
        // here rather than quietly narrowing every other test in the module.
        let p = Palette::for_mode(false);
        let says = |name: &str, want: &str| {
            let cmds = draw(name, &p);
            assert!(
                cmds.iter().any(|c| matches!(
                    c,
                    RenderCommand::Text { text, .. } if text.contains(want)
                )),
                "{name} never drew {want:?}"
            );
        };

        assert!(
            draw("hidden", &p).is_empty(),
            "the hidden dialog drew something"
        );
        says("open: portrait, simplex", "Portrait");
        says("open: portrait, simplex", "Off");
        says("open: landscape, duplex", "Landscape");
        says("open: landscape, duplex", "On");
        says("open: portrait, simplex", "Office Laser");
        says("open: no printer", "None");
        says("open: one error", "Duplex not supported");
        assert!(
            !draw("open: portrait, simplex", &p).iter().any(
                |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("supported"))
            ),
            "the no-error fixture drew an error"
        );
    }

    #[test]
    fn every_text_this_dialog_draws_is_in_the_role_it_claims() {
        // ONE ENTRY PER SOURCE SITE. The four settings rows are one site, not
        // four; the printer name and the title are two sites even though both
        // could be called "a label".
        for (mode, p) in table_palettes() {
            let open = draw("open: portrait, simplex", &p);
            let err = draw("open: one error", &p);

            // Title.
            assert_eq!(rgb(text_exact(&open, "Print", 16.0)), rgb(p.text), "{mode}");
            // Document name.
            assert_eq!(
                rgb(text_containing(&open, "Document:")),
                rgb(p.subtext0),
                "{mode}"
            );
            // "Printer:" caption.
            assert_eq!(
                rgb(text_containing(&open, "Printer:")),
                rgb(p.text),
                "{mode}"
            );
            // The selected printer's name -- judgement 4, follows the accent.
            assert_eq!(
                rgb(text_containing(&open, "Office Laser")),
                rgb(p.accent),
                "{mode}"
            );
            // Settings-row label (one site, four instances).
            assert_eq!(
                rgb(text_containing(&open, "Copies:")),
                rgb(p.text),
                "{mode}"
            );
            // Settings-row value (one site, four instances).
            assert_eq!(
                rgb(text_containing(&open, "Portrait")),
                rgb(p.subtext0),
                "{mode}"
            );
            // Validation error -- a reading of what went wrong, so frozen red.
            assert_eq!(
                rgb(text_containing(&err, "Duplex not supported")),
                rgb(p.red),
                "{mode}"
            );
            // The Print button's label -- judgement 3, ink DERIVED from the
            // fill it sits on rather than named, so a pale accent gets dark
            // ink and a deep one gets light.
            assert_eq!(
                rgb(text_exact(&open, "Print", 12.0)),
                rgb(p.on_accent()),
                "{mode}"
            );
            // Cancel's label. Stated as the role, never as `p.on_accent()`:
            // Cancel is not the default action and must not track the accent.
            assert_eq!(
                rgb(text_exact(&open, "Cancel", 12.0)),
                rgb(p.text),
                "{mode}"
            );
        }
    }

    #[test]
    fn every_rectangle_this_dialog_draws_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let open = draw("open: portrait, simplex", &p);

            // The scrim is black at an alpha, not a role.
            assert_eq!(rgb(fill_of_size(&open, 800.0, 600.0)), (0, 0, 0), "{mode}");
            // Dialog box.
            assert_eq!(
                rgb(fill_of_size(&open, 500.0, 400.0)),
                rgb(p.base),
                "{mode}"
            );
            // Printer field -- judgement 4: the furniture stays put, only the
            // label inside it marks the choice.
            assert_eq!(
                rgb(fill_of_size(&open, 280.0, 24.0)),
                rgb(p.surface0),
                "{mode}"
            );
            // Print button: the default action.
            assert_eq!(
                rgb(fill_of_size(&open, 70.0, 28.0)),
                rgb(p.accent),
                "{mode}"
            );
            // Cancel button: not the default action.
            assert_eq!(
                rgb(fill_of_size(&open, 80.0, 28.0)),
                rgb(p.surface1),
                "{mode}"
            );
        }
    }

    #[test]
    fn every_choice_this_module_makes_hands_over_the_role_it_claims() {
        // `status_color` and `JobState::color` are pure CHOICE sites: neither
        // is called by `render`, so nothing else in this file can reach them.
        // Without this table all nine arms are checked by nothing at all --
        // the sweep waves them through, because every arm names a role and a
        // role is a member of both palettes.
        for (mode, p) in table_palettes() {
            let printer = |online: bool, queued: u32| Printer {
                id: 1,
                name: "P".to_string(),
                model: "M".to_string(),
                connection: PrinterConnection::Usb,
                capabilities: PrinterCapabilities::basic(),
                online,
                is_default: false,
                queue_count: queued,
                ink_level: None,
            };
            assert_eq!(
                rgb(printer(false, 0).status_color(&p)),
                rgb(p.red),
                "offline is not red ({mode})"
            );
            assert_eq!(
                rgb(printer(true, 3).status_color(&p)),
                rgb(p.yellow),
                "busy is not yellow ({mode})"
            );
            assert_eq!(
                rgb(printer(true, 0).status_color(&p)),
                rgb(p.green),
                "ready is not green ({mode})"
            );

            // The six job states. `Queued` is the reason this table renders an
            // off-palette accent: under the stock accent `p.blue` and
            // `p.accent` are the same colour, so a queued job that wrongly
            // followed the accent would read as correct.
            assert_eq!(rgb(JobState::Queued.color(&p)), rgb(p.blue), "{mode}");
            assert_eq!(rgb(JobState::Printing.color(&p)), rgb(p.peach), "{mode}");
            assert_eq!(rgb(JobState::Paused.color(&p)), rgb(p.yellow), "{mode}");
            assert_eq!(rgb(JobState::Completed.color(&p)), rgb(p.green), "{mode}");
            assert_eq!(rgb(JobState::Failed.color(&p)), rgb(p.red), "{mode}");
            assert_eq!(
                rgb(JobState::Cancelled.color(&p)),
                rgb(p.overlay0),
                "{mode}"
            );
        }
    }

    #[test]
    fn nothing_but_the_selection_and_the_default_action_moves_with_the_accent() {
        // Render each fixture under two accents and count what moves.
        //
        // Counting is the load-bearing part. Exempting sites "that are meant
        // to follow the accent" by testing whether they follow the accent
        // exempts the bug too -- a job state or a printer status that wrongly
        // took the accent satisfies that description exactly. The NUMBER of
        // followers is what a new one changes.
        //
        // This dialog is allowed three moving commands: the selected
        // printer's name and the Print button's fill both take the accent,
        // and the Print button's label is derived from it via `on_accent`.
        const A: Color = Color::from_hex(0x00FF_8C1A);
        const B: Color = Color::from_hex(0x0012_9E7D);

        for light in [false, true] {
            let (mut pa, mut pb) = (Palette::for_mode(light), Palette::for_mode(light));
            pa.accent = A;
            pb.accent = B;
            for (what, d) in every_state() {
                let ca = d.render(&pa, &printers(), 0.0, 0.0, 800.0, 600.0);
                let cb = d.render(&pb, &printers(), 0.0, 0.0, 800.0, 600.0);
                assert_eq!(
                    ca.len(),
                    cb.len(),
                    "{what}: the accent changed how much is drawn"
                );
                let want = if what == "hidden" { 0 } else { 3 };
                let (mut on_accent_moves, mut accent_moves) = (0, 0);
                let (xa, xb) = (every_color(&ca), every_color(&cb));
                for (i, (a, b)) in xa.iter().zip(xb.iter()).enumerate() {
                    if rgb(*a) == rgb(*b) {
                        continue;
                    }
                    if (rgb(*a), rgb(*b)) == (rgb(A), rgb(B)) {
                        accent_moves += 1;
                    } else if (rgb(*a), rgb(*b)) == (rgb(pa.on_accent()), rgb(pb.on_accent())) {
                        on_accent_moves += 1;
                    } else {
                        panic!(
                            "{what} (light={light}): command {i} changed with the accent without being the accent or ink derived from it"
                        );
                    }
                }
                assert_eq!(
                    accent_moves + on_accent_moves,
                    want,
                    "{what} (light={light}): {accent_moves} accent sites and \
                     {on_accent_moves} derived-ink sites move, but this state \
                     is allowed exactly {want} in total"
                );
            }
        }
    }

    #[test]
    fn every_state_a_job_can_be_in_stays_apart_from_every_other() {
        // The six colours are what carry the meaning, so no two may collide,
        // and none may collide with the accent that happens to be set -- a
        // user on an orange desktop must still be able to see that a job
        // failed rather than merely printing.
        for light in [false, true] {
            for accent in [
                appearance::MAUVE,
                appearance::TEAL,
                appearance::SAPPHIRE,
                appearance::PINK,
                OFF_PALETTE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let all = [
                    ("queued", JobState::Queued),
                    ("printing", JobState::Printing),
                    ("paused", JobState::Paused),
                    ("completed", JobState::Completed),
                    ("failed", JobState::Failed),
                    ("cancelled", JobState::Cancelled),
                ];
                for (i, (na, a)) in all.iter().enumerate() {
                    for (nb, b) in all.iter().skip(i + 1) {
                        assert_ne!(
                            rgb(a.color(&p)),
                            rgb(b.color(&p)),
                            "{na} and {nb} collide (light={light})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_action_ink_stays_readable_in_both_modes() {
        // `on_accent` is DERIVED, and proving that needs the accent to vary
        // far enough to change the answer. The stock accents are all pastel
        // and every one of them yields the same dark ink, so a hard-coded
        // constant would pass against them. A deliberately dark accent and a
        // deliberately pale one must produce different ink.
        let dark = Color::from_hex(0x0020_3050);
        let pale = Color::from_hex(0x00F5_D0E0);
        for light in [false, true] {
            let ink = |accent: Color| {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = draw("open: portrait, simplex", &p);
                // The Print button's label is the only text drawn on a filled
                // accent rectangle.
                rgb(text_exact(&cmds, "Print", 12.0))
            };
            assert_ne!(
                ink(dark),
                ink(pale),
                "the Print button's ink does not follow its fill (light={light})"
            );
        }
    }

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
            id: 1,
            name: "Test".to_string(),
            model: "Test".to_string(),
            connection: PrinterConnection::Usb,
            capabilities: PrinterCapabilities::basic(),
            online: true,
            is_default: false,
            queue_count: 0,
            ink_level: Some(80),
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
            id: 1,
            document_name: "test.pdf".to_string(),
            printer_id: 1,
            state: JobState::Printing,
            settings: PrintSettings::default_settings(),
            total_pages: 10,
            pages_printed: 5,
            submitted_at: 0,
            completed_at: None,
            size_bytes: 50000,
            owner: "user".to_string(),
        };
        assert_eq!(job.progress_pct(), 50);
        job.pages_printed = 0;
        assert_eq!(job.progress_pct(), 0);
    }

    #[test]
    fn test_job_size_display() {
        let job = PrintJob {
            id: 1,
            document_name: "test".to_string(),
            printer_id: 1,
            state: JobState::Queued,
            settings: PrintSettings::default_settings(),
            total_pages: 1,
            pages_printed: 0,
            submitted_at: 0,
            completed_at: None,
            size_bytes: 2048,
            owner: "user".to_string(),
        };
        assert_eq!(job.size_display(), "2.0 KiB");
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
            id: 0,
            name: "HP".to_string(),
            model: "LaserJet".to_string(),
            connection: PrinterConnection::Network,
            capabilities: PrinterCapabilities::basic(),
            online: true,
            is_default: false,
            queue_count: 0,
            ink_level: None,
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
            id: 0,
            name: "HP".to_string(),
            model: "LJ".to_string(),
            connection: PrinterConnection::Usb,
            capabilities: PrinterCapabilities::basic(),
            online: true,
            is_default: false,
            queue_count: 0,
            ink_level: None,
        };
        let id = mgr.add_printer(p).unwrap();
        assert!(mgr.set_default(id));
        assert_eq!(mgr.default_printer_id, Some(id));
    }

    #[test]
    fn test_submit_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job(
            "doc.pdf",
            pid,
            PrintSettings::default_settings(),
            10,
            5000,
            "user",
            1000,
        );
        assert!(jid.is_some());
        assert_eq!(mgr.jobs.len(), 1);
        assert_eq!(mgr.printers[0].queue_count, 1);
    }

    #[test]
    fn test_submit_job_offline_printer() {
        let mut mgr = PrintManager::new();
        mgr.printers[0].online = false;
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job(
            "doc.pdf",
            pid,
            PrintSettings::default_settings(),
            1,
            100,
            "u",
            0,
        );
        assert!(jid.is_none());
    }

    #[test]
    fn test_submit_job_spooler_off() {
        let mut mgr = PrintManager::new();
        mgr.set_spooler(false);
        let pid = mgr.printers[0].id;
        let jid = mgr.submit_job(
            "doc.pdf",
            pid,
            PrintSettings::default_settings(),
            1,
            100,
            "u",
            0,
        );
        assert!(jid.is_none());
    }

    #[test]
    fn test_cancel_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr
            .submit_job(
                "doc.pdf",
                pid,
                PrintSettings::default_settings(),
                10,
                5000,
                "u",
                0,
            )
            .unwrap();
        assert!(mgr.cancel_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Cancelled);
    }

    #[test]
    fn test_cancel_completed_job_fails() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr
            .submit_job(
                "doc.pdf",
                pid,
                PrintSettings::default_settings(),
                1,
                100,
                "u",
                0,
            )
            .unwrap();
        mgr.advance_job(jid); // complete
        assert!(!mgr.cancel_job(jid));
    }

    #[test]
    fn test_pause_resume_job() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr
            .submit_job(
                "doc.pdf",
                pid,
                PrintSettings::default_settings(),
                5,
                100,
                "u",
                0,
            )
            .unwrap();
        assert!(mgr.pause_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Paused);
        assert!(mgr.resume_job(jid));
        assert_eq!(mgr.jobs[0].state, JobState::Queued);
    }

    #[test]
    fn test_advance_job_to_completion() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        let jid = mgr
            .submit_job(
                "doc.pdf",
                pid,
                PrintSettings::default_settings(),
                3,
                100,
                "u",
                0,
            )
            .unwrap();
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
        mgr.submit_job(
            "a.pdf",
            pid,
            PrintSettings::default_settings(),
            5,
            100,
            "u",
            0,
        );
        mgr.submit_job(
            "b.pdf",
            pid,
            PrintSettings::default_settings(),
            1,
            100,
            "u",
            0,
        );
        let jid2 = mgr.jobs[1].id;
        mgr.advance_job(jid2); // complete b.pdf
        assert_eq!(mgr.active_jobs().len(), 1);
    }

    #[test]
    fn test_purge_terminal() {
        let mut mgr = PrintManager::new();
        let pid = mgr.printers[0].id;
        mgr.submit_job(
            "a.pdf",
            pid,
            PrintSettings::default_settings(),
            1,
            100,
            "u",
            0,
        );
        mgr.submit_job(
            "b.pdf",
            pid,
            PrintSettings::default_settings(),
            5,
            100,
            "u",
            0,
        );
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
        mgr.submit_job(
            "a.pdf",
            pid,
            PrintSettings::default_settings(),
            3,
            100,
            "u",
            0,
        );
        mgr.submit_job(
            "b.pdf",
            pid,
            PrintSettings::default_settings(),
            2,
            100,
            "u",
            0,
        );
        let jid1 = mgr.jobs[0].id;
        let jid2 = mgr.jobs[1].id;
        for _ in 0..3 {
            mgr.advance_job(jid1);
        }
        for _ in 0..2 {
            mgr.advance_job(jid2);
        }
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
        let cmds = dlg.render(&Palette::for_mode(false), &[], 0.0, 0.0, 800.0, 600.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_dialog_render_visible() {
        let mut dlg = PrintDialog::new();
        dlg.open("doc.pdf", 5);
        let cmds = dlg.render(&Palette::for_mode(false), &[], 0.0, 0.0, 800.0, 600.0);
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
