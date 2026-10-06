## TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING -- FIXED (found so on 2026-09-25)

**Status:** FIXED 2026-09-25 (the input settings work). Filed among the resolved on 2026-10-05; what follows is the entry as it stood.

**Earlier status:** resolved by the input settings work, and the heading never said so. `gui/desktop/src/mouse_settings.rs` is gone; the model is `gui/inputsettings` (`input.yaml`), and the compositor applies its double-click time through `Compositor::reload_input` -> `set_input_settings` -> `set_double_click_ms` on a `ReloadInput` request -- the second verb this entry leaned toward. Found while triaging open lane C entries; the body below is the entry as written.

**In short:** Settings has a mouse panel with sliders for double-click speed,
pointer speed and so on. Moving them changes a number in a file and nothing
else. The compositor — the only thing in the tree that actually acts on a
double-click — has never heard of that file and uses its own built-in default.
So the double-click-speed slider does nothing today, and it is the one setting
on the panel that now has a real consumer sitting right there ignoring it.

**Where:**

| | |
|---|---|
| The setting | `desktop::mouse_settings` (`gui/desktop/src/mouse_settings.rs`), `double_click_ms`, default **400**, clamped 100–2000 (`:185`, `:205`) |
| The consumer | `Compositor::double_click_interval` (`gui/compositor/src/lib.rs`), default `DEFAULT_DOUBLE_CLICK_MS = 400`, clamped by `set_double_click_ms` to `MIN_DOUBLE_CLICK_MS`..=`MAX_DOUBLE_CLICK_MS` = 100–2000 |
| What connects them | nothing |

The two defaults and the two clamp ranges were deliberately made to match when
the double-click gesture moved into the compositor, precisely so that **a user
who never touches the slider sees no change when this is wired up**. That is the
mitigation, not the fix: a user who *does* move it still sees nothing.

**Why it is not simply a missing function call.** `set_double_click_ms` is
public and takes the value the panel already produces, so the compositor end is
done. The missing piece is the same one that blocks the appearance settings from
reloading live: **`apps/settings` has no compositor connection at all**
(`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`). The appearance half of this problem
was solved by adding a `ReloadAppearance` control verb (tag `0x0F`, no payload,
no `link.resolve` — see `TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS`
above), and the mouse settings want the same shape.

**The design question to answer first, and it is a real one:** should this be a
second verb (`ReloadInput`), or should the input settings move *into*
`AppearanceSettings` so the existing `ReloadAppearance` carries both? The second
is tempting because it needs no protocol change, but "appearance" is the wrong
home for pointer behaviour, and a struct that accretes every settings panel
because it happens to have a reload verb is how a settings blob becomes
untyped. Lean toward a second verb reading a second file.

**Scope beyond double-click.** `mouse_settings` also holds pointer speed,
acceleration, scroll direction/lines and left-handed button swap. The compositor
consumes none of them, and unlike double-click speed most have no consumer
anywhere — pointer acceleration in particular has nothing to apply it to,
because the compositor is fed absolute coordinates by
`handle_mouse_button`/`handle_mouse_move` rather than raw deltas. Wiring
double-click alone is honest and small; wiring the rest is a larger question
about where input transformation belongs and should not be bundled in.

**Trigger:** do this when `apps/settings` gains a compositor connection — the
same moment `ReloadAppearance` gets its first real caller. Both are lane C's.

**Update 2026-08-22 — the trigger is met, and the job is bigger than this entry
said.** `apps/settings` now has a compositor connection and a real event loop,
and `ReloadAppearance` has its first real caller (see
`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` and `design-decisions.md` §523). So the
blocker named above is gone. Two corrections to this entry's own description
turned up on checking it, and both make the remaining work larger:

1. **There is no file.** This entry says the slider "changes a number in a file".
   It does not. `gui/desktop/src/mouse_settings.rs` has **no persistence
   whatsoever** — no save, no load, no path, no serialisation. It is an
   in-memory struct with clamps. So the mouse settings do not survive a logout,
   never mind reach the compositor. The appearance settings had
   `appearance::AppearanceFile` to build on; this has nothing equivalent.
2. **It is in the wrong crate for the panel that would edit it.**
   `mouse_settings` lives in `gui/desktop` (the shell), and the settings
   application is `apps/settings`. Nothing outside its own module references it
   — the only two mentions in the tree are its `pub mod` line and a comment in
   the compositor saying nothing wires it through. `apps/settings` has a
   "Mouse & Pointer" *section*, but it is on the Accessibility page and holds
   one toggle (Mouse Keys); there is no double-click-speed control in the
   settings application at all.

**Update 2026-09-13 (lane C) — six of the eight settings now reach the pointer,
and the two that do not are one design decision, not eight missing calls.**

Every piece the four-part plan above asked for exists. `gui/inputsettings` is
the third crate, `ReloadInput` is control verb `0x14`, `apps/settings` has a
Mouse page with a double-click slider, and `Compositor::set_input_settings`
applies the value. The road is tested end to end from both ends:
`a_drag_on_the_mouse_page_reaches_the_file_the_compositor_reads` reads the value
back through a fresh `InputSettings::read_from` rather than from this process's
own model, and `a_double_click_change_asks_the_compositor_to_re_read_the_input_file`
checks the notification is sent.

**What consumes what, measured today rather than inferred from this entry:**

| setting | reaches | where |
|---|---|---|
| `double_click_ms` | the compositor's gesture | `set_input_settings` |
| `accel_gain`, `accel_threshold` | the pointer | `present/evdev.rs:291` |
| `button_mapping` | left-handed swap | `present/evdev.rs:668` |
| `natural_scroll` | scroll direction | `present/evdev.rs:732` |
| `scroll_speed` | scroll magnitude | `present/evdev.rs:737` |
| **`scroll_mode`** | **nothing** | — |
| **`scroll_lines`** | **nothing** | — |

The entry's own reasoning for why most of these could not be wired -- "the
compositor is fed absolute coordinates by `handle_mouse_button`/
`handle_mouse_move` rather than raw deltas" -- stopped being true when the
evdev path landed. There are raw deltas now, and five settings ride them.

**The two that remain are one question, and it is bigger than it looks.**
`scroll_lines` is lines-per-notch and `scroll_mode` is Lines/Pages/Smooth.
`guitk::wheel` already owns the notch-to-row conversion and already has the
shape that would take the setting -- `rows_at(dy, rows_per_notch)` exists, and
`ROWS_PER_NOTCH = 3.0` cites `SPI_GETWHEELSCROLLLINES`, which is precisely
what `scroll_lines` is. So the arithmetic is not the problem. The delivery is.

*Measured:* `.rows(` has **176** call sites outside `wheel.rs`, `rows_f` 6 and
`wheel::pixels` 28. Threading a setting through those is not a refactor, it is
176 chances to forget -- the defect this lane spent the day removing. So the
setting has to arrive at a chokepoint, and there are three:

| | *What changes* | Cost |
|---|---|---|
| **A. Scale at the source.** The compositor multiplies `dy` by `scroll_lines / 3` before sending. | Every consumer honours the setting having changed nothing. | Breaks the documented contract that `dy` is **notches**, `1.0` per detent -- the invariant `event.rs` spends twenty lines defending after twelve consumers each invented their own pixel constant. A trackpad's fractions would also mean something new. |
| **B. A process-wide rows-per-notch in `guitk::wheel`,** set when input settings change; `rows`/`rows_f`/`pixels` read it. | Same, and the notch stays a notch. | A global that 176 sites read and tests write is the cross-test interference that made the palette counter flake this morning before it was made thread-local. Tests would have to use `rows_at` explicitly. |
| **C. Leave `scroll_lines` unwired and delete it and `scroll_mode`.** | The two controls disappear from the Mouse page. | Honest, and much smaller. But `SPI_GETWHEELSCROLLLINES` exists on every platform because users do change it. |

`scroll_mode` is the harder half regardless of which is chosen: **Pages** means
a notch scrolls one viewport, which only the consumer knows the height of, so
no source-side or global scaling can express it. Any real implementation of
Pages is per-consumer whatever happens at the chokepoint.

**Recommendation: B for `scroll_lines`, and file `scroll_mode` separately.**
B keeps the wire contract that the tree has already paid to establish, and the
test-interference cost is bounded and known. `scroll_mode` is a different
question -- it is about what a scroll *means* in each view, not about how big
one is -- and bundling it here is what this entry warned against the first
time.

**Not started, deliberately.** This is a decision with real alternatives that
changes scrolling everywhere, and it should begin a session rather than end
one. What is above is the whole of the preparation: the measurement, the three
chokepoints, and why A is tempting and wrong.

**Update 2026-09-13 (later) — `scroll_lines` is wired, and the option table
above was wrong about which options exist.**

Two corrections to the analysis written an hour earlier, both found by looking
instead of reasoning:

- **Option B was impossible as described.** It says "a process-wide
  rows-per-notch in `guitk::wheel`, set when input settings change" -- but
  **an application is never told the input settings.** The protocol carries
  appearance to clients (`App::appearance_changed`) and nothing carries input.
  What does exist is `Reloads.input`, and the strap already watches
  `appearance.yaml` through a generic `settingsfile::Watcher` that takes a
  config name. So the road was one watcher away, not one protocol away.
- **The population was 176 and is 68.** `.rows(` matches `grid.rows()` and
  every other method of that name. Counting only receivers declared as
  `wheel::Accumulator`, plus `rows_f` and `wheel::pixels`, gives 30 + 6 + 30 +
  2 = **68**. Still far too many to thread a setting through by hand, so the
  conclusion stands and only the number changes -- but the first figure was a
  shape match, which is the error this lane keeps making and keeps writing down.

**What landed.** `guitk::wheel` holds the step in a **thread-local**, defaulted
to `ROWS_PER_NOTCH`; `rows`, `rows_f` and `pixels` read it, and `rows_at` still
takes an explicit step for the view that genuinely needs its own. Thread-local
rather than global because the event loop that sets it is the one that later
calls these functions, and because a process-wide value written by tests is the
cross-test interference that made the palette counter pass alone and fail in
the full run this morning. `set_rows_per_notch` refuses a value that is not
finite and positive, and *answers whether it took it* -- a setter that silently
does nothing is how a setting comes to have a control and no effect, which is
this entry.

`oswindow`'s strap gained `ScrollWatch`, beside `ThemeWatch` and on the same
poll. Its own watcher rather than a field of the other, because the two files
change independently: a theme change must not re-read the pointer
configuration, which is why `Reloads` has two flags.

**`scroll_mode` is still unimplemented, and now says so rather than guessing.**
`scroll_lines` is the step *in Lines mode*. Under Pages or Smooth the watch
keeps the default, because handing Pages the lines number would make "Pages"
mean "seven lines" -- a wrong answer delivered confidently, which is worse than
the default, since the setting would look as though it worked.
`a_mode_that_is_not_lines_keeps_the_default_step` pins that.

**Four tests, and the join is the one that matters.** `guitk` proves the stored
step is what a notch converts to and that a broken step is refused; `oswindow`
proves a value written to `input.yaml` reaches the conversion and that an
unchanged file reports no change. Proved able to fail: making `ScrollWatch`
read the file and store the value without applying it -- the exact shape
`scroll_lines` was already in -- makes the join test report *assertion failed:
the user asked for seven lines a notch*, while everything else stays green.

So the only mouse setting still reaching nothing is `scroll_mode`, which is a
question about what a scroll *means* in each view rather than how big one is.

**So the proper fix is now four pieces, not one call:**

- **(a) A shared `gui/inputsettings` crate**, the counterpart of `appearance` —
  the struct, the YAML file, load/save, and the clamps, owned by neither the
  shell nor the compositor. It must be a third crate for the same reason
  `appearance` is: the shell, the compositor and the settings application all
  need to read it, and any two of them depending on the third is a cycle waiting
  to happen. Note that `gui/*` is globbed in the workspace manifest, so a new
  crate there needs **no** edit to the workspace-root `Cargo.toml` — which lane
  C may not touch. `gui/desktop`'s `mouse_settings` then becomes a re-export or
  is deleted.
- **(b) A `ReloadInput` control verb**, as this entry already recommended —
  same shape as `ReloadAppearance` (no payload, no `link.resolve`), and the
  reasoning against folding pointer behaviour into `AppearanceSettings` stands
  unchanged.
- **(c) A real mouse page in `apps/settings`** with the double-click-speed
  control, writing (a) and notifying (b) — exactly the pattern §523 established
  for appearance, including the "the change is in force before anyone is told"
  rule and the dirty flag drained by the loop.
- **(d) The compositor reads (a) on `ReloadInput`** and calls its existing
  `set_double_click_ms`.

Only double-click is in scope; the pointer-speed/acceleration question in
"Scope beyond double-click" above is unchanged and still deferred.

**If never fixed:** a slider that lies. The user moves it, the panel writes the
file, and the double-click speed is whatever the compositor's constant says.
Worse than an absent setting, because an absent one cannot be misread as tried
and rejected.

### Update 2026-08-22 — FIXED. All four pieces are in.

| | What landed |
|---|---|
| **(a)** | `gui/inputsettings` — the model, the YAML format, load/save, the clamps. It also became the **single owner of the double-click numbers**: `MIN_DOUBLE_CLICK_MS`, `MAX_DOUBLE_CLICK_MS` and `DEFAULT_DOUBLE_CLICK_MS` are `pub` there, and the compositor's private copies were deleted. |
| **(b)** | `RequestBody::ReloadInput`, tag `0x14`, no payload — a second verb, as this entry recommended. `oswindow` exposes it as `EventLoop::input_changed`. |
| **(c)** | A Mouse page in `apps/settings`, between Sound and Notifications, with the double-click slider. It saves `input.yaml` and notifies (b). |
| **(d)** | `Compositor::reload_input` re-reads the file and applies the double-click window. |

Rationale for the three judgement calls — who owns the range, why `ReloadInput`
is its own verb rather than a payload or a shared one, and why the page shows
one control — is `design-decisions.md` §524.

**One correction to this entry's own "Where" table:** it said the two clamp
ranges "were deliberately made to match". They did match, but nothing enforced
it — three processes each held a private copy and any one could have moved
without a single test failing. That is now impossible by construction rather
than by care: `SliderId::DoubleClickMs::range()`, `MouseConfig::set_double_click_ms`
and `Compositor::set_double_click_ms` all read the same three constants.

**Still deferred, unchanged:** pointer speed, acceleration, button mapping,
scroll direction and cursor size. The compositor reads them and drops them on
purpose — it has no local input source to apply them to
(`TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`) — and the Mouse page deliberately draws no
control for them, because a control that saves a value nothing applies is the
same defect this entry was filed about. Each gets its slider in the commit that
gives it a consumer.

**Verification.** 21 reintroduction proofs across the four pieces —
`scripts/reintro-mouse-page.py` (11) and `scripts/reintro-reload-input.py` (10)
— each reintroducing one defect and confirming the suite goes red and names it
back, restoring by byte snapshot verified with SHA-256. Workspace gate green
(46,443 passed) apart from lane B's `posix`, filed separately as
`B-POSIX-HSEARCH-TESTS-RACE-ONE-GLOBAL-TABLE-AND-SEGFAULT`.
