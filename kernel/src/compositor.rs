//! Software compositor and window manager.
//!
//! A minimal software-rendered compositor that manages windows on the
//! framebuffer.  This serves as the foundation for the GUI desktop until
//! GPU-accelerated rendering is available.
//!
//! ## Architecture
//!
//! Windows are stored in a z-ordered list (back to front).  Each window has
//! an off-screen pixel buffer.  The compositor blits windows to the
//! framebuffer on demand, handling overlapping correctly.  A simple title bar
//! with close/minimize buttons allows basic interaction.
//!
//! ## Input dispatch
//!
//! Mouse events are dispatched to the topmost window under the cursor.
//! Clicks on the title bar initiate window dragging.  The compositor
//! consumes mouse events from the PS/2 mouse ring buffer.
//!
//! ## Rendering model
//!
//! Damage-tracked: only redraws regions that changed.  In the simple case,
//! the entire framebuffer is recomposed when any window moves or content
//! changes.  Future optimization: dirty rectangles.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::fb;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Title bar height in pixels.
const TITLE_BAR_HEIGHT: u32 = 24;
/// Window border width in pixels.
const BORDER_WIDTH: u32 = 1;
/// Maximum number of windows.
const MAX_WINDOWS: usize = 32;

// Colors (BGRA format via fb::rgb).
const COLOR_TITLE_BAR_ACTIVE: u32 = 0xFF_33_66_99; // Steel blue
const COLOR_TITLE_BAR_INACTIVE: u32 = 0xFF_55_55_55; // Gray
const COLOR_TITLE_TEXT: u32 = 0xFF_FF_FF_FF; // White
const COLOR_BORDER: u32 = 0xFF_44_44_44; // Dark gray
const COLOR_CLOSE_BTN: u32 = 0xFF_CC_33_33; // Red
const COLOR_DESKTOP_BG: u32 = 0xFF_1A_1A_2E; // Dark navy

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// Window identifier (stable across z-order changes).
pub type WindowId = u32;

/// A managed window.
pub struct Window {
    /// Unique identifier.
    pub id: WindowId,
    /// Window title.
    pub title: String,
    /// X position of top-left corner (includes border).
    pub x: i32,
    /// Y position of top-left corner (includes border).
    pub y: i32,
    /// Client area width (excludes border/title bar).
    pub width: u32,
    /// Client area height (excludes border/title bar).
    pub height: u32,
    /// Off-screen pixel buffer for the client area (row-major BGRA).
    pub pixels: Vec<u32>,
    /// Whether the window is visible.
    pub visible: bool,
    /// Whether the window is minimized.
    pub minimized: bool,
}

impl Window {
    /// Create a new window with a blank (black) client area.
    pub fn new(id: WindowId, title: &str, x: i32, y: i32, width: u32, height: u32) -> Self {
        let pixel_count = (width * height) as usize;
        Self {
            id,
            title: String::from(title),
            x,
            y,
            width,
            height,
            pixels: vec![0xFF_00_00_00; pixel_count], // Black background
            visible: true,
            minimized: false,
        }
    }

    /// Total width including border.
    pub fn total_width(&self) -> u32 {
        self.width + 2 * BORDER_WIDTH
    }

    /// Total height including title bar and border.
    pub fn total_height(&self) -> u32 {
        self.height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH
    }

    /// Client area X offset (from window origin to content).
    pub fn client_x(&self) -> i32 {
        self.x + BORDER_WIDTH as i32
    }

    /// Client area Y offset (from window origin to content start).
    pub fn client_y(&self) -> i32 {
        self.y + TITLE_BAR_HEIGHT as i32 + BORDER_WIDTH as i32
    }

    /// Set a pixel in the client area buffer.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) as usize;
            if let Some(p) = self.pixels.get_mut(idx) {
                *p = color;
            }
        }
    }

    /// Fill the client area with a solid color.
    pub fn fill(&mut self, color: u32) {
        for p in self.pixels.iter_mut() {
            *p = color;
        }
    }

    /// Fill a rectangle within the client area.
    pub fn fill_rect(&mut self, rx: u32, ry: u32, rw: u32, rh: u32, color: u32) {
        for row in ry..ry.saturating_add(rh).min(self.height) {
            for col in rx..rx.saturating_add(rw).min(self.width) {
                let idx = (row * self.width + col) as usize;
                if let Some(p) = self.pixels.get_mut(idx) {
                    *p = color;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Compositor state
// ---------------------------------------------------------------------------

/// The compositor managing all windows.
struct CompositorState {
    /// Windows in z-order (index 0 = back, last = front/focused).
    windows: Vec<Window>,
    /// Next window ID to assign.
    next_id: WindowId,
    /// Whether the desktop needs a full redraw.
    dirty: bool,
    /// Whether the compositor is running.
    running: bool,
    /// Currently dragging: (window_id, offset_x, offset_y).
    dragging: Option<(WindowId, i32, i32)>,
}

impl CompositorState {
    const fn new() -> Self {
        Self {
            windows: Vec::new(),
            next_id: 1,
            dirty: true,
            running: false,
            dragging: None,
        }
    }
}

static COMPOSITOR: Mutex<CompositorState> = Mutex::new(CompositorState::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the compositor (draw desktop background, show cursor).
pub fn start() {
    if !fb::is_initialized() {
        crate::serial_println!("[compositor] Cannot start: framebuffer not available");
        return;
    }

    let mut state = COMPOSITOR.lock();
    state.running = true;
    state.dirty = true;
    drop(state);

    ACTIVE.store(true, Ordering::Release);

    // Draw initial desktop.
    compose();
    fb::show_cursor();

    crate::serial_println!("[compositor] Started");
}

/// Stop the compositor and return to text console.
pub fn stop() {
    ACTIVE.store(false, Ordering::Release);
    fb::hide_cursor();

    let mut state = COMPOSITOR.lock();
    state.running = false;
    drop(state);

    // Restore text console.
    crate::console::clear();
    crate::serial_println!("[compositor] Stopped");
}

/// Return true if the compositor is active.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Create a new window and return its ID.
pub fn create_window(title: &str, x: i32, y: i32, width: u32, height: u32) -> WindowId {
    let mut state = COMPOSITOR.lock();
    if state.windows.len() >= MAX_WINDOWS {
        return 0; // No more windows.
    }
    let id = state.next_id;
    state.next_id += 1;
    let window = Window::new(id, title, x, y, width, height);
    state.windows.push(window);
    state.dirty = true;
    id
}

/// Close (destroy) a window by ID.
pub fn close_window(id: WindowId) {
    let mut state = COMPOSITOR.lock();
    state.windows.retain(|w| w.id != id);
    state.dirty = true;
}

/// Move a window to a new position.
pub fn move_window(id: WindowId, x: i32, y: i32) {
    let mut state = COMPOSITOR.lock();
    if let Some(w) = state.windows.iter_mut().find(|w| w.id == id) {
        w.x = x;
        w.y = y;
        state.dirty = true;
    }
}

/// Bring a window to the front (make it focused).
pub fn raise_window(id: WindowId) {
    let mut state = COMPOSITOR.lock();
    if let Some(pos) = state.windows.iter().position(|w| w.id == id) {
        let window = state.windows.remove(pos);
        state.windows.push(window);
        state.dirty = true;
    }
}

/// Access a window's pixel buffer via a closure.
pub fn with_window<F, R>(id: WindowId, f: F) -> Option<R>
where
    F: FnOnce(&mut Window) -> R,
{
    let mut state = COMPOSITOR.lock();
    if let Some(w) = state.windows.iter_mut().find(|w| w.id == id) {
        let result = f(w);
        state.dirty = true;
        Some(result)
    } else {
        None
    }
}

/// Get the number of open windows.
pub fn window_count() -> usize {
    COMPOSITOR.lock().windows.len()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Compose all windows to the framebuffer.
///
/// Draws the desktop background, then each window from back to front.
pub fn compose() {
    let state = COMPOSITOR.lock();
    if !state.running {
        return;
    }

    // Draw desktop background.
    let (fb_w, fb_h) = fb::dimensions();
    fb::fill_rect(0, 0, fb_w, fb_h, COLOR_DESKTOP_BG);

    // Draw each visible window from back to front.
    for (idx, window) in state.windows.iter().enumerate() {
        if !window.visible || window.minimized {
            continue;
        }
        let is_focused = idx == state.windows.len() - 1;
        draw_window(window, is_focused);
    }
}

/// Draw a single window (border + title bar + client area).
fn draw_window(window: &Window, focused: bool) {
    let x = window.x;
    let y = window.y;
    let tw = window.total_width();
    let th = window.total_height();

    // Border.
    fb::draw_rect(x, y, tw, th, COLOR_BORDER);

    // Title bar background.
    let title_color = if focused { COLOR_TITLE_BAR_ACTIVE } else { COLOR_TITLE_BAR_INACTIVE };
    fb::fill_rect(
        x + BORDER_WIDTH as i32,
        y + BORDER_WIDTH as i32,
        window.width,
        TITLE_BAR_HEIGHT - BORDER_WIDTH,
        title_color,
    );

    // Title text (simplified: just draw characters using set_pixel).
    draw_title_text(
        x + BORDER_WIDTH as i32 + 4,
        y + BORDER_WIDTH as i32 + 4,
        &window.title,
        COLOR_TITLE_TEXT,
    );

    // Close button (small red square in top-right).
    let close_x = x + tw as i32 - BORDER_WIDTH as i32 - 18;
    let close_y = y + BORDER_WIDTH as i32 + 4;
    fb::fill_rect(close_x, close_y, 14, 14, COLOR_CLOSE_BTN);
    // Draw X in the close button.
    fb::draw_line(close_x + 3, close_y + 3, close_x + 11, close_y + 11, COLOR_TITLE_TEXT);
    fb::draw_line(close_x + 11, close_y + 3, close_x + 3, close_y + 11, COLOR_TITLE_TEXT);

    // Client area: blit the window's pixel buffer.
    let client_x = window.client_x();
    let client_y = window.client_y();
    fb::blit(&window.pixels, window.width, client_x, client_y, window.width, window.height);
}

/// Draw title text using the kernel's bitmap font.
///
/// Simple glyph rendering at pixel level (8x16 font scaled to title bar).
fn draw_title_text(x: i32, y: i32, text: &str, color: u32) {
    let mut cx = x;
    for ch in text.bytes().take(32) {
        // Only render printable ASCII.
        if ch < 0x20 || ch > 0x7E {
            continue;
        }
        let glyph = crate::font::glyph(ch);
        // Draw at half height (8 pixel rows, skip every other row for fitting).
        for (row_idx, &glyph_row) in glyph.iter().enumerate().step_by(2) {
            for col in 0..8u32 {
                let bit = (glyph_row >> (7 - col)) & 1;
                if bit != 0 {
                    #[allow(clippy::cast_possible_truncation)]
                    fb::set_pixel(
                        (cx + col as i32) as u32,
                        (y + row_idx as i32 / 2) as u32,
                        color,
                    );
                }
            }
        }
        cx += 8; // Character width.
    }
}

// ---------------------------------------------------------------------------
// Input handling
// ---------------------------------------------------------------------------

/// Process all pending mouse events and update the compositor.
///
/// Call this in a loop (e.g., from a kshell "desktop" command or a
/// dedicated compositor task).
pub fn process_input() {
    while let Some(ev) = crate::mouse::try_read_event() {
        // Move cursor.
        fb::move_cursor(ev.dx, ev.dy);

        let (cx, cy) = fb::cursor_pos();

        if ev.buttons & 1 != 0 {
            // Left button pressed.
            handle_left_click(cx as i32, cy as i32);
        } else {
            // Button released — stop dragging.
            let mut state = COMPOSITOR.lock();
            if state.dragging.is_some() {
                state.dragging = None;
                state.dirty = true;
            }
        }

        // Handle dragging.
        let mut state = COMPOSITOR.lock();
        if let Some((wid, off_x, off_y)) = state.dragging {
            if let Some(w) = state.windows.iter_mut().find(|w| w.id == wid) {
                w.x = cx as i32 - off_x;
                w.y = cy as i32 - off_y;
                state.dirty = true;
            }
        }
    }

    // Recompose if dirty.
    let state = COMPOSITOR.lock();
    if state.dirty {
        drop(state);
        compose();
        let mut state = COMPOSITOR.lock();
        state.dirty = false;
    }
}

/// Handle a left-click at screen coordinates (sx, sy).
fn handle_left_click(sx: i32, sy: i32) {
    let mut state = COMPOSITOR.lock();

    // Find the topmost window under the click (iterate back to front).
    let mut clicked_id: Option<WindowId> = None;

    for window in state.windows.iter().rev() {
        if !window.visible || window.minimized {
            continue;
        }
        let wx = window.x;
        let wy = window.y;
        let ww = window.total_width() as i32;
        let wh = window.total_height() as i32;

        if sx >= wx && sx < wx + ww && sy >= wy && sy < wy + wh {
            clicked_id = Some(window.id);

            // Check if click is on the close button.
            let close_x = wx + ww - BORDER_WIDTH as i32 - 18;
            let close_y = wy + BORDER_WIDTH as i32 + 4;
            if sx >= close_x && sx < close_x + 14 && sy >= close_y && sy < close_y + 14 {
                // Close the window.
                let id = window.id;
                drop(state);
                close_window(id);
                return;
            }

            // Check if click is on the title bar (dragging).
            let title_top = wy + BORDER_WIDTH as i32;
            let title_bottom = title_top + TITLE_BAR_HEIGHT as i32;
            if sy >= title_top && sy < title_bottom {
                let off_x = sx - wx;
                let off_y = sy - wy;
                state.dragging = Some((window.id, off_x, off_y));
            }

            break;
        }
    }

    // Raise the clicked window to front.
    if let Some(id) = clicked_id {
        if let Some(pos) = state.windows.iter().position(|w| w.id == id) {
            let window = state.windows.remove(pos);
            state.windows.push(window);
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test / demo
// ---------------------------------------------------------------------------

/// Run a compositor demo: create some windows and process input briefly.
pub fn demo() {
    if !fb::is_initialized() {
        crate::console_println!("Compositor demo requires framebuffer");
        return;
    }

    start();

    // Create a few demo windows.
    let w1 = create_window("Hello World", 50, 50, 200, 150);
    let w2 = create_window("Terminal", 280, 100, 250, 180);
    let w3 = create_window("About", 150, 250, 180, 120);

    // Fill windows with different colors.
    with_window(w1, |w| w.fill(fb::rgb(30, 30, 60)));
    with_window(w2, |w| {
        w.fill(fb::rgb(10, 10, 10));
        // Draw some "text lines" as colored rectangles.
        w.fill_rect(4, 4, 200, 2, fb::rgb(0, 200, 0));
        w.fill_rect(4, 10, 150, 2, fb::rgb(0, 200, 0));
        w.fill_rect(4, 16, 180, 2, fb::rgb(0, 200, 0));
    });
    with_window(w3, |w| {
        w.fill(fb::rgb(40, 40, 50));
        // A simple icon-like square.
        w.fill_rect(70, 30, 40, 40, fb::rgb(100, 150, 255));
    });

    // Compose once to show everything.
    compose();

    crate::console_println!("Compositor demo running. Move the mouse!");
    crate::console_println!("Click title bars to drag, X to close, any key to exit.");

    // Process input until a key is pressed.
    loop {
        process_input();
        // Check for keyboard input to exit.
        if crate::keyboard::try_read_char().is_some() {
            break;
        }
        crate::sched::yield_now();
    }

    stop();

    // Clean up windows.
    close_window(w1);
    close_window(w2);
    close_window(w3);
}
