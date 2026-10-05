## 550. A keystroke carries a string, not a character, and a composition that fails types both keys

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** On a French, German or Spanish keyboard some keys type nothing on
their own. You press `´`, nothing appears, you press `e`, and `é` appears — the
first key was waiting for the second. Our event for "a key was pressed" could
not describe that, because it carried *at most one character* and had no way to
say "none" that a text field would not read as "this was Escape". Two decisions
follow. A keystroke now reports **however many characters it typed** — none,
one, or two — instead of at most one. And when a pair does not compose, such as
`´` then `x`, we type **both** characters, `´x`, rather than silently throwing
the accent away.

### Why one character was not enough

`KeyEvent::text` was `Option<char>`, and `None` already meant something: "this
key produced no text at all" — F5, an arrow, a modifier held alone. A dead key
needs a *third* answer, "this key produced no text **yet**", and there was
nowhere to put it. Reusing `None` is not a small lie; every text field in the
tree keys off exactly that field, so a dead key indistinguishable from F5 is a
dead key that leaves a stale accent in the compositor while the widget below
carries on as though nothing was pressed.

The other end is also wrong by one. A failed composition needs to emit two
characters from one key press, and `Option<char>` can emit one. Whatever a
failure policy turned out to be, it had to be *expressible* first.

| | `Option<char>` | `String` |
|---|---|---|
| A key that types nothing (F5) | `None` | `""` |
| A dead key, waiting | — | `""` |
| A normal key | `Some('a')` | `"a"` |
| A composition that succeeded | `Some('é')` | `"é"` |
| A composition that failed | — | `"´x"` |

`KeyEvent` is `Clone`, never `Copy`, so a `String` field costs it nothing it had.

### The failed-composition policy: type both, and why

Three implementations, two answers:

| System | `´` then `x` |
|---|---|
| X11 / libxkbcommon | types `x` — the accent is discarded |
| Windows | types `´x` |
| macOS | types `´x` |

We follow Windows and macOS. The argument that decides it is not majority rule
but **who is in a position to be surprised**. Discarding is defensible when the
user knows they made a mistake — but the commonest way to reach a dead key is
not to know it is one. Somebody typing a password, a filename or a URL on a
borrowed French laptop presses the key next to Enter, sees nothing, presses `x`,
and gets `x`. Their keystroke vanished and nothing on screen ever said so. Under
the type-both rule they get `´x` — wrong, but *visibly* wrong, and one Backspace
from right. Silent loss of input is the one failure a text field must not have.

The cost is real and small: on a layout where dead keys are routine, a user who
knows exactly what they are doing gets a character they must delete. That is a
visible nuisance for expert users, traded against invisible data loss for
everyone else.

### The wire format, and the version bump that had to come with it

`gui/remote` carries input events between machines, and its encoding of this
field was a present-flag plus a four-byte codepoint. It is now a
length-prefixed string, so `PROTOCOL_VERSION` goes **2 → 3**.

That bump is not bookkeeping. The mismatch is silent rather than loud, which is
the dangerous kind. Consider the commonest event of all — a key *release*, whose
text is empty, encoded now as four zero bytes of length. A version-2 decoder
reads the first of those as "no character present" and then consumes the other
three as the beginning of the next event. No error is raised. Every event after
it is shifted by three bytes, and the far end reports keystrokes nobody made.
A decoder that refuses to talk to a peer it does not understand is the only
version of this that fails safely.

`DecodeError::BadChar` was removed in the same change. It existed to reject a
u32 that is not a scalar value; nothing on the wire is a bare codepoint any
more, so the variant had become unconstructible. Invalid text is caught by
`BadUtf8` instead — an *error*, deliberately not `from_utf8_lossy`, which would
hand a widget a `U+FFFD` indistinguishable from one the sender meant.

### `KeyEvent` widened; `CompositorInput::KeyDown` deliberately did not

The two sides of the compositor's translation are not symmetric, and making
them match would have been a mistake:

- **In**, `CompositorInput::KeyDown { scancode, character: Option<char> }`. A
  keystroke *arrives* carrying at most one character, because a scancode plus a
  level is all the hardware can name. `Option<char>` is exactly right there and
  stays.
- **Out**, `EventNotification::KeyEvent { text: String }`. A keystroke *leaves*
  carrying however many characters the layout made of it.

The widening belongs at the point where layout state enters, which is precisely
what makes the compositor the right home for it (§456).

### `typed()` and `types_text()`, because forty sites were each deciding this

Reading the field is not "take the characters". Text fields want the characters
*minus control characters*, because on most layouts Enter, Tab, Escape and
Backspace all genuinely produce text — `\r`, `\t`, `\x1b`, `\x08`. Roughly
thirty sites spelled that filter out by hand and **seven had forgotten the
second half**, so pressing Escape put `\x1b` in a search box. Rather than patch
seven bugs, the rule now exists once, as `KeyEvent::typed()`, and the sites call
it.

The sites that should *not* use it are the ones choosing *between* things rather
than accumulating text — grid type-ahead search, the calculator's key-to-button
dispatch. A keystroke that produced two characters named no single item, so
those keep `single_char()`, which returns `None` for two characters rather than
picking the first. Both sites now say so in a comment, because "why is this one
different" is exactly the question the next reader will have.

### What this does not yet do

This is the shape, not the behaviour. Nothing yet *produces* an empty string
from a dead key or a two-character string from a failed composition — no layout
declares a dead key, and the compositor has no state to hold a pending one.
Those are the next two steps. Landing the type first is deliberate: it is a
change to 40-odd call sites and one wire format, and mixing it with a state
machine would make both harder to review and impossible to bisect.
