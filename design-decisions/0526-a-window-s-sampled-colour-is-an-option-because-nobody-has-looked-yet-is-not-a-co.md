## 526. A window's sampled colour is an `Option`, because "nobody has looked yet" is not a colour, and the placeholder for it belongs to the renderer

**Date:** 2026-08-23
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Hovering a taskbar button pops up a small preview of that
application's windows. Real thumbnail images do not exist yet, so each preview
is a rectangle filled with one flat colour "sampled" from the window. The field
holding that colour used to *always* hold one, seeded to a fixed dark grey — so
a window nobody had sampled and a window that genuinely was that grey were the
same value, and in light mode the placeholder stayed dark, because the grey was
a hard-coded dark-theme constant. It is now `Option<Color>`: `None` means "not
sampled", and the renderer decides what to draw for it, out of the palette it
was handed.

Terms, once each: a *palette* is the resolved set of colours for the current
light/dark mode and accent (see 525); *Mocha* and *Latte* are the dark and
light variants of the colour scheme the shell uses; a *role* is what a colour
is *for* ("the window background") rather than a hex value like `0x45475A`.

### The alternative was to keep it a plain `Color` and seed it from the palette

That needs `WindowSnapshot::new` to take a `&Palette`. A snapshot is window
bookkeeping — id, application, title, size, minimized, focused — produced by
whatever tracks windows, which has no business knowing what mode the desktop is
in. Threading a palette through it in order to fill a field the renderer is
about to read anyway inverts the dependency: the cheapest way to make the data
structure theme-aware is to not make it theme-aware.

It also bakes the mode in at the wrong moment. A snapshot taken while the
desktop was dark and rendered after the user switched to light would still
carry the dark placeholder, and would keep carrying it until that window was
next sampled. An `Option` cannot go stale that way, because it is holding no
colour that *could* be stale.

### What the `Option` costs

Every consumer has to handle `None`, and the compiler makes that non-optional.
Today it is one line — `snap.dominant_color.unwrap_or(p.surface1)` — and the
next consumer will be told about it at the point it is written, which is the
argument for the shape rather than against it.

Against that cost: the two states are now distinguishable *at all*. Before, a
window whose real dominant colour happened to be `#45475A` was reported as
unsampled, and an unsampled window claimed to have been sampled. Nothing could
tell them apart, because there was nothing to tell apart — they were one value
with two meanings.

### Minimized is a third state, not a repeat of the second

A minimized window is showing nothing, so whatever was sampled from it before
it went is no longer true. Drawing it would be a confident statement about what
that window currently contains, which is worse than admitting ignorance. It
gets `surface0` where an unsampled window gets `surface1` — deliberately one
step dimmer, so that "nothing there" and "nothing known" still read apart
instead of collapsing into a single grey. Three states, three renderer-side
answers, all of them roles, none of them stored on the snapshot.

The counter-argument is real and was weighed: a stale sample is still
*evidence*, and a user about to restore a minimized window might find the old
colour a better cue than a flat placeholder. It was rejected because the
preview's entire claim is "this is what that window looks like", and a claim
that is knowingly out of date is the kind of small dishonesty a shell
accumulates until nothing it displays can be relied on. When real thumbnails
land, the same reasoning decides the same way: a minimized window gets its last
frame *marked* as stale, or it gets nothing.
