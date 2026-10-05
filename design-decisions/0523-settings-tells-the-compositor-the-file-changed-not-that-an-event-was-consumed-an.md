## 523. Settings tells the compositor the *file changed*, not that an *event was consumed* — and the change is in force before anyone is told

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Settings had a complete user interface and no way to run: no
`main` that opened a window, no event loop, no connection to the compositor. It
now has all three. The design question that took the thought was not the loop —
that is twenty lines — but *when the compositor gets told the appearance
changed*. The answer is: when `appearance.yaml` is actually written, which is
not the same moment as "the user clicked something", and not the same moment as
"an event was consumed". Getting that distinction wrong produces a setting that
appears to work and does not, which is worse than one that plainly does nothing.

### Why Settings has to tell the compositor anything at all

Settings is the one application that edits `appearance.yaml`. Window corner
radius, drop shadows, title-bar colours and the accent are drawn by the
*compositor*, from that same file. Settings drawing its own preview correctly
proves nothing about the screen.

Without a notification the user would change the corner radius, watch the
preview in the Settings window update immediately, and see every *real* window
on the desktop keep its old corners until the next login. The preview would be
telling the truth about the file and a lie about the desktop.

`oswindow::EventLoop::appearance_changed()` already existed for this
(`gui/window/src/lib.rs`), sending `ReloadAppearance`. It is a **notification,
not a setter**: it carries no payload, and the compositor answers it by
re-reading the user's own file. That matters for what follows — the compositor
never learns *what* changed from Settings, so Settings can never desynchronise
it by describing a change wrongly. The worst a bad notification can do is cost a
file read.

### The flag is a message to the driver, not a precondition for the change

`SettingsState` grew one private `bool`, `appearance_dirty`, set by
`save_appearance` and drained by `take_appearance_change`. The rule governing it
is a direct application of §522's resting-state doctrine, transplanted from
animations to a different subsystem:

> **The change is saved and in force before the flag is read. A caller that
> never drains the flag still gets a correct file and a correct Settings
> window; all it loses is the *liveness* of the update on other windows.**

Concretely: `handle_event` writes `appearance.yaml` itself and updates the
in-process model itself. `appearance_dirty` is set *afterwards*, as a message to
whoever is driving the loop, and nothing downstream of it is load-bearing for
correctness. This is the same shape as §522's "an animation that nobody ticks is
*finished*, not *invisible*": the no-clock, no-driver, nobody-is-listening path
must land on the working state, not the broken one.

Two consequences were written down as tests rather than left as intentions
(`a_change_is_saved_and_in_force_whether_or_not_anyone_drains_the_flag`, and
`taking_the_appearance_change_clears_it`).

There is a third, subtler one. `save_appearance` sets the flag **even when the
write failed**. That looks wrong for about a second. It is right: the compositor
responds to the notification by re-reading the file, so "look again" is an
honest and harmless thing to say after a failed write — the compositor will read
whatever is actually on disk, which is the truth. Suppressing the notification
on failure would instead leave the compositor showing state that may not match
the file at all, if the write failed *partway*.

### Why the notification tracks the file, not the event

The obvious implementation is to notify whenever `handle_event` returns
`Consumed`. It is wrong in both directions, and the two failure modes are the
reason the flag exists at all rather than the loop just reading the return
value:

| | What actually happens |
|---|---|
| Notify on `Consumed` | Every scroll, every dropdown open, every keystroke in a text field asks the compositor to re-read and re-apply a file that did not change. A drag across a slider becomes hundreds of file reads. |
| Notify only on `Consumed` | A control that writes the file but reports `Ignored` loses its notification entirely. |

The second is hypothetical *today* — there is no such control. It is exactly the
kind of thing a future control will do by accident, and the ordering that
protects against it costs nothing, so the loop drains the flag **before** the
redraw check and independently of `result`:

```rust
let result = state.handle_event(&event);
if state.take_appearance_change()
    && let Err(e) = events.appearance_changed()
{ … }
if result == EventResult::Consumed { … submit … }
```

Both directions are pinned:
`an_event_that_changes_no_appearance_setting_asks_for_no_reload` and
`one_change_produces_one_notification_and_not_one_per_later_event`.

### The loop draws once at start-up, and thereafter only on change

`run` submits a frame before entering `EventLoop::run`, because nothing has
happened yet and so no event is going to ask for the first one. After that a
frame is submitted only when the event was consumed. The alternative — repaint
every event — was rejected for the reason a compositor exists: an idle
application that submits frames costs the compositor a composite pass per mouse
move across its own window.

A resize is a special case worth stating: it must be *applied to the model*
before the frame that answers it, or the answering frame is drawn to the old
size and the window shows a stale layout for one frame at every drag step.
`Event::Resize` therefore updates `window_width`/`window_height` **and** returns
`Consumed`, and the test
`a_resize_is_applied_before_the_frame_that_answers_it` pins both halves — the
two ways to break it (apply without repainting, repaint without applying) each
have their own marker.

### Errors out of a handler that cannot return one

`EventLoop::run`'s handler returns `EventResponse`, not `Result`. A failed
`submit` or a failed `appearance_changed` therefore has nowhere to be reported
from inside it. Swallowing them was not acceptable — it leaves a Settings window
that runs on happily while the screen no longer changes, the single most
confusing failure an application can have.

So `run` keeps a `failure: Option<Error<T>>` outside the closure, the handler
stores into it and returns `EventResponse::Exit`, and `run` returns it after
the loop. The loop stops on the first transport error rather than continuing
blind.

### `TestDesktop::asked()` returns names, not `RequestBody`

The tests need to assert *that* Settings asked the desktop to reload, once, and
not once per mouse move. `TestDesktop` already exposed `seen: Vec<Request>`,
which would do it — by making `apps/settings`' test code match on
`guiremote::RequestBody`.

That is the exact coupling `oswindow` exists to prevent. The crate deliberately
does not re-export `RequestBody`: an application should no more name the display
protocol in its tests than in its `main`, any more than a Unix program names the
socket layer. Adding a `guiremote` dev-dependency to `apps/settings` to get
around that would have been a hole in the abstraction opened for the
convenience of a test.

So `TestDesktop` grew `asked(&mut self) -> Vec<&'static str>` — the wire names
of the requests, in order. Two deliberate details:

- **General, not single-purpose.** A `reload_count()` would have been shorter
  and would have had to be joined by a `title_count()` the next time. A list of
  names answers every question of this shape.
- **Spelled out, not derived from `Debug`.** `Debug` prints the payload too, so
  a test asserting `"SetTitle"` would have to know the title, and would start
  failing when a field was added. The `match` is exhaustive with no wildcard, so
  a new request variant fails to compile there and gets a name deliberately.

### The end-to-end test uses an undecorated, pinned window — on purpose

`mod against_the_real_compositor` runs the shipped `Compositor` on a thread
behind a real socket and drives Settings' shipped `run` against it. The click
test's chain is: a hardware mouse event → the compositor's hit test → the wire
encoder → `oswindow`'s event loop → `SettingsState::handle_event` → the YAML
writer → the `ReloadAppearance` notification → the compositor re-reading the
file. No stand-in anywhere.

For that to assert anything exact, the test has to know where in *desktop*
coordinates a control drawn at *page* coordinates lands. Three ways to get that:

1. Read `WindowInfo.x/y` from the window list. **Wrong** — that is the *outer*
   rect, decorations included, so it is off by the frame inset.
2. Hard-code the frame inset. Fragile: it is a compositor detail the test has no
   business knowing, and it changes when the title bar does.
3. Open the window with `.position(AT).decorations(false).resizable(false)`.

Option 3 was taken. With no frame, `client_rect` *is* the window rect,
`title_bar_rect` and `close_button_rect` are `None` so a press falls straight
through to the client area, `detect_border_drag` returns `None` for a
non-resizable window, and desktop coordinates become page coordinates by
subtracting `AT` and nothing else. The translation is then a fact of the
geometry rather than a number the test hopes is still right.

The click target itself comes from `center_of(…)` over the page's own hit
bands, not a literal coordinate — `hit_bands` and `center_of` were widened to
`pub(super)` for this. Reaching a control *by name* is the only way a test of
the loop can be a test of the loop: one that clicked a hard-coded coordinate
would start passing for the wrong reason the day the layout moved.

### Two drafted tests were deleted rather than kept

Both are recorded because "we wrote a test and threw it away" is the kind of
decision that gets silently re-made:

- **`a_click_that_changes_nothing_leaves_the_compositors_appearance_alone.`**
  The defect it named — a spurious `reload_appearance()` — is genuinely
  *unobservable at the compositor*, because `set_appearance` no-ops when the
  file is unchanged. The test would have asserted something it could not see,
  and would have passed with the bug present. The property it was reaching for
  is real and *is* covered, one layer down, by
  `an_event_that_changes_no_appearance_setting_asks_for_no_reload`, which
  observes the request rather than its effect.
- **`the_translation_is_a_translation_and_not_a_coincidence.`** It duplicated a
  loop test, and it would have written to the real `$HOME`.

A test that cannot fail for the reason it names is worse than no test: it is a
claim of coverage that is not there.

### Proof

All fifteen new tests were proved regression tests by the usual method —
reintroduce the defect the test names, confirm a deterministic failure that
names it back, restore the file from a byte snapshot, verify by SHA-256.
Eighteen markers over `apps/settings/src/main.rs` and `gui/window/src/lib.rs`;
all eighteen produced a failure, and every one of the fifteen tests earned at
least one marker. None had to be recorded as merely additional coverage.

Two pre-existing tests were caught as collateral, which is its own small
confirmation that the markers are real defects and not test-shaped ones:
`tests::test_resize_event` (by `resizeisnotapplied` and `resizedrawsnothing`)
and `tests::test_a_click_that_changes_an_accent_reaches_the_file` (by
`nothingissaved`).

### A process finding, not a code one

Running `cargo test --workspace` rather than `cargo test -p settings` at the end
of this task revealed that `apps/editor`'s **test binary had not compiled since
§521** — the frame clock moved `set_wait_timeout` from an inherent method on
`Socket` onto the `Transport` trait, and `apps/editor`'s real-compositor test
module, which does not otherwise name the trait, stopped building. Nothing
noticed for a day, because every task in between gated on the crate it was
editing.

A crate whose test binary does not compile reports no failures. It reports
nothing at all, which reads identically to "no problems" in a filtered log. The
gate is now the workspace, not the crate; the fix (one `use` line) is in, and
the underlying process hazard is logged as
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING`.
