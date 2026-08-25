//! The only part of local input that cannot be tested off the target.
//!
//! Four system calls — `open`, `read`, `ioctl`, `close` — and the rule for
//! turning a kernel return value into an error. Everything else about reading a
//! keyboard and a mouse is protocol and policy, lives in [`super`], and is
//! exercised on the build machine against a fake device.
//!
//! This mirrors [`drm::sys`](crate::present::drm::sys) deliberately, down to
//! the shape of the traits, because it is the same problem: a display server
//! must be provably correct about a device the development machine does not
//! have.
//!
//! ## Why the seam is where it is
//!
//! An evdev fd is simpler than a DRM one — `read` returns whole records into a
//! caller-supplied buffer and the one ioctl we issue (`EVIOCGKEY`) writes a
//! flat bitmap into another — so the trait can take plain `&mut [u8]` and no
//! part of [`super`] ever forms an address. That is the property that matters:
//! a fake receives slices it can write into safely, and the real
//! implementation's `unsafe` is confined to this file.
//!
//! ## `O_NONBLOCK` is not optional
//!
//! [`Present::input`](crate::present::Present::input) is called once per frame
//! from the same loop that composes and scans out. A blocking `read` on an idle
//! keyboard would stop the desktop until someone typed — no cursor, no
//! animation, no client servicing. The devices are therefore opened
//! `O_RDONLY | O_NONBLOCK`, and [`EAGAIN`] is the ordinary answer, not a fault.

/// A kernel error, as a positive `errno`.
pub type Errno = i32;

/// No such file — from `open`, means this `/dev/input/eventN` does not exist.
pub const ENOENT: Errno = 2;
/// Interrupted by a signal — the call did nothing and can be repeated.
pub const EINTR: Errno = 4;
/// Would block. From `read` on an `O_NONBLOCK` device, means *nothing has
/// happened*, which on an idle desktop is every frame.
pub const EAGAIN: Errno = 11;
/// Permission denied. From `open`, means this process holds no
/// `ResourceType::InputDevice` capability — see the module docs of [`super`],
/// which is the failure that cannot be fixed from inside the compositor.
pub const EACCES: Errno = 13;
/// No such device.
pub const ENODEV: Errno = 19;
/// Invalid argument. From `read`, means the buffer was smaller than one
/// record — the kernel refuses to return a partial one.
pub const EINVAL: Errno = 22;

/// An open input device: something records can be read from and interrogated.
///
/// `&mut self` throughout because an evdev fd has kernel-side per-fd state —
/// its own cursor into the event ring, and whether it holds the exclusive grab
/// — and two threads reading one is a bug regardless of what Rust would allow.
pub trait EventSys {
    /// Read whole `struct input_event` records into `buf`.
    ///
    /// Returns how many bytes were written, always a multiple of
    /// [`uapi::EVENT_SIZE`](super::uapi::EVENT_SIZE). Never returns `Ok(0)`:
    /// an input device has no end of file, so a zero would be the kernel
    /// misbehaving and is reported as [`ENODEV`] rather than quietly read as
    /// "the keyboard is gone".
    ///
    /// # Errors
    ///
    /// The kernel's `errno` — [`EAGAIN`] when nothing has happened, which is
    /// the common case and not a failure.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno>;

    /// Issue an ioctl whose argument is a buffer the kernel fills in.
    ///
    /// Returns the kernel's return value, which for the `EVIOCG*` family is the
    /// number of bytes actually written — a device with fewer keys than the
    /// caller asked about returns a shorter bitmap, and treating the untouched
    /// tail as real would report keys held that do not exist.
    ///
    /// # Errors
    ///
    /// The kernel's `errno`.
    fn ioctl_read(&mut self, request: u32, buf: &mut [u8]) -> Result<usize, Errno>;
}

/// Something that can open `/dev/input/eventN` by index.
///
/// The seam that makes device *selection* testable — which device is the
/// keyboard and which the mouse, and what happens when one of them is missing,
/// is policy, and policy with no test is policy that is wrong on the machine
/// nobody can run it on.
pub trait DeviceSource {
    /// What an opened device is.
    type Sys: EventSys;

    /// Open `/dev/input/event{index}`, read-only and non-blocking.
    ///
    /// # Errors
    ///
    /// The kernel's `errno` — [`ENOENT`] when there is no such node, which is
    /// the ordinary answer for every index past the last real device, and
    /// [`EACCES`] when the process was not granted an input capability.
    fn open(&mut self, index: u32) -> Result<Self::Sys, Errno>;
}

/// The longest `/dev/input/eventN\0` for `N < 100`.
const EVENT_PATH_LEN: usize = 23;

/// A NUL-terminated `/dev/input/eventN` path, without allocating.
///
/// Built the same way [`CardPath`](crate::present::drm::sys::CardPath) is, and
/// for the same reason: the syscall layer should not depend on the allocator
/// being in a working state to name the device it is about to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventPath {
    /// The bytes, NUL-terminated at `len - 1`.
    bytes: [u8; EVENT_PATH_LEN],
    /// How many bytes are used, including the NUL.
    len: usize,
}

impl EventPath {
    /// The path of `/dev/input/event{index}`.
    ///
    /// Indices at or above 100 are clamped to 99, which cannot happen: the only
    /// caller bounds itself by [`MAX_DEVICES`]. Clamping rather than returning
    /// an `Option` keeps this infallible, since a path that cannot be formed is
    /// not a case any caller has a sensible answer for.
    #[must_use]
    pub fn event(index: u32) -> Self {
        const PREFIX: &[u8] = b"/dev/input/event";
        let mut bytes = [0u8; EVENT_PATH_LEN];
        let mut len = 0usize;
        for &b in PREFIX {
            if let Some(slot) = bytes.get_mut(len) {
                *slot = b;
                len = len.saturating_add(1);
            }
        }
        let index = index.min(99);
        if index >= 10 {
            if let Some(slot) = bytes.get_mut(len) {
                *slot = b'0'.saturating_add(u8::try_from(index / 10).unwrap_or(0));
                len = len.saturating_add(1);
            }
        }
        if let Some(slot) = bytes.get_mut(len) {
            *slot = b'0'.saturating_add(u8::try_from(index % 10).unwrap_or(0));
            len = len.saturating_add(1);
        }
        // The NUL. `bytes` is zeroed, so this only advances the length.
        len = len.saturating_add(1);
        Self { bytes, len }
    }

    /// The path including its trailing NUL, as the kernel wants it.
    #[must_use]
    pub fn as_c_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }

    /// The path without its trailing NUL, for a diagnostic.
    #[must_use]
    pub fn as_display_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len.saturating_sub(1)).unwrap_or(&[])
    }
}

impl core::fmt::Display for EventPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Every byte this type can hold is ASCII, so the lossy conversion is
        // exact. It is used rather than `from_utf8` because a formatter cannot
        // return the error a `from_utf8` failure would produce, and there is no
        // input that could produce one.
        f.write_str(&String::from_utf8_lossy(self.as_display_bytes()))
    }
}

/// How many `/dev/input/eventN` nodes are worth trying.
///
/// The SlateOS kernel exposes two today and `/dev/input/` cannot be listed
/// (devfs is flat — see `known-issues.md` and lane A's request), so discovery
/// is "open the first N and ask each what it is". Thirty-two is far above any
/// desktop's device count and keeps a machine with no input devices to
/// thirty-two failed `open`s, once, at startup.
pub const MAX_DEVICES: u32 = 32;

#[cfg(target_os = "linux")]
pub use target::{Device, Devices};

/// The real thing: a `/dev/input/eventN` file descriptor and raw system calls.
///
/// Gated on `target_os = "linux"` rather than on a SlateOS-specific cfg because
/// the SlateOS target *is* `target_os = "linux"` (see
/// `toolchain/x86_64-slateos.json`), and because the ABI this speaks is the
/// real one — a build of the compositor for a Linux host reads a Linux
/// keyboard with this same code.
#[cfg(target_os = "linux")]
mod target {
    use super::{Errno, EventSys};
    use std::arch::asm;

    /// `read`.
    const SYS_READ: u64 = 0;
    /// `open`.
    const SYS_OPEN: u64 = 2;
    /// `close`.
    const SYS_CLOSE: u64 = 3;
    /// `ioctl`.
    const SYS_IOCTL: u64 = 16;

    /// `open` flags: read-only — this is an input device, there is nothing to
    /// write to it.
    const O_RDONLY: u64 = 0;
    /// Never block the compositing loop on an idle keyboard.
    const O_NONBLOCK: u64 = 0o4000;
    /// Don't leak the keyboard into a child across `exec`. A compositor spawns
    /// applications; none of them should inherit a device that can read every
    /// keystroke on the machine, passwords included.
    const O_CLOEXEC: u64 = 0o2_000_000;

    /// Issue a system call with six arguments.
    ///
    /// Returns the kernel's raw return value: negative values in `-4095..0`
    /// are `-errno`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the arguments are valid for the syscall named by
    /// `n` — in particular that any pointer argument points to memory of the
    /// size the kernel will read or write.
    #[inline]
    unsafe fn syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, both
        // declared `lateout(_)` below, and returns its result in `rax`. The
        // register assignment (rdi, rsi, rdx, r10, r8, r9) is the x86-64 Linux
        // syscall ABI. Validity of the arguments themselves is this function's
        // documented precondition.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") n as i64 => ret,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                in("r10") a4,
                in("r8") a5,
                in("r9") a6,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        ret
    }

    /// Turn a raw syscall return into a `Result`.
    ///
    /// Linux signals failure by returning `-errno` in the range `-4095..0`;
    /// everything else is a successful result.
    fn decode(ret: i64) -> Result<i64, Errno> {
        if (-4095..0).contains(&ret) {
            // `ret` is in -4095..0, so the negation is in 1..=4095 and fits in
            // an `i32`. `checked_neg` rather than `-` anyway: the one input
            // that would make the unary minus overflow is `i64::MIN`, and the
            // fact that the range check already excludes it is a fact about
            // two lines that could drift apart.
            Err(ret
                .checked_neg()
                .and_then(|v| Errno::try_from(v).ok())
                .unwrap_or(super::ENODEV))
        } else {
            Ok(ret)
        }
    }

    /// An open input device.
    ///
    /// Closes its file descriptor on drop, which also releases any exclusive
    /// grab taken on it — so a compositor that panics does not leave the
    /// machine's keyboard held by a process that no longer exists.
    #[derive(Debug)]
    pub struct Device {
        /// The file descriptor. Always non-negative while this exists.
        fd: i32,
    }

    impl Device {
        /// Open an input device by path.
        ///
        /// `path` must be NUL-terminated;
        /// [`EventPath::as_c_bytes`](super::EventPath::as_c_bytes) is.
        ///
        /// # Errors
        ///
        /// The kernel's `errno` — `ENOENT` when there is no such device node,
        /// `EACCES` when the compositor holds no input capability.
        pub fn open(path: &[u8]) -> Result<Self, Errno> {
            // SAFETY: `path.as_ptr()` is valid for `path.len()` bytes and the
            // caller's contract is that it is NUL-terminated within them, so
            // the kernel's read stops inside the slice. `open` writes nothing.
            let ret = unsafe {
                syscall6(
                    SYS_OPEN,
                    path.as_ptr() as u64,
                    O_RDONLY | O_NONBLOCK | O_CLOEXEC,
                    0,
                    0,
                    0,
                    0,
                )
            };
            let fd = decode(ret)?;
            Ok(Self {
                fd: i32::try_from(fd).unwrap_or(-1),
            })
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            if self.fd >= 0 {
                // SAFETY: `close` takes an integer and touches no memory. The
                // fd came from a successful `open` and is closed exactly once,
                // because `Device` is not `Clone` and this runs at most once.
                let _ = unsafe { syscall6(SYS_CLOSE, self.fd as u64, 0, 0, 0, 0, 0) };
            }
        }
    }

    impl EventSys for Device {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
            // SAFETY: `buf` is a live, exclusively borrowed slice for the
            // duration of the call, and the kernel is told exactly how many
            // bytes of it it may write.
            let ret = unsafe {
                syscall6(
                    SYS_READ,
                    self.fd as u64,
                    buf.as_mut_ptr() as u64,
                    buf.len() as u64,
                    0,
                    0,
                    0,
                )
            };
            let n = decode(ret)?;
            if n <= 0 {
                // An input device has no end of file. A zero here is the
                // kernel misbehaving, and reading it as "the device is
                // finished" would silently stop the keyboard working.
                return Err(super::ENODEV);
            }
            Ok(usize::try_from(n).unwrap_or(0).min(buf.len()))
        }

        fn ioctl_read(&mut self, request: u32, buf: &mut [u8]) -> Result<usize, Errno> {
            // SAFETY: `buf` is a live, exclusively borrowed slice, and the
            // length the kernel will write is encoded in `request` itself by
            // `uapi::ioc` — the callers in `super` build the request from the
            // very slice they pass, so the two cannot disagree.
            let ret = unsafe {
                syscall6(
                    SYS_IOCTL,
                    self.fd as u64,
                    u64::from(request),
                    buf.as_mut_ptr() as u64,
                    0,
                    0,
                    0,
                )
            };
            let n = decode(ret)?;
            Ok(usize::try_from(n).unwrap_or(0).min(buf.len()))
        }
    }

    /// The real [`DeviceSource`](super::DeviceSource): opens
    /// `/dev/input/eventN`.
    ///
    /// A unit struct rather than a free function so that device *selection* can
    /// be written once, generically, and driven by a fake in a test.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Devices;

    impl super::DeviceSource for Devices {
        type Sys = Device;

        fn open(&mut self, index: u32) -> Result<Self::Sys, Errno> {
            Device::open(super::EventPath::event(index).as_c_bytes())
        }
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

    use super::{EventPath, MAX_DEVICES};

    #[test]
    fn a_device_path_is_the_one_the_kernel_publishes() {
        assert_eq!(EventPath::event(0).as_display_bytes(), b"/dev/input/event0");
        assert_eq!(EventPath::event(1).as_display_bytes(), b"/dev/input/event1");
        assert_eq!(
            EventPath::event(31).as_display_bytes(),
            b"/dev/input/event31"
        );
    }

    #[test]
    fn a_device_path_is_nul_terminated_because_open_takes_a_c_string() {
        let path = EventPath::event(7);
        let bytes = path.as_c_bytes();
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(&bytes[..bytes.len() - 1], b"/dev/input/event7");
    }

    #[test]
    fn every_index_the_search_will_try_forms_a_path_that_fits() {
        // The buffer is fixed-size, so the bound on the search and the size of
        // the buffer have to agree. They are two constants in two places, which
        // is exactly the pair that drifts.
        for index in 0..MAX_DEVICES {
            let path = EventPath::event(index);
            assert_eq!(path.as_c_bytes().last(), Some(&0), "index {index}");
            assert!(
                path.as_display_bytes().starts_with(b"/dev/input/event"),
                "index {index}"
            );
        }
    }

    #[test]
    fn the_path_renders_for_a_diagnostic() {
        assert_eq!(EventPath::event(2).to_string(), "/dev/input/event2");
    }
}
