## TD-COMPOSITOR-PICKS-CARD0 (lane C, 2026-08-21) - **(1) and (2) fixed 2026-08-21; (3) open**

**Status 2026-08-21: the screen no longer stays black, and there is now a way to
tell it otherwise. What remains open is only item (3) below — real multi-GPU
*preference*, as opposed to finding a card that works at all.**

`DrmScanout::card0()` is gone. In its place:

* `DrmScanout::open(wanted: Option<u32>)` and, under it, the generic
  `open_display<C: CardSource>(source, wanted)` in
  `gui/compositor/src/present/drm.rs`. With `None` it tries `card0..card15` and
  keeps the first that is not `NoConnectedDisplay`; with `Some(n)` it opens that
  card and only that card, reporting its failure rather than quietly falling
  back — a `--card 1` that silently gives card 0 is worse than an error, because
  the wrong monitor lights and the flag appears to have been ignored.
* `--card N` / `--card=N` on the compositor, and `$SLATE_DRM_CARD` for the
  service definition that starts it at boot. The flag is a hard error when it
  names something out of range; the variable is a **warning and then ignored**,
  because an inherited variable may be years stale and must not be able to keep
  the desktop from starting. That asymmetry is deliberate and is tested.
* Three failure kinds are now kept apart in the report: "nothing is plugged into
  any card" (`NoConnectedDisplay`), "there is no such card" (`ENOENT`, which is
  the ordinary end of the list and never becomes the reported error), and a real
  failure such as `EACCES`. When several cards fail, the **first** real failure
  is reported — the last would always be `ENOENT` on `/dev/dri/card15`, true and
  useless. A card that fails for a real reason does not stop the search, so a
  broken first card cannot keep a working second one dark.
* `sys::CardPath` builds `/dev/dri/cardN\0` without allocating, and
  `sys::CardSource` is the seam that makes all of the above testable on a
  machine with no graphics card at all — the same trick `KmsSys` plays for the
  ioctls. 13 tests in `present/drm/tests.rs` plus 8 in `main.rs`; every one was
  proved to fail with the defect it names put back (reintroduction markers
  `drmcardzero`, `drmenoentreal`, `drmlasterror`, `drmdarkreal`,
  `drmnamedfallback`, `drmkeeplooking`, `drmpathshort`, `drmpathonedigit`,
  `carddefaultzero`, `cardboundoff`, `cardenvunbounded`, `cardenvnotrim`).

**What is still open — item (3).** The search answers "which card *can* I
drive?", not "which card *should* I drive?". On a laptop where both the
integrated and the discrete GPU have a display attached, the first that works
wins, and that may be the power-hungry one. Real policy needs information DRM
does not hand over cheaply (which adapter is physically wired to the internal
panel, which is on battery-friendly silicon), so it is deferred until there is
hardware to test it on. It is not blocking anything: `--card` covers the case by
hand, and QEMU presents one card.

The original entry follows, unchanged.

**In short:** on a machine with two graphics cards the compositor always uses the
first one, and if a monitor is plugged into the second one the screen stays
black. There is no way to tell it otherwise.

**What.** `DrmScanout::card0()` opens `/dev/dri/card0` and nothing else. A
laptop with integrated graphics plus a discrete GPU has `card0` and `card1`, and
which one is which is not stable across boots. There is no `--card` flag, no
enumeration of `/dev/dri/*`, and no policy for choosing.

**The proper fix**, in the order the cost rises: (1) a `--card N` flag or
`SLATE_DRM_CARD` variable, which is minutes and makes the machine usable by
hand; (2) enumerate `card0..card15` and take the first with a connected
connector, which makes the common case automatic — `DrmScanout::new` already
returns `NoConnectedDisplay` distinguishably, so the loop is exactly "try each,
keep the first that is not that error"; (3) actual multi-GPU policy (prefer the
discrete one, or the one the monitor is physically wired to), which needs
information DRM does not hand over cheaply.

**Severity.** Low today — QEMU presents one card — and it rises the moment this
runs on real hardware. Do (1) and (2) together before any bare-metal bring-up.
