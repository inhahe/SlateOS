## TD-C-BLUR-IS-SOFTWARE-ONLY

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/compositor/src/lib.rs` — `Compositor::blur_behind_window`.

**In short:** the translucent-blurred look the taskbar and menus now ask for
works when the compositor is drawing with the CPU, and quietly does nothing
when it is drawing with the graphics card. Nothing breaks — the surface is just
drawn plain — but on a machine with working graphics acceleration the feature
is invisible, which is the opposite of where you would expect it to work.

**Why.** A backdrop blur has to *read pixels back*: it exists to show a softened
version of what is already behind a surface. The software target has a
`Vec<u32>` to read and rewrite. A hardware backend has no mutable pixel
access, and deliberately so — a GPU blur is a shader sampling a texture, not a
CPU pass over an array — so `RenderBackend::as_software_mut()` returns `None`
and the pass returns early.

**Why it was built this way anyway.** The alternative was to leave 2,223 lines
of written, tested blur code unreachable for longer, which is what it had been
since before the three-lane split (see
`TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED`). `BlurKind` is
advisory in the way `WindowSpec::transparent` is: a compositor that cannot
honour it draws the surface flat and does not tell the client. A plainer
taskbar is not a broken desktop, which is exactly why this is a limitation and
not a bug — unlike `layer`, which is refused rather than demoted, because a
panel behind the windows *is* a broken desktop.

**The proper fix** is a shader path in the hardware backend: render the region
behind the surface to an offscreen texture, blur it there (two-pass separable
Gaussian, which is what `BlurRenderer` already does on the CPU), and sample it
when compositing the surface. The parameters are already resolved
compositor-side from `BlurKind`, so nothing about the protocol changes; the
work is entirely in the backend.

**Do not "fix" this by making `RenderTarget` expose mutable pixels.** That
would put a method on the trait that every implementor but one has to refuse,
and it would make the software fallback the definition of the feature. The
per-backend split is the honest shape.
