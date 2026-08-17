//! A real window on the development machine, so the desktop can be looked at.
//!
//! **This is a development harness, not part of SlateOS.** It exists because
//! the compositor's entire rendering pipeline — text, alpha blending, window
//! decorations, shadows, the cursor — had never been seen by a human eye. Every
//! test asserted on pixel *values*, which catches a wrong colour and cannot
//! catch a layout that is merely ugly, a font baseline one pixel out, or a
//! shadow drawn on the wrong side. The only fix for that is to draw it on a
//! screen and look, and the machine this tree is developed on runs Windows.
//!
//! It is also the only input source the hosted build has. Without it,
//! [`Compositor::handle_input`](crate::Compositor::handle_input) is reachable
//! from a test and from nothing else, so the whole routing path — hit testing,
//! focus, per-window coordinates — runs only against synthesised events.
//!
//! ## Why raw FFI and not a crate
//!
//! `winit`/`softbuffer` would be perhaps thirty lines here instead of four
//! hundred, and would bring a dependency tree into a workspace whose *real*
//! target is `x86_64-slateos`, for the sake of a harness that will be deleted
//! the day SlateOS's display driver exists. What is actually needed is small
//! and stable: register a class, create a window, blit a `u32` buffer, read
//! keyboard and mouse messages. Those five calls have been ABI-stable since
//! 1995 and are declared here directly.
//!
//! ## The one piece of luck worth naming
//!
//! Windows reports, in bits 16–23 of a key message's `lParam`, the **scan code
//! set 1** code — the same convention `keymap.rs` uses, and bit 24 is the
//! `0xE0` extended flag it encodes in the high byte. So a Windows key message
//! translates into a SlateOS scancode by shifting and masking, with no lookup
//! table and no chance of the two tables drifting apart. That is why this
//! harness can drive the *real* keymap rather than a parallel one, which is
//! what makes it a test of the compositor and not merely of itself.
//!
//! ## Threading
//!
//! A Win32 window belongs to the thread that created it: messages are delivered
//! to that thread's queue and to no other. [`Window::new`] therefore records the
//! thread it ran on and every method asserts it is still there — a `HWND` used
//! from another thread silently receives no messages, which presents as a window
//! that never repaints and never responds, and is very hard to recognise.

#![allow(unsafe_code)]
// A clippy false positive, checked rather than assumed: both of this module's
// thread-locals are already initialised with `const` blocks — precisely what
// the lint asks for — and splitting them into separate `thread_local!` blocks
// showed it firing on each independently, including on `Cell::new(false)`,
// which cannot be made any more constant than it is. The `allow` is at file
// scope because the lint is reported against the whole macro invocation, and
// an attribute written on a macro invocation is passed to the macro and
// discarded. Nothing is hidden by the breadth: this module has exactly the two.
#![allow(clippy::missing_const_for_thread_local)]

use std::cell::{Cell, RefCell};
use std::io;
use std::thread::ThreadId;

// The compositor's own `MouseButton`, not `guitk::event::MouseButton`: this
// translates host messages into [`InputEvent`], which is what
// `Compositor::handle_input` takes, and the two enums are not the same type
// even where they name the same buttons.
use crate::{InputEvent, MouseButton};

// ---------------------------------------------------------------------------
// The parts of the Win32 API this needs
// ---------------------------------------------------------------------------

// Every name in here is spelled the way the Windows headers spell it, so that
// this module can be checked against MSDN a line at a time. Renaming `HWND` to
// `Hwnd` and `WNDCLASSW` to `Wndclassw` to satisfy Rust's conventions would
// make a declaration that must match a foreign ABI exactly harder to verify
// against the only document that says what it is — which is the one thing a
// hand-written FFI block cannot afford.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
mod ffi {
    use std::ffi::c_void;

    pub type HANDLE = *mut c_void;
    pub type HWND = HANDLE;
    pub type HDC = HANDLE;
    pub type HINSTANCE = HANDLE;
    pub type HICON = HANDLE;
    pub type HCURSOR = HANDLE;
    pub type HBRUSH = HANDLE;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type LRESULT = isize;

    pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

    #[repr(C)]
    pub struct WNDCLASSW {
        pub style: u32,
        pub lpfn_wnd_proc: Option<WndProc>,
        pub cb_cls_extra: i32,
        pub cb_wnd_extra: i32,
        pub h_instance: HINSTANCE,
        pub h_icon: HICON,
        pub h_cursor: HCURSOR,
        pub hbr_background: HBRUSH,
        pub lpsz_menu_name: *const u16,
        pub lpsz_class_name: *const u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: u32,
        pub w_param: WPARAM,
        pub l_param: LPARAM,
        pub time: u32,
        pub pt: POINT,
        pub private: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct BITMAPINFOHEADER {
        pub bi_size: u32,
        pub bi_width: i32,
        pub bi_height: i32,
        pub bi_planes: u16,
        pub bi_bit_count: u16,
        pub bi_compression: u32,
        pub bi_size_image: u32,
        pub bi_x_pels_per_meter: i32,
        pub bi_y_pels_per_meter: i32,
        pub bi_clr_used: u32,
        pub bi_clr_important: u32,
    }

    /// `BITMAPINFO` with the single-entry colour table `BI_RGB` does not use.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct BITMAPINFO {
        pub header: BITMAPINFOHEADER,
        pub colors: [u32; 3],
    }

    // Window styles.
    pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
    pub const WS_VISIBLE: u32 = 0x1000_0000;
    pub const CW_USEDEFAULT: i32 = -2_147_483_648;

    // Class styles: redraw on either resize, and own a device context so the
    // blit does not have to re-acquire one per frame.
    pub const CS_HREDRAW: u32 = 0x0002;
    pub const CS_VREDRAW: u32 = 0x0001;
    pub const CS_OWNDC: u32 = 0x0020;

    // Messages.
    pub const WM_DESTROY: u32 = 0x0002;
    pub const WM_CLOSE: u32 = 0x0010;
    pub const WM_QUIT: u32 = 0x0012;
    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_KEYUP: u32 = 0x0101;
    pub const WM_SYSKEYDOWN: u32 = 0x0104;
    pub const WM_SYSKEYUP: u32 = 0x0105;
    pub const WM_CHAR: u32 = 0x0102;
    pub const WM_MOUSEMOVE: u32 = 0x0200;
    pub const WM_LBUTTONDOWN: u32 = 0x0201;
    pub const WM_LBUTTONUP: u32 = 0x0202;
    pub const WM_RBUTTONDOWN: u32 = 0x0204;
    pub const WM_RBUTTONUP: u32 = 0x0205;
    pub const WM_MBUTTONDOWN: u32 = 0x0207;
    pub const WM_MBUTTONUP: u32 = 0x0208;
    pub const WM_MOUSEWHEEL: u32 = 0x020A;
    // The two side buttons. Unlike the other five button messages these do not
    // say *which* button in the message id — the high half of `wParam` carries
    // `XBUTTON1`/`XBUTTON2` — which is why `button_for` takes `wParam` at all.
    pub const WM_XBUTTONDOWN: u32 = 0x020B;
    pub const WM_XBUTTONUP: u32 = 0x020C;
    pub const WM_MOUSEHWHEEL: u32 = 0x020E;
    pub const XBUTTON1: u16 = 0x0001;
    pub const XBUTTON2: u16 = 0x0002;

    pub const PM_REMOVE: u32 = 0x0001;
    pub const SW_SHOW: i32 = 5;
    pub const IDC_ARROW: u32 = 32512;
    pub const BI_RGB: u32 = 0;
    pub const DIB_RGB_COLORS: u32 = 0;
    pub const SRCCOPY: u32 = 0x00CC_0020;
    pub const WHEEL_DELTA: f32 = 120.0;

    // Named explicitly rather than relying on whatever the toolchain happens to
    // link by default: `std` pulls in kernel32, but nothing in a Rust program
    // links user32 or gdi32 unless it asks, and the failure is a page of
    // "undefined reference" from the linker rather than anything that names a
    // missing library.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
        pub fn GetLastError() -> u32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn RegisterClassW(class: *const WNDCLASSW) -> u16;
        pub fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
        pub fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: HWND,
            menu: HANDLE,
            instance: HINSTANCE,
            param: *mut c_void,
        ) -> HWND;
        pub fn DefWindowProcW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
        pub fn DestroyWindow(hwnd: HWND) -> i32;
        pub fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
        pub fn PeekMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> i32;
        pub fn TranslateMessage(msg: *const MSG) -> i32;
        pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
        pub fn PostQuitMessage(code: i32);
        pub fn GetDC(hwnd: HWND) -> HDC;
        pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
        pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
        pub fn AdjustWindowRect(rect: *mut RECT, style: u32, menu: i32) -> i32;
        pub fn SetWindowTextW(hwnd: HWND, text: *const u16) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        pub fn StretchDIBits(
            dc: HDC,
            x_dest: i32,
            y_dest: i32,
            w_dest: i32,
            h_dest: i32,
            x_src: i32,
            y_src: i32,
            w_src: i32,
            h_src: i32,
            bits: *const c_void,
            info: *const BITMAPINFO,
            usage: u32,
            rop: u32,
        ) -> i32;
    }
}

// ---------------------------------------------------------------------------
// Translating messages into compositor input
// ---------------------------------------------------------------------------

/// Turn a key message's `lParam` into a scan-code-set-1 code.
///
/// Bits 16–23 hold the code the keyboard actually sent, and bit 24 is set for
/// the keys the hardware prefixes with `0xE0`. `keymap.rs` encodes that prefix
/// in the high byte, so the two conventions differ only by where the extended
/// flag is written — no table, and therefore nothing that can drift.
///
/// Returns `None` for a code of zero, which is what a synthetic key press
/// injected by software (`SendInput` with no scan code, an on-screen keyboard)
/// carries. Forwarding it would reach `key_for_scancode(0)` and become
/// `Key::Unknown(0)`, a keystroke the user did not make.
#[must_use]
pub const fn scancode_from_lparam(l_param: isize) -> Option<u32> {
    #[allow(clippy::cast_sign_loss)]
    let bits = l_param as usize;
    let code = ((bits >> 16) & 0xFF) as u32;
    if code == 0 {
        return None;
    }
    let extended = (bits >> 24) & 1 == 1;
    if extended {
        Some(0xE000 | code)
    } else {
        Some(code)
    }
}

/// Split a mouse message's `lParam` into client-area coordinates.
///
/// The two halves are **signed**: a drag that leaves the window while a button
/// is held keeps reporting, with negative coordinates, and a compositor that
/// read them as unsigned would see the pointer jump to 65,000 and the window
/// being dragged fly off the desktop.
#[must_use]
pub const fn mouse_point(l_param: isize) -> (i32, i32) {
    #[allow(clippy::cast_possible_truncation)]
    let low = l_param as i16;
    #[allow(clippy::cast_possible_truncation)]
    let high = (l_param >> 16) as i16;
    (low as i32, high as i32)
}

/// The wheel notches in a `WM_MOUSEWHEEL`, as a scroll distance.
///
/// The delta is in the **high** word of `wParam` and is signed: a multiple of
/// `WHEEL_DELTA` (120), positive away from the user. Dividing rather than
/// counting notches is deliberate — a high-resolution wheel sends fractions of
/// 120, and truncating them to whole notches makes a precision trackpad scroll
/// in jumps or not at all.
#[must_use]
pub fn wheel_delta(w_param: usize) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let raw = (w_param >> 16) as i16;
    f32::from(raw) / ffi::WHEEL_DELTA
}

/// Which button a message is about, and whether it went down.
///
/// Takes `w_param` because the two side buttons share one pair of messages and
/// distinguish themselves in its high half. Anything other than the two known
/// `XBUTTON` values is `None` rather than a guess: Windows reserves the
/// remaining bits, and inventing a button from a bit pattern that does not mean
/// one would send a click to a window nobody touched.
#[must_use]
pub const fn button_for(message: u32, w_param: usize) -> Option<(MouseButton, bool)> {
    let side = match (w_param >> 16) as u16 {
        ffi::XBUTTON1 => Some(MouseButton::Back),
        ffi::XBUTTON2 => Some(MouseButton::Forward),
        _ => None,
    };
    match message {
        ffi::WM_LBUTTONDOWN => Some((MouseButton::Left, true)),
        ffi::WM_LBUTTONUP => Some((MouseButton::Left, false)),
        ffi::WM_RBUTTONDOWN => Some((MouseButton::Right, true)),
        ffi::WM_RBUTTONUP => Some((MouseButton::Right, false)),
        ffi::WM_MBUTTONDOWN => Some((MouseButton::Middle, true)),
        ffi::WM_MBUTTONUP => Some((MouseButton::Middle, false)),
        // `const fn` cannot use `?` or `map`, hence the match on the option.
        ffi::WM_XBUTTONDOWN => match side {
            Some(b) => Some((b, true)),
            None => None,
        },
        ffi::WM_XBUTTONUP => match side {
            Some(b) => Some((b, false)),
            None => None,
        },
        _ => None,
    }
}

/// Turn one window message into compositor input, if it is any.
///
/// Split out from the window procedure and taking only plain integers, so the
/// translation — which is where the bugs live — is testable without a window,
/// a message loop, or a graphical session. The procedure below is then only
/// plumbing.
#[must_use]
pub fn event_for_message(message: u32, w_param: usize, l_param: isize) -> Option<InputEvent> {
    match message {
        ffi::WM_KEYDOWN | ffi::WM_SYSKEYDOWN => {
            scancode_from_lparam(l_param).map(|scancode| InputEvent::KeyDown {
                // The character comes separately, in `WM_CHAR`, because
                // producing it is the *layout's* job and Windows does it
                // between the two messages. Reporting `None` here and letting
                // the compositor's own keymap decide is what keeps
                // `design-decisions.md` §456 true: the compositor names the
                // key, not the host.
                scancode,
                character: None,
            })
        }
        ffi::WM_KEYUP | ffi::WM_SYSKEYUP => {
            scancode_from_lparam(l_param).map(|scancode| InputEvent::KeyUp { scancode })
        }
        ffi::WM_MOUSEMOVE => {
            let (x, y) = mouse_point(l_param);
            Some(InputEvent::MouseMove { x, y })
        }
        ffi::WM_MOUSEWHEEL => {
            // The position in a wheel message is in *screen* coordinates, not
            // client ones, so it is not converted here; the compositor uses the
            // pointer position it already tracks from `MouseMove`.
            let (x, y) = mouse_point(l_param);
            Some(InputEvent::MouseScroll {
                dx: 0.0,
                dy: wheel_delta(w_param),
                x,
                y,
            })
        }
        ffi::WM_MOUSEHWHEEL => {
            let (x, y) = mouse_point(l_param);
            Some(InputEvent::MouseScroll {
                dx: wheel_delta(w_param),
                dy: 0.0,
                x,
                y,
            })
        }
        other => button_for(other, w_param).map(|(button, pressed)| {
            let (x, y) = mouse_point(l_param);
            InputEvent::MouseButton {
                button,
                pressed,
                x,
                y,
            }
        }),
    }
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

// `PENDING` is where the window procedure puts what it decodes; `CLOSED` is set
// when the user closes the window.
//
// Thread-local rather than hung off the `HWND` with `SetWindowLongPtr` because
// a Win32 window's messages are delivered to its creating thread and to no
// other — so "this thread's queue" is exactly the right scope, and it avoids a
// raw pointer to Rust state living inside the OS across a call it does not
// control.
//
// The `allow` is for a clippy false positive: both initializers are already
// `const` blocks, which is exactly what the lint asks for, and it fires anyway
// — on the macro as a whole, so it cannot be narrowed to the one it means.
// `missing_const_for_thread_local` fires on both of these and is wrong about
// both — see the file-level `allow`, which is where it has to live because an
// attribute on a macro invocation is handed to the macro and dropped.
thread_local! {
    static PENDING: RefCell<Vec<InputEvent>> = const { RefCell::new(Vec::new()) };
    static CLOSED: Cell<bool> = const { Cell::new(false) };
}

/// The window procedure.
///
/// # Safety
///
/// Called by the OS with a valid `HWND` for a window of our class. It
/// dereferences nothing the caller supplied — every parameter is an integer or
/// is passed straight back to `DefWindowProcW` — so the only obligation is that
/// `DefWindowProcW` is called for anything not handled, which it is.
unsafe extern "system" fn wnd_proc(
    hwnd: ffi::HWND,
    message: u32,
    w_param: ffi::WPARAM,
    l_param: ffi::LPARAM,
) -> ffi::LRESULT {
    match message {
        ffi::WM_CLOSE => {
            CLOSED.set(true);
            // Not destroyed here: `Window::drop` owns that, and destroying the
            // window out from under the owner would leave it holding a stale
            // `HWND` that every later call would fail on.
            0
        }
        ffi::WM_DESTROY => {
            CLOSED.set(true);
            unsafe { ffi::PostQuitMessage(0) };
            0
        }
        ffi::WM_CHAR => {
            // A character, produced by the *host's* layout. It is attached to
            // the key press that produced it only in the sense of arriving
            // right after it; the compositor's keymap is what decides the key
            // name, and this is the harness saying "and the user meant this
            // letter". `w_param` is a UTF-16 code unit, so a character outside
            // the BMP arrives as two of them and is dropped — acceptable in a
            // development harness, and noted rather than hidden.
            #[allow(clippy::cast_possible_truncation)]
            if let Some(ch) = char::from_u32(w_param as u32)
                && !ch.is_control()
            {
                PENDING.with(|p| {
                    p.borrow_mut().push(InputEvent::TextInput {
                        text: ch.to_string(),
                    });
                });
            }
            0
        }
        other => {
            if let Some(event) = event_for_message(other, w_param, l_param) {
                PENDING.with(|p| p.borrow_mut().push(event));
                // Still passed on: `DefWindowProcW` is what makes system keys,
                // window activation and the caret behave.
            }
            unsafe { ffi::DefWindowProcW(hwnd, other, w_param, l_param) }
        }
    }
}

/// Encode a Rust string as a NUL-terminated UTF-16 buffer for the `W` APIs.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A host window showing the composited desktop.
///
/// See the module docs: a development harness, and the hosted build's only
/// input source.
pub struct Window {
    hwnd: ffi::HWND,
    dc: ffi::HDC,
    owner: ThreadId,
    /// The last size the client area was seen at, so a caller can notice a
    /// resize without asking the OS.
    size: (u32, u32),
}

impl Window {
    /// Open a window whose client area is `width` by `height`.
    ///
    /// # Errors
    ///
    /// If the window class cannot be registered or the window cannot be
    /// created — in practice, only in a session with no interactive desktop
    /// (a service, or a CI runner with no window station).
    ///
    /// # Panics
    ///
    /// Never; the `Option` unwraps below are on values just constructed.
    pub fn new(title: &str, width: u32, height: u32) -> io::Result<Self> {
        let class_name = wide("SlateOSCompositorHost");
        let title_wide = wide(title);

        // SAFETY: `GetModuleHandleW(null)` returns this process's own module
        // handle and cannot fail for the null argument.
        let instance = unsafe { ffi::GetModuleHandleW(std::ptr::null()) };

        // SAFETY: `IDC_ARROW` is a predefined cursor id, which the API takes in
        // place of a string pointer; a null instance selects the system set.
        let cursor =
            unsafe { ffi::LoadCursorW(std::ptr::null_mut(), ffi::IDC_ARROW as *const u16) };

        let class = ffi::WNDCLASSW {
            style: ffi::CS_HREDRAW | ffi::CS_VREDRAW | ffi::CS_OWNDC,
            lpfn_wnd_proc: Some(wnd_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instance,
            h_icon: std::ptr::null_mut(),
            h_cursor: cursor,
            // Null: every pixel is painted by the blit, so a background brush
            // would only be a flash of colour before it.
            hbr_background: std::ptr::null_mut(),
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class_name.as_ptr(),
        };

        // SAFETY: `class` is a fully initialised `WNDCLASSW` whose string
        // pointers outlive the call — `class_name` is alive for the whole
        // function. A duplicate registration returns 0 with
        // `ERROR_CLASS_ALREADY_EXISTS`, which is fine: a second window of the
        // same class is the intent.
        let atom = unsafe { ffi::RegisterClassW(&raw const class) };
        if atom == 0 {
            // SAFETY: no preconditions; reads this thread's last-error slot.
            let err = unsafe { ffi::GetLastError() };
            const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(io::Error::from_raw_os_error(
                    i32::try_from(err).unwrap_or(-1),
                ));
            }
        }

        // The requested size is the *client* area; the OS wants the outer size,
        // and the difference is the frame. Asking for 1920x1080 and getting
        // 1904x1041 of drawable area is the classic version of this bug, and it
        // shows up as a desktop that is subtly scaled.
        let mut rect = ffi::RECT {
            left: 0,
            top: 0,
            right: i32::try_from(width).unwrap_or(i32::MAX),
            bottom: i32::try_from(height).unwrap_or(i32::MAX),
        };
        // SAFETY: `rect` is a valid, initialised `RECT` we own exclusively.
        unsafe { ffi::AdjustWindowRect(&raw mut rect, ffi::WS_OVERLAPPEDWINDOW, 0) };
        let outer_w = rect.right.saturating_sub(rect.left);
        let outer_h = rect.bottom.saturating_sub(rect.top);

        // SAFETY: both string pointers are NUL-terminated UTF-16 buffers alive
        // for the duration of the call, the class was registered above, and the
        // parent/menu/param arguments are null, which the API accepts for a
        // top-level window with no menu.
        let hwnd = unsafe {
            ffi::CreateWindowExW(
                0,
                class_name.as_ptr(),
                title_wide.as_ptr(),
                ffi::WS_OVERLAPPEDWINDOW | ffi::WS_VISIBLE,
                ffi::CW_USEDEFAULT,
                ffi::CW_USEDEFAULT,
                outer_w,
                outer_h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: no preconditions.
            let err = unsafe { ffi::GetLastError() };
            return Err(io::Error::from_raw_os_error(
                i32::try_from(err).unwrap_or(-1),
            ));
        }

        // A fresh window starts open even if a previous one on this thread set
        // the flag; otherwise a harness that opens a second window would find
        // it already closed.
        CLOSED.set(false);
        PENDING.with(|p| p.borrow_mut().clear());

        // SAFETY: `hwnd` is a window we just created and have not destroyed.
        unsafe { ffi::ShowWindow(hwnd, ffi::SW_SHOW) };

        // `CS_OWNDC` means this device context belongs to the window and stays
        // valid for its lifetime, so it is acquired once rather than per frame.
        // SAFETY: as above.
        let dc = unsafe { ffi::GetDC(hwnd) };
        if dc.is_null() {
            // SAFETY: `hwnd` is ours and still valid.
            unsafe { ffi::DestroyWindow(hwnd) };
            return Err(io::Error::other("the window has no device context"));
        }

        Ok(Self {
            hwnd,
            dc,
            owner: std::thread::current().id(),
            size: (width, height),
        })
    }

    /// The size of the client area, in pixels.
    ///
    /// This is what a compositor should be composing at: the user may have
    /// resized the window.
    #[must_use]
    pub fn client_size(&self) -> (u32, u32) {
        let mut rect = ffi::RECT::default();
        // SAFETY: `hwnd` is valid for this window's lifetime and `rect` is an
        // initialised local we own.
        let ok = unsafe { ffi::GetClientRect(self.hwnd, &raw mut rect) };
        if ok == 0 {
            return self.size;
        }
        let w = u32::try_from(rect.right.saturating_sub(rect.left)).unwrap_or(0);
        let h = u32::try_from(rect.bottom.saturating_sub(rect.top)).unwrap_or(0);
        (w, h)
    }

    /// Set the window's title.
    pub fn set_title(&self, title: &str) {
        let wide_title = wide(title);
        // SAFETY: `hwnd` is valid and the string is NUL-terminated UTF-16 alive
        // for the call.
        unsafe { ffi::SetWindowTextW(self.hwnd, wide_title.as_ptr()) };
    }

    /// Drain the message queue, which is what fills the pending-input list.
    ///
    /// Must be called regularly: a window whose messages are not pumped is one
    /// Windows marks as not responding, and it never repaints.
    fn pump(&mut self) {
        debug_assert_eq!(
            self.owner,
            std::thread::current().id(),
            "a window's messages are delivered only to the thread that created it"
        );
        let mut msg = ffi::MSG::default();
        // Bounded: a message that generates another message could otherwise
        // hold the loop here and stop the desktop drawing. Two hundred is far
        // above a frame's worth of real input.
        for _ in 0..200 {
            // SAFETY: `msg` is an initialised local we own exclusively; a null
            // `hwnd` means "any window belonging to this thread", which is what
            // is wanted.
            let got = unsafe {
                ffi::PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, ffi::PM_REMOVE)
            };
            if got == 0 {
                break;
            }
            if msg.message == ffi::WM_QUIT {
                CLOSED.set(true);
                break;
            }
            // SAFETY: `msg` was filled by `PeekMessageW` and is unmodified.
            unsafe {
                ffi::TranslateMessage(&raw const msg);
                ffi::DispatchMessageW(&raw const msg);
            }
        }
    }
}

impl super::Present for Window {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32) {
        self.pump();
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return;
        };
        let Some(needed) = (width as usize).checked_mul(height as usize) else {
            return;
        };
        if pixels.len() < needed || needed == 0 {
            // A short buffer is the caller's bug. Drawing part of it would
            // read past the end inside GDI, so the frame is skipped: a display
            // server must not be brought down by a bad frame.
            return;
        }

        let info = ffi::BITMAPINFO {
            header: ffi::BITMAPINFOHEADER {
                #[allow(clippy::cast_possible_truncation)]
                bi_size: std::mem::size_of::<ffi::BITMAPINFOHEADER>() as u32,
                bi_width: w,
                // Negative: a DIB is bottom-up by default, and the compositor's
                // buffer is top-down. Without the sign the desktop appears
                // vertically mirrored — which looks like a rendering bug
                // anywhere but here.
                bi_height: -h,
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: ffi::BI_RGB,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            colors: [0; 3],
        };

        let (cw, ch) = self.client_size();
        self.size = (cw, ch);
        let (Ok(dest_w), Ok(dest_h)) = (i32::try_from(cw), i32::try_from(ch)) else {
            return;
        };

        // SAFETY: `dc` belongs to this window (`CS_OWNDC`) and is valid for its
        // lifetime; `info` is a fully initialised `BITMAPINFO` describing
        // exactly `w * h` 32-bit pixels; and `pixels` was checked above to hold
        // at least that many, so GDI reads only within the slice.
        unsafe {
            ffi::StretchDIBits(
                self.dc,
                0,
                0,
                dest_w,
                dest_h,
                0,
                0,
                w,
                h,
                pixels.as_ptr().cast(),
                &raw const info,
                ffi::DIB_RGB_COLORS,
                ffi::SRCCOPY,
            );
        }
    }

    fn input(&mut self) -> Vec<InputEvent> {
        self.pump();
        PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
    }

    fn is_open(&self) -> bool {
        !CLOSED.get()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.owner != std::thread::current().id() {
            // Destroying a window from another thread is a no-op that silently
            // leaks it. Leaking loudly is better than pretending.
            debug_assert!(false, "a window must be dropped on the thread that made it");
            return;
        }
        // SAFETY: `dc` came from `GetDC` on `hwnd`, both still valid.
        unsafe { ffi::ReleaseDC(self.hwnd, self.dc) };
        // SAFETY: `hwnd` is ours and has not been destroyed — `WM_CLOSE` is
        // handled above precisely so that it is not destroyed behind our back.
        unsafe { ffi::DestroyWindow(self.hwnd) };
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{
        button_for, event_for_message, ffi, mouse_point, scancode_from_lparam, wheel_delta, wide,
    };
    use crate::keymap::key_for_scancode;
    use crate::{InputEvent, MouseButton};
    use guitk::event::Key;

    /// Build the `lParam` Windows sends for a key, the way Windows builds it.
    const fn key_lparam(scancode: u8, extended: bool) -> isize {
        let mut bits = (scancode as usize) << 16;
        if extended {
            bits |= 1 << 24;
        }
        #[allow(clippy::cast_possible_wrap)]
        {
            bits as isize
        }
    }

    #[test]
    fn a_plain_key_keeps_its_scancode() {
        // 0x1E is `A` on a set-1 keyboard.
        assert_eq!(scancode_from_lparam(key_lparam(0x1E, false)), Some(0x1E));
    }

    #[test]
    fn an_extended_key_gains_the_prefix_the_keymap_expects() {
        // This is the whole point of the module: 0x4B is keypad 4 and
        // 0xE04B is Left arrow, and they must not be confused. Asserting
        // through the *real* keymap rather than on the number, because the
        // number is only interesting if it names the right key.
        let plain = scancode_from_lparam(key_lparam(0x4B, false)).unwrap();
        let extended = scancode_from_lparam(key_lparam(0x4B, true)).unwrap();
        assert_eq!(extended, 0xE04B);
        assert_eq!(key_for_scancode(extended), Key::Left);
        // The bare code is keypad 4, which the system keymap does not yet name
        // (`known-issues.md` → `TD-ONLY-ONE-KEYBOARD-LAYOUT`). What matters here
        // is only that it is not *Left*: an extended bit dropped on the floor
        // would move the caret every time someone typed a 4 on the numeric pad.
        assert_ne!(key_for_scancode(plain), Key::Left);
        assert_eq!(key_for_scancode(plain), Key::Unknown(0x4B));
    }

    #[test]
    fn a_synthetic_key_with_no_scancode_is_dropped_rather_than_becoming_key_zero() {
        // `key_for_scancode(0)` is `Key::Unknown(0)`, which would reach the
        // focused application as a keystroke the user never made.
        assert_eq!(scancode_from_lparam(key_lparam(0, false)), None);
        assert!(event_for_message(ffi::WM_KEYDOWN, 0, key_lparam(0, false)).is_none());
    }

    #[test]
    fn every_key_the_keymap_names_survives_the_round_trip() {
        // A sweep rather than a spot check: if the extended-bit handling were
        // inverted, half of these would come back as the *other* key, and a
        // single example might easily be one of the half that still works.
        let extended: [u8; 10] = [0x48, 0x50, 0x4B, 0x4D, 0x47, 0x4F, 0x49, 0x51, 0x52, 0x53];
        for code in extended {
            let scancode = scancode_from_lparam(key_lparam(code, true)).unwrap();
            assert_eq!(scancode, 0xE000 | u32::from(code));
            assert_ne!(
                key_for_scancode(scancode),
                key_for_scancode(u32::from(code)),
                "0x{code:02X} means different keys with and without the prefix"
            );
        }
    }

    #[test]
    fn a_key_press_and_release_become_the_matching_events() {
        assert!(matches!(
            event_for_message(ffi::WM_KEYDOWN, 0, key_lparam(0x1E, false)),
            Some(InputEvent::KeyDown {
                scancode: 0x1E,
                character: None
            })
        ));
        assert!(matches!(
            event_for_message(ffi::WM_KEYUP, 0, key_lparam(0x1E, false)),
            Some(InputEvent::KeyUp { scancode: 0x1E })
        ));
    }

    #[test]
    fn a_system_key_is_input_too_and_not_swallowed() {
        // Alt-modified keys arrive as WM_SYSKEYDOWN. An application that binds
        // Alt+F would otherwise never see it.
        assert!(matches!(
            event_for_message(ffi::WM_SYSKEYDOWN, 0, key_lparam(0x21, false)),
            Some(InputEvent::KeyDown { scancode: 0x21, .. })
        ));
    }

    #[test]
    fn mouse_coordinates_are_signed_so_a_drag_can_leave_the_window() {
        // The bug this catches: reading the halves as unsigned turns a pointer
        // one pixel left of the window into x = 65535, and a window being
        // dragged flies to the far edge of the desktop.
        let packed =
            ((-2_i32 as u32 as usize) & 0xFFFF) | (((-3_i32 as u32 as usize) & 0xFFFF) << 16);
        #[allow(clippy::cast_possible_wrap)]
        let l_param = packed as isize;
        assert_eq!(mouse_point(l_param), (-2, -3));
    }

    #[test]
    fn a_point_inside_the_window_survives_unchanged() {
        #[allow(clippy::cast_possible_wrap)]
        let l_param = (400_usize | (300_usize << 16)) as isize;
        assert_eq!(mouse_point(l_param), (400, 300));
        assert!(matches!(
            event_for_message(ffi::WM_MOUSEMOVE, 0, l_param),
            Some(InputEvent::MouseMove { x: 400, y: 300 })
        ));
    }

    #[test]
    fn each_button_message_names_its_own_button_and_direction() {
        let cases = [
            (ffi::WM_LBUTTONDOWN, MouseButton::Left, true),
            (ffi::WM_LBUTTONUP, MouseButton::Left, false),
            (ffi::WM_RBUTTONDOWN, MouseButton::Right, true),
            (ffi::WM_RBUTTONUP, MouseButton::Right, false),
            (ffi::WM_MBUTTONDOWN, MouseButton::Middle, true),
            (ffi::WM_MBUTTONUP, MouseButton::Middle, false),
        ];
        for (message, want_button, want_pressed) in cases {
            let Some(InputEvent::MouseButton {
                button,
                pressed,
                x,
                y,
            }) = event_for_message(message, 0, 0)
            else {
                panic!("message 0x{message:04X} produced no button event");
            };
            assert_eq!(button, want_button);
            assert_eq!(pressed, want_pressed);
            assert_eq!((x, y), (0, 0));
        }
    }

    #[test]
    fn the_side_buttons_are_told_apart_by_the_word_they_arrive_with() {
        // The five ordinary buttons say which they are in the message id; these
        // two share one, so a translation that ignored `wParam` would report
        // Back for a Forward click — the browser-navigation bug that looks like
        // history running the wrong way.
        let cases = [
            (ffi::WM_XBUTTONDOWN, ffi::XBUTTON1, MouseButton::Back, true),
            (ffi::WM_XBUTTONUP, ffi::XBUTTON1, MouseButton::Back, false),
            (
                ffi::WM_XBUTTONDOWN,
                ffi::XBUTTON2,
                MouseButton::Forward,
                true,
            ),
            (
                ffi::WM_XBUTTONUP,
                ffi::XBUTTON2,
                MouseButton::Forward,
                false,
            ),
        ];
        for (message, which, want_button, want_pressed) in cases {
            let w_param = usize::from(which) << 16;
            assert_eq!(
                button_for(message, w_param),
                Some((want_button, want_pressed)),
                "message 0x{message:04X} with XBUTTON{which}"
            );
        }
    }

    #[test]
    fn a_side_button_that_names_no_known_button_is_dropped_rather_than_guessed() {
        // The remaining bits of the high word are reserved. Inventing a button
        // from one would deliver a click to a window nobody touched.
        assert_eq!(button_for(ffi::WM_XBUTTONDOWN, 0x0008 << 16), None);
        assert_eq!(button_for(ffi::WM_XBUTTONDOWN, 0), None);
        assert!(event_for_message(ffi::WM_XBUTTONDOWN, 0, 0).is_none());
    }

    #[test]
    fn a_wheel_notch_is_one_unit_in_the_direction_the_user_turned_it() {
        // Windows sends multiples of 120; the compositor wants notches.
        assert!((wheel_delta(120_usize << 16) - 1.0).abs() < f32::EPSILON);
        assert!((wheel_delta((-120_i32 as u32 as usize) << 16) + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_high_resolution_wheel_reports_a_fraction_rather_than_nothing() {
        // A precision trackpad sends deltas well under 120. Truncating them to
        // whole notches makes it scroll in jumps, or not at all.
        let eighth = wheel_delta(15_usize << 16);
        assert!(
            eighth > 0.0 && eighth < 0.2,
            "an eighth of a notch: {eighth}"
        );
    }

    #[test]
    fn the_two_wheels_move_different_axes() {
        let Some(InputEvent::MouseScroll { dx, dy, .. }) =
            event_for_message(ffi::WM_MOUSEWHEEL, 120_usize << 16, 0)
        else {
            panic!("no scroll event");
        };
        assert!(dx.abs() < f32::EPSILON, "the vertical wheel is vertical");
        assert!(dy > 0.0);

        let Some(InputEvent::MouseScroll { dx, dy, .. }) =
            event_for_message(ffi::WM_MOUSEHWHEEL, 120_usize << 16, 0)
        else {
            panic!("no scroll event");
        };
        assert!(dx > 0.0);
        assert!(dy.abs() < f32::EPSILON, "and the horizontal one is not");
    }

    #[test]
    fn a_message_that_is_not_input_produces_none() {
        assert!(event_for_message(ffi::WM_DESTROY, 0, 0).is_none());
        // `WM_CHAR` is text, and the window procedure turns it into a
        // `TextInput` directly; the message translation must not *also* produce
        // something, or every letter typed would arrive twice.
        assert!(event_for_message(ffi::WM_CHAR, u32::from('a') as usize, 0).is_none());
    }

    #[test]
    fn a_wide_string_is_nul_terminated_and_survives_non_ascii() {
        let w = wide("Slate—OS");
        assert_eq!(w.last(), Some(&0), "the W APIs read until a NUL");
        assert_eq!(
            String::from_utf16(&w[..w.len() - 1]).unwrap(),
            "Slate—OS",
            "and the em dash is not mangled on the way"
        );
    }
}
