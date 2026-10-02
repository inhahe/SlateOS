## TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER
**Status:** OPEN — 2026-09-24 (lane F): the pointer is drawn now (design-decisions §1301), so `cursor_size` and `cursor_scheme` have a reader waiting — and it reads the defaults, per this entry's proper fix, until the models are one. Of the four this entry counts, the two in `gui/desktop` have since been deleted (`9dde7ab85`, `9ddb46bae`), and `apps/settings` holds one more it never saves. Asked which survives: `requests/f-ce-the-pointer-is-drawn-now-which-cursor-size-setting-survives.md`.

**Status:** OPEN for the two cursor rows only, and no longer blocked on lane C.
**2026-09-25:** the cursor-size models are one (`design-decisions.md` §872):
`appearance`'s `cursor_size` and `cursor_scheme` survive, `inputsettings`'
copy is gone, and the Settings application's own enum is lane E's to point at
the survivor. Lane F's compositor draws a pointer now (on `lane-f` as of this
date), so what remains is lane F reading the setting in
`pointer_preferences` and lane E's control writing it --
`requests/c-ef-the-pointer-size-is-appearances-cursors-size.md`. The rows
close when both land. `icon_size` is FIXED 2026-09-24: the correction below -- "blocked
on C-Q17" because the icon layer was never constructed -- stopped being true
on 2026-09-14 when the shell began constructing and drawing it, and nobody
came back to this row. The layer now takes the size from `set_appearance`,
grows its grid to fit, keeps every icon in its row and column across a
change, and moves one that no longer fits to a free cell rather than off the
screen (`DesktopIconLayer::set_icon_size`). Its labels are centred, too: the
comment always said so and the code drew them from the cell's left edge.

**In short:** Open Settings, choose "Slow" animations, and the setting is
saved, survives a restart, and changes nothing — because nothing in the system
reads it. The same is true of desktop icon size and cursor scheme.
(**Animation speed is fixed as of 2026-09-06**; the other three stand.
Taskbar auto-hide, added 2026-09-06, was built the other way round -- setting,
consumer and Settings control in one change -- precisely so it never joined
this list.) Cursor
*size* is worse: it exists as four separate settings in four places, and the
program that draws the cursor reads none of them. These are controls that work
perfectly and do nothing, which is the most expensive kind of broken, because
the user has no way to tell it from a preference the system is ignoring on
purpose.

**Found** 2026-09-06 by lane C, auditing which of `AppearanceSettings`' fifteen
fields have a consumer. This narrows the older observation in
`TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK` ("also unread by any
renderer: `window_corners`, `drop_shadows`, `animation_speed`, `icon_size`,
`cursor_size`, `cursor_scheme`, and `scaling_percent`"), which was written on
2026-08-14 and is now half out of date — four of those seven have since been
wired, and the remaining three plus the cursor-size tangle are what is left.

### What was measured

`grep` for each field across `gui/**` and `apps/**`, discounting the two
settings *editors* (`gui/desktop/src/appearance_settings.rs` and
`apps/settings/src/main.rs`) and the model crate itself, since a field being
read by the panel that edits it is not a consumer.

| Setting | Consumers outside the editors | State |
|---|---|---|
| `theme_mode`, `accent_color`, `custom_accent` | desktop palette | **works** |
| `transparency` | `DesktopTheme` (`lib.rs:1283`, `:1311`) | **works** |
| `taskbar_style` | `DesktopTheme` (`lib.rs:1308`) | **works** |
| `accent_taskbar`, `accent_titlebars` | desktop chrome | **works** |
| `window_corners`, `drop_shadows` | compositor | **works** |
| `scaling_percent` | `guitk::scaling` via `set_appearance` | **works** |
| `fonts` | — | out of scope here; see §400 and C-Q1 |
| ~~`animation_speed`~~ | `AnimationManager`, via `ShellSession` | **fixed 2026-09-06** |
| ~~`icon_size`~~ | `DesktopIconLayer::set_icon_size`, via `set_appearance` | **fixed 2026-09-24** |
| **`cursor_scheme`** | **none** | **dead** |
| **`cursor_size`** | **none** — and three rival copies | **dead, four ways** |

Two of those need their evidence stated, because "grep found nothing" is a weak
claim on its own:

- **`animation_speed` has no possible consumer, not merely no actual one.** The
  shell's `AnimationManager` (`gui/desktop/src/animations.rs`) has no speed,
  multiplier or scale concept anywhere in it — the field it would multiply does
  not exist. So this is not a missing call site; it is a missing feature, and
  the setting was added to the model ahead of the thing it configures. The
  panel even renders `{:.2}x` next to it, so the UI states a multiplier the
  animation code cannot receive.
- **`icon_size`'s only non-editor hits are a false positive.** They are
  `guitk::scaling`'s own `icon_size()` method in that module's tests — a
  scaling-context accessor with the same name, unrelated to this preference.

**Correction, 2026-09-13: `icon_size` is not missing a reader, and that
matters for what to do about it.** `gui/desktop/src/icons.rs` is a complete
desktop icon layer -- 1974 lines, a grid, drag and selection, a `render`, and
its own tests. It is exactly the thing that would read this setting. Nothing
in the tree ever constructs it: `grep` for `DesktopIconLayer` across every
`.rs` file finds the type, its own module, and nothing else, and `lib.rs`'s
`pub mod icons;` is the only reference to the module at all.

So the honest state of this row is **blocked on C-Q17**, not "waiting for
someone to write the consumer". Wiring `icon_size` into that layer today would
connect a dead setting to a dead renderer and change nothing a user can see,
while looking in the git log exactly like a fix. It is listed in
`TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED` for the same
reason, and C-Q17 is the question of whether those get wired up or deleted.

The same is true, for a different reason, of `cursor_scheme` and `cursor_size`:
nothing draws a pointer at all, which is C-Q18. **All three remaining rows of
this entry are therefore waiting on an operator decision rather than on work,**
which was not clear from the table above and is the sort of thing that gets a
reader to spend an afternoon before noticing.

### The cursor-size tangle

**Collapsed 2026-09-25 to one model** -- `appearance`'s -- which gained 64 and
96 px sizes so nothing `inputsettings`' 16-128 range allowed was lost
(`design-decisions.md` §872). The two `gui/desktop` rows below were deleted
earlier (`9dde7ab85`, `9ddb46bae`); `inputsettings`' field went with §872 (its
next save removes the `cursor.size` key it used to write); the Settings
application's own `CursorSize` (`apps/settings/src/main.rs`, never saved --
not in the table below, which predates it) is lane E's to replace. The table
is the state as found:

One user-facing setting, four independent models, no reader:

| Where | Name | Persisted to |
|---|---|---|
| `gui/appearance` | `cursor_size` | `appearance.yaml` |
| `gui/inputsettings` | `cursor_size` (clamped 16–128) | `input.yaml` |
| `gui/desktop/src/a11y.rs` | `cursor.size_scale` | the accessibility file |
| `gui/desktop/src/accessibility_settings.rs` | `cursor_size_multiplier` | — |

And `gui/compositor` — which owns `CursorShape`, tracks `cursor_shape`, and is
the only thing positioned to draw a pointer at a size — has **no cursor size
concept at all**. So the four settings do not merely disagree; there is nothing
for them to disagree *at*.

This is `TD-THREE-INDEPENDENT-APPEARANCE-MODELS` recurring with one more copy
and, unlike that case, without even one working consumer to be the authority.

**Caveat on this one -- TRACED 2026-09-13, and the answer is "nothing reads
it".** The caveat asked whether some lower layer sizes the pointer
independently. It does not. Across all three presenters, every occurrence of
the word:

| presenter | cursor code |
|---|---|
| `present/host.rs` (the Windows dev host) | one `LoadCursorW(IDC_ARROW)` at window-class registration |
| `present/evdev.rs` and `evdev/sys.rs` | none — the two hits are the word in prose, about an event ring and a keyboard |
| `present/drm.rs` | **zero occurrences** |

The kernel does have cursor-plane support (`kernel/src/drm/crtc.rs`,
`drm/uapi.rs` -- "recommended/maximum hardware cursor plane width"), so the
hardware path exists; the compositor's DRM presenter never reaches for it. So
the picture stays "nothing reads it", and wiring `cursor_size` to anything
today would be theatre in the `icon_size` sense: a setting read by code that
draws nothing.

**And the same trace turns up a second inert thing, larger than the size.**
`CursorShape` is computed on every pointer move (`cursor_at`), stored
(`update_cursor_shape`), and exposed by `Compositor::cursor_shape()` -- which
is read by **two tests and nothing else**. So the I-beam over a text field, the
resize arrows on a window edge, the hand over a link: all decided, none drawn.
On the dev host you get Windows' arrow everywhere, because the host window
class names one cursor once and never changes it.

**The first piece is now a question, not a task: C-Q18.** Drawing a pointer is
straightforward until it meets `compose_frame`'s direct-scanout bypass, where a
fullscreen opaque window's picture is handed to the display uncopied — and a
pointer cannot be painted onto a frame that is never painted. Software cursor
always (fullscreen loses the shortcut), software except over fullscreen (the
pointer vanishes there), or a hardware cursor plane (most work, gives nothing
up) is an architectural fork with a measured performance feature on one side,
so it is the operator's. See `open-questions.md` → **C-Q18**. *(2026-09-25:
answered by construction and deferred. Lane F drew the pointer as a layer over
the picture as it is shown, which costs fullscreen nothing on every presenter
that exists, since each copies the fullscreen picture anyway; the question is
now `deferred-questions.md` DQ3, waiting for a presenter that does not copy.)*

**Which makes the order of work clear, and it is not this entry.** Four models
disagreeing about a size matters only once something draws a pointer. The
first piece is a cursor renderer -- a compositor-drawn pointer on the DRM path,
or `SetCursor` per shape on the host path -- and *then* the size setting has
somewhere to land. Reconciling the four models first would be reconciling four
descriptions of a thing that does not exist.

### Why this matters more than the count suggests

A setting that is missing is honest. A setting that is present, editable,
persisted, and inert is a lie the system tells about itself, and it costs the
user the time to change it, restart, decide they misremembered what it did, and
change it back. It also costs *us*: `animation_speed` has a test in
`apps/settings` asserting every speed can be picked
(`test_every_animation_speed_can_be_picked`), which passes, and proves only
that a control moves a value nothing consumes. That is the same shape as the
heatmap whose legend advertised a gradient the graph did not draw
(design-decisions 812's neighbours) — the test asserts the half that works.

### Proper fix

Per setting, and they are not equal:

1. ~~**`animation_speed`**~~ — **done 2026-09-06.** `AnimationManager` gained
   `set_duration_scale`, applied where elapsed time enters `tick` rather than to
   each animation's stated duration, so a new animation kind cannot forget to
   obey it. Two things the plan above did not anticipate: the fractional
   millisecond has to be *carried* (at 1.5x, truncating 10.67 to 10 loses 4%
   every frame, so every slow animation would run consistently late), and the
   feed is **not** `DesktopShell::set_appearance` — the manager lives on
   `ShellSession`, not on the shell, so `session.shell_mut().load_appearance()`
   adopts the settings and leaves the speed inert. `ShellSession::load_appearance`
   is the door that does both.
2. **`icon_size`** — **blocked, and not on effort.** Its natural consumer is
   `gui/desktop/src/icons.rs`, which has **no caller anywhere** and is one of
   the 47 pinned islands on `scripts/orphan-modules-baseline.txt`. Wiring the
   setting to it would connect two dead things and produce a *more* convincing
   lie: a setting that looks wired and still changes nothing on screen.

   Worse, desktop icon layout already exists a second time —
   `kernel/src/fs/deskicons.rs`, marked done at `roadmap.md` line 2385, with a
   grid, an icon size, layout modes, sorting, hit testing, persistence, a
   kshell command and `/proc/deskicons` (all deleted 2026-09-14, A-Q8). Two
   models of one user-visible state
   in two lanes, which is `TD-THREE-INDEPENDENT-APPEARANCE-MODELS` again.
   Asked in `requests/c-a-two-desktop-icon-models-and-mine-cannot-be-wired-until-we-pick.md`;
   until it is answered, wiring either one entrenches a duplicate.
3. **`cursor_scheme`** and **`cursor_size`** — do *not* wire these until the
   caveat above is resolved and the four models are collapsed to one. Wiring one
   of four rival copies to a renderer would make the other three permanently
   wrong instead of uniformly inert, which is harder to notice and harder to
   undo.

### Where

`gui/appearance/src/lib.rs` (the model); `gui/desktop/src/animations.rs` (no
speed concept); `gui/desktop/src/lib.rs` (`set_appearance`, the natural feed);
`gui/inputsettings/src/lib.rs`, `gui/desktop/src/a11y.rs`,
`gui/desktop/src/accessibility_settings.rs` (the rival cursor models);
`gui/compositor/src/lib.rs` (`CursorShape`, no size).



---

**Checked again 2026-09-13, and one of the three is a different problem than
this entry describes.**

`icon_size` has no reader because **the thing that would read it is itself
unreachable**. Desktop icons are drawn by `DesktopIconLayer` in
`gui/desktop/src/icons.rs`, and that type is named nowhere outside its own
file -- `grep -rn DesktopIconLayer` over the whole tree returns only
`icons.rs`. So there is no live consumer to wire the setting to, and adding
one would be theatre: a setting read by code nothing runs is still a setting
that does nothing, and it would *look* fixed on the next audit.

That moves `icon_size` out of this entry's class -- "nobody wrote the
reader" -- and into
`TD-C-TWENTY-FOUR-THOUSAND-LINES-BEHIND-ALLOW-DEAD-CODE`, whose fix is a
decision about whether the desktop icon layer is wired up or deleted. The
same question C-Q17 asks about five other modules.

`cursor_size` and `cursor_scheme` are in the same shape but worse: a search
of `gui/compositor` and `gui/desktop` finds no reader of either, and the
compositor is where a cursor is actually drawn. `gui/inputsettings` has its
*own* `cursor_size` (clamped 16-128) that is equally unread there, which is
the four-places-one-setting problem this entry already names.

**Why this matters for the entry rather than just for the code:** the fix
this entry prescribes -- give the setting a reader -- is only right for a
setting whose consumer exists. For these three the honest sequence is the
other way round: decide whether the consumer lives, and only then wire the
preference to it. Doing it in the prescribed order produces a green audit and
an unchanged desktop.
