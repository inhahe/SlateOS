## §400 — Every GUI process finds its own UI font, lazily, from a compiled-in fallback list

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

### The problem

`osfont` can now parse a real face, scale it, and rasterize its outlines, but
nothing had ever handed it one: both the toolkit's measurement cache and the
compositor's drawing cache were constructed with `FontCache::new()` and left
holding the built-in 8x16 bitmap face. The outline stack was complete and
unreachable.

Getting a face into those caches raises a question that is not obviously
answerable, because the two caches live in *different processes*. An
application measures its own text — that is how it decides a button is wide
enough for its label — and the compositor draws it. If the two disagree about
which file "the UI font" is, every centred label in the system is off by half
the difference between two fonts' metrics, every right-aligned one by the
whole difference, and neither process looks wrong on its own. It is a bug that
presents as "the UI is subtly crooked" with no component to blame.

### The options

**(a) An explicit startup call.** Each process calls something like
`text::init_fonts()` before it draws. Cheapest, and it makes the cost of the
directory scan visible at the point it is paid.

**(b) Configuration: read the family from a settings file.** The desktop
already has an `ui_font` field in `appearance_settings.rs`, so the machinery
half-exists.

**(c) Lazy automatic installation from a list compiled into the toolkit.**
The first call that touches text builds the index and installs the first
family on `DEFAULT_UI_FAMILIES` that resolves.

### The decision

(c), with (b) deferred until settings actually persist.

(a) was rejected on the specific grounds that it is wrong the *first time an
app forgets*, and the failure is silent: an app that skips the call still
draws — in the bitmap face, against a compositor drawing in the real one. An
API whose misuse is invisible and whose correct use has to be repeated in
every process is the wrong shape for something every process needs. The
scan is not expensive enough to be worth that risk: 558 faces in 0.36 s on
this host in a debug build, once per process, and only for processes that
draw text at all.

(b) is where this should end up, and cannot go there yet:
`AppearanceSettings::save()` copies the pending settings into a `saved` field
and writes nothing to disk. There is therefore no cross-process source of
truth to read — a settings-driven font would be read by the app that changed
it and by nothing else, which is exactly the divergence being avoided.
`text::set_font_family` exists and works; it just has no persistent caller
yet.

The fallback list is ordered `Inter` (the design's intended UI font), then the
default sans of each platform this runs on, ending with `DejaVu Sans` /
`Liberation Sans` / `Arial`, which are installed almost everywhere. The
alternative to finding *a* face is the bitmap font, which is legible but looks
nothing like the system it stands in for, so it is worth walking a list of
eight to avoid.

### What makes the two processes agree

Not the list — a shared *function*. `guitk::text::install_ui_faces(&mut
FontCache)` is public precisely so the compositor can call it instead of
resolving a family by its own rule. Two copies of a fallback list agree until
the first time one of them is edited; one function cannot disagree with
itself. The compositor keeps a separate `FontCache` (it only draws, and would
otherwise take the toolkit's global lock once per glyph run and never measure
anything), but the *contents* of that cache come from the toolkit's own
resolution path.

`FontDb::finish()` sorting its faces by path is the other half of this:
directory iteration order is unspecified, so two processes scanning the same
directories could otherwise index the same family in different orders and pick
different files for it.

### Consequences

- Installation is atomic across regular and bold. A family that loaded one
  weight and not the other would draw a label's bold run in one face and the
  rest in another, which reads as a rendering fault rather than as a missing
  font, so `install_family` loads both before touching the cache.
- Toolkit tests that asserted pixel geometry against the bitmap face's 16 px
  line height had to start pinning their own cell size — a test about
  scrolling arithmetic must not depend on which fonts the host has installed.
- The desktop's `ui_font` setting is still inert. Wiring it up is blocked on
  settings persistence, not on this.

**Where it lives.** `gui/toolkit/src/text.rs` (the cache, the fallback list,
`install_ui_faces`/`install_family`/`set_font_family`),
`gui/toolkit/src/fontdb.rs` (the index and the family→file resolution),
`gui/font/src/select.rs` (the CSS Fonts 4 matching rule),
`gui/compositor/src/main.rs` (`RenderEngine::new`).
