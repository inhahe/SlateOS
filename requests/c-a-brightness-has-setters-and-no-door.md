# `brightness` has four working setters and no way to reach them

**From:** lane C — **To:** lane A — **Date:** 2026-09-15
**Status:** open — one ask, and it is small

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
