## `C-NO-PLACE-FOR-AN-APP-TO-KEEP-ANYTHING` (lane C, 2026-08-28) -- **open**, missing service

**What it is.** An application running inside a window has nowhere *agreed* to
write a setting, a score or a preference that survives the window being closed.
`std::fs` exists, and the file-handling apps use it -- but there is no call in
`gui/**` that keeps state on an app's behalf, no per-app directory anybody
agrees on, and no service that owns "what did this program remember". So a game
that wants to keep a high score would have to invent a path, a format and a
failure policy of its own, and the next game would invent different ones. In
practice none of them do, and a window that is closed forgets everything.

**Where it bites.** Anything a program is supposed to carry from one run to the
next. Concretely, today:

| Program | What it claimed | What is true |
|---|---|---|
| `apps/simon` | "High score tracking persists across restarts", in the module documentation. | `best` starts at `0` at construction. There was no load and no save anywhere in the crate, and nothing in the toolkit to save *to*. |
| `apps/game2048` | -- | `best_score` starts at `0` every launch, so the "best" is the best of this session. |
| `apps/mixer` | -- | Every fader is back at its default the next time the window opens. |

Simon's claim is the one that had to be corrected rather than merely noted: a
documented feature with no code behind it is worse than a missing feature,
because the reader has no way to tell "the save failed" from "there is no save".
The documentation now says what is true -- the best is the best of this session
-- and the gap is here rather than papered over there.

**Why it is not simply a missing function.** A place to keep things is not one
API but a set of decisions: where the files live (a per-user, per-app directory
under a home the system has to define), what format they are in (`design.txt`
says YAML for configuration, with comments and formatting preserved), who may
read another app's data (capability-gated, not ambient -- an app's store must
not be reachable by every other app that can open a file), what happens when the
write is interrupted (atomic replace, or a corrupt settings file bricks the app
on next launch), and what happens on upgrade (a schema that changed under data
that did not). Bolting `std::fs::write` into each app would settle all five by
accident, differently in each app, and every one would have to be undone.

**What the proper fix looks like.** A settings/state service reachable over a
channel, and a thin `guitk` accessor over it -- roughly "give me my app's
document, as parsed YAML" and "here is the new one, replace it atomically". Apps
name their store by the same `app_id` the window manager already uses to group
them, so there is one identity rather than two. Until that exists, an app must
not claim to remember anything, and a "best" or a "high score" must be labelled
as being for the session.

**Trigger to revisit.** When a per-user store is reachable from a windowed app:
load `best` in `Simon::new` and save it in `score_round`, do the same for
2048's `best_score`, restore the mixer's levels, and correct the rows of the
table above.


### Lesson 64: a cache whose key is coarser than its value hands out the wrong answer to whoever asks second (lane C, 2026-08-28)

**In short:** the system font cache rounded the requested pixel size to a whole
number to make a lookup key, and then built the font at the size that was
*asked for* rather than at the size the key names. Two callers wanting 13.6 and
14.4 pixels share the key 14, so the first one through the door decided what
both of them got -- and a single request for a size no font can be built at
(zero, a negative, an enormous one) installed a coarse fallback bitmap under a
key that ordinary callers use, poisoning it for the rest of the program's life.
The fix is one line: build the entry at `key_px(key)`, not at `px`.

**The fault.** `gui/font/src/system.rs`:

```rust
let key = round_px(px);                     // clamp to 1..=512, round
self.fonts.entry((key, weight, family))
    .or_insert_with(|| SystemFont::from_shared(face, px) ... )
//                                             ^^ px, not key
```

`round_px` is deliberately lossy -- it exists so that a hundred slightly
different requested sizes share a handful of rasterisations. But the value
stored under the lossy key was built at the *unrounded* size, so the map no
longer had one value per key; it had one value per key, chosen by whoever asked
first. That is order-dependence in a pure-looking accessor: `get(13.6)` then
`get(14.4)` and `get(14.4)` then `get(13.6)` return different fonts, and which
order happens depends on the order a UI paints its text in, which depends on the
window size, which depends on the user.

**Why it took an app to find it.** The failure that exposed it was three
subsystems away and looked nothing like a font bug. `apps/simon` at a 24x24
window drops every band, so a readout box is empty; the readout still computed a
size for its caption -- `0.0` -- and asked for its line height before the
early-return that would have skipped the drawing. `round_px(0.0)` clamps to 1;
`SystemFont::from_shared(face, 0.0)` fails; the 8x16 builtin bitmap is installed
under key 1. Later the *pad numbers*, legitimately 1.472 pixels, round to key 1,
find the poisoned entry, and measure 8.0 x 16.0 -- so a text-inside-the-window
test failed, naming a string that had nothing to do with the readout that
poisoned the cache. The distance between cause and symptom is the whole point:
a cache is shared mutable state, and shared mutable state converts a local
mistake into a remote one.

**The rule.** *Whatever you key a cache by, build the value from the key.* If
the key is derived from an argument by any lossy function -- rounding, clamping,
case-folding, normalising a path, truncating a hash -- then the value must be
computed from the derived key, never from the raw argument. Write the inverse
explicitly (here, `key_px`) so the round trip is visible and testable, rather
than relying on the two call sites happening to agree. And treat a failed build
as a thing not to memoise at all, or to memoise only under a key nobody else can
land on; a negative cache entry that shares a key with a positive one is a
denial of service you inflict on yourself.

**Where else to look.** Anything with a `round`, `clamp`, `to_lowercase`,
`canonicalize` or `& MASK` between an argument and a map key: a glyph atlas
keyed by rounded size; a scaled-image cache keyed by integer dimensions built
from float ones; a DNS or path cache keyed by a case-folded name that stores the
result for the original spelling; a capability lookup keyed by a truncated
identifier. The tell is a key expression and a value expression that mention the
same variable in two different forms. The test that catches it is always the
same shape -- ask twice in both orders and require the same answer, and ask once
for something that cannot succeed and then once for something that can.
