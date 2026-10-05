## §461 — The compositor draws through a `Present` trait, and the first implementation is raw Win32

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The compositor assembled a complete picture of the desktop every
frame and then threw it away, because it had nothing to draw *on* — SlateOS's
display driver does not exist yet, and the development machine's screen belongs
to Windows. For the same reason it could not be typed at: a program running as
a Windows process has no keyboard of its own. Both gaps had one cause, so one
small interface closes both: a "display" is now anything that can accept a
finished frame and report what the user did. The first one written talks to
Windows directly, in about 400 lines of function declarations, rather than
pulling in an off-the-shelf library — which matters because adding a library
here needs another lane's permission and adding these declarations needs
nobody's.

**The problem.** `Compositor::compose_frame()` blends every visible window, the
cursor and the desktop furniture into a buffer and flips it. `front_buffer()`
hands out finished ARGB pixels. `Server::run` looked at them only to increment a
counter. Symmetrically, `Compositor::handle_input` was reachable only from test
code: a hosted build has no keyboard driver, so `route_input` — the whole
mechanism that decides which client a keystroke belongs to — had nothing to
route. The tree therefore had a rendering pipeline that no human had ever seen
the output of, and an input pipeline that no human had ever exercised, and both
were fully written and fully tested against themselves.

That is the specific failure mode `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` was
opened about, one level further down: code can be green against a stand-in on
both ends and still be wrong.

**The interface.** `Present`, in `gui/compositor/src/present.rs`:

```rust
pub trait Present {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32);
    fn input(&mut self) -> Vec<InputEvent> { Vec::new() }
    fn is_open(&self) -> bool { true }
}
```

Three things, and the reasoning for each being there rather than elsewhere:

- **`show` takes pixels, width and height, not a `&Compositor`.** A display
  should not be able to reach back into the compositor's state; and the SlateOS
  implementation will be a `memcpy` into a mapped framebuffer, which wants
  exactly these three values and nothing else.
- **`input` is on the same trait rather than a second one.** Every device that
  can show a frame is also where the user is sitting. Splitting them would mean
  every caller wiring two objects that are always the same object, and the loop
  would have to decide what to do when they disagree about being open.
- **`is_open` exists because a host window can be closed by someone who is not
  us.** A SlateOS framebuffer never closes, so the default is `true`; a Win32
  window returns `false` on `WM_CLOSE`, and that is how the shipped binary
  exits.

**The alternatives considered for the trait's shape.**

*One method, `show`, with input as a separate concern.* Rejected above: the two
are the same device, and the loop needs them ordered with respect to each other
— input must be applied *before* clients are served, so that a click which
raised a window is reflected in the events those clients hear about this frame
rather than next. That ordering is only expressible if one loop owns both.

*A callback/closure instead of a trait.* It reads well for `show` alone and
badly for a thing with three methods and per-frame state. `Recording` — the test
double — holds a `Vec` of frames and a tick counter; as closures that becomes
shared mutable state threaded through `run_with`.

*Give `Compositor` a `present()` method and have implementations be `Display`
subtypes.* This conflates two ideas the crate already keeps apart:
`display.rs`'s `Display` describes a *mode* (resolution, refresh, scale) and is
pure policy with no hardware in it. A `Present` is a *device*. Merging them
would mean the mode table could no longer be constructed in a test without a
screen.

**Why raw `extern "system"` rather than `softbuffer`, `minifb` or `winit`.**
This is the part with a genuine cost on both sides.

| | A crate | Raw FFI |
|---|---|---|
| Workspace `Cargo.toml` | needs an edit lane C does not own — a `requests/` file and a wait | nothing; `#[link]` attributes are source |
| Portability | X11/Wayland/macOS for free | Windows only, today |
| Code to review | none of ours | ~400 lines of declarations + a window procedure |
| Failure mode | a dependency tree of unknown size in a kernel project | a linker error, immediately |
| `unsafe` | hidden behind a safe API | ours, and every block needs a `SAFETY:` comment |

The deciding argument is not the dependency count, though a kernel project
adding a windowing stack to its workspace root is a real thing to be reluctant
about. It is that **this code is scaffolding with a known demolition date.**
Its whole purpose is to let a human look at the compositor's output until
SlateOS can display it; on the day the display driver lands, this file stops
being on any path that matters. Taking a permanent workspace dependency, and
another lane's time to approve it, to host something temporary is the wrong
trade — whereas 400 lines of `extern` blocks in one `#[cfg(windows)]` file are
deleted by deleting the file.

The cost is honest and worth stating: this harness runs on Windows only. A
future contributor on Linux gets `--headless` and the `Recording` double, which
is exactly what the tests use, so nothing is *untestable* there — but nobody on
Linux can look at the desktop. If that becomes a real constraint, an X11
implementation is another file next to this one and the trait does not move.

**Why the binary opens a window by default on Windows.** The other option is
headless-by-default with `--windowed` to opt in. Windowed-by-default was chosen
because the harness's entire value is that someone *looks* at it, and a
development tool that must be asked twice is a development tool nobody uses. It
degrades safely: `Window::new` failing (a service, a session with no desktop)
prints a warning and continues headless rather than exiting, because serving
remote clients is still completely useful without a local screen.

**What this does not decide.** It is not scanout.
`TD-COMPOSITOR-HAS-NO-SCANOUT` stays open, and its item (1) — a mapping of the
SlateOS display driver's framebuffer — remains the actual fix. The claim being
made here is narrower and is the reason the trait was written before the driver
exists: when that driver lands it is one more `impl Present`, and `Server`,
`Compositor` and the binary do not change.

**Revisit if** a second platform needs a window, which is the point at which
"400 lines per platform" starts losing to a crate; or if the SlateOS
framebuffer turns out to need something `show(pixels, w, h)` cannot express —
a page flip that hands over buffer *ownership* rather than copying is the
plausible one, and would make `show` take something like a `&mut Frame` instead.
