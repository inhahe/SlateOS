## 566. A window can be told to be invisible to the mouse, and that is a different fact from being see-through

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop wants to flash a volume indicator over the middle of
the screen when you press the volume key — a small panel that appears for two
seconds and fades. Everything the shell draws lives in a *window*, and until
now every window took clicks: whatever was on top of a point got the click at
that point. So the indicator would have swallowed clicks aimed at the document
underneath it for those two seconds, and the user would have no idea why.
Windows can now be created **click-through**: the compositor answers "what is
under the mouse?" as though such a window were not there, so the click lands
on what is behind. Painting, stacking and typing are unaffected.

### The tension

The overlay has to be *on top* — that is what makes it visible over a
maximised window — and being on top is exactly what makes it take the click.
The two properties come from the same fact (its place in the stack) and have to
be separated by hand.

There was already a flag called `transparent` on a window, and the obvious
cheap move is to reuse it. That would be wrong, and the wrongness is not
theoretical:

| | takes clicks | shows what is behind |
|---|---|---|
| ordinary window | yes | no |
| frosted shell panel | **yes** | **yes** |
| heads-up overlay | **no** | usually |
| debug graph over a game | **no** | **no** |

The middle two rows are the point. `transparent` is a fact about *pixels* — it
tells the compositor to skip the opaque client-area fill so what the client
does not paint shows through. Click-through is a fact about the *hit test*. The
shell's own taskbar is already `transparent: true` and must obviously keep
taking clicks; folding the two together would have made the taskbar
unclickable the moment it went translucent.

### The options

**Give the window a per-pixel input region** (X11's shape extension, Wayland's
`wl_surface.set_input_region`). *What changes:* a client could say "clicks land
on my buttons but pass through my drop shadow", per rectangle.
**Rejected — for now.** It is strictly more expressive and it is what a mature
compositor ends up with, but it is a region list per window consulted on every
pointer motion, and nothing in the tree wants anything but all-or-nothing yet.
The all-or-nothing flag is the degenerate case of the region version, so
adopting regions later does not invalidate this: `input_transparent: true`
becomes "empty input region", and the flag can be kept as the spelling of it.

**Have the shell move the overlay out of the way instead.** *What changes:* the
indicator would dodge the pointer rather than ignore it. **Rejected.** It
solves nothing — a click is aimed at a point, and an overlay that jumps aside
when the pointer approaches is worse than one that sits still, because now the
thing you are reading runs away while you read it.

**Let the overlay take the click and forward it on.** *What changes:* nothing
visible, in the good case. **Rejected.** It requires a client to synthesise a
pointer event for a window it does not own and cannot name, which is precisely
the ambient authority (permission you get by *being* you, rather than by
holding a token for the thing you are acting on) that the capability design
rules out. It also cannot get the coordinates right without knowing the target
window's geometry, which it also cannot ask for.

**A flag on the window, honoured by the compositor's hit test.** *What
changes:* a window created with `input_transparent: true` never appears in the
answer to "what is under the mouse", so the search continues past it.
**Chosen.**

### What it does and does not affect

Deliberately narrow. `input_transparent` is read in exactly two places —
`Compositor::window_at` and `Compositor::window_at_with_decorations` — and
nowhere else. So:

- **Painting is untouched.** An overlay that took no clicks *and* was not drawn
  would be a volume indicator nobody can see, which is the one thing it exists
  to do. `a_click_through_window_is_still_drawn` pins this.
- **Stacking is untouched.** It is still on top; that is the point.
- **Keyboard is untouched.** A click-through window can never be focused *by
  clicking*, because clicking is what it opts out of — but if something else
  focuses it, it gets keys normally. Folding keyboard routing in as well would
  have made the flag mean two things.
- **Both hit tests skip it, not one.** They serve different questions — client
  area versus frame — and a window that the pointer fell through for a click
  but not for a title-bar drag would be draggable by an edge the user cannot
  see.

The skip is a `continue`, not a `return`: the loop goes on to the window
*behind*. A hit test that stopped at the first click-through window and
answered "nothing" would look identical from the overlay's side and be just as
broken, so the test asserts the lower window is returned rather than merely
that the upper one was not.

### The cost, stated plainly

A window that ignores every click looks broken, with no clue on screen as to
why — there is nothing to see, because looking is exactly the sense that still
works. That is why the default is `false` in both `WindowSpec::new` and the
`chrome()` helper the shell builds its surfaces with, why the one surface that
sets it says so at its own call site rather than inside the helper, and why
`a_new_window_is_clickable_until_it_says_otherwise` exists to keep the default
from drifting.

Nothing gates *who* may ask for it. A hostile client can create a full-screen
click-through window, but that is the harmless direction: it can only decline
input, never steal it. The dangerous direction — a full-screen window that
takes every click — was already available to anyone and is a separate problem
(`TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE` and §495 cover the shape of the
answer).

### Protocol cost

`CONTROL_VERSION` 2 → 3. Unlike the 1 → 2 bump, this one genuinely moves
bytes: the new flag is written straight after `transparent` in `CreateWindow`,
so every field after it shifts by one and a version-2 decoder reads the
min-size presence flag out of the new byte and desynchronises for the rest of
the message. There is no way to add a field that an older peer could skip —
the encoding carries no per-field lengths — which is what the version number
is for. `see_through_and_click_through_are_two_different_things` round-trips
all four combinations of the two flags and checks the fields *after* the new
byte still line up, so a desynchronised reader fails there first.

**Where it lives.** `gui/remote/src/control.rs` — `WindowSpec`, the codec, the
version bump. `gui/compositor/src/lib.rs` — `Window::input_transparent`,
`window_at`, `window_at_with_decorations`. `gui/window/src/lib.rs` —
`WindowBuilder::input_transparent`, `WindowAttributes`,
`Window::is_input_transparent`.
