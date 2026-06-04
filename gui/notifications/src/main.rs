//! OurOS Notification Daemon
//!
//! Manages system and application notifications with toast display,
//! a notification center UI, and Do Not Disturb scheduling. Communicates
//! with applications via the notification service protocol (IPC channels).
//!
//! # Architecture
//!
//! ```text
//! Applications ──(IPC)──► NotificationDaemon
//!                              │
//!                    ┌─────────┼─────────┐
//!                    ▼         ▼         ▼
//!              Toast Overlay  History  DND Engine
//!                    │         │
//!                    └────┬────┘
//!                         ▼
//!                   RenderTree → Compositor
//! ```

#[allow(unused_imports)]
use guitk::{
    Color, Event, KeyEvent, MouseButton, MouseEvent,
    RenderCommand, RenderTree,
};
#[allow(unused_imports)]
use guitk::event::{EventResult, Modifiers, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::FontWeightHint;
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Catppuccin Mocha theme colors
// ---------------------------------------------------------------------------

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
#[allow(dead_code)]
const GREEN: Color = Color::from_hex(0xA6E3A1);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const RED: Color = Color::from_hex(0xF38BA8);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
#[allow(dead_code)]
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const TOAST_WIDTH: f32 = 360.0;
const TOAST_MIN_HEIGHT: f32 = 80.0;
const TOAST_PADDING: f32 = 16.0;
const TOAST_MARGIN: f32 = 8.0;
const TOAST_CORNER_RADIUS: f32 = 12.0;
const TOAST_SHADOW_BLUR: f32 = 12.0;
const TOAST_RIGHT_MARGIN: f32 = 16.0;
const TOAST_TOP_MARGIN: f32 = 48.0;
const MAX_VISIBLE_TOASTS: usize = 4;
const CLOSE_BTN_SIZE: f32 = 20.0;
const PROGRESS_BAR_HEIGHT: f32 = 4.0;
const ACTION_BTN_HEIGHT: f32 = 28.0;
const ACTION_BTN_PADDING: f32 = 12.0;

const CENTER_WIDTH: f32 = 400.0;
const CENTER_HEADER_HEIGHT: f32 = 48.0;
const CENTER_ITEM_HEIGHT: f32 = 72.0;
const CENTER_GROUP_HEADER_HEIGHT: f32 = 36.0;

const ANIMATION_DURATION_MS: u64 = 250;

// ---------------------------------------------------------------------------
// Notification types
// ---------------------------------------------------------------------------

/// Priority level determines auto-dismiss timing and DND bypass behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl NotificationPriority {
    /// Auto-dismiss timeout in milliseconds. `None` means never auto-dismiss.
    fn timeout_ms(self) -> Option<u64> {
        match self {
            Self::Low => Some(3_000),
            Self::Normal => Some(5_000),
            Self::High => Some(10_000),
            Self::Critical => None,
        }
    }

    fn accent_color(self) -> Color {
        match self {
            Self::Low => SUBTEXT0,
            Self::Normal => BLUE,
            Self::High => YELLOW,
            Self::Critical => RED,
        }
    }
}

/// Notification category for grouping and filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    System,
    App,
    Message,
    Email,
    Download,
    Update,
    Error,
    Reminder,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::App => "App",
            Self::Message => "Message",
            Self::Email => "Email",
            Self::Download => "Download",
            Self::Update => "Update",
            Self::Error => "Error",
            Self::Reminder => "Reminder",
        }
    }
}

/// An action button displayed on a notification.
#[derive(Clone, Debug)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
}

/// A single notification entry.
#[derive(Clone, Debug)]
pub struct Notification {
    pub id: u64,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub icon_id: Option<u64>,
    pub timestamp_ms: u64,
    pub actions: Vec<NotificationAction>,
    pub category: Category,
    pub progress: Option<u8>,
    pub persistent: bool,
    pub group_key: Option<String>,
    pub read: bool,
}

// ---------------------------------------------------------------------------
// Toast animation state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ToastState {
    notification_id: u64,
    /// Milliseconds since toast was shown.
    age_ms: u64,
    /// Slide-in offset (starts at TOAST_WIDTH, animates to 0).
    slide_offset: f32,
    /// Whether the toast is in the exit animation.
    dismissing: bool,
    /// If dismissing, how many ms into the exit animation.
    dismiss_age_ms: u64,
}

impl ToastState {
    fn new(notification_id: u64) -> Self {
        Self {
            notification_id,
            age_ms: 0,
            slide_offset: TOAST_WIDTH + TOAST_RIGHT_MARGIN,
            dismissing: false,
            dismiss_age_ms: 0,
        }
    }

    fn enter_progress(&self) -> f32 {
        let t = (self.age_ms as f32) / (ANIMATION_DURATION_MS as f32);
        t.clamp(0.0, 1.0)
    }

    fn exit_progress(&self) -> f32 {
        let t = (self.dismiss_age_ms as f32) / (ANIMATION_DURATION_MS as f32);
        t.clamp(0.0, 1.0)
    }

    fn current_offset(&self) -> f32 {
        if self.dismissing {
            let p = ease_out_cubic(self.exit_progress());
            // Slide back out to the right.
            (TOAST_WIDTH + TOAST_RIGHT_MARGIN) * p
        } else {
            let p = ease_out_cubic(self.enter_progress());
            self.slide_offset * (1.0 - p)
        }
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

// ---------------------------------------------------------------------------
// Do Not Disturb
// ---------------------------------------------------------------------------

/// DND schedule (hour-based, 24h clock).
#[derive(Clone, Debug)]
pub struct DndSchedule {
    pub start_hour: u8,
    pub start_minute: u8,
    pub end_hour: u8,
    pub end_minute: u8,
}

impl DndSchedule {
    /// Check if a given time-of-day (in minutes from midnight) falls within
    /// the DND window.
    fn is_active(&self, minutes_from_midnight: u16) -> bool {
        let start = self.start_hour as u16 * 60 + self.start_minute as u16;
        let end = self.end_hour as u16 * 60 + self.end_minute as u16;
        if start <= end {
            // Same-day window (e.g. 08:00 - 17:00)
            minutes_from_midnight >= start && minutes_from_midnight < end
        } else {
            // Overnight window (e.g. 22:00 - 07:00)
            minutes_from_midnight >= start || minutes_from_midnight < end
        }
    }
}

#[derive(Clone, Debug)]
pub struct DndState {
    pub enabled: bool,
    pub schedule: Option<DndSchedule>,
    /// Critical notifications always bypass DND.
    pub bypass_critical: bool,
}

impl Default for DndState {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: None,
            bypass_critical: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-app notification settings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct AppSettings {
    pub enabled: bool,
    pub min_priority: NotificationPriority,
    pub sound_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            min_priority: NotificationPriority::Low,
            sound_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Service protocol — request / response
// ---------------------------------------------------------------------------

/// Requests that applications can send to the notification daemon.
#[derive(Clone, Debug)]
pub enum NotificationRequest {
    Send(Notification),
    Update { id: u64, title: Option<String>, body: Option<String>, progress: Option<u8> },
    Dismiss { id: u64 },
    DismissAll { app_name: String },
    GetActive,
    GetHistory { limit: usize },
    SetDnd { enabled: bool },
    SetAppSettings { app_name: String, settings: AppSettings },
}

/// Responses returned to applications.
#[derive(Clone, Debug)]
pub enum NotificationResponse {
    /// A notification was created; returns its ID.
    Created { id: u64 },
    /// Operation succeeded with no additional data.
    Ok,
    /// List of active (visible) toast notification IDs.
    ActiveList(Vec<u64>),
    /// History entries.
    HistoryList(Vec<Notification>),
    /// An error occurred.
    Error(String),
}

// ---------------------------------------------------------------------------
// Notification Center state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CenterState {
    visible: bool,
    scroll_offset: f32,
    /// Which app groups are collapsed (by app_name).
    collapsed_groups: Vec<String>,
}

impl Default for CenterState {
    fn default() -> Self {
        Self {
            visible: false,
            scroll_offset: 0.0,
            collapsed_groups: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Main daemon struct
// ---------------------------------------------------------------------------

/// The notification daemon: manages active toasts, history, DND state,
/// and per-app settings.
pub struct NotificationDaemon {
    /// Active toast overlays (newest first).
    toasts: Vec<ToastState>,
    /// Full notification history (newest first, capped at `max_history`).
    history: Vec<Notification>,
    /// Per-app notification settings.
    app_settings: HashMap<String, AppSettings>,
    /// Do Not Disturb state.
    dnd: DndState,
    /// Notification center panel state.
    center: CenterState,
    /// Next notification ID.
    next_id: u64,
    /// Maximum history entries.
    max_history: usize,
    /// Current time-of-day in minutes from midnight (updated externally).
    current_minutes: u16,
    /// Current timestamp in ms (monotonic, for timeout tracking).
    current_time_ms: u64,
    /// Viewport width (for positioning toasts in top-right).
    viewport_width: f32,
    /// Viewport height.
    viewport_height: f32,
}

impl NotificationDaemon {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            toasts: Vec::new(),
            history: Vec::new(),
            app_settings: HashMap::new(),
            dnd: DndState::default(),
            center: CenterState::default(),
            next_id: 1,
            max_history: 100,
            current_minutes: 0,
            current_time_ms: 0,
            viewport_width,
            viewport_height,
        }
    }

    // -----------------------------------------------------------------------
    // Public service API
    // -----------------------------------------------------------------------

    /// Handle a service request and return a response.
    pub fn handle_request(&mut self, request: NotificationRequest) -> NotificationResponse {
        match request {
            NotificationRequest::Send(mut notif) => {
                notif.id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                let id = notif.id;
                self.add_notification(notif);
                NotificationResponse::Created { id }
            }
            NotificationRequest::Update { id, title, body, progress } => {
                self.update_notification(id, title, body, progress);
                NotificationResponse::Ok
            }
            NotificationRequest::Dismiss { id } => {
                self.dismiss(id);
                NotificationResponse::Ok
            }
            NotificationRequest::DismissAll { ref app_name } => {
                self.dismiss_all_for_app(app_name);
                NotificationResponse::Ok
            }
            NotificationRequest::GetActive => {
                let ids: Vec<u64> = self.toasts.iter()
                    .map(|t| t.notification_id)
                    .collect();
                NotificationResponse::ActiveList(ids)
            }
            NotificationRequest::GetHistory { limit } => {
                let entries: Vec<Notification> = self.history.iter()
                    .take(limit)
                    .cloned()
                    .collect();
                NotificationResponse::HistoryList(entries)
            }
            NotificationRequest::SetDnd { enabled } => {
                self.dnd.enabled = enabled;
                NotificationResponse::Ok
            }
            NotificationRequest::SetAppSettings { app_name, settings } => {
                self.app_settings.insert(app_name, settings);
                NotificationResponse::Ok
            }
        }
    }

    /// Toggle the notification center visibility.
    pub fn toggle_center(&mut self) {
        self.center.visible = !self.center.visible;
    }

    /// Set the current time of day (for DND scheduling).
    pub fn set_time_of_day(&mut self, minutes_from_midnight: u16) {
        self.current_minutes = minutes_from_midnight;
    }

    /// Update viewport dimensions (e.g., on resize).
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    // -----------------------------------------------------------------------
    // Core logic
    // -----------------------------------------------------------------------

    fn add_notification(&mut self, notif: Notification) {
        // Check per-app settings.
        if let Some(settings) = self.app_settings.get(&notif.app_name) {
            if !settings.enabled {
                // Still add to history but don't show toast.
                self.push_history(notif);
                return;
            }
            if notif.priority < settings.min_priority {
                self.push_history(notif);
                return;
            }
        }

        // Check for group_key replacement (progress updates, etc.)
        if let Some(ref key) = notif.group_key {
            // Replace existing toast with same group_key.
            if let Some(existing) = self.find_history_by_group_key(key) {
                let existing_id = existing;
                self.dismiss(existing_id);
            }
        }

        let should_show_toast = self.should_show_toast(&notif);
        let notif_id = notif.id;
        self.push_history(notif);

        if should_show_toast {
            // Cap the number of non-dismissing toasts. Toasts already in the
            // dismissing state still occupy `self.toasts` until the exit
            // animation completes, so they don't count toward the visible cap.
            //
            // We need at most MAX_VISIBLE_TOASTS - 1 non-dismissing toasts
            // before inserting the new one, so the post-insert count stays
            // within the cap. Mark the oldest non-dismissing toasts (last in
            // the Vec; newest is at index 0) as dismissing until we're under.
            let mut non_dismissing = self.toasts.iter().filter(|t| !t.dismissing).count();
            while non_dismissing >= MAX_VISIBLE_TOASTS {
                if let Some(oldest) = self.toasts.iter_mut().rev().find(|t| !t.dismissing) {
                    oldest.dismissing = true;
                    non_dismissing -= 1;
                } else {
                    break;
                }
            }
            self.toasts.insert(0, ToastState::new(notif_id));
        }
    }

    fn should_show_toast(&self, notif: &Notification) -> bool {
        // DND check.
        if self.is_dnd_active() {
            // Critical bypasses DND if configured.
            if self.dnd.bypass_critical && notif.priority == NotificationPriority::Critical {
                return true;
            }
            return false;
        }
        true
    }

    fn update_notification(
        &mut self,
        id: u64,
        title: Option<String>,
        body: Option<String>,
        progress: Option<u8>,
    ) {
        if let Some(notif) = self.history.iter_mut().find(|n| n.id == id) {
            if let Some(t) = title {
                notif.title = t;
            }
            if let Some(b) = body {
                notif.body = b;
            }
            if let Some(p) = progress {
                notif.progress = Some(p.min(100));
            }
        }
    }

    fn dismiss(&mut self, id: u64) {
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.notification_id == id) {
            toast.dismissing = true;
            toast.dismiss_age_ms = 0;
        }
    }

    fn dismiss_all_for_app(&mut self, app_name: &str) {
        let ids_to_dismiss: Vec<u64> = self.history.iter()
            .filter(|n| n.app_name == app_name)
            .map(|n| n.id)
            .collect();
        for id in ids_to_dismiss {
            self.dismiss(id);
        }
    }

    /// Clear all notifications in the center for a specific app.
    pub fn clear_app_history(&mut self, app_name: &str) {
        self.history.retain(|n| n.app_name != app_name);
    }

    /// Clear all notifications.
    pub fn clear_all_history(&mut self) {
        self.history.clear();
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, id: u64) {
        if let Some(notif) = self.history.iter_mut().find(|n| n.id == id) {
            notif.read = true;
        }
    }

    /// Mark a notification as unread.
    pub fn mark_unread(&mut self, id: u64) {
        if let Some(notif) = self.history.iter_mut().find(|n| n.id == id) {
            notif.read = false;
        }
    }

    fn push_history(&mut self, notif: Notification) {
        self.history.insert(0, notif);
        if self.history.len() > self.max_history {
            self.history.truncate(self.max_history);
        }
    }

    fn find_history_by_group_key(&self, key: &str) -> Option<u64> {
        self.history.iter()
            .find(|n| n.group_key.as_deref() == Some(key))
            .map(|n| n.id)
    }

    fn get_notification(&self, id: u64) -> Option<&Notification> {
        self.history.iter().find(|n| n.id == id)
    }

    fn toast_height(&self, notif: &Notification) -> f32 {
        let mut h = TOAST_MIN_HEIGHT;
        if !notif.actions.is_empty() {
            h += ACTION_BTN_HEIGHT + 8.0;
        }
        if notif.progress.is_some() {
            h += PROGRESS_BAR_HEIGHT + 8.0;
        }
        h
    }

    // -----------------------------------------------------------------------
    // Tick — advance animations and timeouts
    // -----------------------------------------------------------------------

    /// Advance the daemon state by `delta_ms` milliseconds.
    pub fn tick(&mut self, delta_ms: u64) {
        self.current_time_ms = self.current_time_ms.wrapping_add(delta_ms);

        // Advance toast animations and check timeouts.
        for toast in &mut self.toasts {
            if toast.dismissing {
                toast.dismiss_age_ms = toast.dismiss_age_ms.saturating_add(delta_ms);
            } else {
                toast.age_ms = toast.age_ms.saturating_add(delta_ms);
            }
        }

        // Auto-dismiss expired toasts.
        for toast in &mut self.toasts {
            if toast.dismissing {
                continue;
            }
            if let Some(notif) = self.history.iter().find(|n| n.id == toast.notification_id) {
                if notif.persistent {
                    continue;
                }
                if let Some(timeout) = notif.priority.timeout_ms()
                    && toast.age_ms >= timeout {
                        toast.dismissing = true;
                        toast.dismiss_age_ms = 0;
                    }
            }
        }

        // Remove toasts that have finished exit animation.
        self.toasts.retain(|t| {
            if t.dismissing && t.dismiss_age_ms >= ANIMATION_DURATION_MS {
                return false;
            }
            true
        });
    }

    // -----------------------------------------------------------------------
    // Event handling
    // -----------------------------------------------------------------------

    /// Handle an input event. Returns the action triggered (if any).
    pub fn handle_event(&mut self, event: &Event) -> Option<DaemonAction> {
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Key(key) => self.handle_key(key),
            Event::Resize { width, height } => {
                self.viewport_width = *width as f32;
                self.viewport_height = *height as f32;
                None
            }
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> Option<DaemonAction> {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Check toast close buttons and bodies.
                if let Some(action) = self.hit_test_toasts(mouse.x, mouse.y) {
                    return Some(action);
                }
                // Check notification center interactions.
                if self.center.visible {
                    return self.hit_test_center(mouse.x, mouse.y);
                }
                None
            }
            MouseEventKind::Scroll { dy, .. } => {
                if self.center.visible {
                    self.center.scroll_offset =
                        (self.center.scroll_offset - dy).max(0.0);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> Option<DaemonAction> {
        if !key.pressed {
            return None;
        }
        // Ctrl+Shift+N toggles notification center.
        if key.modifiers.ctrl && key.modifiers.shift
            && key.key == guitk::event::Key::N {
                self.toggle_center();
                return Some(DaemonAction::CenterToggled(self.center.visible));
            }
        // Escape closes notification center.
        if key.key == guitk::event::Key::Escape && self.center.visible {
            self.center.visible = false;
            return Some(DaemonAction::CenterToggled(false));
        }
        None
    }

    fn hit_test_toasts(&mut self, mx: f32, my: f32) -> Option<DaemonAction> {
        let base_x = self.viewport_width - TOAST_WIDTH - TOAST_RIGHT_MARGIN;
        let mut y = TOAST_TOP_MARGIN;

        // Iterate over visible (non-dismissing) toasts.
        for toast in &self.toasts {
            if toast.dismissing {
                continue;
            }
            let offset = toast.current_offset();
            let toast_x = base_x + offset;
            let notif = match self.get_notification(toast.notification_id) {
                Some(n) => n,
                None => continue,
            };
            let h = self.toast_height(notif);

            // Is the click within this toast's bounds?
            if mx >= toast_x && mx <= toast_x + TOAST_WIDTH
                && my >= y && my <= y + h
            {
                // Check close button (top-right corner of toast).
                let close_x = toast_x + TOAST_WIDTH - TOAST_PADDING - CLOSE_BTN_SIZE;
                let close_y = y + TOAST_PADDING;
                if mx >= close_x && mx <= close_x + CLOSE_BTN_SIZE
                    && my >= close_y && my <= close_y + CLOSE_BTN_SIZE
                {
                    let id = toast.notification_id;
                    self.dismiss(id);
                    return Some(DaemonAction::Dismissed(id));
                }

                // Click on notification body → primary action.
                let id = toast.notification_id;
                let primary_action = notif.actions.first().map(|a| a.id.clone());
                self.dismiss(id);
                self.mark_read(id);
                return Some(DaemonAction::ActionInvoked {
                    notification_id: id,
                    action_id: primary_action,
                });
            }

            y += h + TOAST_MARGIN;
        }
        None
    }

    fn hit_test_center(&mut self, mx: f32, my: f32) -> Option<DaemonAction> {
        let center_x = self.viewport_width - CENTER_WIDTH;
        let center_y = 0.0_f32;

        // Not within center panel bounds.
        if mx < center_x || mx > self.viewport_width {
            return None;
        }

        // "Clear all" button in header.
        let clear_all_x = center_x + CENTER_WIDTH - 80.0;
        let clear_all_y = center_y + 8.0;
        if mx >= clear_all_x && mx <= clear_all_x + 72.0
            && my >= clear_all_y && my <= clear_all_y + 32.0
        {
            self.clear_all_history();
            return Some(DaemonAction::AllCleared);
        }

        // Items in the scrollable area.
        let content_y = center_y + CENTER_HEADER_HEIGHT - self.center.scroll_offset;
        let mut item_y = content_y;

        let groups = self.grouped_history();
        // Pre-collect collapsed state and flatten hit targets to avoid borrow conflicts.
        let collapsed: Vec<String> = self.center.collapsed_groups.clone();

        // First pass: find what was clicked without mutating self.
        let mut toggle_group: Option<String> = None;
        let mut click_notif: Option<(u64, Option<String>)> = None;

        for (app_name, notifications) in &groups {
            if my >= item_y && my < item_y + CENTER_GROUP_HEADER_HEIGHT {
                toggle_group = Some(app_name.clone());
                break;
            }
            item_y += CENTER_GROUP_HEADER_HEIGHT;

            if collapsed.contains(app_name) {
                continue;
            }

            for notif in notifications {
                if my >= item_y && my < item_y + CENTER_ITEM_HEIGHT {
                    click_notif = Some((notif.id, notif.actions.first().map(|a| a.id.clone())));
                    break;
                }
                item_y += CENTER_ITEM_HEIGHT;
            }
            if click_notif.is_some() {
                break;
            }
        }

        // Now apply mutations based on what was found.
        if let Some(name) = toggle_group {
            if collapsed.contains(&name) {
                self.center.collapsed_groups.retain(|g| g != &name);
            } else {
                self.center.collapsed_groups.push(name.clone());
            }
            return Some(DaemonAction::GroupToggled(name));
        }

        if let Some((id, primary_action)) = click_notif {
            self.mark_read(id);
            return Some(DaemonAction::ActionInvoked {
                notification_id: id,
                action_id: primary_action,
            });
        }

        None
    }

    fn grouped_history(&self) -> Vec<(String, Vec<&Notification>)> {
        let mut groups: Vec<(String, Vec<&Notification>)> = Vec::new();
        for notif in &self.history {
            if let Some(group) = groups.iter_mut().find(|(name, _)| *name == notif.app_name) {
                group.1.push(notif);
            } else {
                groups.push((notif.app_name.clone(), vec![notif]));
            }
        }
        groups
    }

    // -----------------------------------------------------------------------
    // Rendering — Toast Overlay
    // -----------------------------------------------------------------------

    /// Render the toast overlay (always-on-top layer).
    pub fn render_toasts(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let base_x = self.viewport_width - TOAST_WIDTH - TOAST_RIGHT_MARGIN;
        let mut y = TOAST_TOP_MARGIN;

        let visible_count = self.toasts.iter()
            .filter(|t| !t.dismissing || t.exit_progress() < 1.0)
            .count()
            .min(MAX_VISIBLE_TOASTS);

        for toast in self.toasts.iter().take(visible_count) {
            let notif = match self.get_notification(toast.notification_id) {
                Some(n) => n,
                None => continue,
            };

            let offset = toast.current_offset();
            let toast_x = base_x + offset;
            let h = self.toast_height(notif);
            let radii = CornerRadii::all(TOAST_CORNER_RADIUS);

            // Shadow.
            cmds.push(RenderCommand::BoxShadow {
                x: toast_x,
                y,
                width: TOAST_WIDTH,
                height: h,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: TOAST_SHADOW_BLUR,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 80),
                corner_radii: radii,
            });

            // Background.
            cmds.push(RenderCommand::FillRect {
                x: toast_x,
                y,
                width: TOAST_WIDTH,
                height: h,
                color: SURFACE0,
                corner_radii: radii,
            });

            // Priority accent stripe (left edge).
            cmds.push(RenderCommand::FillRect {
                x: toast_x,
                y,
                width: 4.0,
                height: h,
                color: notif.priority.accent_color(),
                corner_radii: CornerRadii {
                    top_left: TOAST_CORNER_RADIUS,
                    bottom_left: TOAST_CORNER_RADIUS,
                    top_right: 0.0,
                    bottom_right: 0.0,
                },
            });

            // App name (small, dimmed).
            cmds.push(RenderCommand::Text {
                x: toast_x + TOAST_PADDING + 8.0,
                y: y + TOAST_PADDING,
                text: notif.app_name.clone(),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(TOAST_WIDTH - TOAST_PADDING * 2.0 - CLOSE_BTN_SIZE - 16.0),
            });

            // Title (bold).
            cmds.push(RenderCommand::Text {
                x: toast_x + TOAST_PADDING + 8.0,
                y: y + TOAST_PADDING + 16.0,
                text: notif.title.clone(),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(TOAST_WIDTH - TOAST_PADDING * 2.0 - CLOSE_BTN_SIZE - 16.0),
            });

            // Body text.
            cmds.push(RenderCommand::Text {
                x: toast_x + TOAST_PADDING + 8.0,
                y: y + TOAST_PADDING + 34.0,
                text: notif.body.clone(),
                color: SUBTEXT1,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(TOAST_WIDTH - TOAST_PADDING * 2.0 - 16.0),
            });

            // Close button (X).
            let close_x = toast_x + TOAST_WIDTH - TOAST_PADDING - CLOSE_BTN_SIZE;
            let close_y = y + TOAST_PADDING;
            cmds.push(RenderCommand::FillRect {
                x: close_x,
                y: close_y,
                width: CLOSE_BTN_SIZE,
                height: CLOSE_BTN_SIZE,
                color: SURFACE1,
                corner_radii: CornerRadii::all(CLOSE_BTN_SIZE / 2.0),
            });
            cmds.push(RenderCommand::Text {
                x: close_x + 5.0,
                y: close_y + 3.0,
                text: String::from("x"),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Progress bar (if set).
            let mut extra_y = y + TOAST_PADDING + 52.0;
            if let Some(progress) = notif.progress {
                let bar_y = extra_y;
                let bar_width = TOAST_WIDTH - TOAST_PADDING * 2.0 - 16.0;
                // Background track.
                cmds.push(RenderCommand::FillRect {
                    x: toast_x + TOAST_PADDING + 8.0,
                    y: bar_y,
                    width: bar_width,
                    height: PROGRESS_BAR_HEIGHT,
                    color: SURFACE2,
                    corner_radii: CornerRadii::all(2.0),
                });
                // Filled portion.
                let fill_width = bar_width * (progress as f32 / 100.0);
                cmds.push(RenderCommand::FillRect {
                    x: toast_x + TOAST_PADDING + 8.0,
                    y: bar_y,
                    width: fill_width,
                    height: PROGRESS_BAR_HEIGHT,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
                extra_y += PROGRESS_BAR_HEIGHT + 8.0;
            }

            // Action buttons.
            if !notif.actions.is_empty() {
                let mut btn_x = toast_x + TOAST_PADDING + 8.0;
                for action in &notif.actions {
                    let btn_width = action.label.len() as f32 * 7.0 + ACTION_BTN_PADDING * 2.0;
                    cmds.push(RenderCommand::FillRect {
                        x: btn_x,
                        y: extra_y,
                        width: btn_width,
                        height: ACTION_BTN_HEIGHT,
                        color: SURFACE1,
                        corner_radii: CornerRadii::all(6.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: btn_x + ACTION_BTN_PADDING,
                        y: extra_y + 7.0,
                        text: action.label.clone(),
                        color: BLUE,
                        font_size: 12.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                    btn_x += btn_width + 8.0;
                }
            }

            y += h + TOAST_MARGIN;
        }

        cmds
    }

    // -----------------------------------------------------------------------
    // Rendering — Notification Center
    // -----------------------------------------------------------------------

    /// Render the notification center panel (right-side slide-out).
    pub fn render_center(&self) -> Vec<RenderCommand> {
        if !self.center.visible {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let center_x = self.viewport_width - CENTER_WIDTH;

        // Background overlay (semi-transparent backdrop for the panel area).
        cmds.push(RenderCommand::FillRect {
            x: center_x,
            y: 0.0,
            width: CENTER_WIDTH,
            height: self.viewport_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header.
        cmds.push(RenderCommand::FillRect {
            x: center_x,
            y: 0.0,
            width: CENTER_WIDTH,
            height: CENTER_HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: center_x + 16.0,
            y: 14.0,
            text: String::from("Notifications"),
            color: TEXT,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Unread count badge.
        let unread_count = self.history.iter().filter(|n| !n.read).count();
        if unread_count > 0 {
            let badge_text = format!("{unread_count}");
            let badge_width = badge_text.len() as f32 * 8.0 + 12.0;
            cmds.push(RenderCommand::FillRect {
                x: center_x + 140.0,
                y: 12.0,
                width: badge_width,
                height: 22.0,
                color: MAUVE,
                corner_radii: CornerRadii::all(11.0),
            });
            cmds.push(RenderCommand::Text {
                x: center_x + 140.0 + 6.0,
                y: 15.0,
                text: badge_text,
                color: CRUST,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // "Clear all" button.
        let clear_x = center_x + CENTER_WIDTH - 80.0;
        cmds.push(RenderCommand::FillRect {
            x: clear_x,
            y: 8.0,
            width: 72.0,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: clear_x + 10.0,
            y: 16.0,
            text: String::from("Clear all"),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Clip scrollable area.
        cmds.push(RenderCommand::PushClip {
            x: center_x,
            y: CENTER_HEADER_HEIGHT,
            width: CENTER_WIDTH,
            height: self.viewport_height - CENTER_HEADER_HEIGHT,
        });
        cmds.push(RenderCommand::PushTranslate {
            dx: 0.0,
            dy: -self.center.scroll_offset,
        });

        // Render grouped notifications.
        let mut item_y = CENTER_HEADER_HEIGHT;
        let groups = self.grouped_history();

        if groups.is_empty() {
            // Empty state.
            cmds.push(RenderCommand::Text {
                x: center_x + CENTER_WIDTH / 2.0 - 60.0,
                y: item_y + 40.0,
                text: String::from("No notifications"),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        for (app_name, notifications) in &groups {
            // Group header.
            cmds.push(RenderCommand::FillRect {
                x: center_x,
                y: item_y,
                width: CENTER_WIDTH,
                height: CENTER_GROUP_HEADER_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });

            let collapse_indicator = if self.center.collapsed_groups.contains(app_name) {
                ">"
            } else {
                "v"
            };
            cmds.push(RenderCommand::Text {
                x: center_x + 12.0,
                y: item_y + 10.0,
                text: format!("{collapse_indicator} {app_name}"),
                color: TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Group notification count.
            let count_text = format!("{}", notifications.len());
            cmds.push(RenderCommand::Text {
                x: center_x + CENTER_WIDTH - 40.0,
                y: item_y + 10.0,
                text: count_text,
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            item_y += CENTER_GROUP_HEADER_HEIGHT;

            // Skip items if collapsed.
            if self.center.collapsed_groups.contains(app_name) {
                continue;
            }

            for notif in notifications {
                self.render_center_item(&mut cmds, center_x, item_y, notif);
                item_y += CENTER_ITEM_HEIGHT;
            }
        }

        cmds.push(RenderCommand::PopTranslate);
        cmds.push(RenderCommand::PopClip);

        cmds
    }

    fn render_center_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        center_x: f32,
        y: f32,
        notif: &Notification,
    ) {
        // Item background (slightly different for unread).
        let bg_color = if notif.read { MANTLE } else { BASE };
        cmds.push(RenderCommand::FillRect {
            x: center_x,
            y,
            width: CENTER_WIDTH,
            height: CENTER_ITEM_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Unread indicator dot.
        if !notif.read {
            cmds.push(RenderCommand::FillRect {
                x: center_x + 6.0,
                y: y + CENTER_ITEM_HEIGHT / 2.0 - 3.0,
                width: 6.0,
                height: 6.0,
                color: BLUE,
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Category/priority accent.
        cmds.push(RenderCommand::FillRect {
            x: center_x,
            y,
            width: 3.0,
            height: CENTER_ITEM_HEIGHT,
            color: notif.priority.accent_color(),
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: center_x + 20.0,
            y: y + 10.0,
            text: notif.title.clone(),
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(CENTER_WIDTH - 100.0),
        });

        // Body (truncated).
        let body_display = if notif.body.len() > 60 {
            let truncated: String = notif.body.chars().take(57).collect();
            format!("{truncated}...")
        } else {
            notif.body.clone()
        };
        cmds.push(RenderCommand::Text {
            x: center_x + 20.0,
            y: y + 28.0,
            text: body_display,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CENTER_WIDTH - 40.0),
        });

        // Relative timestamp.
        let time_text = self.format_relative_time(notif.timestamp_ms);
        cmds.push(RenderCommand::Text {
            x: center_x + CENTER_WIDTH - 80.0,
            y: y + 10.0,
            text: time_text,
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Category label.
        cmds.push(RenderCommand::Text {
            x: center_x + 20.0,
            y: y + CENTER_ITEM_HEIGHT - 18.0,
            text: String::from(notif.category.label()),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: center_x + 16.0,
            y1: y + CENTER_ITEM_HEIGHT - 1.0,
            x2: center_x + CENTER_WIDTH - 16.0,
            y2: y + CENTER_ITEM_HEIGHT - 1.0,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn format_relative_time(&self, timestamp_ms: u64) -> String {
        if self.current_time_ms < timestamp_ms {
            return String::from("now");
        }
        let elapsed_ms = self.current_time_ms.saturating_sub(timestamp_ms);
        let seconds = elapsed_ms / 1000;
        let minutes = seconds / 60;
        let hours = minutes / 60;
        let days = hours / 24;

        if seconds < 60 {
            String::from("now")
        } else if minutes < 60 {
            format!("{minutes} min ago")
        } else if hours < 24 {
            if hours == 1 {
                String::from("1 hour ago")
            } else {
                format!("{hours} hours ago")
            }
        } else if days == 1 {
            String::from("Yesterday")
        } else {
            format!("{days} days ago")
        }
    }

    // -----------------------------------------------------------------------
    // Combined rendering
    // -----------------------------------------------------------------------

    /// Produce the full render tree for the notification overlay.
    /// The compositor should draw this on top of all other windows.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Toast overlay (always rendered).
        for cmd in self.render_toasts() {
            tree.push(cmd);
        }

        // Notification center (rendered when visible).
        for cmd in self.render_center() {
            tree.push(cmd);
        }

        tree
    }

    // -----------------------------------------------------------------------
    // DND convenience methods
    // -----------------------------------------------------------------------

    /// Set a DND schedule.
    pub fn set_dnd_schedule(&mut self, start_hour: u8, start_minute: u8, end_hour: u8, end_minute: u8) {
        self.dnd.schedule = Some(DndSchedule {
            start_hour,
            start_minute,
            end_hour,
            end_minute,
        });
    }

    /// Clear the DND schedule.
    pub fn clear_dnd_schedule(&mut self) {
        self.dnd.schedule = None;
    }

    /// Check whether DND is currently effective.
    pub fn is_dnd_active(&self) -> bool {
        if self.dnd.enabled {
            return true;
        }
        if let Some(ref schedule) = self.dnd.schedule {
            return schedule.is_active(self.current_minutes);
        }
        false
    }

    /// Get the number of unread notifications.
    pub fn unread_count(&self) -> usize {
        self.history.iter().filter(|n| !n.read).count()
    }

    /// Whether the notification center is currently visible.
    pub fn is_center_visible(&self) -> bool {
        self.center.visible
    }
}

// ---------------------------------------------------------------------------
// Actions emitted by the daemon in response to user interaction
// ---------------------------------------------------------------------------

/// Actions emitted when the user interacts with notifications.
#[derive(Clone, Debug)]
pub enum DaemonAction {
    /// A notification was dismissed (by close button or auto-timeout).
    Dismissed(u64),
    /// A notification action was invoked (click on body or action button).
    ActionInvoked {
        notification_id: u64,
        action_id: Option<String>,
    },
    /// The notification center was toggled.
    CenterToggled(bool),
    /// A group was collapsed/expanded.
    GroupToggled(String),
    /// All notifications were cleared.
    AllCleared,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // The notification daemon runs as a system service. On startup it:
    // 1. Registers with the compositor for overlay rendering privileges.
    // 2. Opens an IPC endpoint for receiving notification requests.
    // 3. Enters the main event loop: processing IPC messages, ticking
    //    animations, and rendering the overlay each frame.

    let mut daemon = NotificationDaemon::new(1920.0, 1080.0);

    // Set a default DND schedule (10 PM - 7 AM).
    daemon.set_dnd_schedule(22, 0, 7, 0);

    // Main loop placeholder — in production this integrates with the
    // compositor's event loop and IPC message dispatch.
    //
    // In the real implementation we would:
    // 1. Poll IPC channel for NotificationRequest messages.
    // 2. Handle compositor events (mouse, keyboard, tick).
    // 3. Call daemon.tick(delta_ms) to advance animations.
    // 4. Call daemon.render() and submit to compositor.
    //
    // For now we just return: there is no loop to run without the OS
    // service infrastructure.
    let _ = daemon;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_notification(id_hint: u64, priority: NotificationPriority) -> Notification {
        Notification {
            id: id_hint,
            app_name: String::from("TestApp"),
            title: String::from("Test Title"),
            body: String::from("Test body content"),
            priority,
            icon_id: None,
            timestamp_ms: 1000,
            actions: vec![],
            category: Category::App,
            progress: None,
            persistent: false,
            group_key: None,
            read: false,
        }
    }

    #[test]
    fn test_send_notification_creates_toast() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        let resp = daemon.handle_request(NotificationRequest::Send(notif));
        match resp {
            NotificationResponse::Created { id } => assert_eq!(id, 1),
            _ => panic!("Expected Created response"),
        }
        assert_eq!(daemon.toasts.len(), 1);
        assert_eq!(daemon.history.len(), 1);
    }

    #[test]
    fn test_dismiss_notification() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        daemon.handle_request(NotificationRequest::Dismiss { id: 1 });
        assert!(daemon.toasts[0].dismissing);
    }

    #[test]
    fn test_auto_dismiss_on_timeout() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Low);
        daemon.handle_request(NotificationRequest::Send(notif));
        // Low priority = 3000ms timeout.
        daemon.tick(3001);
        assert!(daemon.toasts[0].dismissing);
    }

    #[test]
    fn test_critical_never_auto_dismisses() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Critical);
        daemon.handle_request(NotificationRequest::Send(notif));
        daemon.tick(100_000);
        assert!(!daemon.toasts[0].dismissing);
    }

    #[test]
    fn test_dnd_suppresses_toast() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.dnd.enabled = true;
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        // Should be in history but no toast.
        assert_eq!(daemon.toasts.len(), 0);
        assert_eq!(daemon.history.len(), 1);
    }

    #[test]
    fn test_dnd_critical_bypass() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.dnd.enabled = true;
        daemon.dnd.bypass_critical = true;
        let notif = make_test_notification(0, NotificationPriority::Critical);
        daemon.handle_request(NotificationRequest::Send(notif));
        // Critical should bypass DND.
        assert_eq!(daemon.toasts.len(), 1);
    }

    #[test]
    fn test_dnd_schedule_overnight() {
        let schedule = DndSchedule {
            start_hour: 22,
            start_minute: 0,
            end_hour: 7,
            end_minute: 0,
        };
        // 23:00 = 1380 minutes -> should be active.
        assert!(schedule.is_active(1380));
        // 02:00 = 120 minutes -> should be active.
        assert!(schedule.is_active(120));
        // 12:00 = 720 minutes -> should NOT be active.
        assert!(!schedule.is_active(720));
    }

    #[test]
    fn test_max_visible_toasts() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        for _ in 0..6 {
            let notif = make_test_notification(0, NotificationPriority::Normal);
            daemon.handle_request(NotificationRequest::Send(notif));
        }
        // Should trigger dismissal of oldest when over MAX_VISIBLE_TOASTS.
        let non_dismissing = daemon.toasts.iter()
            .filter(|t| !t.dismissing)
            .count();
        assert!(non_dismissing <= MAX_VISIBLE_TOASTS);
    }

    #[test]
    fn test_group_key_replacement() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let mut notif1 = make_test_notification(0, NotificationPriority::Normal);
        notif1.group_key = Some(String::from("download-1"));
        notif1.progress = Some(50);
        daemon.handle_request(NotificationRequest::Send(notif1));

        let mut notif2 = make_test_notification(0, NotificationPriority::Normal);
        notif2.group_key = Some(String::from("download-1"));
        notif2.progress = Some(75);
        daemon.handle_request(NotificationRequest::Send(notif2));

        // First toast should be dismissing (replaced by second).
        let dismissing_count = daemon.toasts.iter().filter(|t| t.dismissing).count();
        assert!(dismissing_count >= 1);
    }

    #[test]
    fn test_update_notification_progress() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let mut notif = make_test_notification(0, NotificationPriority::Normal);
        notif.progress = Some(10);
        daemon.handle_request(NotificationRequest::Send(notif));
        daemon.handle_request(NotificationRequest::Update {
            id: 1,
            title: None,
            body: None,
            progress: Some(90),
        });
        assert_eq!(daemon.history[0].progress, Some(90));
    }

    #[test]
    fn test_app_settings_filter() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.handle_request(NotificationRequest::SetAppSettings {
            app_name: String::from("TestApp"),
            settings: AppSettings {
                enabled: true,
                min_priority: NotificationPriority::High,
                sound_enabled: false,
            },
        });
        // Normal priority should be filtered.
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        assert_eq!(daemon.toasts.len(), 0);
        assert_eq!(daemon.history.len(), 1);
    }

    #[test]
    fn test_app_settings_disabled() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.handle_request(NotificationRequest::SetAppSettings {
            app_name: String::from("TestApp"),
            settings: AppSettings {
                enabled: false,
                min_priority: NotificationPriority::Low,
                sound_enabled: true,
            },
        });
        let notif = make_test_notification(0, NotificationPriority::Critical);
        daemon.handle_request(NotificationRequest::Send(notif));
        // Disabled app: no toast, but still in history.
        assert_eq!(daemon.toasts.len(), 0);
        assert_eq!(daemon.history.len(), 1);
    }

    #[test]
    fn test_toggle_center() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        assert!(!daemon.is_center_visible());
        daemon.toggle_center();
        assert!(daemon.is_center_visible());
        daemon.toggle_center();
        assert!(!daemon.is_center_visible());
    }

    #[test]
    fn test_clear_all_history() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        for _ in 0..5 {
            let notif = make_test_notification(0, NotificationPriority::Normal);
            daemon.handle_request(NotificationRequest::Send(notif));
        }
        daemon.clear_all_history();
        assert_eq!(daemon.history.len(), 0);
    }

    #[test]
    fn test_mark_read_unread() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        assert!(!daemon.history[0].read);
        daemon.mark_read(1);
        assert!(daemon.history[0].read);
        daemon.mark_unread(1);
        assert!(!daemon.history[0].read);
    }

    #[test]
    fn test_history_max_capacity() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.max_history = 10;
        for _ in 0..15 {
            let notif = make_test_notification(0, NotificationPriority::Normal);
            daemon.handle_request(NotificationRequest::Send(notif));
        }
        assert_eq!(daemon.history.len(), 10);
    }

    #[test]
    fn test_relative_time_formatting() {
        let daemon = NotificationDaemon::new(1920.0, 1080.0);
        // "now" case.
        let mut d = daemon;
        d.current_time_ms = 10_000;
        assert_eq!(d.format_relative_time(10_000), "now");
        assert_eq!(d.format_relative_time(9_500), "now");
        // Minutes.
        d.current_time_ms = 180_000;
        assert_eq!(d.format_relative_time(0), "3 min ago");
        // Hours.
        d.current_time_ms = 7_200_000;
        assert_eq!(d.format_relative_time(0), "2 hours ago");
        // 1 hour.
        d.current_time_ms = 3_600_000;
        assert_eq!(d.format_relative_time(0), "1 hour ago");
        // Yesterday.
        d.current_time_ms = 86_400_000 + 1000;
        assert_eq!(d.format_relative_time(0), "Yesterday");
    }

    #[test]
    fn test_render_toasts_produces_commands() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        let cmds = daemon.render_toasts();
        // Should produce at least shadow + bg + accent + app_name + title + body + close.
        assert!(cmds.len() >= 7);
    }

    #[test]
    fn test_render_center_empty() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        daemon.center.visible = true;
        let cmds = daemon.render_center();
        // Should have header + empty state text at minimum.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_dismiss_all_for_app() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        for _ in 0..3 {
            let notif = make_test_notification(0, NotificationPriority::Normal);
            daemon.handle_request(NotificationRequest::Send(notif));
        }
        daemon.handle_request(NotificationRequest::DismissAll {
            app_name: String::from("TestApp"),
        });
        let all_dismissing = daemon.toasts.iter().all(|t| t.dismissing);
        assert!(all_dismissing);
    }

    #[test]
    fn test_animation_easing() {
        // ease_out_cubic(0) = 0, ease_out_cubic(1) = 1.
        assert!((ease_out_cubic(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < f32::EPSILON);
        // Monotonically increasing.
        assert!(ease_out_cubic(0.5) > ease_out_cubic(0.25));
    }

    #[test]
    fn test_toast_removal_after_exit_animation() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        daemon.dismiss(1);
        // Advance past animation duration.
        daemon.tick(ANIMATION_DURATION_MS + 1);
        assert_eq!(daemon.toasts.len(), 0);
    }

    #[test]
    fn test_persistent_notification_no_auto_dismiss() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let mut notif = make_test_notification(0, NotificationPriority::Low);
        notif.persistent = true;
        daemon.handle_request(NotificationRequest::Send(notif));
        daemon.tick(100_000);
        assert!(!daemon.toasts[0].dismissing);
    }

    #[test]
    fn test_get_active_returns_ids() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        let notif = make_test_notification(0, NotificationPriority::Normal);
        daemon.handle_request(NotificationRequest::Send(notif));
        let resp = daemon.handle_request(NotificationRequest::GetActive);
        match resp {
            NotificationResponse::ActiveList(ids) => {
                assert_eq!(ids, vec![1]);
            }
            _ => panic!("Expected ActiveList"),
        }
    }

    #[test]
    fn test_get_history_with_limit() {
        let mut daemon = NotificationDaemon::new(1920.0, 1080.0);
        for _ in 0..10 {
            let notif = make_test_notification(0, NotificationPriority::Normal);
            daemon.handle_request(NotificationRequest::Send(notif));
        }
        let resp = daemon.handle_request(NotificationRequest::GetHistory { limit: 3 });
        match resp {
            NotificationResponse::HistoryList(entries) => {
                assert_eq!(entries.len(), 3);
            }
            _ => panic!("Expected HistoryList"),
        }
    }
}
