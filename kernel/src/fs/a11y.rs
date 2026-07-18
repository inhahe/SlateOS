//! Accessibility subsystem — input automation and assistive technology support.
//!
//! Provides infrastructure for screen readers, magnifiers, and input automation
//! tools.  Accessibility tools get a dedicated capability class to inject input
//! events and query UI element state without breaking the security model.
//!
//! ## Design Reference
//!
//! design.txt line 600-602: "Accessibility depends on [input automation].
//! Make sure your capability model doesn't make accessibility tools impossible
//! to use. Have a dedicated accessibility capability class."
//!
//! ## Architecture
//!
//! ```text
//! Accessibility tool (screen reader, magnifier)
//!   → a11y::register_tool(kind, capabilities)
//!   → a11y::inject_key() / inject_click() / inject_text()
//!
//! Compositor
//!   → a11y::query_element(window, point) → AccessibleElement
//!   → a11y::announce(text) → spoken/displayed
//!
//! Settings panel
//!   → a11y::config() / a11y::set_*()
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered accessibility tools.
const MAX_TOOLS: usize = 32;

/// Maximum announcements in the queue.
const MAX_ANNOUNCEMENTS: usize = 256;

/// Maximum tracked UI elements.
const MAX_ELEMENTS: usize = 4096;

/// Maximum sticky keys held.
const MAX_STICKY: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kind of accessibility tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Screen reader (text-to-speech of UI elements).
    ScreenReader,
    /// Screen magnifier.
    Magnifier,
    /// On-screen keyboard.
    OnScreenKeyboard,
    /// Voice control / speech recognition input.
    VoiceControl,
    /// Switch access (hardware buttons for motor impaired).
    SwitchAccess,
    /// Eye tracking input.
    EyeTracking,
    /// Input automation / testing framework.
    Automation,
    /// Custom tool.
    Custom,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ScreenReader => "screen-reader",
            Self::Magnifier => "magnifier",
            Self::OnScreenKeyboard => "on-screen-keyboard",
            Self::VoiceControl => "voice-control",
            Self::SwitchAccess => "switch-access",
            Self::EyeTracking => "eye-tracking",
            Self::Automation => "automation",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "screen-reader" | "reader" | "sr" => Some(Self::ScreenReader),
            "magnifier" | "mag" => Some(Self::Magnifier),
            "osk" | "on-screen-keyboard" => Some(Self::OnScreenKeyboard),
            "voice" | "voice-control" => Some(Self::VoiceControl),
            "switch" | "switch-access" => Some(Self::SwitchAccess),
            "eye" | "eye-tracking" => Some(Self::EyeTracking),
            "automation" | "auto" => Some(Self::Automation),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Role of a UI element in the accessibility tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementRole {
    Window,
    Button,
    TextInput,
    Label,
    Menu,
    MenuItem,
    Checkbox,
    RadioButton,
    Slider,
    ScrollBar,
    ProgressBar,
    List,
    ListItem,
    Tab,
    TabPanel,
    Toolbar,
    StatusBar,
    Dialog,
    Tooltip,
    Image,
    Link,
    TreeItem,
    Generic,
}

impl ElementRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Button => "button",
            Self::TextInput => "text-input",
            Self::Label => "label",
            Self::Menu => "menu",
            Self::MenuItem => "menu-item",
            Self::Checkbox => "checkbox",
            Self::RadioButton => "radio",
            Self::Slider => "slider",
            Self::ScrollBar => "scrollbar",
            Self::ProgressBar => "progress",
            Self::List => "list",
            Self::ListItem => "list-item",
            Self::Tab => "tab",
            Self::TabPanel => "tab-panel",
            Self::Toolbar => "toolbar",
            Self::StatusBar => "statusbar",
            Self::Dialog => "dialog",
            Self::Tooltip => "tooltip",
            Self::Image => "image",
            Self::Link => "link",
            Self::TreeItem => "tree-item",
            Self::Generic => "generic",
        }
    }
}

/// An accessible UI element exposed to assistive technology.
#[derive(Debug, Clone)]
pub struct AccessibleElement {
    /// Unique element ID.
    pub id: u64,
    /// Window this element belongs to.
    pub window_id: u64,
    /// Role.
    pub role: ElementRole,
    /// Accessible name (what the screen reader announces).
    pub name: String,
    /// Description / tooltip.
    pub description: String,
    /// Whether the element is currently focused.
    pub focused: bool,
    /// Whether the element is enabled/interactive.
    pub enabled: bool,
    /// Value (for sliders, text inputs, etc.).
    pub value: String,
    /// Bounding rectangle: (x, y, width, height).
    pub bounds: (i32, i32, u32, u32),
}

/// A registered accessibility tool.
#[derive(Debug, Clone)]
pub struct AccessibilityTool {
    /// Unique tool ID.
    pub id: u64,
    /// Tool kind.
    pub kind: ToolKind,
    /// Display name.
    pub name: String,
    /// Whether the tool is currently active.
    pub active: bool,
    /// Whether the tool can inject input events.
    pub can_inject_input: bool,
    /// Whether the tool can read screen content.
    pub can_read_screen: bool,
}

/// Accessibility configuration.
#[derive(Debug, Clone)]
pub struct A11yConfig {
    /// Whether high-contrast mode is enabled.
    pub high_contrast: bool,
    /// Whether reduced motion is enabled (no animations).
    pub reduce_motion: bool,
    /// Whether screen reader is active.
    pub screen_reader_active: bool,
    /// Font size multiplier (100 = normal, 150 = 1.5x).
    pub font_scale: u32,
    /// Whether sticky keys are enabled.
    pub sticky_keys: bool,
    /// Whether filter keys (key repeat delay) is enabled.
    pub filter_keys: bool,
    /// Filter key repeat delay (ms).
    pub filter_delay_ms: u32,
    /// Whether mouse keys (arrow keys move cursor) is enabled.
    pub mouse_keys: bool,
    /// Cursor size multiplier (100 = normal).
    pub cursor_scale: u32,
    /// Whether to show visual flash on system sounds.
    pub visual_alerts: bool,
    /// Whether captions are enabled for audio content.
    pub captions: bool,
}

impl Default for A11yConfig {
    fn default() -> Self {
        Self {
            high_contrast: false,
            reduce_motion: false,
            screen_reader_active: false,
            font_scale: 100,
            sticky_keys: false,
            filter_keys: false,
            filter_delay_ms: 500,
            mouse_keys: false,
            cursor_scale: 100,
            visual_alerts: false,
            captions: false,
        }
    }
}

/// An announcement for the screen reader.
#[derive(Debug, Clone)]
pub struct Announcement {
    pub id: u64,
    pub text: String,
    pub priority: AnnouncePriority,
    pub timestamp_ns: u64,
}

/// Priority for announcements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnouncePriority {
    /// Low priority — can be interrupted.
    Low,
    /// Normal priority.
    Normal,
    /// High priority — interrupts current speech.
    High,
    /// Alert — always announced immediately.
    Alert,
}

impl AnnouncePriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Alert => "alert",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "normal" => Some(Self::Normal),
            "high" => Some(Self::High),
            "alert" => Some(Self::Alert),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: A11yConfig,
    tools: BTreeMap<u64, AccessibilityTool>,
    elements: BTreeMap<u64, AccessibleElement>,
    announcements: Vec<Announcement>,
    next_tool_id: u64,
    next_element_id: u64,
    next_announce_id: u64,
    /// Currently held sticky keys.
    sticky_held: Vec<u16>,
    /// Focus element ID.
    focused_element: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            config: A11yConfig {
                high_contrast: false,
                reduce_motion: false,
                screen_reader_active: false,
                font_scale: 100,
                sticky_keys: false,
                filter_keys: false,
                filter_delay_ms: 500,
                mouse_keys: false,
                cursor_scale: 100,
                visual_alerts: false,
                captions: false,
            },
            tools: BTreeMap::new(),
            elements: BTreeMap::new(),
            announcements: Vec::new(),
            next_tool_id: 1,
            next_element_id: 1,
            next_announce_id: 1,
            sticky_held: Vec::new(),
            focused_element: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static INJECT_COUNT: AtomicU64 = AtomicU64::new(0);
static ANNOUNCE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Tool registration
// ---------------------------------------------------------------------------

/// Register an accessibility tool.
pub fn register_tool(kind: ToolKind, name: &str, inject: bool, read: bool) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.tools.len() >= MAX_TOOLS {
        return Err(KernelError::ResourceExhausted);
    }
    let id = state.next_tool_id;
    state.next_tool_id = state.next_tool_id.wrapping_add(1);
    state.tools.insert(id, AccessibilityTool {
        id,
        kind,
        name: String::from(name),
        active: true,
        can_inject_input: inject,
        can_read_screen: read,
    });
    Ok(id)
}

/// Unregister a tool.
pub fn unregister_tool(id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.tools.remove(&id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Activate/deactivate a tool.
pub fn set_tool_active(id: u64, active: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let tool = state.tools.get_mut(&id).ok_or(KernelError::NotFound)?;
    tool.active = active;
    Ok(())
}

/// List registered tools.
pub fn list_tools() -> Vec<AccessibilityTool> {
    STATE.lock().tools.values().cloned().collect()
}

// ---------------------------------------------------------------------------
// UI element tree
// ---------------------------------------------------------------------------

/// Register a UI element.
pub fn register_element(
    window_id: u64,
    role: ElementRole,
    name: &str,
    bounds: (i32, i32, u32, u32),
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.elements.len() >= MAX_ELEMENTS {
        return Err(KernelError::ResourceExhausted);
    }
    let id = state.next_element_id;
    state.next_element_id = state.next_element_id.wrapping_add(1);
    state.elements.insert(id, AccessibleElement {
        id,
        window_id,
        role,
        name: String::from(name),
        description: String::new(),
        focused: false,
        enabled: true,
        value: String::new(),
        bounds,
    });
    Ok(id)
}

/// Remove a UI element.
pub fn remove_element(id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.elements.remove(&id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Update element properties.
pub fn update_element(id: u64, name: Option<&str>, value: Option<&str>, enabled: Option<bool>) -> KernelResult<()> {
    let mut state = STATE.lock();
    let elem = state.elements.get_mut(&id).ok_or(KernelError::NotFound)?;
    if let Some(n) = name { elem.name = String::from(n); }
    if let Some(v) = value { elem.value = String::from(v); }
    if let Some(e) = enabled { elem.enabled = e; }
    Ok(())
}

/// Set focus on an element.
pub fn set_focus(element_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Unfocus previous.
    let prev = state.focused_element;
    if let Some(e) = state.elements.get_mut(&prev) {
        e.focused = false;
    }
    // Focus new — collect announcement text before releasing borrow.
    let announce_text = {
        let elem = state.elements.get_mut(&element_id).ok_or(KernelError::NotFound)?;
        elem.focused = true;
        // Collect data we need for announcement while we hold the borrow.
        let text = alloc::format!("{}: {}", elem.role.label(), elem.name);
        text
    };
    state.focused_element = element_id;

    // Auto-announce focused element if screen reader is active.
    if state.config.screen_reader_active {
        let aid = state.next_announce_id;
        state.next_announce_id = state.next_announce_id.wrapping_add(1);
        if state.announcements.len() >= MAX_ANNOUNCEMENTS {
            state.announcements.remove(0);
        }
        state.announcements.push(Announcement {
            id: aid,
            text: announce_text,
            priority: AnnouncePriority::Normal,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        ANNOUNCE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

/// Get the focused element.
pub fn focused_element() -> Option<AccessibleElement> {
    let state = STATE.lock();
    let id = state.focused_element;
    state.elements.get(&id).cloned()
}

/// Query elements in a window.
pub fn elements_in_window(window_id: u64) -> Vec<AccessibleElement> {
    let state = STATE.lock();
    state.elements.values()
        .filter(|e| e.window_id == window_id)
        .cloned()
        .collect()
}

/// Hit-test: find element at coordinates.
pub fn element_at(x: i32, y: i32) -> Option<AccessibleElement> {
    let state = STATE.lock();
    // Return the smallest element containing the point.
    let mut best: Option<&AccessibleElement> = None;
    let mut best_area = u64::MAX;
    for e in state.elements.values() {
        let (ex, ey, ew, eh) = e.bounds;
        if x >= ex && y >= ey && x < ex.saturating_add(ew as i32) && y < ey.saturating_add(eh as i32) {
            let area = (ew as u64).saturating_mul(eh as u64);
            if area < best_area {
                best = Some(e);
                best_area = area;
            }
        }
    }
    best.cloned()
}

// ---------------------------------------------------------------------------
// Input injection
// ---------------------------------------------------------------------------

/// Inject a synthetic key press (for automation / accessibility).
pub fn inject_key(keycode: u16, pressed: bool) -> KernelResult<()> {
    // In a real system, this would inject into the input event queue.
    // Here we just track the operation.
    let _ = (keycode, pressed);
    INJECT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Inject a synthetic mouse click.
pub fn inject_click(x: i32, y: i32, button: u8) -> KernelResult<()> {
    let _ = (x, y, button);
    INJECT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Inject synthetic text input.
pub fn inject_text(text: &str) -> KernelResult<()> {
    let _ = text;
    INJECT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Announcements
// ---------------------------------------------------------------------------

/// Queue a text announcement for the screen reader.
pub fn announce(text: &str, priority: AnnouncePriority) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let id = state.next_announce_id;
    state.next_announce_id = state.next_announce_id.wrapping_add(1);
    if state.announcements.len() >= MAX_ANNOUNCEMENTS {
        state.announcements.remove(0);
    }
    state.announcements.push(Announcement {
        id,
        text: String::from(text),
        priority,
        timestamp_ns: crate::hpet::elapsed_ns(),
    });
    ANNOUNCE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Get pending announcements.
pub fn pending_announcements() -> Vec<Announcement> {
    STATE.lock().announcements.clone()
}

/// Clear announcements.
pub fn clear_announcements() {
    STATE.lock().announcements.clear();
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current config.
pub fn config() -> A11yConfig {
    STATE.lock().config.clone()
}

pub fn set_high_contrast(v: bool) { STATE.lock().config.high_contrast = v; }
pub fn set_reduce_motion(v: bool) { STATE.lock().config.reduce_motion = v; }
pub fn set_screen_reader(v: bool) { STATE.lock().config.screen_reader_active = v; }
pub fn set_font_scale(v: u32) { STATE.lock().config.font_scale = v.clamp(50, 500); }
pub fn set_sticky_keys(v: bool) { STATE.lock().config.sticky_keys = v; }
pub fn set_filter_keys(v: bool) { STATE.lock().config.filter_keys = v; }
pub fn set_filter_delay(ms: u32) { STATE.lock().config.filter_delay_ms = ms; }
pub fn set_mouse_keys(v: bool) { STATE.lock().config.mouse_keys = v; }
pub fn set_cursor_scale(v: u32) { STATE.lock().config.cursor_scale = v.clamp(50, 500); }
pub fn set_visual_alerts(v: bool) { STATE.lock().config.visual_alerts = v; }
pub fn set_captions(v: bool) { STATE.lock().config.captions = v; }

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (tool_count, element_count, inject_count, announce_count).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    (
        state.tools.len(),
        state.elements.len(),
        INJECT_COUNT.load(Ordering::Relaxed),
        ANNOUNCE_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    INJECT_COUNT.store(0, Ordering::Relaxed);
    ANNOUNCE_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.tools.clear();
    state.elements.clear();
    state.announcements.clear();
    state.next_tool_id = 1;
    state.next_element_id = 1;
    state.next_announce_id = 1;
    state.sticky_held.clear();
    state.focused_element = 0;
    state.config = A11yConfig::default();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Register tools.
    serial_println!("  a11y::self_test 1: register tools");
    let sr = register_tool(ToolKind::ScreenReader, "Narrator", true, true)?;
    let mag = register_tool(ToolKind::Magnifier, "Magnifier", false, true)?;
    assert_eq!(list_tools().len(), 2);
    set_tool_active(sr, false)?;
    assert!(!list_tools().iter().find(|t| t.id == sr).unwrap().active);
    set_tool_active(sr, true)?;

    // Test 2: Register UI elements.
    serial_println!("  a11y::self_test 2: UI elements");
    let btn = register_element(1, ElementRole::Button, "OK", (100, 200, 80, 30))?;
    let txt = register_element(1, ElementRole::TextInput, "Name", (100, 250, 200, 30))?;
    let lbl = register_element(1, ElementRole::Label, "Enter your name:", (100, 230, 200, 20))?;
    assert_eq!(elements_in_window(1).len(), 3);

    // Test 3: Focus and hit-test.
    serial_println!("  a11y::self_test 3: focus and hit-test");
    set_focus(btn)?;
    let f = focused_element();
    assert!(f.is_some());
    assert_eq!(f.unwrap().role, ElementRole::Button);
    let hit = element_at(110, 210);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().id, btn);

    // Test 4: Input injection.
    serial_println!("  a11y::self_test 4: input injection");
    inject_key(0x1C, true)?; // Enter press
    inject_key(0x1C, false)?; // Enter release
    inject_click(150, 260, 1)?; // Left click
    inject_text("Hello")?;

    // Test 5: Announcements.
    serial_println!("  a11y::self_test 5: announcements");
    set_screen_reader(true);
    announce("Welcome to the application", AnnouncePriority::Normal)?;
    announce("Error: file not found", AnnouncePriority::Alert)?;
    let pending = pending_announcements();
    assert!(pending.len() >= 2);

    // Test 6: Configuration.
    serial_println!("  a11y::self_test 6: config");
    set_high_contrast(true);
    set_reduce_motion(true);
    set_font_scale(150);
    set_sticky_keys(true);
    set_cursor_scale(200);
    let cfg = config();
    assert!(cfg.high_contrast);
    assert!(cfg.reduce_motion);
    assert_eq!(cfg.font_scale, 150);
    assert!(cfg.sticky_keys);
    assert_eq!(cfg.cursor_scale, 200);

    // Test 7: Update and remove.
    serial_println!("  a11y::self_test 7: update/remove");
    update_element(txt, Some("Full Name"), Some("John"), Some(true))?;
    let elems = elements_in_window(1);
    let updated = elems.iter().find(|e| e.id == txt).unwrap();
    assert_eq!(updated.name, "Full Name");
    assert_eq!(updated.value, "John");
    remove_element(lbl)?;
    assert_eq!(elements_in_window(1).len(), 2);
    unregister_tool(mag)?;
    assert_eq!(list_tools().len(), 1);

    let (tc, ec, ic, ac) = stats();
    assert_eq!(tc, 1);
    assert_eq!(ec, 2);
    assert!(ic > 0);
    assert!(ac > 0);

    clear_all();
    reset_stats();
    serial_println!("  a11y: all tests passed");
    Ok(())
}
