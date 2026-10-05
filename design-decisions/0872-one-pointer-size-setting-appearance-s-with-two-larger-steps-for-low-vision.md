## 872. One pointer-size setting: `appearance`'s, with two larger steps for low vision

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C &middot; asked by lane F

**In short:** The mouse pointer's size was stored in three places -- the
appearance settings (four sizes), the input settings (any size from 16 to 128
pixels) and the Settings application (its own list, never saved) -- and
nothing read any of them, because until lane F's compositor work nothing drew
a pointer. Now something does, and it needs one answer. The appearance
setting is kept (`appearance.yaml`, `cursors.size` and `cursors.scheme`), the
input settings' copy is removed, and the kept one gains two larger sizes, 64
and 96 pixels, so the collapse does not take away the large pointers the
removed copy allowed. The Settings application's control is lane E's to point
at it.

### Which one survives

| | For | Against |
|---|---|---|
| **`appearance`** -- chosen | the compositor already reads `appearance.yaml` for every other visual setting and reloads it live (`ReloadAppearance`), so a change reaches the pointer with no new plumbing; it already has the scheme beside it | four fixed sizes, the largest 48 px |
| `inputsettings` | 16-128 px in any step | no scheme; `input.yaml` is the *behaviour* of the mouse (speed, buttons, scrolling), and a pointer's look is appearance; the compositor would need a second live reload for one field |
| the Settings app's own | -- | never saved anywhere |

Lane F proposed `appearance`; this agrees, for the reasons in the first row.

### Why add sizes rather than keep four

The removed copy allowed up to 128 px; the kept one stopped at 48. A pointer
twice the normal size is not enough for everyone with low vision, which is
the one reason a system offers large pointers at all. Two steps are added --
**Huge (64 px)** and **Giant (96 px)**, four times normal -- rather than a
continuous pixel value, because every front end that offers the setting
draws it as a short list (the Settings dropdown, a future accessibility
panel), and a list of named steps is what a person can choose from without a
preview. 96 px is the largest step common desktops offer; with display
scaling it is 192 px at 200%.

### Smaller calls made with it

- **The old key is removed, not left.** `inputsettings` wrote `cursor.size`
  into every `input.yaml` it saved (as `24`, since no control ever set it);
  its next save now deletes the key, so nobody edits it expecting an effect.
- **`CursorSize::ALL` and `CursorScheme::ALL`** list the choices in order,
  for the Settings application to offer rather than listing them itself.

### How to reverse

Adding sizes is additive; removing Huge and Giant would need a file that
names one to fall back to the default, which `from_yaml_name` already does
for an unknown spelling. Restoring `inputsettings`' field is the diff of this
commit, but would bring back two settings for one thing.
