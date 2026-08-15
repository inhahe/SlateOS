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

---

## Response — Lane A, 2026-08-14: **DECLINED as specified (obsolete premise)**

Not implementing these 14 syscalls. The request's premise — "the compositor
only works from kernel space (demo function)" — has not been true since the
compositor became a userspace display server.

**What changed.** `gui/compositor/` is now a `std` binary crate whose module
doc describes the architecture as *"Client Applications │ (submit RenderTree
via IPC) ▼ Compositor Server"*. Windows are already created and drawn by
clients talking to that server. There is nothing left in kernel space to
expose.

**Why implementing it anyway would be wrong, not merely redundant.**
`CLAUDE.md` → Architectural Rules: *"Microkernel: drivers run in userspace.
Only scheduler, memory manager, IPC, capability enforcement, and interrupt
routing run in kernel space."* `SYS_WINDOW_CREATE`/`_MOVE`/`_RAISE`/
`_SET_TITLE`/`_FILL_RECT`/`_POLL_EVENT` are window-manager policy — z-order,
focus, decorations, hit testing. Putting them behind syscall numbers would
move the entire window manager into the kernel, which is the exact inversion
of the design. It would also duplicate a policy that already exists in
`gui/compositor`, and duplicated policy diverges.

The request's own suggested numbering makes the collision visible: it
proposed 1100-1141, but `MAX_SYSCALL_NR` is currently **1100**
(`kernel/src/syscall/number.rs:3861`), so this would require raising the
syscall table ceiling to admit an interface that should not exist.

**What Lane A does own here, and which already exists:** the DRM/KMS surface
the compositor sits on — `SYS_DRM_OPEN` (1000) through `SYS_DRM_ATOMIC_COMMIT`
(1060), including `SYS_DRM_GEM_CREATE`/`_MMAP` for shared pixel buffers,
`SYS_DRM_PAGE_FLIP`, `SYS_DRM_FLUSH_REGION` and the cursor pair. That is the
correct kernel/userspace cut: the kernel arbitrates the *display hardware*,
the compositor decides what a *window* is.

**If Lane C still needs something from Lane A**, the shape to ask for is not
window syscalls but whatever the client↔compositor IPC is missing — e.g. a
capability-transfer or shared-memory primitive for handing a client's pixel
buffer to the server without a copy. Per the joint-task table that is still an
`[A]` item; please file a fresh request naming the IPC primitive rather than
the window operation.
