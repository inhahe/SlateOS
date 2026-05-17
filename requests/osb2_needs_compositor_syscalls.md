# Request: Compositor syscalls for userspace window management

**From**: osb2 (gui-core, gui-toolkit, desktop, apps zones)
**For**: os (kernel-core zone)

## What's needed

Expose the existing compositor functions as syscalls so userspace
applications can create and manage windows. The compositor already has
`create_window()`, `close_window()`, `move_window()`, `raise_window()`,
`window_write_pixel()`, `window_fill_rect()`, and `compose()`.

Suggested syscall numbers (1100-1199 range, following DRM at 1000-1099):

```rust
// Window lifecycle
pub const SYS_WINDOW_CREATE: u64 = 1100;   // (title_ptr, title_len, x, y, width, height) -> window_id
pub const SYS_WINDOW_CLOSE: u64 = 1101;    // (window_id) -> 0
pub const SYS_WINDOW_MOVE: u64 = 1102;     // (window_id, x, y) -> 0
pub const SYS_WINDOW_RESIZE: u64 = 1103;   // (window_id, width, height) -> 0
pub const SYS_WINDOW_RAISE: u64 = 1104;    // (window_id) -> 0
pub const SYS_WINDOW_SET_TITLE: u64 = 1105; // (window_id, title_ptr, title_len) -> 0

// Drawing to window pixel buffer
pub const SYS_WINDOW_WRITE_PIXEL: u64 = 1110; // (window_id, x, y, color) -> 0
pub const SYS_WINDOW_FILL_RECT: u64 = 1111;   // (window_id, x, y, w, h, color) -> 0
pub const SYS_WINDOW_BLIT: u64 = 1112;        // (window_id, x, y, w, h, pixel_buf_ptr) -> 0

// Compositing
pub const SYS_WINDOW_FLUSH: u64 = 1120;    // () -> 0  (triggers compose + page flip)

// Input events (read from window's event queue)
pub const SYS_WINDOW_POLL_EVENT: u64 = 1130; // (window_id, event_buf_ptr) -> event_type or 0
// Event types: key_press, key_release, mouse_move, mouse_click, resize, close

// Window info
pub const SYS_WINDOW_GET_SIZE: u64 = 1140;    // (window_id, width_ptr, height_ptr) -> 0
pub const SYS_WINDOW_LIST: u64 = 1141;        // (buf_ptr, buf_len) -> count
```

## Why

The GUI toolkit (gui-toolkit zone), desktop shell (desktop zone), and
all applications (apps zone) need to create windows and draw to them.
Currently the compositor only works from kernel space (demo function).
These syscalls will enable userspace GUI development.

## Event structure

```rust
#[repr(C)]
struct WindowEvent {
    event_type: u32,     // 0=none, 1=key_down, 2=key_up, 3=mouse_move,
                         // 4=mouse_button, 5=resize, 6=close_request
    key_code: u32,       // scancode for key events
    mouse_x: i32,        // cursor position for mouse events
    mouse_y: i32,
    mouse_button: u32,   // button index for click events
    width: u32,          // new size for resize events
    height: u32,
}
```

## Priority

High — blocks all GUI application development.
