//! SlateOS System Benchmark
//!
//! Graphical performance benchmarking application with:
//! - CPU benchmark (integer arithmetic, floating point, prime sieve, matrix multiply)
//! - Memory benchmark (sequential read/write throughput, random access latency, bandwidth)
//! - Disk benchmark (sequential read/write, random 4K read/write, IOPS estimation)
//! - Graphics benchmark (fill rate, text rendering count, composite operations score)
//! - Weighted overall composite score
//! - History of last 10 benchmark runs with timestamps
//! - Comparison of current vs previous results (improvement/regression)
//! - Animated progress bars during each benchmark phase
//! - Detailed results with bar charts
//! - Export results as text report
//! - Hardware info display alongside scores
//! - Dark theme (Catppuccin Mocha)
//!
//! Uses the guitk library for UI rendering. Actual hardware benchmarks are
//! simulated with representative computation; on real SlateOS hardware the
//! stubs would be replaced with timed kernel/driver calls.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::rgb(30, 30, 46);
const MANTLE: Color = Color::rgb(24, 24, 37);
const CRUST: Color = Color::rgb(17, 17, 27);
const SURFACE0: Color = Color::rgb(49, 50, 68);
const SURFACE1: Color = Color::rgb(69, 71, 90);
#[allow(dead_code)]
const SURFACE2: Color = Color::rgb(88, 91, 112);
const TEXT_COLOR: Color = Color::rgb(205, 214, 244);
const SUBTEXT0: Color = Color::rgb(166, 173, 200);
const BLUE: Color = Color::rgb(137, 180, 250);
const GREEN: Color = Color::rgb(166, 227, 161);
const RED: Color = Color::rgb(243, 139, 168);
const YELLOW: Color = Color::rgb(249, 226, 175);
const PEACH: Color = Color::rgb(250, 179, 135);
const LAVENDER: Color = Color::rgb(180, 190, 254);
const OVERLAY0: Color = Color::rgb(108, 112, 134);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 740.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
#[allow(dead_code)]
const SIDEBAR_WIDTH: f32 = 220.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const CONTENT_PADDING: f32 = 16.0;
const ROW_HEIGHT: f32 = 24.0;
const BAR_CHART_MAX_WIDTH: f32 = 300.0;
const BAR_CHART_HEIGHT: f32 = 16.0;
const PROGRESS_BAR_HEIGHT: f32 = 20.0;
const BUTTON_WIDTH: f32 = 120.0;
const BUTTON_HEIGHT: f32 = 34.0;
const MAX_HISTORY: usize = 10;

// Score weights for overall composite.
const CPU_WEIGHT: f64 = 0.35;
const MEMORY_WEIGHT: f64 = 0.25;
const DISK_WEIGHT: f64 = 0.25;
const GRAPHICS_WEIGHT: f64 = 0.15;

// ============================================================================
// Benchmark Categories
// ============================================================================

/// Top-level tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Cpu,
    Memory,
    Disk,
    Graphics,
    History,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::Graphics => "Graphics",
            Self::History => "History",
        }
    }

    fn all() -> &'static [Tab] {
        &[
            Tab::Overview,
            Tab::Cpu,
            Tab::Memory,
            Tab::Disk,
            Tab::Graphics,
            Tab::History,
        ]
    }
}

// ============================================================================
// Hardware Info
// ============================================================================

/// Detected (or stubbed) hardware information.
#[derive(Clone, Debug)]
pub struct HardwareInfo {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub cpu_freq_mhz: u32,
    pub ram_total_mb: u64,
    pub ram_type: String,
    pub ram_speed_mhz: u32,
    pub disk_model: String,
    pub disk_capacity_gb: u64,
    pub disk_interface: String,
    pub gpu_model: String,
    pub gpu_vram_mb: u32,
    pub os_version: String,
}

impl Default for HardwareInfo {
    fn default() -> Self {
        Self {
            cpu_model: "Slate OS Virtual CPU @ 3.6 GHz".into(),
            cpu_cores: 4,
            cpu_threads: 8,
            cpu_freq_mhz: 3600,
            ram_total_mb: 8192,
            ram_type: "DDR4".into(),
            ram_speed_mhz: 3200,
            disk_model: "Slate OS VirtIO Block Device".into(),
            disk_capacity_gb: 256,
            disk_interface: "NVMe".into(),
            gpu_model: "Slate OS VirtIO GPU".into(),
            gpu_vram_mb: 256,
            os_version: "Slate OS 0.1.0".into(),
        }
    }
}

impl HardwareInfo {
    /// Format hardware summary as lines for display.
    pub fn summary_lines(&self) -> Vec<(String, String)> {
        vec![
            ("CPU".into(), self.cpu_model.clone()),
            ("Cores / Threads".into(), format!("{} / {}", self.cpu_cores, self.cpu_threads)),
            ("CPU Frequency".into(), format!("{} MHz", self.cpu_freq_mhz)),
            ("RAM".into(), format!("{} MB {}", self.ram_total_mb, self.ram_type)),
            ("RAM Speed".into(), format!("{} MHz", self.ram_speed_mhz)),
            ("Disk".into(), self.disk_model.clone()),
            ("Disk Capacity".into(), format!("{} GB", self.disk_capacity_gb)),
            ("Disk Interface".into(), self.disk_interface.clone()),
            ("GPU".into(), self.gpu_model.clone()),
            ("GPU VRAM".into(), format!("{} MB", self.gpu_vram_mb)),
            ("OS".into(), self.os_version.clone()),
        ]
    }

    /// Format as text for export.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("=== Hardware Info ===\n");
        for (label, value) in self.summary_lines() {
            out.push_str(&format!("  {:<20} {}\n", label, value));
        }
        out
    }
}

// ============================================================================
// Individual Test Results
// ============================================================================

/// A single named sub-test result within a benchmark category.
#[derive(Clone, Debug)]
pub struct SubTestResult {
    /// Human-readable test name.
    pub name: String,
    /// Measured score (higher is better, except latency where lower is better).
    pub score: f64,
    /// Unit label (e.g., "ops/s", "MB/s", "ns", "fps").
    pub unit: String,
    /// Whether lower scores are better (e.g., latency).
    pub lower_is_better: bool,
}

impl SubTestResult {
    pub fn new(name: &str, score: f64, unit: &str, lower_is_better: bool) -> Self {
        Self {
            name: name.into(),
            score,
            unit: unit.into(),
            lower_is_better,
        }
    }

    /// Format score with unit.
    pub fn formatted_score(&self) -> String {
        if self.score >= 1_000_000.0 {
            format!("{:.2}M {}", self.score / 1_000_000.0, self.unit)
        } else if self.score >= 1_000.0 {
            format!("{:.1}K {}", self.score / 1_000.0, self.unit)
        } else if self.score < 1.0 && self.score > 0.0 {
            format!("{:.3} {}", self.score, self.unit)
        } else {
            format!("{:.1} {}", self.score, self.unit)
        }
    }
}

// ============================================================================
// Category Results
// ============================================================================

/// Results for a single benchmark category (CPU, Memory, Disk, or Graphics).
#[derive(Clone, Debug)]
pub struct CategoryResult {
    /// Category name.
    pub name: String,
    /// Sub-test results within this category.
    pub sub_tests: Vec<SubTestResult>,
    /// Composite score for this category (0-10000 scale).
    pub composite_score: f64,
}

impl CategoryResult {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            sub_tests: Vec::new(),
            composite_score: 0.0,
        }
    }

    /// Compute a normalized composite score from sub-test results.
    /// Each sub-test contributes equally to the composite.
    pub fn compute_composite(&mut self) {
        if self.sub_tests.is_empty() {
            self.composite_score = 0.0;
            return;
        }
        let mut total = 0.0;
        let count = self.sub_tests.len() as f64;
        for sub in &self.sub_tests {
            // Normalize each sub-test score to roughly 0-10000.
            // The normalization factors are tuned per-category in the
            // benchmark runner.
            total += sub.score;
        }
        self.composite_score = total / count;
    }

    /// Format as text lines for export.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(&format!("--- {} (Score: {:.0}) ---\n", self.name, self.composite_score));
        for sub in &self.sub_tests {
            let direction = if sub.lower_is_better { " (lower=better)" } else { "" };
            out.push_str(&format!("  {:<30} {}{}\n", sub.name, sub.formatted_score(), direction));
        }
        out
    }
}

// ============================================================================
// Full Benchmark Run Result
// ============================================================================

/// Complete results from a single benchmark run.
#[derive(Clone, Debug)]
pub struct BenchmarkResult {
    /// CPU benchmark results.
    pub cpu: CategoryResult,
    /// Memory benchmark results.
    pub memory: CategoryResult,
    /// Disk benchmark results.
    pub disk: CategoryResult,
    /// Graphics benchmark results.
    pub graphics: CategoryResult,
    /// Overall weighted composite score.
    pub overall_score: f64,
    /// Unix-epoch timestamp when the run completed.
    pub timestamp: u64,
    /// Hardware info snapshot at time of benchmark.
    pub hardware: HardwareInfo,
}

impl BenchmarkResult {
    /// Compute the weighted overall score from category composites.
    pub fn compute_overall(&mut self) {
        self.overall_score = self.cpu.composite_score * CPU_WEIGHT
            + self.memory.composite_score * MEMORY_WEIGHT
            + self.disk.composite_score * DISK_WEIGHT
            + self.graphics.composite_score * GRAPHICS_WEIGHT;
    }

    /// Format as a complete text report.
    pub fn to_text_report(&self) -> String {
        let mut out = String::with_capacity(2048);
        out.push_str("========================================\n");
        out.push_str("     Slate OS System Benchmark Report\n");
        out.push_str("========================================\n\n");
        out.push_str(&self.hardware.to_text());
        out.push('\n');
        out.push_str(&format!("Overall Score: {:.0}\n\n", self.overall_score));
        out.push_str(&self.cpu.to_text());
        out.push('\n');
        out.push_str(&self.memory.to_text());
        out.push('\n');
        out.push_str(&self.disk.to_text());
        out.push('\n');
        out.push_str(&self.graphics.to_text());
        out.push('\n');
        out.push_str(&format!("Timestamp: {}\n", self.timestamp));
        out.push_str("========================================\n");
        out
    }

    /// Compute percentage change from a previous result for each category.
    pub fn comparison_vs(&self, previous: &BenchmarkResult) -> ComparisonResult {
        ComparisonResult {
            cpu_change_pct: percent_change(previous.cpu.composite_score, self.cpu.composite_score),
            memory_change_pct: percent_change(previous.memory.composite_score, self.memory.composite_score),
            disk_change_pct: percent_change(previous.disk.composite_score, self.disk.composite_score),
            graphics_change_pct: percent_change(previous.graphics.composite_score, self.graphics.composite_score),
            overall_change_pct: percent_change(previous.overall_score, self.overall_score),
        }
    }
}

/// Percentage change from previous to current.
/// Returns positive for improvement, negative for regression.
pub fn percent_change(previous: f64, current: f64) -> f64 {
    if previous.abs() < f64::EPSILON {
        if current.abs() < f64::EPSILON {
            return 0.0;
        }
        return 100.0;
    }
    ((current - previous) / previous) * 100.0
}

/// Comparison between two benchmark runs.
#[derive(Clone, Debug)]
pub struct ComparisonResult {
    pub cpu_change_pct: f64,
    pub memory_change_pct: f64,
    pub disk_change_pct: f64,
    pub graphics_change_pct: f64,
    pub overall_change_pct: f64,
}

impl ComparisonResult {
    /// All per-category changes as labeled pairs.
    pub fn as_pairs(&self) -> Vec<(&str, f64)> {
        vec![
            ("CPU", self.cpu_change_pct),
            ("Memory", self.memory_change_pct),
            ("Disk", self.disk_change_pct),
            ("Graphics", self.graphics_change_pct),
            ("Overall", self.overall_change_pct),
        ]
    }

    /// True if all categories improved or stayed the same.
    pub fn all_improved(&self) -> bool {
        self.cpu_change_pct >= 0.0
            && self.memory_change_pct >= 0.0
            && self.disk_change_pct >= 0.0
            && self.graphics_change_pct >= 0.0
    }

    /// True if any category regressed.
    pub fn has_regression(&self) -> bool {
        self.cpu_change_pct < 0.0
            || self.memory_change_pct < 0.0
            || self.disk_change_pct < 0.0
            || self.graphics_change_pct < 0.0
    }
}

// ============================================================================
// Benchmark History
// ============================================================================

/// History of benchmark runs, capped at MAX_HISTORY entries.
#[derive(Clone, Debug)]
pub struct BenchmarkHistory {
    runs: VecDeque<BenchmarkResult>,
}

impl BenchmarkHistory {
    pub fn new() -> Self {
        Self {
            runs: VecDeque::with_capacity(MAX_HISTORY),
        }
    }

    /// Add a new result, evicting the oldest if at capacity.
    pub fn push(&mut self, result: BenchmarkResult) {
        if self.runs.len() >= MAX_HISTORY {
            self.runs.pop_front();
        }
        self.runs.push_back(result);
    }

    /// Number of stored runs.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Get the most recent result.
    pub fn latest(&self) -> Option<&BenchmarkResult> {
        self.runs.back()
    }

    /// Get the second-most-recent result (for comparison).
    pub fn previous(&self) -> Option<&BenchmarkResult> {
        if self.runs.len() >= 2 {
            self.runs.get(self.runs.len().saturating_sub(2))
        } else {
            None
        }
    }

    /// Iterate over all runs (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &BenchmarkResult> {
        self.runs.iter()
    }

    /// Get run by index (0 = oldest).
    pub fn get(&self, index: usize) -> Option<&BenchmarkResult> {
        self.runs.get(index)
    }

    /// Best overall score across all runs.
    pub fn best_overall(&self) -> Option<f64> {
        self.runs.iter().map(|r| r.overall_score).reduce(f64::max)
    }

    /// Average overall score across all runs.
    pub fn average_overall(&self) -> Option<f64> {
        if self.runs.is_empty() {
            return None;
        }
        let sum: f64 = self.runs.iter().map(|r| r.overall_score).sum();
        Some(sum / self.runs.len() as f64)
    }

    /// Worst overall score across all runs.
    pub fn worst_overall(&self) -> Option<f64> {
        self.runs.iter().map(|r| r.overall_score).reduce(f64::min)
    }

    /// Export all runs as a text report.
    pub fn export_as_text(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str("Slate OS Benchmark History\n");
        out.push_str("=======================\n\n");
        if self.runs.is_empty() {
            out.push_str("No benchmark runs recorded.\n");
            return out;
        }
        for (i, run) in self.runs.iter().enumerate() {
            out.push_str(&format!("Run #{} (timestamp: {})\n", i.saturating_add(1), run.timestamp));
            out.push_str(&format!("  Overall:  {:.0}\n", run.overall_score));
            out.push_str(&format!("  CPU:      {:.0}\n", run.cpu.composite_score));
            out.push_str(&format!("  Memory:   {:.0}\n", run.memory.composite_score));
            out.push_str(&format!("  Disk:     {:.0}\n", run.disk.composite_score));
            out.push_str(&format!("  Graphics: {:.0}\n\n", run.graphics.composite_score));
        }
        if let Some(best) = self.best_overall() {
            out.push_str(&format!("Best overall:    {:.0}\n", best));
        }
        if let Some(avg) = self.average_overall() {
            out.push_str(&format!("Average overall: {:.0}\n", avg));
        }
        if let Some(worst) = self.worst_overall() {
            out.push_str(&format!("Worst overall:   {:.0}\n", worst));
        }
        out
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.runs.clear();
    }
}

impl Default for BenchmarkHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Progress Tracking
// ============================================================================

/// Phase of the overall benchmark run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchPhase {
    Idle,
    RunningCpu,
    RunningMemory,
    RunningDisk,
    RunningGraphics,
    Complete,
}

impl BenchPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::RunningCpu => "CPU Benchmark",
            Self::RunningMemory => "Memory Benchmark",
            Self::RunningDisk => "Disk Benchmark",
            Self::RunningGraphics => "Graphics Benchmark",
            Self::Complete => "Complete",
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::RunningCpu | Self::RunningMemory | Self::RunningDisk | Self::RunningGraphics
        )
    }

    pub fn is_idle(self) -> bool {
        self == Self::Idle
    }

    pub fn is_complete(self) -> bool {
        self == Self::Complete
    }

    /// Color associated with this phase.
    pub fn color(self) -> Color {
        match self {
            Self::Idle => SUBTEXT0,
            Self::RunningCpu => BLUE,
            Self::RunningMemory => GREEN,
            Self::RunningDisk => PEACH,
            Self::RunningGraphics => LAVENDER,
            Self::Complete => GREEN,
        }
    }

    /// Phase index (0-3) for phases that run, None otherwise.
    pub fn phase_index(self) -> Option<usize> {
        match self {
            Self::RunningCpu => Some(0),
            Self::RunningMemory => Some(1),
            Self::RunningDisk => Some(2),
            Self::RunningGraphics => Some(3),
            _ => None,
        }
    }
}

/// Progress tracker for the entire benchmark run.
#[derive(Clone, Debug)]
pub struct ProgressTracker {
    /// Current phase.
    pub phase: BenchPhase,
    /// Progress within current phase (0.0 to 1.0).
    pub phase_progress: f32,
    /// Total phases count.
    pub total_phases: u32,
    /// Completed phases count.
    pub completed_phases: u32,
    /// Elapsed time in the current run (milliseconds).
    pub elapsed_ms: u64,
    /// Current sub-test name being run.
    pub current_test_name: String,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self {
            phase: BenchPhase::Idle,
            phase_progress: 0.0,
            total_phases: 4,
            completed_phases: 0,
            elapsed_ms: 0,
            current_test_name: String::new(),
        }
    }

    /// Overall progress across all phases (0.0 to 1.0).
    pub fn overall_progress(&self) -> f32 {
        if self.total_phases == 0 {
            return 1.0;
        }
        let base = self.completed_phases as f32 / self.total_phases as f32;
        let phase_contribution = self.phase_progress / self.total_phases as f32;
        (base + phase_contribution).min(1.0)
    }

    /// Advance to the next phase.
    pub fn advance_phase(&mut self) {
        self.completed_phases = self.completed_phases.saturating_add(1);
        self.phase_progress = 0.0;
        self.phase = match self.phase {
            BenchPhase::Idle => BenchPhase::RunningCpu,
            BenchPhase::RunningCpu => BenchPhase::RunningMemory,
            BenchPhase::RunningMemory => BenchPhase::RunningDisk,
            BenchPhase::RunningDisk => BenchPhase::RunningGraphics,
            BenchPhase::RunningGraphics => BenchPhase::Complete,
            BenchPhase::Complete => BenchPhase::Complete,
        };
    }

    /// Reset to idle state.
    pub fn reset(&mut self) {
        self.phase = BenchPhase::Idle;
        self.phase_progress = 0.0;
        self.completed_phases = 0;
        self.elapsed_ms = 0;
        self.current_test_name.clear();
    }

    /// Set progress within current phase.
    pub fn set_progress(&mut self, fraction: f32, test_name: &str) {
        self.phase_progress = fraction.clamp(0.0, 1.0);
        self.current_test_name.clear();
        self.current_test_name.push_str(test_name);
    }

    /// Format elapsed time as mm:ss.
    pub fn elapsed_display(&self) -> String {
        let secs = self.elapsed_ms / 1000;
        let mins = secs / 60;
        let remainder = secs % 60;
        format!("{:02}:{:02}", mins, remainder)
    }
}

impl Default for ProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Simulated Benchmark Runners
// ============================================================================

// In a real SlateOS environment, these functions would use precise timing
// (rdtsc, kernel timers) to measure actual hardware performance.  For
// initial development we compute deterministic scores that exercise the
// scoring/aggregation/rendering pipeline.

/// Run CPU benchmarks. Returns a `CategoryResult` with all sub-tests.
pub fn run_cpu_benchmark() -> CategoryResult {
    let mut cat = CategoryResult::new("CPU");

    // Integer arithmetic score — simulated as ops/s.
    let integer_score = simulate_integer_benchmark();
    cat.sub_tests.push(SubTestResult::new(
        "Integer Arithmetic",
        integer_score,
        "Mops/s",
        false,
    ));

    // Floating point score.
    let fp_score = simulate_float_benchmark();
    cat.sub_tests.push(SubTestResult::new(
        "Floating Point",
        fp_score,
        "Mflops/s",
        false,
    ));

    // Prime sieve score.
    let prime_score = simulate_prime_sieve();
    cat.sub_tests.push(SubTestResult::new(
        "Prime Sieve (1M)",
        prime_score,
        "primes/s",
        false,
    ));

    // Matrix multiply score.
    let matrix_score = simulate_matrix_multiply();
    cat.sub_tests.push(SubTestResult::new(
        "Matrix Multiply (256x256)",
        matrix_score,
        "Mflops/s",
        false,
    ));

    // Normalize all scores to a 0-10000 composite.
    // Reference: integer ~5000 Mops, fp ~3000 Mflops, prime ~80000 primes/s,
    //            matrix ~2000 Mflops. We normalize each to 2500 at reference.
    let normalized_int = (integer_score / 5000.0) * 2500.0;
    let normalized_fp = (fp_score / 3000.0) * 2500.0;
    let normalized_prime = (prime_score / 80000.0) * 2500.0;
    let normalized_matrix = (matrix_score / 2000.0) * 2500.0;
    cat.composite_score = normalized_int + normalized_fp + normalized_prime + normalized_matrix;
    cat.composite_score = cat.composite_score.max(0.0);

    cat
}

/// Simulated integer arithmetic benchmark.
fn simulate_integer_benchmark() -> f64 {
    // Perform actual integer work so this isn't trivially optimized away.
    let mut accumulator: u64 = 0;
    let iterations: u64 = 500_000;
    let mut i: u64 = 0;
    while i < iterations {
        accumulator = accumulator.wrapping_add(i.wrapping_mul(17));
        accumulator ^= accumulator >> 3;
        i = i.wrapping_add(1);
    }
    // Use the accumulator to prevent dead-code elimination.
    // Score is ops/s, scaled for display. Simulated reference ~5000 Mops/s.
    let base_score = 5200.0;
    // Tiny perturbation based on accumulator parity to prevent const-folding.
    if accumulator & 1 == 0 {
        base_score + 12.0
    } else {
        base_score - 8.0
    }
}

/// Simulated floating-point benchmark.
fn simulate_float_benchmark() -> f64 {
    let mut sum: f64 = 0.0;
    let iterations = 200_000;
    let mut i = 0u64;
    while i < iterations {
        let x = (i as f64) * 0.001;
        sum += (x * 1.5).sqrt();
        i = i.wrapping_add(1);
    }
    let base_score = 3100.0;
    if sum > 0.0 { base_score + 5.0 } else { base_score }
}

/// Simulated prime sieve benchmark.
fn simulate_prime_sieve() -> f64 {
    let limit: usize = 10_000;
    let mut sieve = vec![true; limit];
    if limit > 0
        && let Some(slot) = sieve.get_mut(0) {
            *slot = false;
        }
    if limit > 1
        && let Some(slot) = sieve.get_mut(1) {
            *slot = false;
        }
    let mut p = 2;
    while p * p < limit {
        if sieve.get(p).copied().unwrap_or(false) {
            let mut multiple = p * p;
            while multiple < limit {
                if let Some(slot) = sieve.get_mut(multiple) {
                    *slot = false;
                }
                multiple = multiple.saturating_add(p);
            }
        }
        p = p.saturating_add(1);
    }
    let prime_count = sieve.iter().filter(|&&is_prime| is_prime).count();
    // Score based on prime count found (real bench would be timed).
    // Reference: ~78500 primes/s at limit=1M.
    let base = 78500.0;
    // Use prime_count to avoid dead-code elimination.
    base + (prime_count as f64 * 0.01)
}

/// Simulated matrix multiply benchmark.
fn simulate_matrix_multiply() -> f64 {
    // Small matrix multiply to exercise FP pipeline.
    let n = 32;
    let mut a = vec![0.0f64; n * n];
    let mut b = vec![0.0f64; n * n];
    let mut c = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            if let Some(slot) = a.get_mut(i * n + j) {
                *slot = (i as f64) * 0.1 + (j as f64) * 0.01;
            }
            if let Some(slot) = b.get_mut(i * n + j) {
                *slot = (j as f64) * 0.1 + (i as f64) * 0.01;
            }
        }
    }
    for i in 0..n {
        for j in 0..n {
            let mut sum = 0.0f64;
            for k in 0..n {
                let a_val = a.get(i * n + k).copied().unwrap_or(0.0);
                let b_val = b.get(k * n + j).copied().unwrap_or(0.0);
                sum += a_val * b_val;
            }
            if let Some(slot) = c.get_mut(i * n + j) {
                *slot = sum;
            }
        }
    }
    let trace: f64 = (0..n).filter_map(|i| c.get(i * n + i).copied()).sum();
    let base = 2050.0;
    if trace > 0.0 { base + 15.0 } else { base }
}

/// Run memory benchmarks.
pub fn run_memory_benchmark() -> CategoryResult {
    let mut cat = CategoryResult::new("Memory");

    let seq_write = simulate_seq_write_throughput();
    cat.sub_tests.push(SubTestResult::new(
        "Sequential Write",
        seq_write,
        "MB/s",
        false,
    ));

    let seq_read = simulate_seq_read_throughput();
    cat.sub_tests.push(SubTestResult::new(
        "Sequential Read",
        seq_read,
        "MB/s",
        false,
    ));

    let random_latency = simulate_random_access_latency();
    cat.sub_tests.push(SubTestResult::new(
        "Random Access Latency",
        random_latency,
        "ns",
        true,
    ));

    let bandwidth = simulate_memory_bandwidth();
    cat.sub_tests.push(SubTestResult::new(
        "Memory Bandwidth",
        bandwidth,
        "GB/s",
        false,
    ));

    // Normalize: seq_write ref ~12000 MB/s, seq_read ref ~14000 MB/s,
    // random_latency ref ~80 ns (lower=better, invert), bandwidth ref ~25 GB/s.
    let norm_sw = (seq_write / 12000.0) * 2500.0;
    let norm_sr = (seq_read / 14000.0) * 2500.0;
    // For latency, lower is better: score = ref/actual * 2500.
    let norm_lat = if random_latency > 0.0 {
        (80.0 / random_latency) * 2500.0
    } else {
        2500.0
    };
    let norm_bw = (bandwidth / 25.0) * 2500.0;
    cat.composite_score = (norm_sw + norm_sr + norm_lat + norm_bw).max(0.0);

    cat
}

fn simulate_seq_write_throughput() -> f64 {
    // Simulate sequential write by filling a buffer.
    let size = 64 * 1024; // 64 KiB
    let mut buf = vec![0u8; size];
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = (i & 0xFF) as u8;
    }
    let checksum: u64 = buf.iter().map(|&b| b as u64).sum();
    let base = 11800.0;
    if checksum > 0 { base + 50.0 } else { base }
}

fn simulate_seq_read_throughput() -> f64 {
    let size = 64 * 1024;
    let buf: Vec<u8> = (0..size).map(|i| (i & 0xFF) as u8).collect();
    let checksum: u64 = buf.iter().map(|&b| b as u64).sum();
    let base = 14200.0;
    if checksum > 0 { base + 30.0 } else { base }
}

fn simulate_random_access_latency() -> f64 {
    // Lower is better. Simulate pointer-chasing.
    let size = 4096;
    let data: Vec<u32> = (0..size).map(|i| ((i * 7 + 13) % size) as u32).collect();
    let mut idx: u32 = 0;
    for _ in 0..1000 {
        idx = data.get(idx as usize % size).copied().unwrap_or(0);
    }
    let base = 78.0;
    if idx > 0 { base + 1.5 } else { base }
}

fn simulate_memory_bandwidth() -> f64 {
    let size = 32 * 1024;
    let src: Vec<u64> = (0..size).map(|i| i as u64).collect();
    let mut dst = vec![0u64; size];
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = *s;
    }
    let checksum: u64 = dst.iter().sum();
    let base = 25.5;
    if checksum > 0 { base + 0.3 } else { base }
}

/// Run disk benchmarks.
pub fn run_disk_benchmark() -> CategoryResult {
    let mut cat = CategoryResult::new("Disk");

    let seq_write = simulate_disk_seq_write();
    cat.sub_tests.push(SubTestResult::new(
        "Sequential Write",
        seq_write,
        "MB/s",
        false,
    ));

    let seq_read = simulate_disk_seq_read();
    cat.sub_tests.push(SubTestResult::new(
        "Sequential Read",
        seq_read,
        "MB/s",
        false,
    ));

    let rand_4k_read = simulate_disk_random_4k_read();
    cat.sub_tests.push(SubTestResult::new(
        "Random 4K Read",
        rand_4k_read,
        "MB/s",
        false,
    ));

    let rand_4k_write = simulate_disk_random_4k_write();
    cat.sub_tests.push(SubTestResult::new(
        "Random 4K Write",
        rand_4k_write,
        "MB/s",
        false,
    ));

    let iops = simulate_disk_iops();
    cat.sub_tests.push(SubTestResult::new(
        "IOPS (4K Random)",
        iops,
        "IOPS",
        false,
    ));

    // Normalize: seq_write ref ~3000 MB/s, seq_read ref ~3500 MB/s,
    // rand_4k_read ref ~50 MB/s, rand_4k_write ref ~45 MB/s, iops ref ~500K.
    let norm_sw = (seq_write / 3000.0) * 2000.0;
    let norm_sr = (seq_read / 3500.0) * 2000.0;
    let norm_4kr = (rand_4k_read / 50.0) * 2000.0;
    let norm_4kw = (rand_4k_write / 45.0) * 2000.0;
    let norm_iops = (iops / 500000.0) * 2000.0;
    cat.composite_score = (norm_sw + norm_sr + norm_4kr + norm_4kw + norm_iops).max(0.0);

    cat
}

fn simulate_disk_seq_write() -> f64 {
    let base = 3100.0;
    let work: u64 = (0..1000u64).sum();
    if work > 0 { base + 20.0 } else { base }
}

fn simulate_disk_seq_read() -> f64 {
    let base = 3500.0;
    let work: u64 = (0..1000u64).sum();
    if work > 0 { base + 30.0 } else { base }
}

fn simulate_disk_random_4k_read() -> f64 {
    let base = 52.0;
    let work: u64 = (0..500u64).map(|x| x.wrapping_mul(3)).sum();
    if work > 0 { base + 1.0 } else { base }
}

fn simulate_disk_random_4k_write() -> f64 {
    let base = 46.0;
    let work: u64 = (0..500u64).map(|x| x.wrapping_mul(5)).sum();
    if work > 0 { base + 0.8 } else { base }
}

fn simulate_disk_iops() -> f64 {
    let base = 520000.0;
    let work: u64 = (0..2000u64).sum();
    if work > 0 { base + 5000.0 } else { base }
}

/// Run graphics benchmarks.
pub fn run_graphics_benchmark() -> CategoryResult {
    let mut cat = CategoryResult::new("Graphics");

    let fill_rate = simulate_fill_rate();
    cat.sub_tests.push(SubTestResult::new(
        "Fill Rate",
        fill_rate,
        "Mpix/s",
        false,
    ));

    let text_render = simulate_text_rendering();
    cat.sub_tests.push(SubTestResult::new(
        "Text Rendering",
        text_render,
        "glyphs/s",
        false,
    ));

    let composite = simulate_composite_ops();
    cat.sub_tests.push(SubTestResult::new(
        "Composite Operations",
        composite,
        "ops/s",
        false,
    ));

    // Normalize: fill ref ~2000 Mpix/s, text ref ~500K glyphs/s,
    // composite ref ~100K ops/s.
    let norm_fill = (fill_rate / 2000.0) * 3333.0;
    let norm_text = (text_render / 500000.0) * 3333.0;
    let norm_comp = (composite / 100000.0) * 3334.0;
    cat.composite_score = (norm_fill + norm_text + norm_comp).max(0.0);

    cat
}

fn simulate_fill_rate() -> f64 {
    // Simulate pixel fill work.
    let pixels = 1920 * 1080;
    let mut buf = vec![0u32; pixels];
    for (i, pixel) in buf.iter_mut().enumerate() {
        *pixel = (i as u32) | 0xFF00_0000;
    }
    let sample = buf.get(pixels / 2).copied().unwrap_or(0);
    let base = 2100.0;
    if sample > 0 { base + 30.0 } else { base }
}

fn simulate_text_rendering() -> f64 {
    // Simulate glyph rasterization work.
    let glyph_count = 10_000;
    let mut total_area: u64 = 0;
    for i in 0..glyph_count {
        let w: u64 = 8 + (i % 12);
        let h: u64 = 12 + (i % 8);
        total_area = total_area.wrapping_add(w * h);
    }
    let base = 520000.0;
    if total_area > 0 { base + 5000.0 } else { base }
}

fn simulate_composite_ops() -> f64 {
    // Simulate alpha-composite blending.
    let ops = 5000;
    let mut result: u32 = 0;
    for i in 0u32..ops {
        let src_a = (i * 7) & 0xFF;
        let dst = (i * 13) & 0xFF;
        // Simple alpha blend: src_a * src + (255 - src_a) * dst / 255.
        let blended = (src_a.wrapping_mul(i & 0xFF))
            .wrapping_add((255u32.wrapping_sub(src_a)).wrapping_mul(dst))
            / 255;
        result = result.wrapping_add(blended);
    }
    let base = 105000.0;
    if result > 0 { base + 2000.0 } else { base }
}

/// Run all benchmarks and produce a complete result.
pub fn run_all_benchmarks(hardware: &HardwareInfo) -> BenchmarkResult {
    let cpu = run_cpu_benchmark();
    let memory = run_memory_benchmark();
    let disk = run_disk_benchmark();
    let graphics = run_graphics_benchmark();

    let mut result = BenchmarkResult {
        cpu,
        memory,
        disk,
        graphics,
        overall_score: 0.0,
        timestamp: 1747573200, // Placeholder; real impl uses system clock.
        hardware: hardware.clone(),
    };
    result.compute_overall();
    result
}

// ============================================================================
// Main Application State
// ============================================================================

/// The benchmark application UI state.
pub struct BenchmarkApp {
    /// Current tab.
    pub active_tab: Tab,
    /// Window dimensions.
    pub width: f32,
    pub height: f32,
    /// Hardware info.
    pub hardware: HardwareInfo,
    /// Progress tracker.
    pub progress: ProgressTracker,
    /// Benchmark history.
    pub history: BenchmarkHistory,
    /// Current comparison (if previous result exists).
    pub comparison: Option<ComparisonResult>,
    /// Scroll offset in the content area.
    pub scroll_y: f32,
    /// Hover state for the "Run" button.
    pub run_button_hover: bool,
    /// Hover state for the "Export" button.
    pub export_button_hover: bool,
    /// Hover state for the "Clear History" button.
    pub clear_button_hover: bool,
    /// Selected history index for detail view.
    pub selected_history_idx: Option<usize>,
    /// Tick counter for animation.
    pub tick_counter: u64,
}

impl BenchmarkApp {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Overview,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            hardware: HardwareInfo::default(),
            progress: ProgressTracker::new(),
            history: BenchmarkHistory::new(),
            comparison: None,
            scroll_y: 0.0,
            run_button_hover: false,
            export_button_hover: false,
            clear_button_hover: false,
            selected_history_idx: None,
            tick_counter: 0,
        }
    }

    /// Run the full benchmark suite (synchronous/simulated).
    pub fn run_benchmark(&mut self) {
        // Start progress tracking.
        self.progress.reset();
        self.progress.phase = BenchPhase::RunningCpu;
        self.progress.set_progress(0.0, "Starting CPU tests...");

        // In a real async implementation, each phase would be run in a
        // background task with progress updates. Here we run synchronously.
        let result = run_all_benchmarks(&self.hardware);

        // Mark complete.
        self.progress.phase = BenchPhase::Complete;
        self.progress.phase_progress = 1.0;
        self.progress.completed_phases = 4;

        // Compute comparison with previous run.
        if let Some(prev) = self.history.latest() {
            self.comparison = Some(result.comparison_vs(prev));
        } else {
            self.comparison = None;
        }

        // Store result.
        self.history.push(result);

        // Switch to overview tab to show results.
        self.active_tab = Tab::Overview;
    }

    /// Handle a UI event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => {
                let x = mouse_event.x;
                let y = mouse_event.y;
                match mouse_event.kind {
                    MouseEventKind::Press(MouseButton::Left) => self.handle_click(x, y),
                    MouseEventKind::Move => self.handle_mouse_move(x, y),
                    MouseEventKind::Scroll { dy, .. } => {
                        self.scroll_y = (self.scroll_y - dy * 20.0).max(0.0);
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::Tick { elapsed_ms } => {
                self.tick_counter = self.tick_counter.wrapping_add(*elapsed_ms);
                if self.progress.phase.is_running() {
                    self.progress.elapsed_ms = self.progress.elapsed_ms.saturating_add(*elapsed_ms);
                }
                EventResult::Consumed
            }
            Event::Resize { width, height } => {
                self.width = *width as f32;
                self.height = *height as f32;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::F5 => {
                if !self.progress.phase.is_running() {
                    self.run_benchmark();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Key::Tab if !key.modifiers.shift => {
                self.cycle_tab_forward();
                EventResult::Consumed
            }
            Key::Tab if key.modifiers.shift => {
                self.cycle_tab_backward();
                EventResult::Consumed
            }
            Key::E if key.modifiers.ctrl => {
                if !self.history.is_empty() {
                    let _report = self.export_report();
                }
                EventResult::Consumed
            }
            Key::Num1 => { self.active_tab = Tab::Overview; EventResult::Consumed }
            Key::Num2 => { self.active_tab = Tab::Cpu; EventResult::Consumed }
            Key::Num3 => { self.active_tab = Tab::Memory; EventResult::Consumed }
            Key::Num4 => { self.active_tab = Tab::Disk; EventResult::Consumed }
            Key::Num5 => { self.active_tab = Tab::Graphics; EventResult::Consumed }
            Key::Num6 => { self.active_tab = Tab::History; EventResult::Consumed }
            Key::Escape => {
                self.selected_history_idx = None;
                EventResult::Consumed
            }
            Key::Home => {
                self.scroll_y = 0.0;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn cycle_tab_forward(&mut self) {
        let tabs = Tab::all();
        let current_idx = tabs.iter().position(|&t| t == self.active_tab).unwrap_or(0);
        let next_idx = (current_idx + 1) % tabs.len();
        self.active_tab = tabs.get(next_idx).copied().unwrap_or(Tab::Overview);
    }

    fn cycle_tab_backward(&mut self) {
        let tabs = Tab::all();
        let current_idx = tabs.iter().position(|&t| t == self.active_tab).unwrap_or(0);
        let prev_idx = if current_idx == 0 { tabs.len() - 1 } else { current_idx - 1 };
        self.active_tab = tabs.get(prev_idx).copied().unwrap_or(Tab::Overview);
    }

    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        // Tab bar clicks.
        if (TITLE_BAR_HEIGHT..=TITLE_BAR_HEIGHT + TAB_BAR_HEIGHT).contains(&y) {
            let tab_width = self.width / Tab::all().len() as f32;
            let idx = (x / tab_width) as usize;
            if let Some(&tab) = Tab::all().get(idx) {
                self.active_tab = tab;
                self.scroll_y = 0.0;
                return EventResult::Consumed;
            }
        }

        // Run button (bottom-right area).
        let btn_x = self.width - BUTTON_WIDTH - 20.0;
        let btn_y = self.height - STATUS_BAR_HEIGHT - BUTTON_HEIGHT - 10.0;
        if x >= btn_x && x <= btn_x + BUTTON_WIDTH && y >= btn_y && y <= btn_y + BUTTON_HEIGHT
            && !self.progress.phase.is_running() {
                self.run_benchmark();
                return EventResult::Consumed;
            }

        // Export button.
        let export_x = btn_x - BUTTON_WIDTH - 10.0;
        if x >= export_x
            && x <= export_x + BUTTON_WIDTH
            && y >= btn_y
            && y <= btn_y + BUTTON_HEIGHT
        {
            if !self.history.is_empty() {
                let _report = self.export_report();
            }
            return EventResult::Consumed;
        }

        // Clear history button (only on History tab).
        if self.active_tab == Tab::History {
            let clear_x = export_x - BUTTON_WIDTH - 10.0;
            if x >= clear_x
                && x <= clear_x + BUTTON_WIDTH
                && y >= btn_y
                && y <= btn_y + BUTTON_HEIGHT
            {
                self.history.clear();
                self.comparison = None;
                self.selected_history_idx = None;
                return EventResult::Consumed;
            }
        }

        // History row click (on History tab).
        if self.active_tab == Tab::History {
            let content_y = TITLE_BAR_HEIGHT + TAB_BAR_HEIGHT + CONTENT_PADDING;
            let header_y = content_y + 30.0; // Skip title.
            if x >= CONTENT_PADDING
                && x <= self.width - CONTENT_PADDING
                && y >= header_y
            {
                let row_idx = ((y - header_y + self.scroll_y) / ROW_HEIGHT) as usize;
                if row_idx < self.history.len() {
                    self.selected_history_idx = Some(row_idx);
                    return EventResult::Consumed;
                }
            }
        }

        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        let btn_x = self.width - BUTTON_WIDTH - 20.0;
        let btn_y = self.height - STATUS_BAR_HEIGHT - BUTTON_HEIGHT - 10.0;
        self.run_button_hover = x >= btn_x
            && x <= btn_x + BUTTON_WIDTH
            && y >= btn_y
            && y <= btn_y + BUTTON_HEIGHT;

        let export_x = btn_x - BUTTON_WIDTH - 10.0;
        self.export_button_hover = x >= export_x
            && x <= export_x + BUTTON_WIDTH
            && y >= btn_y
            && y <= btn_y + BUTTON_HEIGHT;

        if self.active_tab == Tab::History {
            let clear_x = export_x - BUTTON_WIDTH - 10.0;
            self.clear_button_hover = x >= clear_x
                && x <= clear_x + BUTTON_WIDTH
                && y >= btn_y
                && y <= btn_y + BUTTON_HEIGHT;
        } else {
            self.clear_button_hover = false;
        }

        EventResult::Ignored
    }

    /// Export a full report of the latest benchmark run.
    pub fn export_report(&self) -> String {
        if let Some(latest) = self.history.latest() {
            let mut report = latest.to_text_report();
            if let Some(ref comp) = self.comparison {
                report.push_str("\n--- Comparison vs Previous Run ---\n");
                for (label, pct) in comp.as_pairs() {
                    let arrow = if pct >= 0.0 { "+" } else { "" };
                    report.push_str(&format!("  {:<12} {}{:.1}%\n", label, arrow, pct));
                }
            }
            report
        } else {
            "No benchmark results to export.\n".into()
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Full window background.
        tree.fill_rect(0.0, 0.0, self.width, self.height, CRUST);

        self.render_title_bar(&mut tree);
        self.render_tab_bar(&mut tree);
        self.render_content(&mut tree);
        self.render_status_bar(&mut tree);
        self.render_buttons(&mut tree);

        tree
    }

    fn render_title_bar(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: TITLE_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        tree.push(RenderCommand::Text {
            x: 16.0,
            y: 12.0,
            text: "Slate OS System Benchmark".into(),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        // Hardware summary on title bar right side.
        let hw_summary = format!(
            "{} | {} MB RAM",
            self.hardware.cpu_model, self.hardware.ram_total_mb
        );
        tree.push(RenderCommand::Text {
            x: self.width - 400.0,
            y: 14.0,
            text: hw_summary,
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(380.0),
        });
    }

    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let y = TITLE_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: TAB_BAR_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let tabs = Tab::all();
        let tab_width = self.width / tabs.len() as f32;
        for (i, tab) in tabs.iter().enumerate() {
            let tx = i as f32 * tab_width;
            let is_active = *tab == self.active_tab;

            if is_active {
                tree.push(RenderCommand::FillRect {
                    x: tx,
                    y,
                    width: tab_width,
                    height: TAB_BAR_HEIGHT,
                    color: SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
                // Active indicator line.
                tree.push(RenderCommand::FillRect {
                    x: tx,
                    y: y + TAB_BAR_HEIGHT - 2.0,
                    width: tab_width,
                    height: 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            tree.push(RenderCommand::Text {
                x: tx + tab_width / 2.0 - 20.0,
                y: y + 10.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if is_active { TEXT_COLOR } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 8.0),
            });
        }

        // Separator line below tabs.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT,
            x2: self.width,
            y2: y + TAB_BAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_content(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + TAB_BAR_HEIGHT;
        let content_h =
            self.height - content_y - STATUS_BAR_HEIGHT - BUTTON_HEIGHT - 20.0;

        // Clip content area.
        tree.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: self.width,
            height: content_h,
        });

        match self.active_tab {
            Tab::Overview => self.render_overview(tree, content_y, content_h),
            Tab::Cpu => self.render_category_detail(tree, content_y, &self.history.latest().map(|r| &r.cpu), "CPU"),
            Tab::Memory => self.render_category_detail(tree, content_y, &self.history.latest().map(|r| &r.memory), "Memory"),
            Tab::Disk => self.render_category_detail(tree, content_y, &self.history.latest().map(|r| &r.disk), "Disk"),
            Tab::Graphics => self.render_category_detail(tree, content_y, &self.history.latest().map(|r| &r.graphics), "Graphics"),
            Tab::History => self.render_history_tab(tree, content_y, content_h),
        }

        tree.push(RenderCommand::PopClip);
    }

    fn render_overview(&self, tree: &mut RenderTree, base_y: f32, _content_h: f32) {
        let x = CONTENT_PADDING;
        let mut y = base_y + CONTENT_PADDING - self.scroll_y;

        // Progress bar (if running or just completed).
        if self.progress.phase.is_running() || self.progress.phase.is_complete() {
            self.render_progress_bar(tree, x, y, self.width - 2.0 * CONTENT_PADDING);
            y += PROGRESS_BAR_HEIGHT + 20.0;
        }

        if let Some(result) = self.history.latest() {
            // Overall score display.
            tree.push(RenderCommand::Text {
                x,
                y,
                text: "Overall Score".into(),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            y += 20.0;

            tree.push(RenderCommand::Text {
                x,
                y,
                text: format!("{:.0}", result.overall_score),
                font_size: 28.0,
                color: score_color(result.overall_score),
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Comparison delta.
            if let Some(ref comp) = self.comparison {
                let delta_text = format_delta(comp.overall_change_pct);
                let delta_color = delta_color(comp.overall_change_pct);
                tree.push(RenderCommand::Text {
                    x: x + 160.0,
                    y: y + 6.0,
                    text: delta_text,
                    font_size: 14.0,
                    color: delta_color,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            y += 40.0;

            // Category score cards.
            let card_width = (self.width - 3.0 * CONTENT_PADDING) / 2.0;
            let card_height = 90.0;
            let categories: [(& str, f64, Color); 4] = [
                ("CPU", result.cpu.composite_score, BLUE),
                ("Memory", result.memory.composite_score, GREEN),
                ("Disk", result.disk.composite_score, PEACH),
                ("Graphics", result.graphics.composite_score, LAVENDER),
            ];

            for (i, (name, score, color)) in categories.iter().enumerate() {
                let col = i % 2;
                let row = i / 2;
                let cx = x + col as f32 * (card_width + CONTENT_PADDING);
                let cy = y + row as f32 * (card_height + 10.0);

                // Card background.
                tree.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: card_width,
                    height: card_height,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Category name.
                tree.push(RenderCommand::Text {
                    x: cx + 12.0,
                    y: cy + 10.0,
                    text: (*name).into(),
                    font_size: 12.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Score.
                tree.push(RenderCommand::Text {
                    x: cx + 12.0,
                    y: cy + 30.0,
                    text: format!("{:.0}", score),
                    font_size: 22.0,
                    color: *color,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Bar chart showing score relative to 10000.
                let bar_max_w = card_width - 24.0;
                let bar_frac = (*score / 10000.0).min(1.0) as f32;
                let bar_y = cy + 62.0;

                tree.push(RenderCommand::FillRect {
                    x: cx + 12.0,
                    y: bar_y,
                    width: bar_max_w,
                    height: BAR_CHART_HEIGHT,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(3.0),
                });
                if bar_frac > 0.0 {
                    tree.push(RenderCommand::FillRect {
                        x: cx + 12.0,
                        y: bar_y,
                        width: bar_max_w * bar_frac,
                        height: BAR_CHART_HEIGHT,
                        color: *color,
                        corner_radii: CornerRadii::all(3.0),
                    });
                }

                // Comparison delta for this category.
                if let Some(ref comp) = self.comparison {
                    let pct = match *name {
                        "CPU" => comp.cpu_change_pct,
                        "Memory" => comp.memory_change_pct,
                        "Disk" => comp.disk_change_pct,
                        "Graphics" => comp.graphics_change_pct,
                        _ => 0.0,
                    };
                    let delta_text = format_delta(pct);
                    tree.push(RenderCommand::Text {
                        x: cx + card_width - 80.0,
                        y: cy + 12.0,
                        text: delta_text,
                        font_size: 11.0,
                        color: delta_color(pct),
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(70.0),
                    });
                }
            }

            y += 2.0 * (card_height + 10.0) + 20.0;

            // Hardware info section.
            tree.push(RenderCommand::Text {
                x,
                y,
                text: "Hardware Info".into(),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            y += 24.0;

            for (i, (label, value)) in self.hardware.summary_lines().iter().enumerate() {
                let row_y = y + i as f32 * ROW_HEIGHT;
                let bg_color = if i % 2 == 0 { BASE } else { SURFACE0 };
                tree.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width: self.width - 2.0 * CONTENT_PADDING,
                    height: ROW_HEIGHT,
                    color: bg_color,
                    corner_radii: CornerRadii::ZERO,
                });
                tree.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: row_y + 4.0,
                    text: label.clone(),
                    font_size: 12.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(150.0),
                });
                tree.push(RenderCommand::Text {
                    x: x + 170.0,
                    y: row_y + 4.0,
                    text: value.clone(),
                    font_size: 12.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(self.width - 200.0),
                });
            }
        } else {
            // No results yet.
            tree.push(RenderCommand::Text {
                x: self.width / 2.0 - 120.0,
                y: base_y + 100.0,
                text: "No benchmark results yet".into(),
                font_size: 16.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: self.width / 2.0 - 140.0,
                y: base_y + 130.0,
                text: "Press F5 or click Run to start benchmarking".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_progress_bar(&self, tree: &mut RenderTree, x: f32, y: f32, bar_width: f32) {
        let overall = self.progress.overall_progress();

        // Background track.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: bar_width,
            height: PROGRESS_BAR_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });

        // Filled portion.
        if overall > 0.0 {
            let fill_width = bar_width * overall;
            tree.push(RenderCommand::FillRect {
                x,
                y,
                width: fill_width,
                height: PROGRESS_BAR_HEIGHT,
                color: self.progress.phase.color(),
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Phase label.
        tree.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 3.0,
            text: format!(
                "{} - {:.0}%",
                self.progress.phase.label(),
                overall * 100.0
            ),
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(bar_width - 100.0),
        });

        // Elapsed time.
        tree.push(RenderCommand::Text {
            x: x + bar_width - 60.0,
            y: y + 3.0,
            text: self.progress.elapsed_display(),
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_category_detail(
        &self,
        tree: &mut RenderTree,
        base_y: f32,
        category: &Option<&CategoryResult>,
        title: &str,
    ) {
        let x = CONTENT_PADDING;
        let mut y = base_y + CONTENT_PADDING - self.scroll_y;

        tree.push(RenderCommand::Text {
            x,
            y,
            text: format!("{} Benchmark Results", title),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 30.0;

        if let Some(cat) = category {
            // Composite score.
            tree.push(RenderCommand::Text {
                x,
                y,
                text: format!("Composite Score: {:.0}", cat.composite_score),
                font_size: 14.0,
                color: score_color(cat.composite_score),
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            y += 30.0;

            // Column headers.
            tree.push(RenderCommand::FillRect {
                x,
                y,
                width: self.width - 2.0 * CONTENT_PADDING,
                height: ROW_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            tree.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 4.0,
                text: "Test".into(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });
            tree.push(RenderCommand::Text {
                x: x + 250.0,
                y: y + 4.0,
                text: "Score".into(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: x + 420.0,
                y: y + 4.0,
                text: "Bar".into(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            y += ROW_HEIGHT + 2.0;

            // Sub-test rows with bar charts.
            let max_score = cat
                .sub_tests
                .iter()
                .map(|s| s.score)
                .reduce(f64::max)
                .unwrap_or(1.0)
                .max(1.0);

            for (i, sub) in cat.sub_tests.iter().enumerate() {
                let row_y = y + i as f32 * (ROW_HEIGHT + 4.0);
                let bg_color = if i % 2 == 0 { BASE } else { SURFACE0 };

                tree.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width: self.width - 2.0 * CONTENT_PADDING,
                    height: ROW_HEIGHT,
                    color: bg_color,
                    corner_radii: CornerRadii::ZERO,
                });

                // Test name.
                tree.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: row_y + 4.0,
                    text: sub.name.clone(),
                    font_size: 12.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(230.0),
                });

                // Score value.
                let direction_indicator = if sub.lower_is_better { " *" } else { "" };
                tree.push(RenderCommand::Text {
                    x: x + 250.0,
                    y: row_y + 4.0,
                    text: format!("{}{}", sub.formatted_score(), direction_indicator),
                    font_size: 12.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(160.0),
                });

                // Bar chart.
                let bar_frac = if sub.lower_is_better {
                    // Invert for lower-is-better so shorter bar = higher score.
                    (1.0 - sub.score / max_score).max(0.1)
                } else {
                    sub.score / max_score
                } as f32;
                let bar_x = x + 420.0;
                let bar_w = BAR_CHART_MAX_WIDTH;

                tree.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: row_y + 4.0,
                    width: bar_w,
                    height: BAR_CHART_HEIGHT,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(2.0),
                });
                if bar_frac > 0.0 {
                    let bar_color = category_color(title);
                    tree.push(RenderCommand::FillRect {
                        x: bar_x,
                        y: row_y + 4.0,
                        width: bar_w * bar_frac.min(1.0),
                        height: BAR_CHART_HEIGHT,
                        color: bar_color,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }
            }

            // Lower-is-better footnote.
            let has_lower = cat.sub_tests.iter().any(|s| s.lower_is_better);
            if has_lower {
                let footnote_y =
                    y + cat.sub_tests.len() as f32 * (ROW_HEIGHT + 4.0) + 10.0;
                tree.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: footnote_y,
                    text: "* lower is better".into(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        } else {
            tree.push(RenderCommand::Text {
                x,
                y,
                text: format!("No {} benchmark results yet. Press F5 to run.", title),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_history_tab(&self, tree: &mut RenderTree, base_y: f32, _content_h: f32) {
        let x = CONTENT_PADDING;
        let mut y = base_y + CONTENT_PADDING - self.scroll_y;

        tree.push(RenderCommand::Text {
            x,
            y,
            text: "Benchmark History".into(),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 30.0;

        if self.history.is_empty() {
            tree.push(RenderCommand::Text {
                x,
                y,
                text: "No benchmark runs recorded.".into(),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        // Summary stats.
        if let Some(best) = self.history.best_overall() {
            tree.push(RenderCommand::Text {
                x,
                y,
                text: format!("Best: {:.0}  |  ", best),
                font_size: 12.0,
                color: GREEN,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        if let Some(avg) = self.history.average_overall() {
            tree.push(RenderCommand::Text {
                x: x + 120.0,
                y,
                text: format!("Avg: {:.0}  |  ", avg),
                font_size: 12.0,
                color: BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        if let Some(worst) = self.history.worst_overall() {
            tree.push(RenderCommand::Text {
                x: x + 240.0,
                y,
                text: format!("Worst: {:.0}", worst),
                font_size: 12.0,
                color: RED,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        y += 24.0;

        // Column headers.
        let row_width = self.width - 2.0 * CONTENT_PADDING;
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: row_width,
            height: ROW_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        let headers = ["#", "Overall", "CPU", "Memory", "Disk", "Graphics", "Time"];
        let col_positions = [8.0, 40.0, 130.0, 220.0, 320.0, 410.0, 520.0];
        for (i, header) in headers.iter().enumerate() {
            let col_x = col_positions.get(i).copied().unwrap_or(0.0);
            tree.push(RenderCommand::Text {
                x: x + col_x,
                y: y + 4.0,
                text: (*header).into(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        y += ROW_HEIGHT + 2.0;

        // History rows.
        for (i, run) in self.history.iter().enumerate() {
            let row_y = y + i as f32 * ROW_HEIGHT;
            let is_selected = self.selected_history_idx == Some(i);
            let bg_color = if is_selected {
                SURFACE1
            } else if i % 2 == 0 {
                BASE
            } else {
                SURFACE0
            };

            tree.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: row_width,
                height: ROW_HEIGHT,
                color: bg_color,
                corner_radii: CornerRadii::ZERO,
            });

            let run_num = format!("{}", i.saturating_add(1));
            let values = [
                run_num,
                format!("{:.0}", run.overall_score),
                format!("{:.0}", run.cpu.composite_score),
                format!("{:.0}", run.memory.composite_score),
                format!("{:.0}", run.disk.composite_score),
                format!("{:.0}", run.graphics.composite_score),
                format!("{}", run.timestamp),
            ];

            for (j, val) in values.iter().enumerate() {
                let col_x = col_positions.get(j).copied().unwrap_or(0.0);
                let col_color = match j {
                    1 => score_color(run.overall_score),
                    2 => BLUE,
                    3 => GREEN,
                    4 => PEACH,
                    5 => LAVENDER,
                    _ => TEXT_COLOR,
                };
                tree.push(RenderCommand::Text {
                    x: x + col_x,
                    y: row_y + 4.0,
                    text: val.clone(),
                    font_size: 11.0,
                    color: col_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // Score trend mini-chart if multiple runs.
        if self.history.len() >= 2 {
            let chart_y = y + self.history.len() as f32 * ROW_HEIGHT + 20.0;
            self.render_trend_chart(tree, x, chart_y, row_width, 100.0);
        }
    }

    fn render_trend_chart(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        chart_width: f32,
        chart_height: f32,
    ) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text: "Score Trend".into(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        let chart_y = y + 20.0;

        // Chart background.
        tree.push(RenderCommand::FillRect {
            x,
            y: chart_y,
            width: chart_width,
            height: chart_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        if self.history.len() < 2 {
            return;
        }

        // Find score range for normalization.
        let scores: Vec<f64> = self.history.iter().map(|r| r.overall_score).collect();
        let min_score = scores.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let max_score = scores.iter().copied().reduce(f64::max).unwrap_or(1.0);
        let range = (max_score - min_score).max(1.0);

        let n = scores.len();
        let step_x = if n > 1 {
            (chart_width - 20.0) / (n - 1) as f32
        } else {
            chart_width
        };
        let padding_top = 10.0;
        let padding_bottom = 10.0;
        let usable_height = chart_height - padding_top - padding_bottom;

        // Draw line segments between data points.
        for i in 1..n {
            let prev_score = scores.get(i - 1).copied().unwrap_or(0.0);
            let curr_score = scores.get(i).copied().unwrap_or(0.0);
            let prev_frac = ((prev_score - min_score) / range) as f32;
            let curr_frac = ((curr_score - min_score) / range) as f32;

            let x1 = x + 10.0 + (i - 1) as f32 * step_x;
            let y1 = chart_y + chart_height - padding_bottom - prev_frac * usable_height;
            let x2 = x + 10.0 + i as f32 * step_x;
            let y2 = chart_y + chart_height - padding_bottom - curr_frac * usable_height;

            tree.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: BLUE,
                width: 2.0,
            });
        }

        // Draw data point dots.
        for (i, &score) in scores.iter().enumerate() {
            let frac = ((score - min_score) / range) as f32;
            let px = x + 10.0 + i as f32 * step_x;
            let py = chart_y + chart_height - padding_bottom - frac * usable_height;

            tree.push(RenderCommand::FillRect {
                x: px - 3.0,
                y: py - 3.0,
                width: 6.0,
                height: 6.0,
                color: LAVENDER,
                corner_radii: CornerRadii::all(3.0),
            });
        }
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.height - STATUS_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: STATUS_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Status text.
        let status = if self.progress.phase.is_running() {
            format!(
                "Running: {} ({:.0}%)",
                self.progress.phase.label(),
                self.progress.overall_progress() * 100.0
            )
        } else if self.progress.phase.is_complete() {
            "Benchmark complete".into()
        } else {
            "Ready - Press F5 to run benchmark".into()
        };

        tree.push(RenderCommand::Text {
            x: 12.0,
            y: y + 7.0,
            text: status,
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 200.0),
        });

        // Run count.
        tree.push(RenderCommand::Text {
            x: self.width - 160.0,
            y: y + 7.0,
            text: format!("Runs: {}/{}", self.history.len(), MAX_HISTORY),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_buttons(&self, tree: &mut RenderTree) {
        let btn_y = self.height - STATUS_BAR_HEIGHT - BUTTON_HEIGHT - 10.0;

        // Run button.
        let run_x = self.width - BUTTON_WIDTH - 20.0;
        let run_color = if self.progress.phase.is_running() {
            SURFACE1
        } else if self.run_button_hover {
            BLUE
        } else {
            SURFACE0
        };
        tree.push(RenderCommand::FillRect {
            x: run_x,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: run_color,
            corner_radii: CornerRadii::all(6.0),
        });
        if !self.progress.phase.is_running() {
            tree.push(RenderCommand::StrokeRect {
                x: run_x,
                y: btn_y,
                width: BUTTON_WIDTH,
                height: BUTTON_HEIGHT,
                color: BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        tree.push(RenderCommand::Text {
            x: run_x + BUTTON_WIDTH / 2.0 - 18.0,
            y: btn_y + 10.0,
            text: if self.progress.phase.is_running() {
                "Running...".into()
            } else {
                "Run (F5)".into()
            },
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Export button.
        let export_x = run_x - BUTTON_WIDTH - 10.0;
        let export_color = if self.export_button_hover {
            GREEN
        } else {
            SURFACE0
        };
        tree.push(RenderCommand::FillRect {
            x: export_x,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: export_color,
            corner_radii: CornerRadii::all(6.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: export_x,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: GREEN,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });
        tree.push(RenderCommand::Text {
            x: export_x + BUTTON_WIDTH / 2.0 - 28.0,
            y: btn_y + 10.0,
            text: "Export (Ctrl+E)".into(),
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Clear history button (only on History tab).
        if self.active_tab == Tab::History {
            let clear_x = export_x - BUTTON_WIDTH - 10.0;
            let clear_color = if self.clear_button_hover {
                RED
            } else {
                SURFACE0
            };
            tree.push(RenderCommand::FillRect {
                x: clear_x,
                y: btn_y,
                width: BUTTON_WIDTH,
                height: BUTTON_HEIGHT,
                color: clear_color,
                corner_radii: CornerRadii::all(6.0),
            });
            tree.push(RenderCommand::StrokeRect {
                x: clear_x,
                y: btn_y,
                width: BUTTON_WIDTH,
                height: BUTTON_HEIGHT,
                color: RED,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
            tree.push(RenderCommand::Text {
                x: clear_x + BUTTON_WIDTH / 2.0 - 35.0,
                y: btn_y + 10.0,
                text: "Clear History".into(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }
}

impl Default for BenchmarkApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Color for a score value (0-10000 scale). Green for high, yellow for mid, red for low.
fn score_color(score: f64) -> Color {
    if score >= 7500.0 {
        GREEN
    } else if score >= 5000.0 {
        BLUE
    } else if score >= 2500.0 {
        YELLOW
    } else {
        RED
    }
}

/// Color for a percentage delta. Green for positive, red for negative.
fn delta_color(pct: f64) -> Color {
    if pct > 0.5 {
        GREEN
    } else if pct < -0.5 {
        RED
    } else {
        SUBTEXT0
    }
}

/// Format a percentage delta with arrow.
fn format_delta(pct: f64) -> String {
    if pct.abs() < 0.1 {
        "~0%".into()
    } else if pct >= 0.0 {
        format!("+{:.1}%", pct)
    } else {
        format!("{:.1}%", pct)
    }
}

/// Color for a benchmark category name.
fn category_color(name: &str) -> Color {
    match name {
        "CPU" => BLUE,
        "Memory" => GREEN,
        "Disk" => PEACH,
        "Graphics" => LAVENDER,
        _ => TEXT_COLOR,
    }
}

// ============================================================================
// Entry point (placeholder for SlateOS)
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- SubTestResult tests ---

    #[test]
    fn sub_test_formatted_score_large() {
        let sub = SubTestResult::new("test", 5200.0, "Mops/s", false);
        let formatted = sub.formatted_score();
        assert!(formatted.contains("5.2K"));
        assert!(formatted.contains("Mops/s"));
    }

    #[test]
    fn sub_test_formatted_score_millions() {
        let sub = SubTestResult::new("test", 1_500_000.0, "ops", false);
        let formatted = sub.formatted_score();
        assert!(formatted.contains("1.50M"));
    }

    #[test]
    fn sub_test_formatted_score_small() {
        let sub = SubTestResult::new("test", 0.5, "ms", true);
        let formatted = sub.formatted_score();
        assert!(formatted.contains("0.500"));
    }

    #[test]
    fn sub_test_formatted_score_medium() {
        let sub = SubTestResult::new("test", 123.4, "MB/s", false);
        let formatted = sub.formatted_score();
        assert!(formatted.contains("123.4"));
    }

    #[test]
    fn sub_test_lower_is_better_flag() {
        let sub = SubTestResult::new("latency", 80.0, "ns", true);
        assert!(sub.lower_is_better);
    }

    #[test]
    fn sub_test_higher_is_better_flag() {
        let sub = SubTestResult::new("throughput", 5000.0, "MB/s", false);
        assert!(!sub.lower_is_better);
    }

    // --- CategoryResult tests ---

    #[test]
    fn category_result_new_empty() {
        let cat = CategoryResult::new("CPU");
        assert_eq!(cat.name, "CPU");
        assert!(cat.sub_tests.is_empty());
        assert_eq!(cat.composite_score, 0.0);
    }

    #[test]
    fn category_result_compute_composite_empty() {
        let mut cat = CategoryResult::new("CPU");
        cat.compute_composite();
        assert_eq!(cat.composite_score, 0.0);
    }

    #[test]
    fn category_result_compute_composite_single() {
        let mut cat = CategoryResult::new("CPU");
        cat.sub_tests
            .push(SubTestResult::new("test", 1000.0, "ops", false));
        cat.compute_composite();
        assert!((cat.composite_score - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn category_result_compute_composite_multiple() {
        let mut cat = CategoryResult::new("Memory");
        cat.sub_tests
            .push(SubTestResult::new("a", 1000.0, "MB/s", false));
        cat.sub_tests
            .push(SubTestResult::new("b", 3000.0, "MB/s", false));
        cat.compute_composite();
        assert!((cat.composite_score - 2000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn category_result_to_text_contains_name() {
        let mut cat = CategoryResult::new("Disk");
        cat.composite_score = 5000.0;
        cat.sub_tests
            .push(SubTestResult::new("SeqRead", 3500.0, "MB/s", false));
        let text = cat.to_text();
        assert!(text.contains("Disk"));
        assert!(text.contains("5000"));
        assert!(text.contains("SeqRead"));
    }

    // --- BenchmarkResult tests ---

    #[test]
    fn benchmark_result_compute_overall() {
        let mut result = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        result.compute_overall();
        let expected = 5000.0 * CPU_WEIGHT
            + 4000.0 * MEMORY_WEIGHT
            + 3000.0 * DISK_WEIGHT
            + 2000.0 * GRAPHICS_WEIGHT;
        assert!((result.overall_score - expected).abs() < 0.01);
    }

    #[test]
    fn benchmark_result_to_text_report_contains_sections() {
        let result = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        let report = result.to_text_report();
        assert!(report.contains("Slate OS System Benchmark Report"));
        assert!(report.contains("Overall Score"));
        assert!(report.contains("CPU"));
        assert!(report.contains("Memory"));
        assert!(report.contains("Disk"));
        assert!(report.contains("Graphics"));
    }

    #[test]
    fn benchmark_result_comparison_no_change() {
        let a = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        let b = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        let comp = b.comparison_vs(&a);
        assert!(comp.cpu_change_pct.abs() < 0.01);
        assert!(comp.memory_change_pct.abs() < 0.01);
        assert!(comp.overall_change_pct.abs() < 0.01);
    }

    #[test]
    fn benchmark_result_comparison_improvement() {
        let a = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        let b = make_test_result(6000.0, 5000.0, 4000.0, 3000.0);
        let comp = b.comparison_vs(&a);
        assert!(comp.cpu_change_pct > 0.0);
        assert!(comp.memory_change_pct > 0.0);
        assert!(comp.overall_change_pct > 0.0);
    }

    #[test]
    fn benchmark_result_comparison_regression() {
        let a = make_test_result(6000.0, 5000.0, 4000.0, 3000.0);
        let b = make_test_result(5000.0, 4000.0, 3000.0, 2000.0);
        let comp = b.comparison_vs(&a);
        assert!(comp.cpu_change_pct < 0.0);
        assert!(comp.memory_change_pct < 0.0);
        assert!(comp.overall_change_pct < 0.0);
    }

    // --- percent_change tests ---

    #[test]
    fn percent_change_zero_to_zero() {
        assert!((percent_change(0.0, 0.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_change_zero_to_nonzero() {
        assert!((percent_change(0.0, 100.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_change_double() {
        assert!((percent_change(100.0, 200.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_change_halved() {
        assert!((percent_change(200.0, 100.0) - (-50.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn percent_change_no_change() {
        assert!((percent_change(500.0, 500.0)).abs() < f64::EPSILON);
    }

    // --- ComparisonResult tests ---

    #[test]
    fn comparison_all_improved_true() {
        let comp = ComparisonResult {
            cpu_change_pct: 5.0,
            memory_change_pct: 3.0,
            disk_change_pct: 1.0,
            graphics_change_pct: 0.0,
            overall_change_pct: 2.0,
        };
        assert!(comp.all_improved());
        assert!(!comp.has_regression());
    }

    #[test]
    fn comparison_has_regression() {
        let comp = ComparisonResult {
            cpu_change_pct: 5.0,
            memory_change_pct: -2.0,
            disk_change_pct: 1.0,
            graphics_change_pct: 0.0,
            overall_change_pct: 1.0,
        };
        assert!(!comp.all_improved());
        assert!(comp.has_regression());
    }

    #[test]
    fn comparison_as_pairs_count() {
        let comp = ComparisonResult {
            cpu_change_pct: 1.0,
            memory_change_pct: 2.0,
            disk_change_pct: 3.0,
            graphics_change_pct: 4.0,
            overall_change_pct: 5.0,
        };
        assert_eq!(comp.as_pairs().len(), 5);
    }

    // --- BenchmarkHistory tests ---

    #[test]
    fn history_new_is_empty() {
        let hist = BenchmarkHistory::new();
        assert!(hist.is_empty());
        assert_eq!(hist.len(), 0);
    }

    #[test]
    fn history_push_one() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_test_result(5000.0, 4000.0, 3000.0, 2000.0));
        assert_eq!(hist.len(), 1);
        assert!(!hist.is_empty());
    }

    #[test]
    fn history_latest() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_test_result(5000.0, 4000.0, 3000.0, 2000.0));
        hist.push(make_test_result(6000.0, 5000.0, 4000.0, 3000.0));
        let latest = hist.latest().unwrap();
        assert!((latest.cpu.composite_score - 6000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_previous() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_test_result(5000.0, 4000.0, 3000.0, 2000.0));
        hist.push(make_test_result(6000.0, 5000.0, 4000.0, 3000.0));
        let prev = hist.previous().unwrap();
        assert!((prev.cpu.composite_score - 5000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_previous_none_with_one_entry() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_test_result(5000.0, 4000.0, 3000.0, 2000.0));
        assert!(hist.previous().is_none());
    }

    #[test]
    fn history_evicts_oldest_at_max() {
        let mut hist = BenchmarkHistory::new();
        for i in 0..MAX_HISTORY + 3 {
            hist.push(make_test_result(
                (i as f64) * 100.0,
                1000.0,
                1000.0,
                1000.0,
            ));
        }
        assert_eq!(hist.len(), MAX_HISTORY);
        // Oldest should be entry #3 (0-indexed).
        let oldest = hist.get(0).unwrap();
        assert!((oldest.cpu.composite_score - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_best_overall() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(3000.0));
        hist.push(make_scored_result(5000.0));
        hist.push(make_scored_result(4000.0));
        assert!((hist.best_overall().unwrap() - 5000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_worst_overall() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(3000.0));
        hist.push(make_scored_result(5000.0));
        hist.push(make_scored_result(4000.0));
        assert!((hist.worst_overall().unwrap() - 3000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_average_overall() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(3000.0));
        hist.push(make_scored_result(5000.0));
        hist.push(make_scored_result(4000.0));
        assert!((hist.average_overall().unwrap() - 4000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_best_overall_empty() {
        let hist = BenchmarkHistory::new();
        assert!(hist.best_overall().is_none());
    }

    #[test]
    fn history_average_overall_empty() {
        let hist = BenchmarkHistory::new();
        assert!(hist.average_overall().is_none());
    }

    #[test]
    fn history_export_empty() {
        let hist = BenchmarkHistory::new();
        let text = hist.export_as_text();
        assert!(text.contains("No benchmark runs recorded"));
    }

    #[test]
    fn history_export_with_data() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(5000.0));
        let text = hist.export_as_text();
        assert!(text.contains("Run #1"));
        assert!(text.contains("5000"));
    }

    #[test]
    fn history_clear() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(5000.0));
        hist.clear();
        assert!(hist.is_empty());
    }

    #[test]
    fn history_iter_order() {
        let mut hist = BenchmarkHistory::new();
        hist.push(make_scored_result(1000.0));
        hist.push(make_scored_result(2000.0));
        hist.push(make_scored_result(3000.0));
        let scores: Vec<f64> = hist.iter().map(|r| r.overall_score).collect();
        assert_eq!(scores.len(), 3);
        assert!((scores[0] - 1000.0).abs() < f64::EPSILON);
        assert!((scores[2] - 3000.0).abs() < f64::EPSILON);
    }

    // --- ProgressTracker tests ---

    #[test]
    fn progress_new_is_idle() {
        let p = ProgressTracker::new();
        assert!(p.phase.is_idle());
        assert!((p.overall_progress()).abs() < f64::EPSILON as f32);
    }

    #[test]
    fn progress_overall_zero_at_start() {
        let p = ProgressTracker::new();
        assert!((p.overall_progress() - 0.0).abs() < 0.001);
    }

    #[test]
    fn progress_advance_phases() {
        let mut p = ProgressTracker::new();
        p.phase = BenchPhase::RunningCpu;
        p.advance_phase();
        assert_eq!(p.phase, BenchPhase::RunningMemory);
        assert_eq!(p.completed_phases, 1);
    }

    #[test]
    fn progress_advance_all_phases() {
        let mut p = ProgressTracker::new();
        p.phase = BenchPhase::RunningCpu;
        p.advance_phase(); // -> Memory
        p.advance_phase(); // -> Disk
        p.advance_phase(); // -> Graphics
        p.advance_phase(); // -> Complete
        assert!(p.phase.is_complete());
        assert_eq!(p.completed_phases, 4);
    }

    #[test]
    fn progress_overall_at_completion() {
        let mut p = ProgressTracker::new();
        p.completed_phases = 4;
        p.phase_progress = 0.0;
        assert!((p.overall_progress() - 1.0).abs() < 0.001);
    }

    #[test]
    fn progress_set_progress_clamps() {
        let mut p = ProgressTracker::new();
        p.set_progress(1.5, "test");
        assert!((p.phase_progress - 1.0).abs() < f32::EPSILON);
        p.set_progress(-0.5, "test");
        assert!(p.phase_progress.abs() < f32::EPSILON);
    }

    #[test]
    fn progress_elapsed_display_format() {
        let mut p = ProgressTracker::new();
        p.elapsed_ms = 65000; // 1 min 5 sec
        assert_eq!(p.elapsed_display(), "01:05");
    }

    #[test]
    fn progress_reset() {
        let mut p = ProgressTracker::new();
        p.phase = BenchPhase::RunningDisk;
        p.completed_phases = 2;
        p.elapsed_ms = 50000;
        p.reset();
        assert!(p.phase.is_idle());
        assert_eq!(p.completed_phases, 0);
        assert_eq!(p.elapsed_ms, 0);
    }

    // --- BenchPhase tests ---

    #[test]
    fn bench_phase_labels() {
        assert_eq!(BenchPhase::Idle.label(), "Ready");
        assert_eq!(BenchPhase::RunningCpu.label(), "CPU Benchmark");
        assert_eq!(BenchPhase::RunningMemory.label(), "Memory Benchmark");
        assert_eq!(BenchPhase::RunningDisk.label(), "Disk Benchmark");
        assert_eq!(BenchPhase::RunningGraphics.label(), "Graphics Benchmark");
        assert_eq!(BenchPhase::Complete.label(), "Complete");
    }

    #[test]
    fn bench_phase_is_running() {
        assert!(!BenchPhase::Idle.is_running());
        assert!(BenchPhase::RunningCpu.is_running());
        assert!(BenchPhase::RunningMemory.is_running());
        assert!(BenchPhase::RunningDisk.is_running());
        assert!(BenchPhase::RunningGraphics.is_running());
        assert!(!BenchPhase::Complete.is_running());
    }

    #[test]
    fn bench_phase_index() {
        assert_eq!(BenchPhase::RunningCpu.phase_index(), Some(0));
        assert_eq!(BenchPhase::RunningMemory.phase_index(), Some(1));
        assert_eq!(BenchPhase::RunningDisk.phase_index(), Some(2));
        assert_eq!(BenchPhase::RunningGraphics.phase_index(), Some(3));
        assert_eq!(BenchPhase::Idle.phase_index(), None);
        assert_eq!(BenchPhase::Complete.phase_index(), None);
    }

    // --- CPU benchmark tests ---

    #[test]
    fn cpu_benchmark_produces_results() {
        let cat = run_cpu_benchmark();
        assert_eq!(cat.name, "CPU");
        assert_eq!(cat.sub_tests.len(), 4);
        assert!(cat.composite_score > 0.0);
    }

    #[test]
    fn cpu_benchmark_sub_test_names() {
        let cat = run_cpu_benchmark();
        let names: Vec<&str> = cat.sub_tests.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Integer Arithmetic"));
        assert!(names.contains(&"Floating Point"));
        assert!(names.contains(&"Prime Sieve (1M)"));
        assert!(names.contains(&"Matrix Multiply (256x256)"));
    }

    #[test]
    fn cpu_benchmark_all_scores_positive() {
        let cat = run_cpu_benchmark();
        for sub in &cat.sub_tests {
            assert!(sub.score > 0.0, "Sub-test {} has non-positive score", sub.name);
        }
    }

    // --- Memory benchmark tests ---

    #[test]
    fn memory_benchmark_produces_results() {
        let cat = run_memory_benchmark();
        assert_eq!(cat.name, "Memory");
        assert_eq!(cat.sub_tests.len(), 4);
        assert!(cat.composite_score > 0.0);
    }

    #[test]
    fn memory_benchmark_has_latency_test() {
        let cat = run_memory_benchmark();
        let latency_test = cat
            .sub_tests
            .iter()
            .find(|s| s.name == "Random Access Latency");
        assert!(latency_test.is_some());
        assert!(latency_test.unwrap().lower_is_better);
    }

    // --- Disk benchmark tests ---

    #[test]
    fn disk_benchmark_produces_results() {
        let cat = run_disk_benchmark();
        assert_eq!(cat.name, "Disk");
        assert_eq!(cat.sub_tests.len(), 5);
        assert!(cat.composite_score > 0.0);
    }

    #[test]
    fn disk_benchmark_has_iops() {
        let cat = run_disk_benchmark();
        let iops_test = cat.sub_tests.iter().find(|s| s.name.contains("IOPS"));
        assert!(iops_test.is_some());
    }

    // --- Graphics benchmark tests ---

    #[test]
    fn graphics_benchmark_produces_results() {
        let cat = run_graphics_benchmark();
        assert_eq!(cat.name, "Graphics");
        assert_eq!(cat.sub_tests.len(), 3);
        assert!(cat.composite_score > 0.0);
    }

    // --- run_all_benchmarks tests ---

    #[test]
    fn run_all_produces_complete_result() {
        let hw = HardwareInfo::default();
        let result = run_all_benchmarks(&hw);
        assert!(result.overall_score > 0.0);
        assert!(!result.cpu.sub_tests.is_empty());
        assert!(!result.memory.sub_tests.is_empty());
        assert!(!result.disk.sub_tests.is_empty());
        assert!(!result.graphics.sub_tests.is_empty());
    }

    #[test]
    fn run_all_overall_is_weighted() {
        let hw = HardwareInfo::default();
        let result = run_all_benchmarks(&hw);
        let expected = result.cpu.composite_score * CPU_WEIGHT
            + result.memory.composite_score * MEMORY_WEIGHT
            + result.disk.composite_score * DISK_WEIGHT
            + result.graphics.composite_score * GRAPHICS_WEIGHT;
        assert!((result.overall_score - expected).abs() < 0.01);
    }

    // --- HardwareInfo tests ---

    #[test]
    fn hardware_info_default_has_fields() {
        let hw = HardwareInfo::default();
        assert!(!hw.cpu_model.is_empty());
        assert!(hw.cpu_cores > 0);
        assert!(hw.ram_total_mb > 0);
        assert!(!hw.disk_model.is_empty());
        assert!(!hw.gpu_model.is_empty());
    }

    #[test]
    fn hardware_info_summary_lines_count() {
        let hw = HardwareInfo::default();
        let lines = hw.summary_lines();
        assert_eq!(lines.len(), 11);
    }

    #[test]
    fn hardware_info_to_text_contains_cpu() {
        let hw = HardwareInfo::default();
        let text = hw.to_text();
        assert!(text.contains("CPU"));
        assert!(text.contains(&hw.cpu_model));
    }

    // --- BenchmarkApp tests ---

    #[test]
    fn app_new_starts_idle() {
        let app = BenchmarkApp::new();
        assert!(app.progress.phase.is_idle());
        assert_eq!(app.active_tab, Tab::Overview);
        assert!(app.history.is_empty());
    }

    #[test]
    fn app_default_same_as_new() {
        let app = BenchmarkApp::default();
        assert!(app.progress.phase.is_idle());
    }

    #[test]
    fn app_run_benchmark_completes() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        assert!(app.progress.phase.is_complete());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn app_run_benchmark_sets_comparison_on_second_run() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        assert!(app.comparison.is_none());
        app.run_benchmark();
        assert!(app.comparison.is_some());
    }

    #[test]
    fn app_run_benchmark_history_grows() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        app.run_benchmark();
        app.run_benchmark();
        assert_eq!(app.history.len(), 3);
    }

    #[test]
    fn app_export_report_no_results() {
        let app = BenchmarkApp::new();
        let report = app.export_report();
        assert!(report.contains("No benchmark results"));
    }

    #[test]
    fn app_export_report_with_results() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        let report = app.export_report();
        assert!(report.contains("Slate OS System Benchmark Report"));
        assert!(report.contains("CPU"));
    }

    #[test]
    fn app_export_report_with_comparison() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        app.run_benchmark();
        let report = app.export_report();
        assert!(report.contains("Comparison"));
    }

    #[test]
    fn app_render_produces_commands() {
        let app = BenchmarkApp::new();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn app_render_after_benchmark_has_more_commands() {
        let idle_app = BenchmarkApp::new();
        let idle_cmds = idle_app.render().len();

        let mut tested_app = BenchmarkApp::new();
        tested_app.run_benchmark();
        let tested_cmds = tested_app.render().len();

        assert!(tested_cmds > idle_cmds);
    }

    #[test]
    fn app_tab_cycling_forward() {
        let mut app = BenchmarkApp::new();
        assert_eq!(app.active_tab, Tab::Overview);
        app.cycle_tab_forward();
        assert_eq!(app.active_tab, Tab::Cpu);
        app.cycle_tab_forward();
        assert_eq!(app.active_tab, Tab::Memory);
    }

    #[test]
    fn app_tab_cycling_backward() {
        let mut app = BenchmarkApp::new();
        assert_eq!(app.active_tab, Tab::Overview);
        app.cycle_tab_backward();
        assert_eq!(app.active_tab, Tab::History);
    }

    #[test]
    fn app_tab_cycling_wraps_forward() {
        let mut app = BenchmarkApp::new();
        for _ in 0..Tab::all().len() {
            app.cycle_tab_forward();
        }
        assert_eq!(app.active_tab, Tab::Overview);
    }

    #[test]
    fn app_render_each_tab() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        for tab in Tab::all() {
            app.active_tab = *tab;
            let tree = app.render();
            assert!(!tree.is_empty(), "Tab {:?} produced no render commands", tab);
        }
    }

    #[test]
    fn app_render_history_tab_empty() {
        let mut app = BenchmarkApp::new();
        app.active_tab = Tab::History;
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn app_render_history_tab_with_data() {
        let mut app = BenchmarkApp::new();
        app.run_benchmark();
        app.run_benchmark();
        app.active_tab = Tab::History;
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    // --- score_color tests ---

    #[test]
    fn score_color_high_is_green() {
        assert_eq!(score_color(8000.0), GREEN);
    }

    #[test]
    fn score_color_mid_is_blue() {
        assert_eq!(score_color(6000.0), BLUE);
    }

    #[test]
    fn score_color_low_mid_is_yellow() {
        assert_eq!(score_color(3000.0), YELLOW);
    }

    #[test]
    fn score_color_very_low_is_red() {
        assert_eq!(score_color(1000.0), RED);
    }

    // --- delta_color tests ---

    #[test]
    fn delta_color_positive_is_green() {
        assert_eq!(delta_color(5.0), GREEN);
    }

    #[test]
    fn delta_color_negative_is_red() {
        assert_eq!(delta_color(-5.0), RED);
    }

    #[test]
    fn delta_color_zero_is_neutral() {
        assert_eq!(delta_color(0.0), SUBTEXT0);
    }

    // --- format_delta tests ---

    #[test]
    fn format_delta_positive() {
        let s = format_delta(5.5);
        assert!(s.starts_with('+'));
        assert!(s.contains("5.5%"));
    }

    #[test]
    fn format_delta_negative() {
        let s = format_delta(-3.2);
        assert!(s.starts_with('-'));
        assert!(s.contains("3.2%"));
    }

    #[test]
    fn format_delta_near_zero() {
        let s = format_delta(0.05);
        assert_eq!(s, "~0%");
    }

    // --- category_color tests ---

    #[test]
    fn category_color_known() {
        assert_eq!(category_color("CPU"), BLUE);
        assert_eq!(category_color("Memory"), GREEN);
        assert_eq!(category_color("Disk"), PEACH);
        assert_eq!(category_color("Graphics"), LAVENDER);
    }

    #[test]
    fn category_color_unknown() {
        assert_eq!(category_color("Unknown"), TEXT_COLOR);
    }

    // --- Tab tests ---

    #[test]
    fn tab_all_count() {
        assert_eq!(Tab::all().len(), 6);
    }

    #[test]
    fn tab_labels() {
        assert_eq!(Tab::Overview.label(), "Overview");
        assert_eq!(Tab::Cpu.label(), "CPU");
        assert_eq!(Tab::Memory.label(), "Memory");
        assert_eq!(Tab::Disk.label(), "Disk");
        assert_eq!(Tab::Graphics.label(), "Graphics");
        assert_eq!(Tab::History.label(), "History");
    }

    // --- Test helpers ---

    fn make_test_result(cpu: f64, mem: f64, disk: f64, gpu: f64) -> BenchmarkResult {
        let mut cpu_cat = CategoryResult::new("CPU");
        cpu_cat.composite_score = cpu;
        let mut mem_cat = CategoryResult::new("Memory");
        mem_cat.composite_score = mem;
        let mut disk_cat = CategoryResult::new("Disk");
        disk_cat.composite_score = disk;
        let mut gpu_cat = CategoryResult::new("Graphics");
        gpu_cat.composite_score = gpu;

        let mut result = BenchmarkResult {
            cpu: cpu_cat,
            memory: mem_cat,
            disk: disk_cat,
            graphics: gpu_cat,
            overall_score: 0.0,
            timestamp: 1747573200,
            hardware: HardwareInfo::default(),
        };
        result.compute_overall();
        result
    }

    fn make_scored_result(overall: f64) -> BenchmarkResult {
        let mut result = make_test_result(overall, overall, overall, overall);
        // Override the computed overall to the exact desired value.
        result.overall_score = overall;
        result
    }
}
