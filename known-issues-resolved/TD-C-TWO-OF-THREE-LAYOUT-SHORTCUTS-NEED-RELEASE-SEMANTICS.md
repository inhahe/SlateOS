## TD-C-TWO-OF-THREE-LAYOUT-SHORTCUTS-NEED-RELEASE-SEMANTICS — FIXED 2026-09-13

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/desktop/src/input_method.rs` — `SwitchShortcut`;
`gui/desktop/src/hotkeys.rs` — `HotkeyAction::SwitchInputLayout`.

**In short:** the keyboard-layout switcher offers three shortcuts to choose
from — Alt+Shift, Ctrl+Shift, and Super+Space. Only Super+Space is wired up.
The other two are the kind of shortcut where you hold two modifier keys and
*let go* without pressing anything else, and the desktop currently has no way
to notice that; it can only notice "this key went down while those modifiers
were held". Wiring them as if they were ordinary shortcuts would not merely
fail — it would break Alt+Shift+Tab, which is how you switch windows backwards.

**The mechanism, precisely.** A shell asks the compositor to route a chord with
`grab_key(window, key, modifiers)`, and that fires on the **press** of `key`.
`Key` does have `LeftShift` and `LeftAlt`, so `(LeftShift, alt)` is
*expressible* — but it means "Shift went down while Alt was held", which is the
first half of Alt+Shift+Tab. Every reverse Alt-Tab would switch the keyboard
layout, and the Tab that followed would arrive with the shell holding the grab.

What Alt+Shift means on every desktop that offers it is: the modifier pair was
released with **no other key pressed in between**. That is a different
predicate, over a key-*down*/key-*up* sequence, and neither the grab protocol
nor `HotkeyRegistry` can currently state it.

**What was done instead.** `SwitchInputLayout` is bound to Super+Space, which
is an ordinary chord and works. `SwitchShortcut::AltShift` and `::CtrlShift`
remain in the model, unbound. A user who selects one of them today gets no
layout switching — which is why this is logged rather than left to be
discovered.

**The proper fix**, in the order the pieces have to arrive:

1. The compositor learns to recognise a modifier-only chord: on the release of
   a modifier, fire if the matching set was held and no non-modifier key went
   down while it was.
2. The protocol gains a way to ask for one — a `grab_modifier_chord` beside
   `grab_key`, rather than overloading `grab_key` with a key that is itself a
   modifier, so that the two predicates cannot be confused at the call site.
3. `HotkeyRegistry` gains a binding kind for it, and `SwitchShortcut`'s other
   two variants bind through that.

**Until then, do not bind them.** A layout switch on the press of Shift-with-Alt
is worse than no layout switch: it is a working shortcut (Alt+Shift+Tab) taken
away in exchange for one that fires at the wrong time.

**FIXED 2026-09-13**, all three steps, in the order they are written above.

1. The compositor has a `ModifierEpisode` — the stretch of time with at least
   one modifier held, its modifier set kept as a high-water mark and a flag for
   whether anything has ruled a chord out. Asked on a release, because by then
   the live modifier set has already lost the key being released.
2. `grab_modifier_chord` / `ungrab_modifier_chord`, as a separate request pair
   (control version 11, tags `0x1F`/`0x20`) delivering
   `Event::ModifierChord` (input version 4, tag `0x0B`).
3. Not `HotkeyRegistry`, and that is the one departure from the plan above.
   The registry maps a *key chord* to an action, and a modifier-only gesture
   has no key; giving it a placeholder one would reintroduce at the call site
   exactly the confusion this entry is about. The shell holds the chord
   directly instead, from `DesktopShell::modifier_chords()`, reconciled by the
   session the way `global_chords()` already is.

Two things turned up in the doing, both worse than what the entry describes:

* **Alt+Shift is the default**, so out of the box the setting named a shortcut
  bound to nothing and only Super+Space — which the setting does not mention
  unless you pick it — switched a layout.
* **`SwitchShortcut` was never persisted.** It had a serializer and a parser
  from the day it was written, in a private config format nothing outside its
  own tests called, so the choice reset every session. It is
  `keyboard.layout_switch` in `input.yaml` now.

Deliberately *not* made exclusive: choosing Alt+Shift leaves Super+Space
working, because Super+Space is an ordinary entry in the user's own hotkey
registry and is rebindable there. A setting in one panel silently deleting a
binding shown in another is worse than two shortcuts doing one job — which is
also what Windows does, and what this shortcut is modelled on.
