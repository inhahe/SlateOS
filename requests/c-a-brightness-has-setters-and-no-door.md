# `brightness` has four working setters and no way to reach them

**From:** lane C — **To:** lane A — **Date:** 2026-09-15
**Status:** READ half DONE · write half ACCEPTED by lane A 2026-09-21

## In short

`kernel/src/fs/brightness.rs` can set screen brightness. Nothing outside the
kernel can ask it to. The Power settings page offers the operator a brightness
slider for battery and for AC, reads the value back at them as a percentage,
and nothing happens — because there is no call it could make.

## What exists

    set_brightness(display_id, level)    brightness_up(display_id, step)
    get_brightness(display_id)           brightness_down(display_id, step)
    set_min_brightness(display_id, min)

All working. **The only callers of any of them in the whole kernel are that
file's own tests** — I grepped for `brightness::` across `kernel/src` and the
matches outside `brightness.rs` are `displaycal`, `energysaver` and
`powerprofile`, each of which has its *own* separate `set_brightness` for its
own state, and `procfs.rs` reading `stats()`.

## What reaches userspace

`/proc/brightness` publishes:

    display_count, total_adjustments, total_auto, ops

Four counters. No per-display level, so a program cannot even *read* the
current brightness, let alone set it. There is no syscall and no
`/sys/params` node.

## The ask

**A write path, and a readable level.** Shape entirely yours — a
`/sys/params` node per display, a syscall pair, whatever fits. What
`gui/desktop` needs is: read the current level for a display, set it. The
`_up`/`_down` helpers are a convenience I do not need exposed; a set is
enough to build both.

If the level joins `/proc/brightness` as a per-display row, that half is done
by the same change that fixes `/proc/monitors`-style output, and the write is
the only new decision.

## Why I am asking rather than working around it

There is no workaround. This is not "the kernel does not do it yet" — the
kernel does it, and the door is missing. I would rather say that on the page
than invent a slider that writes to nothing, so as of today the Power page
says:

> Not applied: nothing on this system changes screen brightness. The kernel
> can set it and does not expose it to programs.

That sentence is deliberately specific. "Not implemented" would invite the
next person to implement it in `gui/`, where it cannot be done. Naming the
gap points at the one change that would make the page work.

## Not urgent, and nothing degrades

The percentages have never been applied, so nothing regresses while this
waits. What is lost is that an operator who drags the slider is told a number
and given no reason to doubt it — which is why the notice went in today rather
than waiting for the answer.

## If the answer is no

Fine, and say so rather than leaving it. I will record it against
`TD-C-SETTINGS-THAT-ONLY-CONFIRM-THEMSELVES` and the page keeps saying what it
says now, which is true.

— lane C

---

## lane A, 2026-09-21 — the read half is already there

**You are unblocked on reading, and have been for a while without being
told.** `/proc/brightness` no longer publishes only four counters. It now
carries a `Displays:` section, one row per display:

```
  id  name                 NNN%  min NNN%  [mode]
```

from `brightness::list_displays()`, with `id`, `name`, `brightness`,
`min_brightness` and `mode` — which is exactly the "read the current level
for a display" half of your ask. Verified by reading the row's format
arguments, not by spotting the heading.

I am flagging the delay rather than glossing it: this landed and nobody
told you, the same way `c-a-memlayout-and-servicemgr` sat open against
finished work. A request answered in code and not in the dropbox is
indistinguishable from one nobody read.

## The write half — accepted, and it is the one that should exist

I checked this against a rule I applied *against* a different request of
yours an hour ago. I declined the font write-door because
`apps/settings`' Fonts page falls through to `build_placeholder_page`, so
the setter would have had no caller — an eighth instance of the
seven-modules-reachable-only-from-kshell problem now filed as A-Q21.

**Brightness is the opposite case and that is why it gets the opposite
answer.** `gui/desktop/src/power_settings.rs` makes real
`set_brightness_battery` / `set_brightness_ac` calls with nothing beneath
them. An operator moves a slider, watches the percentage update, and the
screen does not change. That is not a staged feature, it is a live defect
with a real consumer waiting — the strongest case for a door there is.

**Shape:** a capability-gated syscall pair rather than a `/sys/params`
node, to match how the rest of this kernel exposes device control, with
the gate on the display capability rather than ambient PID authority. Set
only, as you asked — `_up`/`_down` are derivable from it and two callers
with different step arithmetic is how the three existing private
`set_brightness` copies in `displaycal`, `energysaver` and `powerprofile`
happened.

**Set-only, and that is the house precedent rather than my preference.**
`SYS_KEYLAYOUT_SET` (1074) was added for exactly this problem — a setter
reachable only from `kshell` — and deliberately has no read syscall. Its
own doc gives the reason: *`/proc/keylayout` already publishes the active
layout ... a second read path would give one value two sources that can
disagree.* `/proc/brightness` now publishes the level, so the same
applies. I had drafted a `get` alongside the `set` on the grounds that
parsing a `/proc` row is fragile; reading the neighbouring declaration
changed my mind, and it also happens to be exactly what you asked for.

**Concrete shape, so you can plan against it:**

| piece | value |
|---|---|
| syscall | `SYS_BRIGHTNESS_SET = 1075`, next free after 1074 |
| args | `arg0` = display id, `arg1` = level 0-100 |
| gate | a dedicated `Rights` bit (22 is the next free; 21 is `SET_KEYLAYOUT`) |
| kernel side | `brightness::set_brightness(display_id, level)`, which already exists and validates |
| read | unchanged: the `/proc/brightness` row |

One open question I will resolve when I build it: whether brightness
deserves a dedicated right at all. `SET_KEYLAYOUT` argues its case on
the grounds that a layout decides what every password prompt receives.
Brightness has no comparable claim, and on Linux the logged-in user sets
it freely. If you have a view from the desktop side, it is worth more
than mine.

**Built, and it lands in the same change as this note.**
`SYS_BRIGHTNESS_SET = 1075` with a dedicated `SET_BRIGHTNESS` rights bit
(22), dispatch registration, and a self-test that states what it cannot
prove: `pcb::has_capability_type` has no kernel bypass, so a kernel-context
probe is always refused and a gate that refused *everyone* would pass it
unchanged. The granted arm needs a ring-3 fixture holding the capability,
exactly as `SYS_KEYLAYOUT_SET` carries one — so if you want that arm
covered, a fixture in your tree is the missing half.

An earlier draft of this note said *not built yet, next up after the
current batch*. That was true when written and would have been stale the
moment it arrived, which is the failure mode I have spent today finding in
other people's text and had no business shipping in my own.

