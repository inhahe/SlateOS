### TD-C-THE-SHELL-KEEPS-FIVE-KEYBOARD-LAYOUTS-THAT-NOTHING-TYPES-WITH — 2026-08-24 — RESOLVED 2026-08-24 (see the closing note)

**In short.** The shell has a keyboard-layout switcher: a tray chip reading
`EN`, a pop-up preview of the key caps, five built-in layouts (US QWERTY,
Dvorak, Colemak, German QWERTZ, French AZERTY), a per-application memory of
which layout you last used, and a config file. None of it changes what any key
produces. The thing that actually turns a key-press into a letter is a
different, hard-coded table in a different crate, and the two have no
connection at all — so a user who switches to AZERTY sees the tray say `FR`,
sees the preview redraw with AZERTY caps, and then types QWERTY.

**Where:** `gui/desktop/src/input_method.rs` (the switcher) and
`gui/compositor/src/keymap.rs` (the table that is actually consulted).

**The two stores, and the gap between them.**

- `InputMethodManager` in `input_method.rs` owns the five `KeyboardLayout`
  values and the active index. `grep -rn 'InputMethodManager\|input_method' gui
  apps` finds exactly one hit outside the file: `pub mod input_method;` in
  `gui/desktop/src/lib.rs:104`. Nothing constructs it, so today it is a
  switcher nobody has switched.
- `key_for_scancode` in `gui/compositor/src/keymap.rs` holds one US-QWERTY
  scan-code table and is the only scancode→character path in the tree. It
  takes no layout argument and reads nothing the shell owns. That table is
  already tracked as `TD-ONLY-ONE-KEYBOARD-LAYOUT`; **this** entry is the
  observation that a *second*, richer layout store now exists on the other side
  of the process boundary and duplicates the same data in an incompatible
  shape.

**Which fields are pure decoration.** Four of `KeyboardLayout`'s eight fields
are written by all five constructors and read by nothing outside the module's
own unit tests:

| Field | Set by | Read by |
|---|---|---|
| `rows_shifted` | all five layouts, 4 rows each | **nothing at all** — not even the preview, which draws `rows_unshifted` only (line 483) |
| `has_dead_keys` | `true` on German and French | one assertion, `test_german_layout_has_dead_keys` (line 713) |
| `is_rtl` | `false` on all five | one assertion (line 694) |
| `language` | all five | nothing |

`is_rtl` is the one that will bite, because it is not merely unread — it is
unread *and* the code it would have to govern assumes the opposite.
`render_preview` walks `rows_unshifted` left to right and places cap *n* at
`x + n * KEY_SIZE` unconditionally (line 483 onward), so an Arabic or Hebrew
layout would preview with its keys mirrored. Nothing catches this today
because no RTL layout is offered, which means the field's only current effect
is to make the module look as though it has been thought about.

**The capability it would need already exists.** `gui/font` has a full bidi
implementation — `gui/font/src/bidi.rs` resolves paragraph direction and
`gui/font/src/shape.rs` places runs in visual order, both with their own
`is_rtl`. So the RTL preview is not blocked on missing machinery; the desktop
module simply never asks.

**What the proper fix looks like.** Not "make the preview read `is_rtl`" —
that would polish the decoration. The layouts belong in one place, and by
§456's own reasoning that place is the compositor, so that one system keymap
governs every client at once. Concretely: move the five row tables to
`gui/compositor/src/keymap.rs` (or a crate both can depend on), give
`key_for_scancode` a layout parameter, and reduce the desktop's
`InputMethodManager` to a *selector* — it names the active layout and asks the
compositor to switch, rather than holding a private copy of the data. The
per-application memory and the config file are worth keeping and are already
correct; it is only the row tables that must not live in two places.
`rows_shifted` should be deleted or wired up at that point, and `is_rtl`
either honoured by the preview or dropped along with `language`.

**Trigger:** sequenced after `TD-ONLY-ONE-KEYBOARD-LAYOUT` step 1 (layout
selection in the compositor), which is what gives the selector something to
select. Deleting the four dead fields does not have to wait for that and can
be done at any time.

**If never fixed:** the switcher is a lie with a UI. It is worse than an
absent feature, because a non-US user will find it, use it, watch the tray
label change, and conclude the OS is broken in some deeper way when the
letters stay wrong — rather than concluding, correctly, that layout switching
was never implemented. The duplicated row tables also guarantee the two copies
drift: a fix to the compositor's QWERTY table will not reach the preview, so
the picture the user is shown will stop matching what they get.

**RESOLVED 2026-08-24.** Fixed as the entry prescribed — the row tables moved
to a crate both sides depend on (`gui/keylayout`, chosen over the compositor
itself so that reading the layouts does not oblige the shell to link a display
server), `key_for_scancode` gained a layout-aware sibling `key_for_layout`, and
`InputMethodManager` is now a selector holding no table of its own. Full
reasoning in `design-decisions.md` §549; the compositor half is recorded under
`TD-ONLY-ONE-KEYBOARD-LAYOUT`'s 2026-08-24 update.

The four decorative fields went four different ways, which is the part worth
remembering:

| Field | Outcome |
|---|---|
| `rows_shifted` | **wired up.** It was unread because the thing that would read it did not exist; now Shift, Caps Lock and AltGr all select a face through `Layout::character`. |
| `has_dead_keys` | **deleted.** It was set on German and French and consulted by one assertion. A flag saying a layout needs a facility nothing implements reads as support for that facility; the absence is now stated in prose, in the module doc and in `TD-ONLY-ONE-KEYBOARD-LAYOUT` (3). |
| `is_rtl` | **deleted, and this entry's suggested fix was wrong.** "Honour it in the preview" would have mirrored the caps — but Arabic and Hebrew boards put their letters on physically standard positions (ض and / are both on the key engraved `Q`), so mirroring would draw a keyboard that does not exist. RTL is a fact about the text a layout writes, not about where its caps sit. See §549. |
| `language` | **kept, and given a reader.** It separates "a rearrangement of English" from "a national layout", which is the precondition of a real invariant: only the English layouts must reach the whole alphabet from the plain level. |

The per-application memory and the config file survived as the entry
recommended, and the parser got better in passing: `apply_config_text` now
honours the `layout_N=` lines that `to_config_text` has always written, since
there is finally a catalogue to resolve a name against. Each name is looked up
among the installed layouts *before* the catalogue, so a layout a user wrote
themselves survives a save/reload the built-ins have never heard of.

One thing the entry asked for is still true and still worth noting: **nothing
constructs an `InputMethodManager`.** `grep -rn 'InputMethodManager' gui apps`
still finds only `pub mod input_method;`. The switcher is now correct rather
than a lie, but it is not yet reachable — that belongs with
`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` and the taskbar's tray, and is what
would turn "the compositor can type Dvorak" into "the user can choose Dvorak
without editing a YAML file by hand".
