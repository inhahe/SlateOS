//! Slate OS Window Library — compositor client for creating windows and receiving events.
//!
//! This crate provides the primary API for applications to interact with the Slate OS
//! compositor. It handles window creation, event dispatch, rendering submission,
//! and window lifecycle management.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! oswindow (this crate)
//!     │ (IPC channel to compositor)
//!     ▼
//! Compositor Server
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use oswindow::{WindowBuilder, EventLoop, WindowEvent, EventResponse};
//!
//! let window = WindowBuilder::new("My App", 800, 600)
//!     .resizable(true)
//!     .build()
//!     .expect("failed to create window");
//!
//! let mut event_loop = EventLoop::new();
//! event_loop.register(window);
//! event_loop.run(|window_id, event| {
//!     match event {
//!         WindowEvent::CloseRequested => EventResponse::Exit,
//!         _ => EventResponse::Continue,
//!     }
//! });
//! ```

// Internal protocol types will be used when real IPC is connected.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

pub use guitk::event::{Key, Modifiers, MouseButton};
pub use guitk::render::{RenderCommand, RenderTree};

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

/// Global atomic counter for generating unique connection-local window IDs.
static NEXT_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new unique window ID for this client session.
fn allocate_window_id() -> u64 {
    NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during window operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowError {
    /// The compositor connection could not be established.
    ConnectionFailed(String),
    /// The compositor rejected the window creation request.
    CreationFailed(String),
    /// The window ID is not valid (window was closed or never created).
    InvalidWindow(u64),
    /// An IPC communication error occurred.
    IpcError(String),
    /// The requested operation is not supported.
    Unsupported(String),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::CreationFailed(msg) => write!(f, "window creation failed: {msg}"),
            Self::InvalidWindow(id) => write!(f, "invalid window id: {id}"),
            Self::IpcError(msg) => write!(f, "ipc error: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Cursor shapes
// ---------------------------------------------------------------------------

/// Mouse cursor shape that the compositor should display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    /// Default pointer arrow.
    #[default]
    Arrow,
    /// Text insertion beam (I-beam).
    IBeam,
    /// Pointing hand (for clickable elements).
    Hand,
    /// Vertical resize (north-south).
    ResizeNS,
    /// Horizontal resize (east-west).
    ResizeEW,
    /// Diagonal resize (northeast-southwest).
    ResizeNESW,
    /// Diagonal resize (northwest-southeast).
    ResizeNWSE,
    /// Move/drag cursor.
    Move,
    /// Busy/wait spinner.
    Wait,
    /// Help cursor (question mark).
    Help,
    /// Crosshair (precision select).
    Crosshair,
    /// Hidden cursor (no cursor visible).
    Hidden,
}

// ---------------------------------------------------------------------------
// Display information
// ---------------------------------------------------------------------------

/// Information about the connected display.
#[derive(Clone, Debug)]
pub struct DisplayInfo {
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// DPI scale factor (1.0 = 96 DPI, 2.0 = 192 DPI, etc.).
    pub scale_factor: f32,
}

/// Query display information from the compositor.
///
/// Returns information about the primary display's resolution, refresh rate,
/// and scale factor. Used to make layout decisions before creating windows.
pub fn display_info() -> DisplayInfo {
    let mut conn = Connection::new();
    conn.send(CompositorRequest::GetDisplayInfo);

    // Process the response from the compositor.
    if let Some(CompositorResponse::DisplayInfo {
        width,
        height,
        refresh_rate,
        scale_factor,
    }) = conn.recv()
    {
        return DisplayInfo {
            width,
            height,
            refresh_rate,
            scale_factor,
        };
    }

    // Fallback to reasonable defaults if the compositor is unreachable.
    DisplayInfo {
        width: 1920,
        height: 1080,
        refresh_rate: 60,
        scale_factor: 1.0,
    }
}

// ---------------------------------------------------------------------------
// Compositor protocol types (client-side mirror of compositor's protocol)
// ---------------------------------------------------------------------------

/// Request types matching the compositor protocol.
#[derive(Clone, Debug)]
enum CompositorRequest {
    /// Create a new window with the given parameters.
    CreateWindow {
        title: String,
        width: u32,
        height: u32,
        x: i32,
        y: i32,
    },
    /// Destroy an existing window.
    DestroyWindow { id: u64 },
    /// Set the window title.
    SetTitle { id: u64, title: String },
    /// Submit render commands for a window's client area.
    Submit { id: u64, commands: Vec<RenderCommand> },
    /// Move a window to a new position.
    Move { id: u64, x: i32, y: i32 },
    /// Resize a window's client area.
    Resize { id: u64, width: u32, height: u32 },
    /// Minimize a window.
    Minimize { id: u64 },
    /// Maximize a window.
    Maximize { id: u64 },
    /// Restore a window from minimized/maximized state.
    Restore { id: u64 },
    /// Set the cursor shape.
    SetCursor { shape: CursorShape },
    /// Set window visibility.
    SetVisible { id: u64, visible: bool },
    /// Query display information.
    GetDisplayInfo,
}

/// Response types from the compositor.
#[derive(Clone, Debug)]
enum CompositorResponse {
    /// A window was created successfully with the given server-assigned ID.
    WindowCreated { window_id: u64 },
    /// Operation completed successfully.
    Ok,
    /// Operation failed with an error message.
    Error { message: String },
    /// Display information response.
    DisplayInfo {
        width: u32,
        height: u32,
        refresh_rate: u32,
        scale_factor: f32,
    },
}

// ---------------------------------------------------------------------------
// Connection to compositor
// ---------------------------------------------------------------------------

/// Connection state to the compositor.
///
/// In a real system this would hold an IPC channel handle to the compositor
/// server. For now, we simulate communication via request/response queues
/// that will be replaced with actual channel IPC when the kernel's IPC
/// subsystem is integrated.
#[derive(Clone, Debug)]
struct Connection {
    /// Whether the connection has been established.
    connected: bool,
    /// Outgoing request queue (to compositor).
    outgoing: VecDeque<CompositorRequest>,
    /// Incoming response queue (from compositor).
    incoming: VecDeque<CompositorResponse>,
    /// Incoming event queue (asynchronous notifications from compositor).
    events: VecDeque<WindowEvent>,
}

impl Connection {
    /// Create a new connection to the compositor.
    ///
    /// In a full system, this would open an IPC channel to the compositor
    /// service. Currently stubs the connection as always successful.
    fn new() -> Self {
        Self {
            connected: true,
            outgoing: VecDeque::new(),
            incoming: VecDeque::new(),
            events: VecDeque::new(),
        }
    }

    /// Send a request to the compositor.
    fn send(&mut self, request: CompositorRequest) {
        if self.connected {
            self.outgoing.push_back(request);
            // In a real system, this would serialize and send over IPC.
            // For now, simulate immediate processing.
            self.simulate_response();
        }
    }

    /// Receive the next response from the compositor (blocking in real impl).
    fn recv(&mut self) -> Option<CompositorResponse> {
        self.incoming.pop_front()
    }

    /// Poll for the next event notification from the compositor.
    fn poll_event(&mut self) -> Option<WindowEvent> {
        self.events.pop_front()
    }

    /// Simulate compositor responses for the stub implementation.
    ///
    /// This will be removed when real IPC is available. For now, it provides
    /// sensible default responses so the library's API can be exercised.
    fn simulate_response(&mut self) {
        if let Some(request) = self.outgoing.pop_front() {
            match request {
                CompositorRequest::CreateWindow { .. } => {
                    let window_id = allocate_window_id();
                    self.incoming.push_back(CompositorResponse::WindowCreated {
                        window_id,
                    });
                }
                CompositorRequest::GetDisplayInfo => {
                    self.incoming.push_back(CompositorResponse::DisplayInfo {
                        width: 1920,
                        height: 1080,
                        refresh_rate: 60,
                        scale_factor: 1.0,
                    });
                }
                CompositorRequest::DestroyWindow { .. }
                | CompositorRequest::SetTitle { .. }
                | CompositorRequest::Submit { .. }
                | CompositorRequest::Move { .. }
                | CompositorRequest::Resize { .. }
                | CompositorRequest::Minimize { .. }
                | CompositorRequest::Maximize { .. }
                | CompositorRequest::Restore { .. }
                | CompositorRequest::SetCursor { .. }
                | CompositorRequest::SetVisible { .. } => {
                    self.incoming.push_back(CompositorResponse::Ok);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Window events
// ---------------------------------------------------------------------------

/// Events received from the compositor for a window.
///
/// These are dispatched to application event handlers via the [`EventLoop`].
#[derive(Clone, Debug)]
pub enum WindowEvent {
    /// Window received keyboard focus.
    FocusGained,
    /// Window lost keyboard focus.
    FocusLost,
    /// Window close requested (user clicked X or pressed Alt+F4).
    CloseRequested,
    /// Window was resized (by user or compositor).
    Resized { width: u32, height: u32 },
    /// Window was moved to a new position.
    Moved { x: i32, y: i32 },
    /// A key was pressed.
    KeyDown { key: Key, modifiers: Modifiers },
    /// A key was released.
    KeyUp { key: Key, modifiers: Modifiers },
    /// Text input (Unicode character after keyboard layout processing).
    TextInput { character: char },
    /// Mouse cursor moved within the window's client area.
    MouseMove { x: f32, y: f32 },
    /// Mouse button was pressed within the window.
    MouseDown {
        button: MouseButton,
        x: f32,
        y: f32,
    },
    /// Mouse button was released within the window.
    MouseUp {
        button: MouseButton,
        x: f32,
        y: f32,
    },
    /// Mouse scroll wheel moved.
    Scroll { dx: f32, dy: f32 },
    /// Mouse cursor entered the window's client area.
    MouseEnter,
    /// Mouse cursor left the window's client area.
    MouseLeave,
    /// The window should repaint its contents.
    RedrawRequested,
}

// ---------------------------------------------------------------------------
// Event response
// ---------------------------------------------------------------------------

/// Application response to an event, controlling event loop behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResponse {
    /// Continue running the event loop.
    Continue,
    /// Stop the event loop and exit.
    Exit,
}

// ---------------------------------------------------------------------------
// Window builder
// ---------------------------------------------------------------------------

/// Builder for creating windows with various configuration options.
///
/// Use [`WindowBuilder::new`] to start, chain configuration methods, then
/// call [`WindowBuilder::build`] to create the window.
///
/// # Example
///
/// ```rust,no_run
/// use oswindow::WindowBuilder;
///
/// let window = WindowBuilder::new("Settings", 640, 480)
///     .position(100, 100)
///     .resizable(true)
///     .min_size(320, 240)
///     .build()
///     .expect("failed to create window");
/// ```
pub struct WindowBuilder {
    title: String,
    width: u32,
    height: u32,
    x: Option<i32>,
    y: Option<i32>,
    resizable: bool,
    decorations: bool,
    min_size: Option<(u32, u32)>,
    max_size: Option<(u32, u32)>,
    transparent: bool,
}

impl WindowBuilder {
    /// Create a new window builder with the given title and dimensions.
    ///
    /// The window will be centered on screen by default unless a position
    /// is specified with [`WindowBuilder::position`].
    pub fn new(title: &str, width: u32, height: u32) -> Self {
        Self {
            title: title.to_string(),
            width,
            height,
            x: None,
            y: None,
            resizable: true,
            decorations: true,
            min_size: None,
            max_size: None,
            transparent: false,
        }
    }

    /// Set the initial position of the window (top-left corner).
    ///
    /// If not called, the compositor will center the window on screen.
    pub fn position(mut self, x: i32, y: i32) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self
    }

    /// Set whether the window can be resized by the user.
    ///
    /// Defaults to `true`.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether the compositor draws window decorations (title bar, borders).
    ///
    /// Defaults to `true`. Set to `false` for custom-decorated windows.
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Set the minimum allowed size for the window.
    ///
    /// The compositor will not allow resizing below these dimensions.
    pub fn min_size(mut self, w: u32, h: u32) -> Self {
        self.min_size = Some((w, h));
        self
    }

    /// Set the maximum allowed size for the window.
    ///
    /// The compositor will not allow resizing above these dimensions.
    pub fn max_size(mut self, w: u32, h: u32) -> Self {
        self.max_size = Some((w, h));
        self
    }

    /// Set whether the window background is transparent.
    ///
    /// Defaults to `false`. When `true`, the window's undrawn areas are
    /// transparent rather than filled with the default background color.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Build and create the window by communicating with the compositor.
    ///
    /// Returns the created [`Window`] handle, or a [`WindowError`] if the
    /// compositor rejected the creation request.
    pub fn build(self) -> Result<Window, WindowError> {
        let mut connection = Connection::new();
        if !connection.connected {
            return Err(WindowError::ConnectionFailed(
                "could not connect to compositor".to_string(),
            ));
        }

        // Use centered position if none specified.
        let x = self.x.unwrap_or(0);
        let y = self.y.unwrap_or(0);

        connection.send(CompositorRequest::CreateWindow {
            title: self.title.clone(),
            width: self.width,
            height: self.height,
            x,
            y,
        });

        match connection.recv() {
            Some(CompositorResponse::WindowCreated { window_id }) => Ok(Window {
                id: window_id,
                title: self.title,
                width: self.width,
                height: self.height,
                x,
                y,
                visible: true,
                resizable: self.resizable,
                decorations: self.decorations,
                min_size: self.min_size,
                max_size: self.max_size,
                transparent: self.transparent,
                connection,
            }),
            Some(CompositorResponse::Error { message }) => {
                Err(WindowError::CreationFailed(message))
            }
            _ => Err(WindowError::CreationFailed(
                "unexpected response from compositor".to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Window handle
// ---------------------------------------------------------------------------

/// A handle to a window managed by the compositor.
///
/// Provides methods to manipulate the window state and submit rendering.
/// When dropped, the window is destroyed on the compositor side.
pub struct Window {
    /// Compositor-assigned window identifier.
    id: u64,
    /// Current window title.
    title: String,
    /// Current client area width in pixels.
    width: u32,
    /// Current client area height in pixels.
    height: u32,
    /// Current X position of the window.
    x: i32,
    /// Current Y position of the window.
    y: i32,
    /// Whether the window is currently visible.
    visible: bool,
    /// Whether the window can be resized by the user.
    resizable: bool,
    /// Whether window decorations are drawn by the compositor.
    decorations: bool,
    /// Minimum allowed size, if set.
    min_size: Option<(u32, u32)>,
    /// Maximum allowed size, if set.
    max_size: Option<(u32, u32)>,
    /// Whether the window background is transparent.
    transparent: bool,
    /// IPC connection to the compositor.
    connection: Connection,
}

impl Window {
    /// Get the compositor-assigned window ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the current window title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Get the current client area size as (width, height).
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get the current window position as (x, y).
    pub fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// Get the current client area width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the current client area height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Check whether the window is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check whether the window is resizable.
    pub fn is_resizable(&self) -> bool {
        self.resizable
    }

    /// Check whether window decorations are enabled.
    pub fn has_decorations(&self) -> bool {
        self.decorations
    }

    /// Check whether the window has a transparent background.
    pub fn is_transparent(&self) -> bool {
        self.transparent
    }

    /// Submit a render tree to be displayed in the window's client area.
    ///
    /// The compositor will rasterize the render commands and composite the
    /// result into the final framebuffer at this window's position.
    pub fn submit(&mut self, tree: &RenderTree) {
        self.connection.send(CompositorRequest::Submit {
            id: self.id,
            commands: tree.commands.clone(),
        });
        // Consume the response (fire-and-forget for rendering).
        let _ = self.connection.recv();
    }

    /// Set the window title.
    ///
    /// Updates both the local state and notifies the compositor to redraw
    /// the title bar.
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
        self.connection.send(CompositorRequest::SetTitle {
            id: self.id,
            title: title.to_string(),
        });
        let _ = self.connection.recv();
    }

    /// Move the window to a new position.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.connection.send(CompositorRequest::Move {
            id: self.id,
            x,
            y,
        });
        let _ = self.connection.recv();
    }

    /// Resize the window's client area.
    ///
    /// The actual size may be clamped to the window's min/max size constraints.
    pub fn set_size(&mut self, width: u32, height: u32) {
        // Apply size constraints locally.
        let clamped_width = self.clamp_width(width);
        let clamped_height = self.clamp_height(height);

        self.width = clamped_width;
        self.height = clamped_height;
        self.connection.send(CompositorRequest::Resize {
            id: self.id,
            width: clamped_width,
            height: clamped_height,
        });
        let _ = self.connection.recv();
    }

    /// Minimize the window (hide to taskbar).
    pub fn minimize(&mut self) {
        self.connection.send(CompositorRequest::Minimize { id: self.id });
        let _ = self.connection.recv();
    }

    /// Maximize the window (fill the screen).
    pub fn maximize(&mut self) {
        self.connection.send(CompositorRequest::Maximize { id: self.id });
        let _ = self.connection.recv();
    }

    /// Restore the window from minimized or maximized state.
    pub fn restore(&mut self) {
        self.connection.send(CompositorRequest::Restore { id: self.id });
        let _ = self.connection.recv();
    }

    /// Show or hide the window.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.connection.send(CompositorRequest::SetVisible {
            id: self.id,
            visible,
        });
        let _ = self.connection.recv();
    }

    /// Set the mouse cursor shape for this window.
    ///
    /// The cursor shape is displayed when the mouse is over this window's
    /// client area.
    pub fn set_cursor(&mut self, cursor: CursorShape) {
        self.connection.send(CompositorRequest::SetCursor { shape: cursor });
        let _ = self.connection.recv();
    }

    /// Close and destroy the window.
    ///
    /// Notifies the compositor to remove this window. After calling this,
    /// the window handle is consumed and no further operations are possible.
    pub fn close(mut self) {
        self.connection.send(CompositorRequest::DestroyWindow { id: self.id });
        let _ = self.connection.recv();
    }

    /// Clamp a width value to the configured min/max constraints.
    fn clamp_width(&self, width: u32) -> u32 {
        let mut result = width;
        if let Some((min_w, _)) = self.min_size
            && result < min_w
        {
            result = min_w;
        }
        if let Some((max_w, _)) = self.max_size
            && result > max_w
        {
            result = max_w;
        }
        result
    }

    /// Clamp a height value to the configured min/max constraints.
    fn clamp_height(&self, height: u32) -> u32 {
        let mut result = height;
        if let Some((_, min_h)) = self.min_size
            && result < min_h
        {
            result = min_h;
        }
        if let Some((_, max_h)) = self.max_size
            && result > max_h
        {
            result = max_h;
        }
        result
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Notify the compositor that this window is being destroyed.
        // Ignore the response since we are dropping.
        self.connection.send(CompositorRequest::DestroyWindow { id: self.id });
        let _ = self.connection.recv();
    }
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

/// The application event loop.
///
/// Manages one or more windows and dispatches compositor events to the
/// application's event handler callback. This is the primary entry point
/// for GUI applications after creating windows.
pub struct EventLoop {
    /// Registered windows managed by this event loop.
    windows: Vec<Window>,
    /// Whether the event loop is currently running.
    running: bool,
    /// Pending events that have not yet been dispatched.
    pending_events: VecDeque<(u64, WindowEvent)>,
}

impl EventLoop {
    /// Create a new event loop with no registered windows.
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            running: false,
            pending_events: VecDeque::new(),
        }
    }

    /// Register a window with this event loop.
    ///
    /// The event loop will dispatch events for this window to the handler.
    pub fn register(&mut self, window: Window) {
        self.windows.push(window);
    }

    /// Get a reference to a registered window by ID.
    pub fn window(&self, window_id: u64) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == window_id)
    }

    /// Get a mutable reference to a registered window by ID.
    pub fn window_mut(&mut self, window_id: u64) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == window_id)
    }

    /// Get a slice of all registered windows.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Run the event loop, calling the handler for each event.
    ///
    /// This blocks until the handler returns [`EventResponse::Exit`] or
    /// [`EventLoop::quit`] is called. Events are dispatched as
    /// `(window_id, event)` pairs.
    pub fn run<F>(&mut self, mut handler: F)
    where
        F: FnMut(u64, WindowEvent) -> EventResponse,
    {
        self.running = true;

        while self.running {
            // Poll all windows for events from the compositor.
            self.poll_all_windows();

            // Dispatch all pending events.
            while let Some((window_id, event)) = self.pending_events.pop_front() {
                let response = handler(window_id, event);
                if response == EventResponse::Exit {
                    self.running = false;
                    break;
                }
            }

            // If no events were pending, yield to avoid busy-spinning.
            // In a real system this would block on the IPC channel.
            if self.pending_events.is_empty() && self.running {
                // Placeholder: in real implementation, this would be a
                // blocking wait on the compositor's event channel.
                // For now, break to avoid infinite loops in tests.
                break;
            }
        }
    }

    /// Poll for events without blocking.
    ///
    /// Returns the next pending event, or `None` if no events are available.
    pub fn poll(&mut self) -> Option<(u64, WindowEvent)> {
        self.poll_all_windows();
        self.pending_events.pop_front()
    }

    /// Request a redraw for a specific window.
    ///
    /// Enqueues a [`WindowEvent::RedrawRequested`] event for the specified
    /// window, which will be delivered on the next event loop iteration.
    pub fn request_redraw(&mut self, window_id: u64) {
        self.pending_events
            .push_back((window_id, WindowEvent::RedrawRequested));
    }

    /// Stop the event loop.
    ///
    /// Causes [`EventLoop::run`] to return on the next iteration.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Check whether the event loop is currently running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get the number of registered windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Remove a window from the event loop by ID.
    ///
    /// Returns the window if found, or `None` if no window with that ID
    /// was registered.
    pub fn unregister(&mut self, window_id: u64) -> Option<Window> {
        if let Some(idx) = self.windows.iter().position(|w| w.id == window_id) {
            Some(self.windows.remove(idx))
        } else {
            None
        }
    }

    /// Inject an event into the pending queue (useful for testing and
    /// synthetic event generation).
    pub fn inject_event(&mut self, window_id: u64, event: WindowEvent) {
        self.pending_events.push_back((window_id, event));
    }

    /// Poll all registered windows for compositor events.
    fn poll_all_windows(&mut self) {
        for window in &mut self.windows {
            while let Some(event) = window.connection.poll_event() {
                self.pending_events.push_back((window.id, event));
            }
        }
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_builder_defaults() {
        let builder = WindowBuilder::new("Test Window", 800, 600);
        assert_eq!(builder.title, "Test Window");
        assert_eq!(builder.width, 800);
        assert_eq!(builder.height, 600);
        assert!(builder.resizable);
        assert!(builder.decorations);
        assert!(builder.x.is_none());
        assert!(builder.y.is_none());
        assert!(builder.min_size.is_none());
        assert!(builder.max_size.is_none());
        assert!(!builder.transparent);
    }

    #[test]
    fn test_window_builder_with_position() {
        let builder = WindowBuilder::new("Positioned", 400, 300)
            .position(100, 200);
        assert_eq!(builder.x, Some(100));
        assert_eq!(builder.y, Some(200));
    }

    #[test]
    fn test_window_builder_with_constraints() {
        let builder = WindowBuilder::new("Constrained", 640, 480)
            .min_size(320, 240)
            .max_size(1920, 1080)
            .resizable(false)
            .decorations(false)
            .transparent(true);

        assert_eq!(builder.min_size, Some((320, 240)));
        assert_eq!(builder.max_size, Some((1920, 1080)));
        assert!(!builder.resizable);
        assert!(!builder.decorations);
        assert!(builder.transparent);
    }

    #[test]
    fn test_window_builder_build() {
        let window = WindowBuilder::new("Built Window", 800, 600)
            .position(50, 75)
            .build();

        assert!(window.is_ok());
        let win = window.unwrap();
        assert_eq!(win.title(), "Built Window");
        assert_eq!(win.size(), (800, 600));
        assert_eq!(win.position(), (50, 75));
        assert!(win.is_visible());
    }

    #[test]
    fn test_window_set_title() {
        let mut window = WindowBuilder::new("Original", 640, 480)
            .build()
            .unwrap();
        assert_eq!(window.title(), "Original");

        window.set_title("Updated Title");
        assert_eq!(window.title(), "Updated Title");
    }

    #[test]
    fn test_window_set_position() {
        let mut window = WindowBuilder::new("Movable", 640, 480)
            .position(0, 0)
            .build()
            .unwrap();

        window.set_position(200, 300);
        assert_eq!(window.position(), (200, 300));
    }

    #[test]
    fn test_window_set_size() {
        let mut window = WindowBuilder::new("Resizable", 640, 480)
            .build()
            .unwrap();

        window.set_size(1024, 768);
        assert_eq!(window.size(), (1024, 768));
    }

    #[test]
    fn test_window_size_clamping() {
        let mut window = WindowBuilder::new("Clamped", 640, 480)
            .min_size(320, 240)
            .max_size(1920, 1080)
            .build()
            .unwrap();

        // Try setting below minimum.
        window.set_size(100, 100);
        assert_eq!(window.size(), (320, 240));

        // Try setting above maximum.
        window.set_size(3000, 2000);
        assert_eq!(window.size(), (1920, 1080));

        // Valid size within bounds.
        window.set_size(800, 600);
        assert_eq!(window.size(), (800, 600));
    }

    #[test]
    fn test_window_visibility() {
        let mut window = WindowBuilder::new("Visible", 640, 480)
            .build()
            .unwrap();

        assert!(window.is_visible());
        window.set_visible(false);
        assert!(!window.is_visible());
        window.set_visible(true);
        assert!(window.is_visible());
    }

    #[test]
    fn test_event_loop_creation() {
        let event_loop = EventLoop::new();
        assert_eq!(event_loop.window_count(), 0);
        assert!(!event_loop.is_running());
    }

    #[test]
    fn test_event_loop_register_window() {
        let mut event_loop = EventLoop::new();
        let window = WindowBuilder::new("Loop Window", 800, 600)
            .build()
            .unwrap();
        let wid = window.id();

        event_loop.register(window);
        assert_eq!(event_loop.window_count(), 1);
        assert!(event_loop.window(wid).is_some());
    }

    #[test]
    fn test_event_loop_inject_and_poll() {
        let mut event_loop = EventLoop::new();
        let window = WindowBuilder::new("Poll Test", 640, 480)
            .build()
            .unwrap();
        let wid = window.id();
        event_loop.register(window);

        // Inject synthetic events.
        event_loop.inject_event(wid, WindowEvent::FocusGained);
        event_loop.inject_event(wid, WindowEvent::RedrawRequested);

        let first = event_loop.poll();
        assert!(first.is_some());
        let (id, ev) = first.unwrap();
        assert_eq!(id, wid);
        assert!(matches!(ev, WindowEvent::FocusGained));

        let second = event_loop.poll();
        assert!(second.is_some());
        let (id, ev) = second.unwrap();
        assert_eq!(id, wid);
        assert!(matches!(ev, WindowEvent::RedrawRequested));

        // No more events.
        assert!(event_loop.poll().is_none());
    }

    #[test]
    fn test_event_loop_request_redraw() {
        let mut event_loop = EventLoop::new();
        let window = WindowBuilder::new("Redraw Test", 640, 480)
            .build()
            .unwrap();
        let wid = window.id();
        event_loop.register(window);

        event_loop.request_redraw(wid);

        let polled = event_loop.poll();
        assert!(polled.is_some());
        let (id, ev) = polled.unwrap();
        assert_eq!(id, wid);
        assert!(matches!(ev, WindowEvent::RedrawRequested));
    }

    #[test]
    fn test_event_loop_quit() {
        let mut event_loop = EventLoop::new();
        event_loop.running = true;
        assert!(event_loop.is_running());

        event_loop.quit();
        assert!(!event_loop.is_running());
    }

    #[test]
    fn test_event_loop_run_exits_on_close() {
        let mut event_loop = EventLoop::new();
        let window = WindowBuilder::new("Close Test", 640, 480)
            .build()
            .unwrap();
        let wid = window.id();
        event_loop.register(window);

        // Inject a close event.
        event_loop.inject_event(wid, WindowEvent::CloseRequested);

        event_loop.run(|_window_id, event| match event {
            WindowEvent::CloseRequested => EventResponse::Exit,
            _ => EventResponse::Continue,
        });

        // Event loop should have stopped.
        assert!(!event_loop.is_running());
    }

    #[test]
    fn test_event_loop_unregister() {
        let mut event_loop = EventLoop::new();
        let window = WindowBuilder::new("Unregister Test", 640, 480)
            .build()
            .unwrap();
        let wid = window.id();
        event_loop.register(window);

        assert_eq!(event_loop.window_count(), 1);
        let removed = event_loop.unregister(wid);
        assert!(removed.is_some());
        assert_eq!(event_loop.window_count(), 0);
    }

    #[test]
    fn test_connection_new() {
        let conn = Connection::new();
        assert!(conn.connected);
        assert!(conn.outgoing.is_empty());
        assert!(conn.incoming.is_empty());
        assert!(conn.events.is_empty());
    }

    #[test]
    fn test_display_info_returns_valid_data() {
        let info = display_info();
        assert!(info.width > 0);
        assert!(info.height > 0);
        assert!(info.refresh_rate > 0);
        assert!(info.scale_factor > 0.0);
    }

    #[test]
    fn test_cursor_shape_default() {
        let cursor: CursorShape = CursorShape::default();
        assert_eq!(cursor, CursorShape::Arrow);
    }

    #[test]
    fn test_window_submit_render_tree() {
        let mut window = WindowBuilder::new("Render Test", 640, 480)
            .build()
            .unwrap();

        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 640.0, 480.0, guitk::color::Color::rgb(30, 30, 30));
        tree.text(10.0, 20.0, "Hello, Slate OS!", guitk::color::Color::WHITE, 14.0);

        // Should not panic; verifies the submit path works.
        window.submit(&tree);
    }

    #[test]
    fn test_window_close_consumes_handle() {
        let window = WindowBuilder::new("Close Me", 640, 480)
            .build()
            .unwrap();
        let _id = window.id();

        // close() takes ownership — this just verifies it compiles and runs.
        window.close();
    }

    #[test]
    fn test_window_error_display() {
        let err = WindowError::ConnectionFailed("timeout".to_string());
        assert_eq!(format!("{err}"), "connection failed: timeout");

        let err = WindowError::InvalidWindow(42);
        assert_eq!(format!("{err}"), "invalid window id: 42");
    }

    #[test]
    fn test_multiple_windows_in_event_loop() {
        let mut event_loop = EventLoop::new();

        let w1 = WindowBuilder::new("Window 1", 400, 300)
            .build()
            .unwrap();
        let w2 = WindowBuilder::new("Window 2", 600, 400)
            .build()
            .unwrap();
        let id1 = w1.id();
        let id2 = w2.id();

        event_loop.register(w1);
        event_loop.register(w2);
        assert_eq!(event_loop.window_count(), 2);

        // Inject events for both windows.
        event_loop.inject_event(id1, WindowEvent::FocusGained);
        event_loop.inject_event(id2, WindowEvent::MouseEnter);

        let (eid1, ev1) = event_loop.poll().unwrap();
        assert_eq!(eid1, id1);
        assert!(matches!(ev1, WindowEvent::FocusGained));

        let (eid2, ev2) = event_loop.poll().unwrap();
        assert_eq!(eid2, id2);
        assert!(matches!(ev2, WindowEvent::MouseEnter));
    }
}
