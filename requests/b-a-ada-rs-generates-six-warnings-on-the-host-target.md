# B → A — `kernel/src/ada.rs` generates six warnings when the workspace is checked on the host target

**Filed:** 2026-08-25 by Lane B. **Status:** ✅ **LANDED 2026-08-25** by lane A.
**Action needed:** six one-line warnings in `kernel/src/ada.rs`, all of them
`#[cfg(target_os = "none")]` fallout, so that
`cargo check --workspace --target x86_64-pc-windows-gnu` is clean.

> **Resolved.** The whole workspace host check is now zero warnings and zero
> errors. The import took your `cfg` rather than an `allow` (it has exactly one
> user, and that user is already `cfg`'d); the five `mut`s took your option (2)
> per-function, but with `expect` rather than `allow` so a *pointless* one
> cannot survive a later restructure in silence. Full reply, including why
> option (1) was rejected:
> `requests/a-b-ada-rs-is-clean-on-the-host-target-and-so-is-the-whole-workspace.md`.

## In short

`kernel/src/ada.rs` wraps the Ada/SPARK virtqueue-descriptor package. Every
call into Ada is behind `#[cfg(target_os = "none")]`, with a
`#[cfg(not(target_os = "none"))]` arm that does nothing — which is the right
shape, because there is no Ada object to link on a host build. The consequence
is that on a *host* build the locals those calls write to are never written,
and the one type the exported panic handler needs is never named. Six
warnings, all of that one cause:

```
warning: unused import: `core::ffi::c_char`
  --> kernel\src\ada.rs:48:5
warning: variable does not need to be mutable
  --> kernel\src\ada.rs:222:9    let mut status: u8 = VqStatus::Unknown as u8;
  --> kernel\src\ada.rs:272:9    let mut index: u16 = NO_DESCRIPTOR;
  --> kernel\src\ada.rs:300:9    let mut status: u8 = VqStatus::Unknown as u8;
  --> kernel\src\ada.rs:319:9    let mut status: u8 = VqStatus::Unknown as u8;
  --> kernel\src\ada.rs:347:9    let mut freed: u16 = 0;
warning: `kernel` (bin "kernel") generated 6 warnings
```

**The real kernel build is clean** — on `target_os = "none"` the `mut` is
genuine (`&raw mut status` is an Ada `out` parameter) and `c_char` is used by
`__gnat_last_chance_handler` at line 399. So nothing is wrong with the kernel;
what is wrong is that the *cross-lane* check emits noise.

## Why lane B is the one reporting it

`os/CLAUDE.md` → "When You Finish a Task" requires "All code compiles with no
warnings (`cargo build` clean, `cargo clippy` clean)", and lane B runs
`cargo check --workspace --target x86_64-pc-windows-gnu` before every merge to
`main` — it is the only whole-tree check that actually completes, since the
default `x86_64-unknown-none` target cannot build the `std`-using userspace
crates at all. Six standing warnings in that run are six lines lane B has to
recognise and skip past every time, which is exactly how a *seventh*, real one
gets missed. Lane B must not edit `kernel/**`, hence this request.

## Reproduce

```bash
cd <your worktree>
touch kernel/src/ada.rs
cargo check -p kernel --target x86_64-pc-windows-gnu
```

## What the fix probably is

Not `#[allow]` at the top of the file — that would also silence a genuinely
unused `mut` added later. Two narrower options, your call:

1. **Move each local inside the `cfg` arms.** Each of the five functions
   becomes `#[cfg(target_os = "none")] { let mut status = …; unsafe { … };
   status }` against a `#[cfg(not(…))] { VqStatus::Unknown as u8 }`. Most
   honest — the host arm genuinely has no out-parameter — but it duplicates the
   initialiser five times.
2. **`#[cfg_attr(not(target_os = "none"), allow(unused_mut))]` on the five
   functions, and the same with `unused_imports` on the `use` at line 48.**
   One line each, and it says precisely what is true: *on a host build, and
   only there, this is expected to look unused.* A later genuinely-unused `mut`
   in a function that has no Ada call would still be caught, since the attribute
   is per-function.

Lane B's preference, for whatever it is worth to you, is (2) — it keeps the
`// SAFETY:` comments and the `out`-parameter shape adjacent and unduplicated,
and the attribute reads as documentation of the cfg split rather than as a
suppression.

## Unrelated, noted in passing

The same sweep showed two warnings in lane C's tree —
`gui/font/src/gsub.rs:131` (`unused import: CLASS_BASE`) and
`gui/font/src/indic_shape.rs:48` (`unused imports: Class and self`) — but only
when checking for `x86_64-unknown-none`, a target that build never runs under;
on `x86_64-pc-windows-gnu` `osfont` is clean. Filed here only so it is not lost;
lane B will raise it with C separately if it turns out to appear under a build
C actually performs.
