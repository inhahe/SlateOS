## TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED -- ALL FOUR CONSTRUCTED 2026-09-13; tray hiding, pinning and a lasting order left

**Status, 2026-09-29.** The title said "one left" from 2026-09-13 on; the
one, `tray_dnd.rs`, was constructed the same day (43a651a25 "tray icons can
be dragged", e780ed228, 1d4f23f02 -- 25 references in `lib.rs` now). What the
entry still tracks is under **What is left** below: hiding and pinning a tray
icon have no way in, and the icons' order cannot outlive the programs'
process ids.

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/desktop/src/` — `login_screen.rs` (2 417 lines), `blur.rs`
(2 224), `input_method.rs` (1 279), `tray_dnd.rs` (1 185). 7 105 lines.

**In short:** four features of the desktop shell are fully written, declared
as modules, and constructed by nothing. Not one public item in any of them is
referenced from anywhere else in the tree — including the shell's own
`main.rs`. There is no login screen at runtime, no window blur, no
input-method switching, and no drag-and-drop in the system tray, however much
code there is for each.

**Update 2026-09-13 (lane C) — it is two, not four, and until today none of
them could have run whatever the reference count said.**

**The shell had no binary.** `ShellSession` -- the four surfaces, the login
screen, the hotkeys, the animations -- documents itself as "the real loop, and
it is what a live session runs", and `ShellSession::start` was called from
**nothing but its own tests**. `src/main.rs` was a scripted demo that says, in
its own first line, that it is not the shell. So the question this entry asks
-- which modules are constructed -- had a ceiling above it: nothing constructed
the thing that constructs them.

`gui/desktop`'s `desktop` binary now dials the compositor, starts the session
and runs it; the demo is `desktop-demo`, kept because a library whose only
caller is its own test suite drifts from what a real session does, but no
longer holding the name that should start the desktop. This is the same defect
`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` describes for applications, closed for
all 135 of them earlier the same day -- the shell was the one client left, and
it is the client every other one is drawn on top of.

**It does not use `ShellSession::run`, and that is worth knowing before writing
another caller.** Two of the session's outputs are drained by the caller on
purpose -- `take_launches` and `take_login_power` -- because policy about how a
program starts belongs outside the window manager. `run` never yields between
pumps, so anything launched under it is queued and never started. The binary
drives `pump` itself and drains after each turn.

**Re-measured, against code rather than the 2026-09-08 sweep:**

| module | state |
|---|---|
| `login_screen.rs` | **constructed** -- `session.rs:83` imports `LoginScreen`, `:501` its user sources |
| `input_method.rs` | **constructed** -- `DesktopShell::input_methods`, built `with_builtins()`, and its `SwitchShortcut` is read from `keyboard.layout_switch` |
| `blur.rs` | still nothing |
| `tray_dnd.rs` | still nothing |

The first two were wired at some point after this entry was written and the
entry was never re-read -- the fifth stale entry found today. What is different
now is that being constructed finally means something, because there is a
process to be constructed in.

**Re-measured 2026-09-13: it is one, and the entry was accurate for a single
day.** Filed 2026-09-08; three of its four were resolved on 2026-09-09 and the
entry was never re-read. Traced to the commits rather than inferred:

| module | what happened | commit |
|---|---|---|
| `blur.rs` | **moved to `gui/compositor/src/blur.rs`**, where a framebuffer exists, and wired -- `blur::` appears 7 times in the compositor's `lib.rs` | `7692576d3`, 2026-09-09 |
| `login_screen.rs` | constructed by `ShellSession` -- `session.rs:83` imports `LoginScreen`, `:501` its user sources | `43bd98bcf`, 2026-09-09 |
| `input_method.rs` | constructed as `DesktopShell::input_methods` with `with_builtins()`, and its `SwitchShortcut` reads `keyboard.layout_switch` | `96839ec3b`, 2026-09-09 |
| **`tray_dnd.rs`** | **still nothing.** 1 184 lines, and the only reference anywhere is `pub mod tray_dnd;` in `lib.rs:143` | -- |

So "7 105 lines" is 1 184, and the `blur.rs` row was not a mistake in the
sweep: the file was in `gui/desktop/src/` when the sweep ran and moved the next
day. `git log -- gui/desktop/src/blur.rs` says so, which is the check worth
doing before calling a past measurement wrong.

**Why this kept happening.** Five entries were found stale today and this is
the sixth thing corrected; the pattern in all of them is that the work updated
the code and the entry stayed as filed. The specific trap here is that the
entry names a *count* in its title, and a count in a title is a claim that goes
out of date silently -- nothing fails when three of four are fixed, and the
heading still says four.

**And the one that is left is not "unconstructed" — it is the fourth model of a
feature that exists four times and connects nowhere.** `tray_dnd.rs` drags,
drops, pins and reorders **tray icons**. Four things in this tree model a tray
icon, and no two of them meet:

| | what | state |
|---|---|---|
| `apps/systray` (3 809 lines) | a running application that draws a tray: volume, network, battery, clock, notifications, power | **wired** -- `launch("systray", ...)`, one of the 135 |
| `gui/desktop`'s taskbar tray | clock, notification bell, desktop indicator, keyboard-layout indicator, laid out right-to-left from the display edge | **wired**, and these are shell items, not application icons |
| `gui/desktop/src/tray_dnd.rs` (1 184 lines) | `TrayIconSlot`, `TrayIconArrangement`, drag/drop/pin/reorder | nothing constructs it |
| `kernel/src/fs/systray.rs` (607 lines) | the persistence model -- badges, click actions, per-app overrides, visibility | lane A's tree |

**The join that is missing is the one that makes it a system tray at all: an
application cannot put an icon in any of them.** `apps/systray::register_icon`
is public and takes exactly what a third-party app would need -- app name, icon
character, tooltip -- and **all nine of its callers are its own tests**. There
is no control verb, in `gui/remote` or anywhere else, by which one process asks
another to show an icon on its behalf. The icons `apps/systray` draws are
built-in ones it constructs itself.

So the spec's `design.txt:714-717` -- "a system tray like on Windows", "can drag
and drop icons into and out of the system tray", apps that start in or minimise
to it, a per-app override -- has an implementation of every *part* and no
process boundary crossed anywhere in it. This is the shape
`TD-THREE-INDEPENDENT-APPEARANCE-MODELS` and this file's accessibility entry
both describe, at four copies rather than three.

**What the work is, when someone takes it:** a registration verb and a
per-client registry with reaping, then a subscription frame so a tray learns
the list. `gui/remote/src/window_list.rs` is the model to copy and says why in
its own first paragraph -- "a taskbar has to list the windows it did not open,
and had no way to ask" is the same sentence with "icons" in it. **Which of the four is the real tray is now answered** -- `design-decisions.md`
842, and it needed no new judgement: 815's dividing line (*"is this something
the desktop shows you, or a screen you open?"*) puts an always-visible taskbar
strip in the shell plainly. `gui/desktop`'s tray is the real one, `apps/systray`
is the copy, and its unique parts -- quick settings, the volume and network
popups -- are also things the desktop shows you, so they move to the shell
rather than to Settings. The argument from sunk work -- keep the bigger, newer
`apps/systray` -- loses on geometry: a tray is a strip inside the taskbar, and
the taskbar is the shell's. A separate window would have to be parented inside
another process's panel and kept there through every resize, theme change and
scale change.

**Update 2026-09-13 (later): three of the four layers are built, and the
reason for not starting stopped applying the same day.**

The objection recorded above was that layers without reachable consumers are
what produced four models in the first place. Two things answered it: 842
settled that icons go in the shell's tray, and `gui/desktop` gained a binary,
so that destination became a running program rather than a library.

| layer | state |
|---|---|
| 1. a control verb to register, update and remove | **done** — `SetTrayIcon` 0x21, `RemoveTrayIcon` 0x22, `CONTROL_VERSION` 12 |
| 2. a registry in the compositor, reaped per client | **done** — `Compositor::set_tray_icon` / `remove_tray_icon` / `reap_tray_icons` |
| 3. a `TRAY` frame and a subscription | **done** — `guiremote::tray`, `SubscribeTrayIcons` 0x23, `route_tray_list` |
| 4. drag, drop, pin, reorder (`tray_dnd.rs`) | **reorder done** — drag the row; pin and hide have no door yet |
| 5. a click reaching the program that owns the icon | **done** — `ClickTrayIcon` 0x24, `Event::TrayIconClicked` 0x0C, `App::tray_icon_clicked` |
| 6. the user being able to tell the icons apart | **done** — resting on one shows the `tooltip` its program registered, which the shell had been receiving and never displaying |

Plus the two ends: `oswindow::EventLoop` has `watch_tray`, `set_tray_icon`,
`remove_tray_icon` and `tray_icons`, and the shell subscribes, folds each
frame in on its own revision, and draws the icons in the taskbar.

**Nineteen tests**, and the one that matters reads the render tree for the
glyph and asserts it is absent before and present after. Every other step of
the road already had tests and none of them proves a pixel — which is
precisely the state `apps/systray::register_icon` is in: public, correctly
shaped, thoroughly tested, reaching nothing.

**Update, later still: the click routes too, and it needed a delivery path
that did not exist.**

Every notification the compositor sends is addressed to a window, and
`route_input` gives it to the link that `owns` that window. A program may
have a tray icon and **no window at all** — which is exactly what
`design.txt:716` asks for when it says a program may start in the tray. So
there is a second queue, `pending_client_events`, addressed by pid and
drained into the same batch.

A second queue rather than a sentinel window id in the first: an id that is
not a window would have to be recognised at every site that reads one, and
the sites that forgot would look up a window, find nothing, and drop the
event.

**And `oswindow` would have swallowed it silently.** Its dispatch filters on
`id != window` and returns early, so an event with window 0 would have been
encoded, sent, decoded, delivered, and dropped one line before reaching the
application — a program that never answers its own icon, with nothing in
any log. `App::tray_icon_clicked` is dispatched before that filter, so
`on_event` keeps its contract of "events for your window". Found by reading
the strap rather than assuming it.

**What the full workspace run caught that five crates' tests did not.**
`cargo test` over the five crates that changed was green. `apps/explorer`
and `apps/stickynotes` match `guitk::Event` exhaustively, so a new variant
broke two crates nobody had named, and the run reported *targets passed: 0*
with a build error rather than a test failure. That is
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING` happening to the
person who had read it the same afternoon.

**Update, later still again: the row is the user's, and it can no longer
take the taskbar away from them.**

Two things landed after the click. First the order became the shell's:
`TrayIconArrangement` folds each list from the compositor in, so a program
swapping its glyph no longer drags every icon back to registration order,
and a drag moves one. The identity had to be fixed to do it — the module
keyed an icon on a single number, and the wire keys one on `(owner, id)`,
which `guiremote::tray` states outright and the compositor has a test for.
Keyed on `id` alone, hiding one program's icon would have hidden another's.

**Second, and this one is a defect rather than a feature.** Measured on a
1920-wide bar with eighty icons, before the cap: `tray_width` came to 2167,
so `tray_x` clamped to zero, the icon run covered the whole bar including
the clock at x=1805, and `taskbar_button_rect(0).w` was *zero* — no window
buttons at all, on a shell whose only window switcher that is. Nothing
rationed the strip: `MAX_TRAY_ICONS` is 4096 and a program picks its own
ids, so any process that could reach the compositor could make the desktop
unusable, and nothing about it would look like a crash. The run now gets a
quarter of the bar, with a chevron for the rest — see
`design-decisions.md` §844 for why a share and not a count.

**What is left:**

- ~~**`apps/systray` is still the copy**~~ — **done 2026-09-14.** The operator
  answered C-Q12 with option A (§845): the tray is the shell's and that program
  stops existing. Deleted, 3 809 lines. The five things it offered that have
  no home yet — a volume popup with per-application volumes, a network popup,
  airplane mode, battery saver, a brightness slider — are recorded in
  `TD-C-FIVE-TRAY-FEATURES-EXISTED-ONLY-IN-A-PROGRAM-NOTHING-LAUNCHED`, each
  needing a service that does not exist rather than a place to be drawn.
- **Hiding and pinning have no way in.** `TrayIconSlot` carries `visible`
  and `pinned`, the arrangement honours both, and nothing can set either:
  right-click belongs to the program that owns the icon, so the shell's own
  per-icon menu needs a different door. Until it has one those two fields
  are exercised only by tests.
- **The order does not survive a reboot**, and cannot yet: keys are
  `(pid, id)` pairs, so an order written to disk would restore onto
  processes that no longer exist. It needs the compositor to report a
  *stable* name for the program behind a connection, and `ClientLink`
  carries `client_pid` and nothing else.

**A correction to how this was found, because it is the error this file keeps
recording.** The first pass concluded "there are no tray icons anywhere" from a
grep across `gui/remote`, `gui/compositor`, `gui/window` and `gui/desktop` --
four crates chosen because they are where a protocol would live. `apps/` and
`kernel/` were not in it, and that is where three of the four models are. A
population picked for where the answer *should* be is not a population.

**These are not the settings panels, and must not be treated the same way.**
Three unreachable `*_settings.rs` panels were deleted the same day under
`design-decisions.md` §815 — but §815 draws its line precisely here: *"The
volume overlay and the login screen are the desktop showing you something, so
they stay in the shell and get wired up."* The login screen is named in the
decision as a thing to **wire up**, not remove. The other three are the same
kind of thing: chrome the desktop shows you, not screens you open. **Deleting
any of these would be a misreading of §815.**

**How they were found.** A sweep of all 56 shell modules counting external
references to every public item — `struct`, `enum`, `trait`, `type`, `const`,
`static`, `fn` and `mod`. `gui/desktop` is self-contained (its own `main.rs`,
no other crate depends on it), so nothing outside the corpus could reach them.

| module | what it holds | what is missing |
|---|---|---|
| `login_screen.rs` | ~~`LoginScreen`, `LoginPhase`, `LoginUser`, `LoginBackground`, `LoginPowerAction`, `LoginConfig`~~ | **Done, 2026-09-08.** `ShellSession` constructs one when the account database names anybody (`design-decisions.md` §824), draws it on a fifth full-screen surface created last within `Layer::Overlay` so nothing the shell owns is over it, routes every key and click to it while it is up, and answers with `authlib`. What it still lacks is the *session hand-off* — a successful login unmaps the screen and reveals the desktop, but nothing starts a session as that user, because there is nowhere to send that (the shell has no channel to the process server; same gap as `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`). Autologin is acted on since 2026-09-27 (`design-decisions.md` §1427, `gui/desktop/src/autologin.rs`): the marked account signs in before the first frame. Originally: Construction and a session hand-off. §815 says wire it up. *(Correction, 2026-09-08: an earlier version of this row said §818 has to take effect here. It does not — §818 is about the **lock** screen, `apps/lockscreen`, which is a separate program. See `TD-C-DESIGN-DECISION-818-HAS-NOWHERE-TO-BE-IMPLEMENTED`.)* |
| `blur.rs` | ~~`BlurEffect`, `BlurRegion`, `BlurRenderer`, `BlurManager`~~ | **Done, 2026-09-08: moved to `gui/compositor` and wired.** A surface asks with `WindowSpec::blur_behind` (a *role*, so the compositor resolves the parameters from its own palette), and `Compositor::blur_behind_window` runs the pass over the region immediately before that window is drawn — the one moment the framebuffer holds everything behind it and nothing in front. Software targets only; see `TD-C-BLUR-IS-SOFTWARE-ONLY`. Originally it could not be wired in the shell at all: It works on a *framebuffer* (`BlurManager::update_all(&mut [u32], w, h)`) and the shell has no framebuffer: it submits render trees and never sees a pixel of what is behind its surfaces. `blur.rs` was the only file in the whole `gui/desktop` crate to mention `[u32]`. The pixels behind a window are the compositor's, so the pass now lives where it can run; what remains is a protocol way for a surface to ask for it, and a call in the compositor's paint path. Originally: A caller in the compositing path. Note the `TransparencyLevel` appearance setting already exists and has somewhere to be read *from*, so this may be a shorter connection than its size suggests. |
| `input_method.rs` | ~~`InputMethodManager`, `SwitchShortcut`~~ | **Wired 2026-09-08, as the *switcher* it is.** `DesktopShell` owns an `InputMethodManager`; `HotkeyAction::SwitchInputLayout` (Super+Space) advances it and writes `input.yaml`, which the compositor already watches — so the keys actually move, and the choice survives a restart. Two of the three offered shortcuts remain unbound and cannot be bound yet: see `TD-C-TWO-OF-THREE-LAYOUT-SHORTCUTS-NEED-RELEASE-SEMANTICS`. **This is still not an IME** and the note below stands in full. Originally: A caller, **and an actual engine.** This is a *switcher*, not an IME: zero mentions of pinyin, kana, hangul or candidate lists. Wiring it would not by itself make CJK text typable — that needs an engine behind it, and `gui/compositor` only has the `InputEvent::TextInput` hook and a comment saying "a full IME system would handle this separately". Do not record this as "CJK input is one wiring job away". |
| `tray_dnd.rs` | `TrayDragSource`, `TrayDropTarget`, `TrayIconSlot`, `TrayIconArrangement`, `TraySlotConfig`, `TrayArrangementConfig`, `StartInTrayConfig` | A caller in the tray's event path. |

**Order worth doing them in.** `login_screen` first: it is named in §815, it
gates §818, and a machine with no login screen is a machine with no user
accounts in any meaningful sense. **(As of 2026-09-08 three of the four are
done — `login_screen`, `blur` and `input_method`. Only `tray_dnd` is left, and
it is blocked on C-Q12: there are two system trays and the drag-and-drop sits
with the half that has no icons.)** Then `tray_dnd` (self-contained, one event
path). Then `blur` (needs a compositing decision about where the pass runs —
compare the colour-filter work, which had the same question). `input_method`
last, because wiring is the small half of it.

**Why this is being recorded rather than fixed here.** Each is a feature-sized
job with a design question in it, and this was a dead-code sweep. What matters
is that the sweep is written down: before it, nothing in the tree said these
four were disconnected, and their size makes them look finished.
