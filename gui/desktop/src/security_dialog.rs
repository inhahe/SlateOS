//! Security Dialog — desktop shell component.
//!
//! A UAC-style security prompt that displays when a process requests a
//! capability escalation. Shows the requesting process, the capability
//! being requested, the reason given, and allows the user to approve
//! or deny the request.
//!
//! Integrates with the kernel's capability request system:
//! - `SYS_CAP_REQUEST` (401) — process submits a capability request
//! - `SYS_CAP_REQUEST_STATUS` (402) — process polls for approval
//! - `SYS_CAP_REQUEST_CANCEL` (403) — process cancels its request
//!
//! The desktop shell registers as the system's capability request handler
//! and uses this dialog to present requests to the user.
//!
//! # Architecture
//!
//! ```text
//! Process → SYS_CAP_REQUEST → kernel CapRequest queue
//!                                     ↓
//!                            Desktop shell polls list_pending()
//!                                     ↓
//!                            SecurityDialog renders prompt
//!                                     ↓
//!                            User clicks Allow/Deny
//!                                     ↓
//!                            Shell calls approve()/deny()
//!                                     ↓
//!                            Process's status query returns result
//! ```
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut dialog = SecurityDialog::new();
//!
//! // When a capability request is detected:
//! dialog.push_request(CapRequestInfo {
//!     id: 42,
//!     pid: 1234,
//!     process_name: "my_app".into(),
//!     resource_type: ResourceType::File,
//!     rights: Rights::READ | Rights::WRITE,
//!     reason: "Needs file access to save document".into(),
//!     created_at_ms: 1234567890,
//! });
//!
//! // Forward events while visible:
//! dialog.handle_key_event(&key_event);
//! dialog.handle_mouse_event(&mouse_event);
//!
//! // Each frame:
//! let commands = dialog.render();
//!
//! // Process decisions:
//! for event in dialog.drain_events() {
//!     match event {
//!         SecurityDialogEvent::Approved(id) => { /* call approve(id) */ }
//!         SecurityDialogEvent::Denied(id) => { /* call deny(id) */ }
//!         SecurityDialogEvent::DeniedAll => { /* deny all pending */ }
//!     }
//! }
//! ```

use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 160);
    pub const DIMMER: Color = Color::rgba(0, 0, 0, 120);
    pub const SHIELD_BG: Color = Color::from_hex(0x313244);
    pub const RISK_LOW: Color = Color::from_hex(0xA6E3A1);
    pub const RISK_MEDIUM: Color = Color::from_hex(0xF9E2AF);
    pub const RISK_HIGH: Color = Color::from_hex(0xFAB387);
    pub const RISK_CRITICAL: Color = Color::from_hex(0xF38BA8);
    pub const BUTTON_BG: Color = Color::from_hex(0x45475A);
    pub const BUTTON_HOVER: Color = Color::from_hex(0x585B70);
    pub const ALLOW_BUTTON: Color = Color::from_hex(0xA6E3A1);
    pub const ALLOW_TEXT: Color = Color::from_hex(0x1E1E2E);
    pub const DENY_BUTTON: Color = Color::from_hex(0xF38BA8);
    pub const DENY_TEXT: Color = Color::from_hex(0x1E1E2E);
    pub const DETAILS_BG: Color = Color::from_hex(0x181825);
    pub const DETAILS_BORDER: Color = Color::from_hex(0x45475A);
}

// ============================================================================
// Constants
// ============================================================================

const DIALOG_WIDTH: f32 = 520.0;
const DIALOG_HEIGHT: f32 = 420.0;
const DIALOG_RADIUS: f32 = 10.0;
const PADDING: f32 = 20.0;
const HEADER_HEIGHT: f32 = 60.0;
const SHIELD_SIZE: f32 = 40.0;
const BUTTON_HEIGHT: f32 = 34.0;
const BUTTON_WIDTH: f32 = 120.0;
const BUTTON_SPACING: f32 = 12.0;
const BUTTON_RADIUS: f32 = 6.0;

const TITLE_FONT_SIZE: f32 = 16.0;
const SUBTITLE_FONT_SIZE: f32 = 13.0;
const BODY_FONT_SIZE: f32 = 12.0;
const DETAIL_FONT_SIZE: f32 = 11.0;
const SMALL_FONT_SIZE: f32 = 10.0;

const DETAIL_ROW_HEIGHT: f32 = 22.0;
const DETAIL_PANEL_RADIUS: f32 = 6.0;

/// Maximum pending requests to show indicator for.
const MAX_QUEUE_DISPLAY: usize = 10;
/// Maximum length of reason text displayed.
const MAX_REASON_DISPLAY: usize = 200;
/// Timeout display threshold (seconds remaining that triggers warning color).
const TIMEOUT_WARN_SECS: u64 = 10;

// ============================================================================
// Resource types (mirrors kernel CapRequest ResourceType)
// ============================================================================

/// Resource type being requested — mirrors the kernel's ResourceType enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Channel,
    Pipe,
    SharedMemory,
    EventFd,
    CompletionPort,
    Process,
    Thread,
    PortIo,
    DeviceIrq,
    File,
    Socket,
    Timer,
    IoScheduler,
    Service,
    Namespace,
}

impl ResourceType {
    /// Human-readable label for the resource type.
    fn label(self) -> &'static str {
        match self {
            Self::Channel => "IPC Channel",
            Self::Pipe => "Pipe",
            Self::SharedMemory => "Shared Memory",
            Self::EventFd => "Event",
            Self::CompletionPort => "Completion Port",
            Self::Process => "Process Control",
            Self::Thread => "Thread Control",
            Self::PortIo => "Hardware I/O Port",
            Self::DeviceIrq => "Device Interrupt",
            Self::File => "File Access",
            Self::Socket => "Network Socket",
            Self::Timer => "System Timer",
            Self::IoScheduler => "I/O Scheduler",
            Self::Service => "System Service",
            Self::Namespace => "Namespace",
        }
    }

    /// Icon character representing this resource type (a letter abbreviation
    /// rendered inside the shield icon).
    fn icon_char(self) -> char {
        match self {
            Self::Channel | Self::Pipe => 'C',
            Self::SharedMemory => 'M',
            Self::EventFd | Self::CompletionPort => 'E',
            Self::Process | Self::Thread => 'P',
            Self::PortIo | Self::DeviceIrq => 'H',
            Self::File => 'F',
            Self::Socket => 'N',
            Self::Timer => 'T',
            Self::IoScheduler => 'I',
            Self::Service => 'S',
            Self::Namespace => 'R',
        }
    }

    /// Risk level of granting this resource type.
    fn risk_level(self) -> RiskLevel {
        match self {
            Self::Timer | Self::EventFd | Self::CompletionPort => RiskLevel::Low,
            Self::Channel | Self::Pipe | Self::SharedMemory
            | Self::File | Self::Service => RiskLevel::Medium,
            Self::Socket | Self::Process | Self::Thread
            | Self::IoScheduler | Self::Namespace => RiskLevel::High,
            Self::PortIo | Self::DeviceIrq => RiskLevel::Critical,
        }
    }
}

// ============================================================================
// Rights bitflags (mirrors kernel Rights)
// ============================================================================

/// Capability rights being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(pub u32);

impl Rights {
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const EXECUTE: Self = Self(4);
    pub const CREATE: Self = Self(8);
    pub const DELETE: Self = Self(16);
    pub const ADMIN: Self = Self(32);

    /// Build a human-readable list of rights.
    fn labels(self) -> Vec<&'static str> {
        let mut result = Vec::new();
        if self.0 & Self::READ.0 != 0 { result.push("Read"); }
        if self.0 & Self::WRITE.0 != 0 { result.push("Write"); }
        if self.0 & Self::EXECUTE.0 != 0 { result.push("Execute"); }
        if self.0 & Self::CREATE.0 != 0 { result.push("Create"); }
        if self.0 & Self::DELETE.0 != 0 { result.push("Delete"); }
        if self.0 & Self::ADMIN.0 != 0 { result.push("Admin"); }
        if result.is_empty() { result.push("(none)"); }
        result
    }
}

// ============================================================================
// Risk level
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low Risk",
            Self::Medium => "Medium Risk",
            Self::High => "High Risk",
            Self::Critical => "Critical",
        }
    }

    fn color(self) -> guitk::Color {
        match self {
            Self::Low => theme::RISK_LOW,
            Self::Medium => theme::RISK_MEDIUM,
            Self::High => theme::RISK_HIGH,
            Self::Critical => theme::RISK_CRITICAL,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Low => "This request has minimal security implications.",
            Self::Medium => "This request grants access to system resources.",
            Self::High => "This request grants significant system access.",
            Self::Critical => "This request grants direct hardware access. Only allow if you trust this application completely.",
        }
    }
}

// ============================================================================
// Capability request info (from kernel)
// ============================================================================

/// Information about a pending capability request, populated from the
/// kernel's `CapRequest` struct via `list_pending()`.
#[derive(Debug, Clone)]
pub struct CapRequestInfo {
    /// Unique request ID assigned by the kernel.
    pub id: u64,
    /// PID of the requesting process.
    pub pid: u64,
    /// Human-readable process name.
    pub process_name: String,
    /// What type of resource is being requested.
    pub resource_type: ResourceType,
    /// What rights are being requested on that resource.
    pub rights: Rights,
    /// Human-readable reason the process gave for the request.
    pub reason: String,
    /// When the request was created (monotonic ms).
    pub created_at_ms: u64,
}

// ============================================================================
// Events emitted by the dialog
// ============================================================================

/// Events produced by the security dialog for the shell to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecurityDialogEvent {
    /// User approved a specific request.
    Approved(u64),
    /// User denied a specific request.
    Denied(u64),
    /// User chose "Deny All" for all pending requests.
    DeniedAll,
    /// User toggled "Remember this decision" for the current request.
    RememberToggled(u64, bool),
}

// ============================================================================
// Button identifiers
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonId {
    Allow,
    Deny,
    Details,
    DenyAll,
}

// ============================================================================
// SecurityDialog
// ============================================================================

/// The security prompt dialog that displays pending capability requests.
///
/// Renders as a modal overlay with a dimmed background, a shield icon,
/// the request details, and Allow/Deny buttons. If multiple requests
/// are queued, shows a counter and processes them one at a time.
pub struct SecurityDialog {
    /// Whether the dialog is currently visible.
    visible: bool,
    /// Queue of pending requests to present to the user.
    queue: Vec<CapRequestInfo>,
    /// Whether the details panel is expanded.
    details_expanded: bool,
    /// Whether "Remember this decision" is checked.
    remember: bool,
    /// Which button is currently hovered (for highlight).
    hovered_button: Option<ButtonId>,
    /// Accumulated events to be drained by the shell.
    events: Vec<SecurityDialogEvent>,
    /// Screen dimensions for centering.
    screen_width: f32,
    screen_height: f32,
    /// Current time in monotonic milliseconds (updated each render).
    current_time_ms: u64,
    /// Decision history for "remember" feature.
    remembered_decisions: Vec<RememberedDecision>,
}

/// A remembered allow/deny decision for auto-responding to future requests.
#[derive(Debug, Clone)]
struct RememberedDecision {
    /// Process name pattern.
    process_name: String,
    /// Resource type.
    resource_type: ResourceType,
    /// Rights mask.
    rights: Rights,
    /// Whether the decision was to allow.
    allowed: bool,
    /// When this decision was recorded (monotonic ms).
    recorded_at_ms: u64,
}

impl SecurityDialog {
    /// Create a new security dialog, initially hidden.
    pub fn new() -> Self {
        Self {
            visible: false,
            queue: Vec::new(),
            details_expanded: false,
            remember: false,
            hovered_button: None,
            events: Vec::new(),
            screen_width: 1920.0,
            screen_height: 1080.0,
            current_time_ms: 0,
            remembered_decisions: Vec::new(),
        }
    }

    /// Update screen dimensions for centering the dialog.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Update the current monotonic time (call each frame).
    pub fn set_current_time(&mut self, time_ms: u64) {
        self.current_time_ms = time_ms;
    }

    /// Push a new capability request onto the queue.
    ///
    /// If the dialog is not visible and there are no pending requests,
    /// shows the dialog automatically. If a remembered decision exists
    /// for this exact (process_name, resource_type, rights) combination,
    /// the decision is applied immediately and the dialog is not shown.
    pub fn push_request(&mut self, request: CapRequestInfo) {
        // Check remembered decisions first
        if let Some(decision) = self.find_remembered_decision(&request) {
            if decision.allowed {
                self.events.push(SecurityDialogEvent::Approved(request.id));
            } else {
                self.events.push(SecurityDialogEvent::Denied(request.id));
            }
            return;
        }

        self.queue.push(request);
        if !self.visible {
            self.visible = true;
            self.details_expanded = false;
            self.remember = false;
            self.hovered_button = None;
        }
    }

    /// Check if there is a remembered decision matching this request.
    fn find_remembered_decision(&self, request: &CapRequestInfo) -> Option<&RememberedDecision> {
        self.remembered_decisions.iter().find(|d| {
            d.process_name == request.process_name
                && d.resource_type == request.resource_type
                && (d.rights.0 & request.rights.0) == request.rights.0
        })
    }

    /// Record a decision for future auto-response.
    fn record_decision(&mut self, request: &CapRequestInfo, allowed: bool) {
        // Remove any existing decision for this combination
        self.remembered_decisions.retain(|d| {
            !(d.process_name == request.process_name
                && d.resource_type == request.resource_type)
        });

        self.remembered_decisions.push(RememberedDecision {
            process_name: request.process_name.clone(),
            resource_type: request.resource_type,
            rights: request.rights,
            allowed,
            recorded_at_ms: self.current_time_ms,
        });

        // Cap at 100 remembered decisions
        if self.remembered_decisions.len() > 100 {
            self.remembered_decisions.remove(0);
        }
    }

    /// Returns true if the dialog is visible and should receive input.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns the number of pending requests in the queue.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Drain all accumulated events.
    pub fn drain_events(&mut self) -> Vec<SecurityDialogEvent> {
        core::mem::take(&mut self.events)
    }

    /// Get the current request being displayed, if any.
    fn current_request(&self) -> Option<&CapRequestInfo> {
        self.queue.first()
    }

    /// Process the allow action for the current request.
    fn allow_current(&mut self) {
        if let Some(request) = self.queue.first().cloned() {
            self.events.push(SecurityDialogEvent::Approved(request.id));
            if self.remember {
                self.record_decision(&request, true);
                self.events.push(SecurityDialogEvent::RememberToggled(request.id, true));
            }
            self.queue.remove(0);
            self.advance_or_hide();
        }
    }

    /// Process the deny action for the current request.
    fn deny_current(&mut self) {
        if let Some(request) = self.queue.first().cloned() {
            self.events.push(SecurityDialogEvent::Denied(request.id));
            if self.remember {
                self.record_decision(&request, false);
                self.events.push(SecurityDialogEvent::RememberToggled(request.id, false));
            }
            self.queue.remove(0);
            self.advance_or_hide();
        }
    }

    /// Deny all pending requests at once.
    fn deny_all(&mut self) {
        for request in &self.queue {
            self.events.push(SecurityDialogEvent::Denied(request.id));
        }
        self.events.push(SecurityDialogEvent::DeniedAll);
        self.queue.clear();
        self.visible = false;
    }

    /// After processing a request, either show the next one or hide.
    fn advance_or_hide(&mut self) {
        if self.queue.is_empty() {
            self.visible = false;
        } else {
            // Reset UI state for the next request
            self.details_expanded = false;
            self.remember = false;
            self.hovered_button = None;
        }
    }

    // ========================================================================
    // Input handling
    // ========================================================================

    /// Handle a keyboard event. Returns true if the event was consumed.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> bool {
        if !self.visible { return false; }

        match event.key {
            // Enter = Allow (focused action)
            Key::Enter => {
                self.allow_current();
                true
            }
            // Escape = Deny
            Key::Escape => {
                self.deny_current();
                true
            }
            // D = Deny, A = Allow (keyboard shortcuts)
            Key::A if !event.modifiers.ctrl => {
                self.allow_current();
                true
            }
            Key::D if !event.modifiers.ctrl => {
                self.deny_current();
                true
            }
            // Ctrl+D = Deny All
            Key::D if event.modifiers.ctrl => {
                self.deny_all();
                true
            }
            // Space = toggle details
            Key::Space => {
                self.details_expanded = !self.details_expanded;
                true
            }
            // R = toggle remember
            Key::R if !event.modifiers.ctrl => {
                self.remember = !self.remember;
                if let Some(request) = self.current_request() {
                    let id = request.id;
                    self.events.push(SecurityDialogEvent::RememberToggled(id, self.remember));
                }
                true
            }
            // Tab = cycle hovered button
            Key::Tab => {
                self.hovered_button = match self.hovered_button {
                    None | Some(ButtonId::DenyAll) => Some(ButtonId::Allow),
                    Some(ButtonId::Allow) => Some(ButtonId::Deny),
                    Some(ButtonId::Deny) => Some(ButtonId::Details),
                    Some(ButtonId::Details) => Some(ButtonId::DenyAll),
                };
                true
            }
            _ => true, // Consume all keys while visible (modal)
        }
    }

    /// Handle a mouse event. Returns true if the event was consumed.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> bool {
        if !self.visible { return false; }

        let dialog_x = (self.screen_width - DIALOG_WIDTH) / 2.0;
        let dialog_y = (self.screen_height - self.dialog_height()) / 2.0;
        let local_x = event.x - dialog_x;
        let local_y = event.y - dialog_y;

        match event.kind {
            MouseEventKind::Move => {
                // Hit-test buttons
                self.hovered_button = self.hit_test_button(local_x, local_y);
                true
            }
            MouseEventKind::Press(MouseButton::Left) => {
                // Check button clicks
                if let Some(button) = self.hit_test_button(local_x, local_y) {
                    match button {
                        ButtonId::Allow => self.allow_current(),
                        ButtonId::Deny => self.deny_current(),
                        ButtonId::Details => {
                            self.details_expanded = !self.details_expanded;
                        }
                        ButtonId::DenyAll => self.deny_all(),
                    }
                    return true;
                }

                // Check "remember" checkbox click
                if self.hit_test_remember(local_x, local_y) {
                    self.remember = !self.remember;
                    if let Some(request) = self.current_request() {
                        let id = request.id;
                        self.events.push(SecurityDialogEvent::RememberToggled(id, self.remember));
                    }
                    return true;
                }

                true // Consume click even if not on a button (modal)
            }
            _ => true, // Consume all mouse events while visible (modal)
        }
    }

    /// Calculate the dynamic dialog height based on whether details are expanded.
    fn dialog_height(&self) -> f32 {
        if self.details_expanded {
            DIALOG_HEIGHT + 120.0
        } else {
            DIALOG_HEIGHT
        }
    }

    /// Hit-test a point against the button layout.
    fn hit_test_button(&self, x: f32, y: f32) -> Option<ButtonId> {
        let h = self.dialog_height();

        // Allow button: right side of button row, near bottom
        let btn_y = h - PADDING - BUTTON_HEIGHT;
        let allow_x = DIALOG_WIDTH - PADDING - BUTTON_WIDTH;
        if x >= allow_x && x <= allow_x + BUTTON_WIDTH
            && y >= btn_y && y <= btn_y + BUTTON_HEIGHT
        {
            return Some(ButtonId::Allow);
        }

        // Deny button: left of Allow
        let deny_x = allow_x - BUTTON_SPACING - BUTTON_WIDTH;
        if x >= deny_x && x <= deny_x + BUTTON_WIDTH
            && y >= btn_y && y <= btn_y + BUTTON_HEIGHT
        {
            return Some(ButtonId::Deny);
        }

        // Details toggle: small link below the reason text
        let details_y = self.details_link_y();
        if x >= PADDING && x <= PADDING + 100.0
            && y >= details_y && y <= details_y + 18.0
        {
            return Some(ButtonId::Details);
        }

        // Deny All: only shown when queue > 1, bottom-left
        if self.queue.len() > 1 {
            let deny_all_x = PADDING;
            if x >= deny_all_x && x <= deny_all_x + 90.0
                && y >= btn_y && y <= btn_y + BUTTON_HEIGHT
            {
                return Some(ButtonId::DenyAll);
            }
        }

        None
    }

    /// Hit-test the "remember" checkbox area.
    fn hit_test_remember(&self, x: f32, y: f32) -> bool {
        let h = self.dialog_height();
        let checkbox_y = h - PADDING - BUTTON_HEIGHT - 28.0;
        x >= PADDING && x <= PADDING + 200.0
            && y >= checkbox_y && y <= checkbox_y + 20.0
    }

    /// Y position of the "Show details" / "Hide details" link.
    fn details_link_y(&self) -> f32 {
        // After the reason text area
        HEADER_HEIGHT + PADDING + 90.0 + 8.0
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the dialog, returning a list of render commands.
    ///
    /// The dialog is rendered as a modal overlay:
    /// 1. Full-screen dimmer
    /// 2. Centered dialog box with shadow
    /// 3. Shield icon + title bar
    /// 4. Request details (process, resource, rights, reason)
    /// 5. Optional expanded details panel
    /// 6. Remember checkbox
    /// 7. Allow / Deny / Deny All buttons
    /// 8. Queue counter (if multiple pending)
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let Some(request) = self.current_request() else {
            return Vec::new();
        };

        let mut cmds: Vec<RenderCommand> = Vec::with_capacity(64);
        let risk = request.resource_type.risk_level();
        let dw = DIALOG_WIDTH;
        let dh = self.dialog_height();
        let dx = (self.screen_width - dw) / 2.0;
        let dy = (self.screen_height - dh) / 2.0;

        // --- Full-screen dimmer overlay ---
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.screen_width,
            height: self.screen_height,
            color: theme::DIMMER,
            corner_radii: CornerRadii::ZERO,
        });

        // --- Dialog shadow ---
        cmds.push(RenderCommand::BoxShadow {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 24.0,
            spread: 0.0,
            color: theme::SHADOW,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // --- Dialog background ---
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: theme::BASE,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // --- Dialog border ---
        cmds.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: theme::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // --- Header section with colored accent bar ---
        let accent_color = risk.color();
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: HEADER_HEIGHT,
            color: theme::MANTLE,
            corner_radii: CornerRadii {
                top_left: DIALOG_RADIUS,
                top_right: DIALOG_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Accent line at top of header
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: 3.0,
            color: accent_color,
            corner_radii: CornerRadii {
                top_left: DIALOG_RADIUS,
                top_right: DIALOG_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // --- Shield icon ---
        let shield_x = dx + PADDING;
        let shield_y = dy + (HEADER_HEIGHT - SHIELD_SIZE) / 2.0 + 1.0;
        self.render_shield_icon(&mut cmds, shield_x, shield_y, accent_color, request.resource_type.icon_char());

        // --- Title text ---
        cmds.push(RenderCommand::Text {
            x: shield_x + SHIELD_SIZE + 12.0,
            y: dy + 14.0,
            text: "Security Permission Request".into(),
            font_size: TITLE_FONT_SIZE,
            color: theme::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dw - SHIELD_SIZE - PADDING * 3.0),
        });

        // --- Subtitle: "AppName wants to access ResourceType" ---
        let subtitle = format!(
            "\"{}\" requests {} access",
            truncate_str(&request.process_name, 30),
            request.resource_type.label().to_ascii_lowercase(),
        );
        cmds.push(RenderCommand::Text {
            x: shield_x + SHIELD_SIZE + 12.0,
            y: dy + 34.0,
            text: subtitle,
            font_size: SUBTITLE_FONT_SIZE,
            color: theme::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - SHIELD_SIZE - PADDING * 3.0),
        });

        // --- Body section ---
        let body_y = dy + HEADER_HEIGHT + PADDING;

        // Risk level badge
        let risk_label = risk.label();
        let badge_width = risk_label.len() as f32 * 7.0 + 16.0;
        cmds.push(RenderCommand::FillRect {
            x: dx + PADDING,
            y: body_y,
            width: badge_width,
            height: 22.0,
            color: risk.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: body_y + 4.0,
            text: risk_label.into(),
            font_size: SMALL_FONT_SIZE,
            color: theme::CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(badge_width),
        });

        // Risk description
        cmds.push(RenderCommand::Text {
            x: dx + PADDING + badge_width + 10.0,
            y: body_y + 4.0,
            text: risk.description().into(),
            font_size: SMALL_FONT_SIZE,
            color: theme::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - PADDING * 2.0 - badge_width - 10.0),
        });

        // --- Detail rows ---
        let row_y = body_y + 32.0;

        // Process info row
        self.render_detail_row(&mut cmds, dx + PADDING, row_y,
            "Process:", &format!("{} (PID {})", request.process_name, request.pid),
            dw - PADDING * 2.0);

        // Resource type row
        self.render_detail_row(&mut cmds, dx + PADDING, row_y + DETAIL_ROW_HEIGHT,
            "Resource:", request.resource_type.label(),
            dw - PADDING * 2.0);

        // Rights row
        let rights_str = request.rights.labels().join(", ");
        self.render_detail_row(&mut cmds, dx + PADDING, row_y + DETAIL_ROW_HEIGHT * 2.0,
            "Rights:", &rights_str,
            dw - PADDING * 2.0);

        // Reason row
        let reason_display = if request.reason.is_empty() {
            "(no reason provided)".to_string()
        } else {
            truncate_str(&request.reason, MAX_REASON_DISPLAY).to_string()
        };
        self.render_detail_row(&mut cmds, dx + PADDING, row_y + DETAIL_ROW_HEIGHT * 3.0,
            "Reason:", &reason_display,
            dw - PADDING * 2.0);

        // --- Details toggle link ---
        let details_y = dy + self.details_link_y();
        let details_text = if self.details_expanded { "Hide details" } else { "Show details" };
        let details_color = if self.hovered_button == Some(ButtonId::Details) {
            theme::BLUE
        } else {
            theme::SUBTEXT0
        };
        cmds.push(RenderCommand::Text {
            x: dx + PADDING,
            y: details_y,
            text: format!("▾ {details_text}"),
            font_size: DETAIL_FONT_SIZE,
            color: details_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // --- Expanded details panel ---
        if self.details_expanded {
            let panel_y = details_y + 22.0;
            let panel_w = dw - PADDING * 2.0;
            let panel_h = 100.0;

            cmds.push(RenderCommand::FillRect {
                x: dx + PADDING,
                y: panel_y,
                width: panel_w,
                height: panel_h,
                color: theme::DETAILS_BG,
                corner_radii: CornerRadii::all(DETAIL_PANEL_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: dx + PADDING,
                y: panel_y,
                width: panel_w,
                height: panel_h,
                color: theme::DETAILS_BORDER,
                line_width: 1.0,
                corner_radii: CornerRadii::all(DETAIL_PANEL_RADIUS),
            });

            let dp = 8.0; // detail panel inner padding
            let mut ty = panel_y + dp;

            // Request ID
            cmds.push(RenderCommand::Text {
                x: dx + PADDING + dp,
                y: ty,
                text: format!("Request ID: {}", request.id),
                font_size: DETAIL_FONT_SIZE,
                color: theme::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - dp * 2.0),
            });
            ty += 16.0;

            // Timestamp
            let elapsed_secs = self.current_time_ms.saturating_sub(request.created_at_ms) / 1000;
            let timeout_remaining = 30_u64.saturating_sub(elapsed_secs);
            let time_color = if timeout_remaining <= TIMEOUT_WARN_SECS {
                theme::YELLOW
            } else {
                theme::OVERLAY0
            };
            cmds.push(RenderCommand::Text {
                x: dx + PADDING + dp,
                y: ty,
                text: format!("Requested: {elapsed_secs}s ago (timeout in {timeout_remaining}s)"),
                font_size: DETAIL_FONT_SIZE,
                color: time_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - dp * 2.0),
            });
            ty += 16.0;

            // Rights bitmask
            cmds.push(RenderCommand::Text {
                x: dx + PADDING + dp,
                y: ty,
                text: format!("Rights bitmask: 0x{:04X}", request.rights.0),
                font_size: DETAIL_FONT_SIZE,
                color: theme::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - dp * 2.0),
            });
            ty += 16.0;

            // Keyboard shortcuts reminder
            cmds.push(RenderCommand::Text {
                x: dx + PADDING + dp,
                y: ty,
                text: "Keys: A=Allow  D=Deny  Ctrl+D=Deny All  Space=Details  R=Remember".into(),
                font_size: DETAIL_FONT_SIZE,
                color: theme::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - dp * 2.0),
            });
        }

        // --- Remember checkbox ---
        let checkbox_y = dy + dh - PADDING - BUTTON_HEIGHT - 28.0;
        self.render_checkbox(&mut cmds, dx + PADDING, checkbox_y, self.remember, "Remember this decision");

        // --- Action buttons ---
        let btn_y = dy + dh - PADDING - BUTTON_HEIGHT;

        // Deny All (only when queue > 1)
        if self.queue.len() > 1 {
            let deny_all_color = if self.hovered_button == Some(ButtonId::DenyAll) {
                theme::BUTTON_HOVER
            } else {
                theme::BUTTON_BG
            };
            self.render_button(&mut cmds, dx + PADDING, btn_y, 90.0,
                &format!("Deny All ({})", self.queue.len()),
                deny_all_color, theme::RED);
        }

        // Deny button
        let deny_x = dx + dw - PADDING - BUTTON_WIDTH * 2.0 - BUTTON_SPACING;
        let deny_bg = if self.hovered_button == Some(ButtonId::Deny) {
            theme::DENY_BUTTON
        } else {
            theme::BUTTON_BG
        };
        let deny_fg = if self.hovered_button == Some(ButtonId::Deny) {
            theme::DENY_TEXT
        } else {
            theme::RED
        };
        self.render_button(&mut cmds, deny_x, btn_y, BUTTON_WIDTH, "Deny", deny_bg, deny_fg);

        // Allow button
        let allow_x = dx + dw - PADDING - BUTTON_WIDTH;
        let allow_bg = if self.hovered_button == Some(ButtonId::Allow) {
            theme::ALLOW_BUTTON
        } else {
            theme::BUTTON_BG
        };
        let allow_fg = if self.hovered_button == Some(ButtonId::Allow) {
            theme::ALLOW_TEXT
        } else {
            theme::GREEN
        };
        self.render_button(&mut cmds, allow_x, btn_y, BUTTON_WIDTH, "Allow", allow_bg, allow_fg);

        // --- Queue indicator ---
        if self.queue.len() > 1 {
            let indicator = format!("Request 1 of {}", self.queue.len().min(MAX_QUEUE_DISPLAY));
            cmds.push(RenderCommand::Text {
                x: dx + dw - PADDING - 120.0,
                y: dy + dh - PADDING - BUTTON_HEIGHT - 28.0 + 3.0,
                text: indicator,
                font_size: SMALL_FONT_SIZE,
                color: theme::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
        }

        cmds
    }

    // ========================================================================
    // Render helpers
    // ========================================================================

    /// Render a shield icon with a letter inside it.
    fn render_shield_icon(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32,
                          color: guitk::Color, letter: char) {
        // Shield background (rounded square)
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: SHIELD_SIZE,
            height: SHIELD_SIZE,
            color: theme::SHIELD_BG,
            corner_radii: CornerRadii::all(8.0),
        });

        // Shield border with risk color
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: SHIELD_SIZE,
            height: SHIELD_SIZE,
            color,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Inner colored accent square
        cmds.push(RenderCommand::FillRect {
            x: x + 6.0,
            y: y + 6.0,
            width: SHIELD_SIZE - 12.0,
            height: SHIELD_SIZE - 12.0,
            color,
            corner_radii: CornerRadii::all(4.0),
        });

        // Letter
        cmds.push(RenderCommand::Text {
            x: x + SHIELD_SIZE / 2.0 - 5.0,
            y: y + SHIELD_SIZE / 2.0 - 8.0,
            text: letter.to_string(),
            font_size: 14.0,
            color: theme::CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SHIELD_SIZE),
        });
    }

    /// Render a detail row (label + value).
    fn render_detail_row(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32,
                          label: &str, value: &str, width: f32) {
        let label_width = 80.0;
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: label.into(),
            font_size: BODY_FONT_SIZE,
            color: theme::SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(label_width),
        });
        cmds.push(RenderCommand::Text {
            x: x + label_width,
            y: y + 2.0,
            text: value.into(),
            font_size: BODY_FONT_SIZE,
            color: theme::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - label_width),
        });
    }

    /// Render a button.
    fn render_button(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32,
                      width: f32, label: &str, bg: guitk::Color, fg: guitk::Color) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: BUTTON_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(BUTTON_RADIUS),
        });
        // Center text in button
        let text_x = x + (width - label.len() as f32 * 7.0) / 2.0;
        let text_y = y + (BUTTON_HEIGHT - BODY_FONT_SIZE) / 2.0;
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.into(),
            font_size: BODY_FONT_SIZE,
            color: fg,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
    }

    /// Render a checkbox with label.
    fn render_checkbox(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32,
                        checked: bool, label: &str) {
        let box_size = 16.0;

        // Checkbox background
        cmds.push(RenderCommand::FillRect {
            x,
            y: y + 1.0,
            width: box_size,
            height: box_size,
            color: if checked { theme::BLUE } else { theme::SURFACE0 },
            corner_radii: CornerRadii::all(3.0),
        });

        // Checkbox border
        cmds.push(RenderCommand::StrokeRect {
            x,
            y: y + 1.0,
            width: box_size,
            height: box_size,
            color: if checked { theme::BLUE } else { theme::SURFACE2 },
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Checkmark
        if checked {
            cmds.push(RenderCommand::Text {
                x: x + 2.0,
                y: y + 1.0,
                text: "✓".into(),
                font_size: 12.0,
                color: theme::CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(box_size),
            });
        }

        // Label
        cmds.push(RenderCommand::Text {
            x: x + box_size + 8.0,
            y: y + 2.0,
            text: label.into(),
            font_size: DETAIL_FONT_SIZE,
            color: theme::SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });
    }
}

// ============================================================================
// Utility
// ============================================================================

/// Truncate a string to at most `max` characters, appending "…" if truncated.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find last char boundary at or before max
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(id: u64) -> CapRequestInfo {
        CapRequestInfo {
            id,
            pid: 1234,
            process_name: "test_app".into(),
            resource_type: ResourceType::File,
            rights: Rights(Rights::READ.0 | Rights::WRITE.0),
            reason: "Need file access for saving document".into(),
            created_at_ms: 1000,
        }
    }

    #[test]
    fn test_new_dialog_hidden() {
        let dialog = SecurityDialog::new();
        assert!(!dialog.is_visible());
        assert_eq!(dialog.pending_count(), 0);
    }

    #[test]
    fn test_push_request_shows_dialog() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        assert!(dialog.is_visible());
        assert_eq!(dialog.pending_count(), 1);
    }

    #[test]
    fn test_allow_current_emits_event() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(42));
        dialog.allow_current();
        let events = dialog.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SecurityDialogEvent::Approved(42));
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_deny_current_emits_event() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(42));
        dialog.deny_current();
        let events = dialog.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SecurityDialogEvent::Denied(42));
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_deny_all_multiple() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        dialog.push_request(sample_request(2));
        dialog.push_request(sample_request(3));
        assert_eq!(dialog.pending_count(), 3);

        dialog.deny_all();
        let events = dialog.drain_events();
        // Should have 3 individual denials + 1 DeniedAll
        assert_eq!(events.len(), 4);
        assert_eq!(events[0], SecurityDialogEvent::Denied(1));
        assert_eq!(events[1], SecurityDialogEvent::Denied(2));
        assert_eq!(events[2], SecurityDialogEvent::Denied(3));
        assert_eq!(events[3], SecurityDialogEvent::DeniedAll);
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_queue_processes_one_at_a_time() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        dialog.push_request(sample_request(2));
        assert_eq!(dialog.pending_count(), 2);

        // Allow first, dialog stays visible for second
        dialog.allow_current();
        assert!(dialog.is_visible());
        assert_eq!(dialog.pending_count(), 1);

        // Deny second, dialog hides
        dialog.deny_current();
        assert!(!dialog.is_visible());
        assert_eq!(dialog.pending_count(), 0);
    }

    #[test]
    fn test_remember_decision_auto_approves() {
        let mut dialog = SecurityDialog::new();
        dialog.remember = true;
        dialog.push_request(sample_request(1));
        dialog.allow_current(); // Records remembered allow decision
        let events1 = dialog.drain_events();
        assert!(events1.contains(&SecurityDialogEvent::Approved(1)));

        // Push same type of request — should be auto-approved
        dialog.push_request(CapRequestInfo {
            id: 2,
            pid: 5678,
            process_name: "test_app".into(),
            resource_type: ResourceType::File,
            rights: Rights(Rights::READ.0),
            reason: "Another file request".into(),
            created_at_ms: 2000,
        });
        let events2 = dialog.drain_events();
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0], SecurityDialogEvent::Approved(2));
        // Dialog should NOT be visible since it was auto-handled
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_remember_decision_auto_denies() {
        let mut dialog = SecurityDialog::new();
        dialog.remember = true;
        dialog.push_request(sample_request(1));
        dialog.deny_current(); // Records remembered deny decision
        dialog.drain_events();

        // Push same type — should be auto-denied
        dialog.push_request(CapRequestInfo {
            id: 2,
            pid: 5678,
            process_name: "test_app".into(),
            resource_type: ResourceType::File,
            rights: Rights(Rights::READ.0),
            reason: "Another request".into(),
            created_at_ms: 2000,
        });
        let events = dialog.drain_events();
        assert_eq!(events[0], SecurityDialogEvent::Denied(2));
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_remember_different_process_not_matched() {
        let mut dialog = SecurityDialog::new();
        dialog.remember = true;
        dialog.push_request(sample_request(1));
        dialog.allow_current();
        dialog.drain_events();

        // Different process name — should NOT be auto-approved
        dialog.push_request(CapRequestInfo {
            id: 2,
            pid: 9999,
            process_name: "other_app".into(),
            resource_type: ResourceType::File,
            rights: Rights(Rights::READ.0 | Rights::WRITE.0),
            reason: "I also need files".into(),
            created_at_ms: 3000,
        });
        assert!(dialog.is_visible()); // Dialog shown, not auto-handled
    }

    #[test]
    fn test_resource_type_labels() {
        assert_eq!(ResourceType::File.label(), "File Access");
        assert_eq!(ResourceType::Socket.label(), "Network Socket");
        assert_eq!(ResourceType::PortIo.label(), "Hardware I/O Port");
        assert_eq!(ResourceType::Channel.label(), "IPC Channel");
    }

    #[test]
    fn test_risk_levels() {
        assert_eq!(ResourceType::Timer.risk_level(), RiskLevel::Low);
        assert_eq!(ResourceType::File.risk_level(), RiskLevel::Medium);
        assert_eq!(ResourceType::Socket.risk_level(), RiskLevel::High);
        assert_eq!(ResourceType::PortIo.risk_level(), RiskLevel::Critical);
        assert_eq!(ResourceType::DeviceIrq.risk_level(), RiskLevel::Critical);
    }

    #[test]
    fn test_rights_labels() {
        let r = Rights(Rights::READ.0 | Rights::WRITE.0);
        assert_eq!(r.labels(), vec!["Read", "Write"]);

        let all = Rights(0x3F);
        assert_eq!(all.labels(), vec!["Read", "Write", "Execute", "Create", "Delete", "Admin"]);

        let none = Rights(0);
        assert_eq!(none.labels(), vec!["(none)"]);
    }

    #[test]
    fn test_render_hidden_returns_empty() {
        let dialog = SecurityDialog::new();
        assert!(dialog.render().is_empty());
    }

    #[test]
    fn test_render_visible_returns_commands() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        let cmds = dialog.render();
        // Should have: dimmer + shadow + bg + border + header bg + accent line +
        //   shield bg + shield border + shield inner + shield letter +
        //   title + subtitle + risk badge + risk text + 4 detail rows (8 texts) +
        //   details link + checkbox (3 parts) + deny button (2 parts) + allow button (2 parts)
        assert!(cmds.len() >= 20, "Expected at least 20 render commands, got {}", cmds.len());
    }

    #[test]
    fn test_render_expanded_details() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        dialog.set_current_time(5000);

        let cmds_collapsed = dialog.render();
        dialog.details_expanded = true;
        let cmds_expanded = dialog.render();

        // Expanded should have more commands (the details panel + its contents)
        assert!(cmds_expanded.len() > cmds_collapsed.len(),
            "Expanded {} should be > collapsed {}", cmds_expanded.len(), cmds_collapsed.len());
    }

    #[test]
    fn test_keyboard_allow() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));

        let event = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };
        assert!(dialog.handle_key_event(&event));
        let events = dialog.drain_events();
        assert_eq!(events[0], SecurityDialogEvent::Approved(1));
    }

    #[test]
    fn test_keyboard_deny() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));

        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };
        assert!(dialog.handle_key_event(&event));
        let events = dialog.drain_events();
        assert_eq!(events[0], SecurityDialogEvent::Denied(1));
    }

    #[test]
    fn test_keyboard_deny_all() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        dialog.push_request(sample_request(2));

        let event = KeyEvent {
            key: Key::D,
            pressed: true,
            modifiers: Modifiers { ctrl: true, alt: false, shift: false, super_key: false },
            text: None,
        };
        assert!(dialog.handle_key_event(&event));
        let events = dialog.drain_events();
        assert!(events.contains(&SecurityDialogEvent::DeniedAll));
    }

    #[test]
    fn test_keyboard_toggle_details() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        assert!(!dialog.details_expanded);

        let event = KeyEvent {
            key: Key::Space,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };
        dialog.handle_key_event(&event);
        assert!(dialog.details_expanded);

        dialog.handle_key_event(&event);
        assert!(!dialog.details_expanded);
    }

    #[test]
    fn test_keyboard_toggle_remember() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        assert!(!dialog.remember);

        let event = KeyEvent {
            key: Key::R,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };
        dialog.handle_key_event(&event);
        assert!(dialog.remember);
    }

    #[test]
    fn test_keyboard_not_consumed_when_hidden() {
        let dialog = SecurityDialog::new();
        let event = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };
        // Can't call on immutable ref; need mutable
        let mut dialog = dialog;
        assert!(!dialog.handle_key_event(&event));
    }

    #[test]
    fn test_tab_cycles_buttons() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));

        let tab = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            text: None,
        };

        assert_eq!(dialog.hovered_button, None);
        dialog.handle_key_event(&tab);
        assert_eq!(dialog.hovered_button, Some(ButtonId::Allow));
        dialog.handle_key_event(&tab);
        assert_eq!(dialog.hovered_button, Some(ButtonId::Deny));
        dialog.handle_key_event(&tab);
        assert_eq!(dialog.hovered_button, Some(ButtonId::Details));
        dialog.handle_key_event(&tab);
        assert_eq!(dialog.hovered_button, Some(ButtonId::DenyAll));
        dialog.handle_key_event(&tab);
        assert_eq!(dialog.hovered_button, Some(ButtonId::Allow)); // wraps
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("", 5), "");
    }

    #[test]
    fn test_dialog_height_changes_with_details() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));

        let h1 = dialog.dialog_height();
        dialog.details_expanded = true;
        let h2 = dialog.dialog_height();
        assert!(h2 > h1, "Expanded height {} should be > collapsed {}", h2, h1);
    }

    #[test]
    fn test_empty_reason_display() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(CapRequestInfo {
            id: 1,
            pid: 1,
            process_name: "app".into(),
            resource_type: ResourceType::File,
            rights: Rights(1),
            reason: String::new(),
            created_at_ms: 0,
        });
        // Should render without panic (reason shown as "(no reason provided)")
        let cmds = dialog.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_multiple_resource_types_different_risks() {
        // Verify we have a spread of risk levels
        let types = [
            ResourceType::Timer,
            ResourceType::File,
            ResourceType::Socket,
            ResourceType::PortIo,
        ];
        let risks: Vec<_> = types.iter().map(|t| t.risk_level()).collect();
        assert_eq!(risks[0], RiskLevel::Low);
        assert_eq!(risks[1], RiskLevel::Medium);
        assert_eq!(risks[2], RiskLevel::High);
        assert_eq!(risks[3], RiskLevel::Critical);
    }

    #[test]
    fn test_remembered_decisions_capped_at_100() {
        let mut dialog = SecurityDialog::new();
        dialog.remember = true;

        // Push and allow 110 unique requests
        for i in 0..110 {
            dialog.push_request(CapRequestInfo {
                id: i,
                pid: 1,
                process_name: format!("app_{i}"),
                resource_type: ResourceType::File,
                rights: Rights(1),
                reason: String::new(),
                created_at_ms: 0,
            });
            dialog.allow_current();
            dialog.drain_events();
        }

        assert!(dialog.remembered_decisions.len() <= 100);
    }

    #[test]
    fn test_set_screen_size() {
        let mut dialog = SecurityDialog::new();
        dialog.set_screen_size(2560.0, 1440.0);
        assert!((dialog.screen_width - 2560.0).abs() < f32::EPSILON);
        assert!((dialog.screen_height - 1440.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_drain_clears_events() {
        let mut dialog = SecurityDialog::new();
        dialog.push_request(sample_request(1));
        dialog.allow_current();
        assert!(!dialog.drain_events().is_empty());
        assert!(dialog.drain_events().is_empty()); // Second drain should be empty
    }

    #[test]
    fn test_resource_type_icon_chars() {
        // Every resource type has an icon char
        let types = [
            ResourceType::Channel, ResourceType::Pipe, ResourceType::SharedMemory,
            ResourceType::EventFd, ResourceType::CompletionPort,
            ResourceType::Process, ResourceType::Thread,
            ResourceType::PortIo, ResourceType::DeviceIrq,
            ResourceType::File, ResourceType::Socket, ResourceType::Timer,
            ResourceType::IoScheduler, ResourceType::Service, ResourceType::Namespace,
        ];
        for t in types {
            assert!(t.icon_char().is_ascii_alphabetic(), "{t:?} should have an alpha icon char");
        }
    }
}
