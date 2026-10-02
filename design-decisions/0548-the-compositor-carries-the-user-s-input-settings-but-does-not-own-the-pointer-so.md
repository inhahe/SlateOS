## 548. The compositor carries the user's input settings but does not own the pointer, so the display loop asks once a tick rather than being told

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The Settings → Mouse page lets you change how fast the pointer
moves, how it accelerates, which button is the "main" one, and how fast a held
key repeats. It writes those to a file, the compositor reads that file — and
until now the compositor kept exactly one of the settings (how long you have
between two clicks for them to count as a double-click) and dropped the rest.
Everything else waited for the next login, so dragging the speed slider looked
like it did nothing. The fix is a route from the file to the mouse. The
decision recorded here is the *shape* of that route: the display loop asks the
compositor, once per frame, what the settings are, and hands them on if they
have changed — rather than the settings being pushed along a queue when the
Settings page saves.

### Why the compositor cannot just do it

`Compositor` is deliberately display-agnostic. That is what lets one compositor
run headless in a test, into a recording, onto a Win32 window on the dev
machine, and onto a DRM card on real hardware. It has no reference to its own
display and should not acquire one.

But pointer speed is not something the compositor can apply. A mouse reports
*relative* motion — "I moved 20 units right" — and 20 units is only a number of
pixels once somebody has multiplied it by a speed and clamped it to the screen.
The thing doing that multiplying is the input source (`EvdevInput`), which
lives inside a `Paired` inside the `Present` that `Server::run_with` holds. So
the settings have to travel from a type that must not know about displays to a
type that is one, and `Server::run_with` is the only place that holds both.

### The two shapes considered

**Told (a push / a queue).** `Compositor::reload_input` records "a reload
happened"; `Server::run_with` notices the flag or drains a queue and forwards
the settings. This is what `known-issues.md` originally sketched.

**Asked (a poll).** `Compositor` keeps the whole `InputSettings` and exposes
`input_settings()`. `Server::run_with` reads it every tick, compares it against
the last thing it handed over, and forwards on a difference.

|  | Told | Asked |
|---|---|---|
| Work per idle frame | none | one `PartialEq` on a small struct |
| A missed or dropped notification | the setting silently never arrives | self-corrects on the next tick |
| Two reloads before one tick | needs coalescing, or two pushes | one push, of the newest value |
| A display attached mid-session | starts with stale settings unless the flag is re-armed | gets the current settings on its first tick |
| New state | a flag or queue that can disagree with the settings | none — the settings *are* the state |

**Asked won**, and not on the balance of those rows alone. `present.rs` already
documents this exact doctrine for `Present::monitors` — polled, idempotent,
`None` meaning "no opinion" — and chose it there for the same reason: a queue
is a second description of a thing, and a second description is a thing that
can be wrong about the first. Having monitor hotplug polled and input settings
pushed would mean two mechanisms in one loop body, three lines apart, for two
problems of identical shape.

The cost is real and is worth naming: an equality test on an `InputSettings`
runs sixty times a second forever. It is a handful of scalar comparisons on a
struct that fits in a cache line, in a loop that composites a desktop, and it
buys the self-correcting property in the table above.

### `None` is not `InputSettings::default()`

`Compositor::input` is an `Option`, and the `None` is load-bearing.

The input source is *constructed* with the user's settings — `main` reads the
file, builds the compositor from it, and hands the same settings to
`EvdevInput::open`. If "the compositor has not read the file" were spelled as
the defaults, the very first tick of every session would find a difference
between "defaults" and "nothing pushed yet", push the defaults into the source,
and undo the settings it was built with. The pointer would run at stock speed
for the whole session unless the user happened to edit the file.

So the poll returns `Option`, and `None` means *do not push* rather than *push
the defaults*. This is the same distinction §547 drew for a dialog that has not
been drawn yet, arriving from the other direction: there, an unknown rectangle
was collapsed into an empty one and every click counted as "outside"; here, an
unknown settings would be collapsed into stock ones and every session would
start by discarding a preference.

### Pushed before the poll, not after

Within the tick the order is: reconcile monitors, push settings, poll input,
serve clients, composite, show.

The push has to precede the poll because the events a poll returns were
*already* scaled by whatever the source was holding when it was asked. Pushing
afterwards would spend one frame moving the pointer at the speed the user has
just stopped wanting — a control that visibly lags itself by a frame. It is a
small wrong, but it is the kind that is invisible in a test that only counts
calls, which is why there is a test that records the order instead.

### The file is read once

`main` used to call `inputsettings::InputFile::load()` a second time at the
`EvdevInput::open` call site, moments after `make_compositor` had already
loaded it. Two reads of one file are two answers to one question if the user
saves between them — and worse, the compositor's copy is the one the loop will
go on pushing from, so a source started from the *other* copy would have its
first push be a correction. `main` now takes the compositor's copy.

### Where this is proved

`scripts/reintro-input-settings.py` — ten one-line defects, one for each link
in the chain from the file to the pointer: the compositor storing nothing (the
original bug verbatim), the loop never asking, the loop asking too late, `None`
read as the defaults, the change-test or the memory of the last push dropped,
the pairing forwarding to the screen or to nothing, and the device accepting
the settings without adopting them. Every one compiles.
