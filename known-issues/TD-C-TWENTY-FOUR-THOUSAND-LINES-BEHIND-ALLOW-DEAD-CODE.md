## TD-C-TWENTY-FOUR-THOUSAND-LINES-BEHIND-ALLOW-DEAD-CODE

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/toolkit/src/` — `textview.rs` (4,018 lines), `svg.rs` (3,392),
`menubar.rs` (3,341), `colorpicker.rs` (2,595), `filetypes.rs` (2,203),
`disabled.rs` (1,739), `context_ext.rs` (1,604). Plus
`apps/imageviewer/src/video.rs` (2,435) and
`apps/procexplorer/src/features.rs` (2,539).

**In short:** roughly twenty-four thousand lines of this lane's code are
reached by nothing. Seven of them are widgets in the shared toolkit — a text
view, an SVG renderer, a menu bar, a colour picker, a file-type table, a
disabled-state helper, a context-menu extension — that no application uses.
Two are features inside applications (video playback in the image viewer,
and a features module in the process explorer) that their own `main.rs` never
calls. Every one of these files opens with `#![allow(dead_code)]`, so the
compiler has never once mentioned it.

**The measurement**, so it can be repeated rather than believed:

```
for f in $(grep -rl "^#!\[allow(dead_code)\]" --include=*.rs gui/ apps/); do
  # ... count `<module>::` references in sibling files, and for a library
  # crate, count crates outside it that name `guitk::<module>`
done
```

For the toolkit the sibling count is not the test — its modules are `pub` and
exist to be used from outside — so the number that matters is **crates outside
`gui/toolkit` that mention `guitk::<module>`**. For the seven above that number
is **zero**. For comparison, the same measurement gives `grid` 5, `scaling` 3
and `pathbar` 1, which is what a used module looks like.

**A toolkit widget nobody has used yet is not automatically waste** — that is
the honest counter-argument, and it is why this is a debt entry and not a
deletion. A toolkit is built ahead of its callers. The reason it is *debt* is
the next paragraph.

**One verified case of an application reimplementing what the toolkit already
had.** `guitk::colorpicker` defines `Hsv { h: f32, s: f32, v: f32 }` with
`hsv_to_rgb`, `rgb_to_hsv` and `color_to_hex_string`. `apps/colorpicker`
defines its own `Hsv { h: f32, s: f32, v: f32 }` — the same three fields, the
same units, documented the same way — and its own conversions. Neither knows
about the other. That is the cost the `allow` is hiding: not the unused lines,
but the second implementation written because nobody could see the first.

**Deliberately not claimed:** that the other six are duplicated too. It was
checked for `filetypes` against `apps/explorer`'s file-type handling and the
two are *not* the same thing, so the pattern is not assumed. `textview`,
`svg`, `menubar`, `disabled` and `context_ext` have not been checked
either way.

**What to do, in order.**

1. ~~**Take the `#![allow(dead_code)]` off `gui/toolkit`'s modules**~~ —
   **done 2026-09-08.** It was as quiet as predicted: seven attributes removed,
   **three** warnings, all of them genuinely-unused *private* items. A `pub`
   item in a library is not dead code to rustc, so the attribute had been
   earning nothing on those files while suppressing the three things it should
   have been reporting. Those three are worth listing, because two of them were
   not what they looked like:

   * `disabled::DISABLED_OPACITY` — the module documents 50% as the standard
     dimming for a disabled control, `render_disabled` takes the opacity as a
     *parameter*, and the constant was private. So every caller would have
     written `0.5` itself, which is how a standard stops being one. Now `pub`.
   * `colorpicker::DialogLayout::height` — a stored copy of a value that *is*
     used (`button_y` is measured back from it) but only during construction.
     The field was removed. **The first attempt at this comment claimed the
     layout ignored its height entirely and that the buttons could fall off a
     short dialog; the compiler disproved that three lines later.** Worth
     recording as a caution: an unused *field* is not evidence that the
     *value* is unused.
   * `textview::col_x` — deleted as dead, and it was not: its only caller is a
     `#[cfg(test)]` test that asserts its clamping rule, which a *lib* build's
     "never used" warning does not account for. Restored, and marked
     `#[cfg(test)]` so both builds are quiet. **The general lesson: "never
     used" from `cargo build` means "no non-test caller", which is a different
     claim.**
2. **Decide the two application modules.** `imageviewer/video.rs` and
   `procexplorer/features.rs` are unreachable inside their own binaries, where
   `pub` buys nothing.

   **Checked 2026-09-08, and "wire it" is wrong for both** — which is worth
   recording, because it was written above as if the only question were
   plumbing.

   * `procexplorer/features.rs` holds six *finished* widgets — window picker,
     blocking analyser, affinity mask, priority selector, environment viewer,
     memory map — each with its own `render() -> Vec<RenderCommand>`. Wiring
     them into the Details tab really is a few lines. But
     `ProcessExplorer::refresh()` is a **placeholder that calls nothing**: its
     own comment says "in production, call kernel syscalls here", and the data
     comes from `load_demo_data()`. So the whole application displays invented
     processes, and wiring `features.rs` would connect one pile of demo data to
     another — a *more elaborate* display of things that are not true, which is
     worse than a plain one. The unwired module is a symptom; the missing data
     source is the disease. See
     `TD-C-THE-GUI-PROCESS-EXPLORER-HAS-NO-DATA-SOURCE-AND-ONE-EXISTS`.
   * `imageviewer/video.rs` says plainly in its own module doc that "actual
     codec decoding is deferred to a future hardware-accelerated decoder
     service". Wired today it would parse containers, show transport controls,
     and display no frames. That is the same trap: a feature that appears to
     exist and does nothing. Its trigger is a decoder service, not a caller.
3. ~~**Route `apps/colorpicker` through `guitk::colorpicker`**~~ — **done
   2026-09-08**, and it turned out not to be tidying. The app's own
   `from_hsv` computed its sector as `(h / 60.0).floor() as u32`, and `as u32`
   *saturates* in Rust: every negative hue became sector 0, so **every hue
   below zero rendered as the same red**. Hue −60 gave `(255, 0, 0)` where it
   should give magenta. The shared version takes the hue modulo 360 and clamps
   s and v first, so routing through it fixed the bug as a side effect of
   removing the duplicate.

   Latent rather than live: the app's own `to_hsv` always returns a hue in
   [0, 360), so nothing in the current UI reaches the bad path. It was
   reachable by any caller constructing an `Hsv` directly — which is what the
   new regression test does, and it fails against the old implementation.

   **This is the argument for the whole entry, in one case.** The duplicate was
   not two copies of a correct thing; it was a correct one and a subtly broken
   one, and the `#![allow(dead_code)]` on the correct one is why nobody
   compared them.

   **The generalisation was swept, and came back clean** — recorded so nobody
   repeats it. `clippy::cast_sign_loss` is `allow` workspace-wide with a
   documented reason (~2000 hits, casts usually deliberate, "leave to manual
   review"), so this was the manual review for the one shape that actually
   bites: a float that can be negative, cast to an unsigned type, where Rust's
   `as` *saturates to zero* rather than wrapping or trapping. Every
   `.floor() as u{8,16,32,size}` in `gui/**`, `apps/**`, `net*/**` and `pkg/**`
   was checked — fifteen sites. Fourteen are guarded, most by an explicit
   `.max(0.0)` and `guitk::grid`'s hit test by an early `if content_x < 0.0 {
   return None }`. The fifteenth was `apps/colorpicker`, above. `gui/compositor`
   already carries written warnings about the same hazard at two sites
   (`lib.rs:377`, `:13576`), so that crate had learned it independently.

**Related.** `TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING` is
the same mechanism with 5,628 more lines, found the same way and logged
separately because it also has a *duplicate wired page*, which is a worse
problem than being unreachable. Together they are about 30,000 lines.
