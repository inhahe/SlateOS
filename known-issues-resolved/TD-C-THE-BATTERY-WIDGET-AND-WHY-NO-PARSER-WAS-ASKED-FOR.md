## TD-C-THE-BATTERY-WIDGET-AND-WHY-NO-PARSER-WAS-ASKED-FOR -- FIXED 2026-09-16

**In short:** the desktop's battery widget drew the words "85%" and "3h 42m
remaining". Not numbers computed from something -- those exact strings, on
every desktop, including machines with no battery. It now reports the shell's
battery model, which says "No battery" because there is none.

**Date:** 2026-09-16. **Lane:** C. **Decided by:** Claude (autonomous).

**The fix was small because the honest answer was already in the same crate.**
`crate::power::BatteryInfo` exists and its `Default` is `present: false,
state: NoBattery` -- already correct, and already what production code
constructs. The widget reached past a correct model to invent two literals.

Keyed on `present`, not on a charge of zero: "there is no battery" and "the
battery is flat" are different facts, and a desktop reader acts on them
differently. The time estimate is drawn only when there is one.

**Why no `procinfo` parser was requested, which is the part worth keeping.**
`/proc/battery` publishes `sources`, `charge_pct`, `state`, `cycle_count` and
a row per source, so the obvious next step was to ask lane B for a parser, as
was done for `/proc/ioport`, `/proc/kmod` and `/proc/autostart`.

Checking the *producer* rather than the publisher stopped that.
`kernel/src/fs/battery.rs` starts with **no power sources at all**, and says
why in its own comments:

> A battery/UPS is observed hardware state […] seeded values would surface as
> hardware readings through `/proc/battery` and the `battery` shell command as
> if a real ACPI power source had reported them. A desktop may have no battery
> at all. Real sources appear only when an ACPI/power driver calls
> `register_source()`.

No such driver exists. So a parser would faithfully carry "zero sources"
across three crates and arrive at the same answer the widget already gives.
**Asking for it would have been work for lane B whose result was already on
the screen.**

That is the mirror of the question this lane has been asking all week. The
usual failure is a setter with no consumer -- lane B found one the same day,
having wired `SYS_KEYLAYOUT_SET` to a kernel field that nothing in the
keystroke path reads. This is the same question pointed upstream: before
asking for a reader, check that anything *writes*.

**The trigger for revisiting:** `battery.rs` carries a `DEFERRED PROPER FIX`
note to wire `register_source()`/`update_status()` to a real ACPI driver. On
the day that lands, `/proc/battery` starts carrying real readings and a
`procinfo` parser becomes worth asking for -- and the widget needs only its
`LiveReadings.battery` filled, because everything downstream of that already
works.
