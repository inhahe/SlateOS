## TD-C-STICKY-FILTER-AND-MOUSE-KEYS-ARE-BUILT-TESTED-AND-CONNECTED-TO-NOTHING -- FIXED 2026-09-13

**Date:** 2026-09-07. **Lane:** C.
**Where:** `gui/desktop/src/a11y.rs` — `StickyKeys` (317), `FilterKeys` (467),
`MouseKeys` (545); `gui/desktop/src/accessibility_settings.rs`;
`apps/settings/src/main.rs`.

**In short:** Sticky keys, filter keys (slow/bounce keys) and mouse keys are
fully written and thoroughly tested, and none of them is connected to the
keyboard. A user who turns sticky keys on gets nothing — no error, and the
toggle stays on. These are the accessibility features people *depend* on to
use a computer at all, not preferences, and all three are inert.

**The evidence, from the uniquely-named methods** (the ones that cannot be
confused with a same-named method on another type — `is_active` and
`is_locked` are useless for this because dozens of unrelated types have them):

| method | owner | production callers | calls from its own tests |
|---|---|---|---|
| `on_modifier_press` | `StickyKeys` | **none** | 9 |
| `on_key_press` | `StickyKeys` | **none** | 2 |
| `should_accept` | `FilterKeys` | **none** | 8 |
| `move_delta` | `MouseKeys` | **none** | 9 |

`FilterKeys::new()` is constructed only inside the test module. The logic is
real — `should_accept` implements both the slow-keys hold threshold and the
bounce-keys repeat window — and nothing ever asks it.

**There are three parallel models of the same settings, and none of them meet.**

1. `a11y.rs` — the *implementations* above.
2. `a11y.rs` — `AccessibilityConfig`'s flat fields (`sticky_keys_enabled`,
   `slow_keys_ms`, `bounce_keys_ms`, `mouse_keys_enabled`, `mouse_keys_speed`,
   `filter_keys_enabled`). These are **parsed and serialized and read by
   nothing else**: their only non-test appearances in the whole tree are the
   two lines that write them to the config file and the two that read them
   back.
3. `accessibility_settings.rs` — `StickyKeysConfig`, `FilterKeysConfig`,
   `MouseKeysConfig`, plus an `A11yFeature` enum; and `apps/settings` has a
   third set again under `ToggleId`. `accessibility_settings.rs` does not
   import `a11y` at all.

So a toggle in Settings writes model 3, the config file round-trips model 2,
and the code that would actually change key handling is model 1, which nobody
constructs.

**A field-by-field census of `AccessibilityConfig`** (18 fields): 8 reach
something — `high_contrast`, `color_filter`, `reduced_motion`, `magnifier`,
`cursor`, `screen_reader`, `text_scale`, `visual_alerts`. 10 do not:
`sticky_keys_enabled`, `sticky_keys_sound`, `sticky_keys_double_lock`,
`filter_keys_enabled`, `slow_keys_ms`, `bounce_keys_ms`, `mouse_keys_enabled`,
`mouse_keys_speed`, `caret_width`, `focus_indicator`. Of those, three
(`sticky_keys_sound`, `sticky_keys_double_lock`, `focus_indicator`) are not
even in the serializer — they are read and written by no code whatsoever.

**How you would notice.** Settings → Accessibility → turn on Sticky Keys.
Press and release Shift, then press A. You get `a`, not `A`. Same for Slow
Keys (no hold delay is enforced) and Mouse Keys (the numeric keypad does not
move the pointer).

**The proper fix**, and it is a design decision, not a patch: pick *one* model
and delete the other two. The natural shape is that `AccessibilityConfig` is
the persisted truth, the Settings pages edit it, and the compositor owns live
`StickyKeys`/`FilterKeys`/`MouseKeys` instances rebuilt from it whenever it
changes — with the key-event path consulting them before dispatch. That last
part is the piece that does not exist anywhere: there is currently no hook in
the input path at all, which is why nothing could have been wired even if the
models agreed.

**Update, 2026-09-07 — the keyboard half is done; the Settings half is not.**

The three features now work. `inputsettings` owns the settings and persists
them (`AccessibilityKeysConfig`, under `accessibility:` in `input.yaml`), the
compositor owns the live state machines (`compositor::a11ykeys`) and applies
them in `handle_key`, and `set_input_settings` is the single road between the
two. Sticky Shift capitalises the next letter; bounce keys drops a repeat;
slow keys holds a press until its threshold expires with the key still down
(`design-decisions.md` §821); the keypad drives the pointer. Eleven
integration tests go in through `handle_input` as the input driver does,
because the previous implementation had thorough unit tests *and did nothing*,
so a test that calls the state machines directly proves only what was already
known.

Three defects were found in the old logic while moving it, all now fixed: a
refused keystroke used to start a bounce window (so one tremor silenced that
key for the whole window after it — the opposite of the feature's purpose);
switching sticky keys off stranded whatever was held; and `release_on_two_keys`
was in the config and implemented nowhere.

**Update 2, 2026-09-07 — closed for the three features.** The Settings app's
toggles now write `input.settings.accessibility`, which `handle_event`'s
whole-struct comparison saves to `input.yaml` and the compositor re-reads. A
test clicks the Sticky Keys row and reads the file back off disk. The
superseded copies are deleted: 543 lines from `desktop::a11y` (the state
machines, the six config fields, their serialiser and parser, their sixteen
tests) and the duplicate config structs in
`desktop::accessibility_settings`, which now re-exports `inputsettings`'.

So the road is whole and single: Settings window -> `input.yaml` ->
`Compositor::set_input_settings` -> `compositor::a11ykeys` -> `handle_key`.

**Two things this entry stays open for**, both from the original census and
neither part of the three features above:

- `caret_width` and `focus_indicator` on `AccessibilityConfig` still reach
  nothing. They are not superseded — they are features never built — so the
  fields were left rather than deleted, since deleting them would remove the
  only record that they are wanted.
- ~~`gui/desktop/src/accessibility_settings.rs`, the 2 019-line panel nothing
  constructs.~~ **Done 2026-09-08:** deleted under §815. Twelve of its sixteen
  features were already in the Settings app; the other four are logged as
  `TD-C-FOUR-ACCESSIBILITY-FEATURES-EXISTED-ONLY-AS-A-DEAD-PANELS-CONTROLS`.

**Superseded — what was still not connected: the Settings UI.**
`gui/desktop/src/accessibility_settings.rs` and `apps/settings` still write to
their own `StickyKeysConfig`/`FilterKeysConfig`/`MouseKeysConfig` and to
`A11yFeature`/`ToggleId`, none of which is `inputsettings`. So the toggle in
Settings still changes nothing — a user who edits `input.yaml` by hand gets
all three features, and a user who uses the settings screen gets none. That is
a smaller and much more ordinary job than the one above: point those panels at
`inputsettings::AccessibilityKeysConfig` and delete the duplicates, along with
`desktop::a11y`'s now-superseded state machines and the ten dead
`AccessibilityConfig` fields.

**Also still open from the census:** `caret_width` and `focus_indicator` reach
nothing and are not part of the above.

**Where the hook goes, since that was the unknown.** There is exactly one
funnel: `Compositor::handle_key(scancode, pressed, character)` at
`gui/compositor/src/lib.rs:6913`, reached only from `InputEvent::KeyDown` and
`InputEvent::KeyUp` (lines 6532-6533). Both accessibility filters fit inside
it, in an order the existing code already implies:

- **Filter keys** first, at the very top, before `self.modifiers.update()` —
  a rejected keystroke must not move the modifier state either. It needs a
  press timestamp and a hold duration, which `handle_key` does not currently
  receive; that is the one signature change the work requires.
- **Sticky keys** folded into the modifier step, since `self.modifiers` is
  already the thing that decides whether Shift is down when `A` arrives, and
  sticky keys are precisely a rule about how long that stays true.
- **Mouse keys** is separate and easier: it turns key events into pointer
  motion, so it belongs beside the existing pointer handling rather than in
  the modifier path.

`handle_key` already consults window grabs "after the chord is known, before
the event is delivered", so the structure for intercepting is there; nothing
about this needs new architecture, only a decision about which config model
feeds it.

**Why this is not fixed here.** The keyboard event path is the compositor's,
the fix spans three files that each hold a competing model, and choosing which
model survives is exactly the "band-aid accumulation — stop and redesign"
case in `CLAUDE.md`. It wants its own task, not a corner of a print-format
change. Logged now because it was found while checking a *different* stale
"what remains" claim, and an undocumented bug is an invisible one.

**How it was found.** §816 ended "What remains is that no Settings control
sets it yet", which was stale — the high-contrast control does exist. Checking
whether the *other* accessibility settings were wired turned up this instead.

---


**Update 3, 2026-09-13 — the last two fields are built, and the entry closes.**

It stayed open for `caret_width` and `focus_indicator`, the two fields from the
original census that were "not superseded -- features never built", left in
place because deleting them would have removed the only record that they were
wanted. Both now exist, so the record is no longer the only copy.

| | where it lives now | who supplies it | what draws it |
|---|---|---|---|
| caret width | `AppearanceSettings::caret_width_scale`, 839 | `apps/launcher`, the shell's run dialog | `guitk::textedit` |
| focus ring | `AppearanceSettings::focus_ring_scale` | `apps/diskcleanup` through `App::appearance_changed` | `guitk::modal`'s `AlertDialog` |

`guitk::style::FOCUS_RING_WIDTH` is the base measurement, in `style` rather
than in one widget's module because a focus ring is shared visual vocabulary --
`CARET_WIDTH` sits in `textedit` because only a text field has a caret, and a
ring has no such excuse. `focus_ring_width()` does the multiplication once, for
the reason 839 gives: a scale multiplied at each call site is a scale some call
site will fail to multiply, which is exactly how the copy deleted below came to
have passing tests and no reader.

**The colour is deliberately not a setting**, unlike the deleted model's
`color: Option<Color>`. A ring is drawn over an unknown background, so no fixed
hue can be guaranteed legible, and a user who picks an invisible ring while
believing they have made it *more* visible is worse off than one who was never
offered the choice. Width is safe in a way hue is not: thicker is never less
visible.

**Deleted: `desktop::a11y`'s `FocusIndicator`, 146 lines** including the field
on `AccessibilityConfig`, two tests that existed only to exercise it, and its
arms in three broader tests -- each of which pairs it with `CursorSettings`,
which asserts the same property and stays. That is this entry's own prescription
("pick one model and delete the other two") applied to the last of them.

**Three tests, one per join**, because each half passing proves nothing about
the road between them -- and a road with tested ends and no middle is precisely
what the deleted copy was. `appearance` proves the scale becomes a width;
`guitk` renders the dialog and reads the `StrokeRect`; `diskcleanup` drives the
real `appearance_changed` hook and reads the pixels out of its confirmation
dialog, which is where a focus ring matters most -- the destructive button is
one Tab from the safe one. Proved able to fail: dropping the
`with_focus_ring_width` call makes the last one report *no ring was drawn at
the 6px the settings ask for; widths drawn were [2.0]*.

**Still open elsewhere, and not this entry's:** `AlertDialog` is the only
widget in the toolkit that draws a focus ring at all. The others that hold
focus show it another way -- `textedit` a caret, `colorpicker` a border,
`menu` a highlight -- except `grid.rs`'s `GridView`, whose focus index reaches
no renderer because nothing in the tree constructs a `GridView` at all. That is
C-Q17's question, not this one's.
