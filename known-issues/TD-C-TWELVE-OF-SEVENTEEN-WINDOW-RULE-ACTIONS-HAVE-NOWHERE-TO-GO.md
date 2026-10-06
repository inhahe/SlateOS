## `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO` (lane C, 2026-08-26) -- **now two of seventeen**

**Update 2026-09-07: sixteen of the seventeen work.** Eleven of the twelve
this entry describes were built in one session, in the order the entry's own
increment list proposed. What remains is `target_monitor`, which waits on
multi-monitor support and is the one the entry always put last.

**Correction 2026-10-05: fifteen, not sixteen.** `no_decorations` has no
request either -- the table below always said so, and
`DesktopShell::rule_requests` sends nothing for it. A window's frame is set by
its own program in its `WindowSpec` when it is created; taking it away needs a
shell request of the `ShellSetWindowPolicy` kind, and a compositor that can
drop a frame from a live window. The count above was found wrong while
correcting two doc comments that still said "five of seventeen"
(`gui/desktop/src/lib.rs` `rule_requests`, `gui/desktop/src/window_rules.rs`).

**Update 2026-10-05: sixteen of eighteen.** An eighteenth action,
`to_tray` (`tray:` in `window-rules.yaml`), sends a window to the system tray
when it is minimised -- and, with `state: minimized`, starts it there. It is
carried out in the shell, like `skip_taskbar`, so it was working the day it
was added. The two that do nothing are still `target_monitor` and
`no_decorations`.

The title is left as it was written. It is wrong now and that is the point:
renaming it would lose the thing worth remembering, which is that a settings
page can accept, save and *display* twelve settings that do nothing, and that
the only way a user finds out is by writing one and watching nothing happen.

**What the twelve needed, in the end.** Four new privileged requests
(`ShellSetOpacity`, `ShellMove`/`ShellResize`, `ShellSetStackTier`,
`ShellSetSizeLimits`, `ShellSetWindowPolicy`), one new verb
(`ShellControlAction::Fullscreen`), and `CONTROL_VERSION` 4 → 10. Every one
follows the same shape: a separate wire tag, `require_shell()` rather than
`link.resolve()`, and -- where an equivalent self-only operation already
existed -- the *same* `CompositorRequest`, because only the right to ask
differs.

**Two of the entry's own reasons turned out to be wrong**, and both cost
investigation before the work could start:

- "the compositor has no per-window constraint store at all" -- it had one,
  enforced by every resize, by maximise, and at creation. Only the setter was
  missing.
- "a per-window layer override" for `always_on_top` -- a layer is the
  client's, chosen at creation, so an `AboveNormal` layer would let any
  program put itself above the taskbar. It needed a *tier within* a layer.

### The original entry, for the record

**`TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO` (lane C,
2026-08-26)** -- as filed, when twelve of the seventeen actions had nowhere to
go. Demoted from a `##` heading to bold on 2026-09-21: it is a quoted copy of
the entry above, and as a heading every triage count saw this entry twice --
once closed and once open. Same defect as the two recorded in
`scripts/check-known-issues-index.py`, hidden for two weeks longer because the
backticked spelling was outside what that checker could see.

**In short:** The Settings panel has a "Window rules" page where you can say
things like *"the editor should always open maximised on desktop 2"* or *"chat
windows should be 80% transparent and always on top"*. As of today five of the
seventeen things you can ask for actually happen. The other twelve are accepted
by the panel, saved to the config file, shown in the rule list — and then
nothing. There is no error and no greyed-out control: the rule simply has no
effect, and the only way to find out is to write one and watch nothing happen.

**Where:** `gui/desktop/src/window_rules.rs` (the `rule_actions!` field list,
~line 255) declares the seventeen -- since 2026-10-05 it is
`gui/windowrules/src/lib.rs`, the crate the rules moved to when they gained a
file (§1465). `gui/desktop/src/lib.rs`
`DesktopShell::rule_requests` turns the five that work into `ShellRequest`s.
`gui/remote/src/control.rs` is the protocol that would have to grow for the
rest.

**What works today**

| Field | How it is carried out |
|---|---|
| `skip_taskbar` | shell-local: `ManagedWindow::skip_taskbar`, filtered out of `taskbar_windows` |
| `skip_alt_tab` | shell-local: filtered out of `switcher_windows` |
| `to_tray` | shell-local, added 2026-10-05: minimised, the window leaves `taskbar_windows` and `switcher_windows` for an entry of the shell's own in the tray (`ManagedWindow::in_tray`) |
| `initial_state` | `ShellControlAction::Minimize` / `Maximize` / `Fullscreen` (all three, since 2026-09-07) |
| `snap_zone` | `ShellControlAction::SnapToZone(SnapSlot)` |
| `desktop` | `ShellRequest::MoveWindowToDesktop` |

**What does not, and why.** The shell may only ask the compositor for things
`ShellControlAction` names — eight verbs about *another client's* window
(`Activate`, `Minimize`, `Restore`, `Maximize`, `Close`, `SnapLeft`,
`SnapRight`, `SnapToZone`). Every other request in the protocol —
`Move`, `Resize`, `SetOpacity`, `SetFullscreen`, `SetVisible` — resolves
against **the sender's own window**, so a shell cannot use them on someone
else's. That is a deliberate property of the protocol, not an oversight: it is
what stops any client that can talk to the compositor from moving every other
client's windows around.

| Field | What it would need |
|---|---|
| ~~`position`~~ | **Done 2026-09-07**, except `CenterOnMonitor(n>0)`. `RequestBody::ShellMove`, tag `0x1A`. |
| ~~`size`~~ | **Done 2026-09-07.** `RequestBody::ShellResize`, tag `0x1B`. |
| ~~`min_size`, `max_size`~~ | **Done 2026-09-07.** The store existed all along — see increment 4. |
| ~~`opacity`~~ | **Done 2026-09-07.** `RequestBody::ShellSetOpacity`, tag `0x19`, `CONTROL_VERSION` 5 → 6. |
| ~~`always_on_top`, `always_on_bottom`~~ | **Done 2026-09-07.** A `StackTier` within the layer, not a layer override — see increment 3. |
| `target_monitor` | multi-monitor placement, which the compositor does not model yet |
| `no_decorations` | decorations are the client's own; there is no request to strip them |
| ~~`prevent_close`, `prevent_move`, `prevent_resize`~~ | **Done 2026-09-07.** `WindowPolicy`, enforced in the compositor exactly where the entry said it had to be. |
| ~~`initial_state: Fullscreen`~~ | **Done 2026-09-07.** `ShellControlAction::Fullscreen`, `CONTROL_VERSION` 4 → 5. |

**The proper fix**, and why it is not one commit: the eight-verb
`ShellControlAction` is a lane-C-owned enum in `gui/remote`, so adding verbs is
cheap — but `CONTROL_VERSION` is a wire version and each addition costs a bump
plus a compositor-side implementation, and three of the twelve (`prevent_*`)
need a policy store the compositor does not have. The honest increments are:

1. ~~`ShellMove` + `ShellResize`~~ — **done 2026-09-07**, `CONTROL_VERSION`
   6 → 7. Both follow `ShellSetOpacity` exactly: a separate tag, gated on
   `require_shell` and naming the window directly, mapping to the *existing*
   `CompositorRequest::Move`/`Resize`. `RememberLast` needed nothing new --
   `resolve_remembered` already turns it into `Absolute`/`Exact` before the
   requests are built, so the bookkeeping that "currently feeds nothing" now
   feeds these.

   **Size is asked for before position, and that is not cosmetic**: a centred
   placement is computed *from* the size, so a window sized after being
   centred would be centred for the size it used to have. There is a test on
   the ordering.

   **Two cases are declined rather than guessed**, and the declining is the
   part worth reading:

   - *Centring with no size in the rule.* The shell is told window positions
     in the window list but not the size a program is about to choose, so
     there is nothing to centre. Declining leaves the window where the program
     put it; centring against a guess moves it somewhere wrong.
   - *`CenterOnMonitor(n)` for n > 0.* This shell has bounds for one display.
     Putting a window on the wrong screen is a worse answer than leaving it
     alone, so it waits for multi-monitor with `target_monitor`.

   Percentages *are* resolved, against the display the shell was built for.
2. ~~`ShellSetOpacity` and a `Fullscreen` verb~~ — **half done 2026-09-07**:
   the `Fullscreen` verb is in. It took a wire byte *past* the zone range
   rather than the free slot at 7, because the zone bytes are
   `ZONE_BYTE_BASE + slot` and taking 7 would have shifted all twenty-two of
   them — renumbering actions every deployed peer already agrees on. An older
   peer answers `None` to the new byte, which is what it should do with a verb
   it does not know.

   The compositor arm sets fullscreen to `true` rather than toggling: a rule
   says "open this fullscreen", and a verb that flipped the state would make
   the result depend on what the window was already doing and undo itself if
   the rule ran twice.

   **The test that mattered was the one that nearly was not written.** Wiring
   the arm to `maximize_window` instead passed all 695 compositor tests --
   `every_shell_control_action_reaches_its_own_operation` did not know about
   the new verb, and the shell-side tests only prove the *request* is sent.
   The action test now distinguishes fullscreen from maximize explicitly, and
   its doc records the near miss. Mutation-checked afterwards.

   **`ShellSetOpacity` done 2026-09-07 too, so increment 2 is complete.** It
   is a new request rather than an action byte, and it maps to the *existing*
   `CompositorRequest::SetOpacity`: the operation is identical to the
   self-only one and only the right to ask differs, so a second internal
   variant would have been two copies of "set this window's opacity" free to
   drift. What differs is the wire arm -- `require_shell()` and a direct
   `WindowId::from_raw` instead of `link.resolve`, exactly as `ShellControl`
   does. A separate *tag* rather than a flag on the existing request, so the
   privileged path cannot be reached by getting a boolean wrong.

   `ShellRequest::SetOpacity` carries a `u8`, not the `f32` the rule stores
   and the wire carries. `f32` is not `Eq`, and keeping it would have dropped
   `Eq` from `ShellRequest`, `ShellAction` and `HotkeyOutcome` over one field.
   It is also what survives: the compositor blends with an eight-bit alpha, so
   a finer opacity is discarded a layer below. 0.0 and 1.0 convert to 0 and
   255 exactly, which a test pins -- "fully opaque" arriving as 254 would be
   almost impossible to see and is the failure worth naming.

   The privilege test drives the *same foreign window* through both requests
   and asserts the shell one is accepted and the ordinary one refused. Either
   half alone would pass while the distinction was broken.
3. ~~A per-window layer override for `always_on_top` / `always_on_bottom`.~~
   **Done 2026-09-07**, `CONTROL_VERSION` 7 → 8, and *not* as a layer
   override — that framing turned out to be the wrong one.

   **A layer is the client's; a tier is the shell's.** `Layer` is chosen by
   the window's own client at creation, so an `AboveNormal` layer would let
   any program put itself above the taskbar simply by asking. `StackTier`
   (`Bottom` / `Normal` / `Top`) is set only by a shell applying a user's
   rule, and orders windows against their neighbours *within* a layer. The
   stacking key became `(Layer, StackTier)`, so an always-on-top window is
   above the other applications and still below the desktop's own furniture.

   The test that pins this asserts **both** halves, because only the pair
   rules out the obvious wrong implementation: reusing `Layer::Overlay` for
   "always on top" passes "above its neighbours" and fails "below the shell".

   `raise_within_layer` sorts on the same key, which is what makes "always"
   mean always -- clicking another window cannot lift it past a pinned one.
   Mutation-checked: dropping the tier from the sort key fails two tests.

   The shell resolves `always_on_top` + `always_on_bottom` set together into
   one tier rather than sending the compositor a contradiction, and a rule
   silent on stacking asks for nothing at all.
4. **Done 2026-09-07.** `min_size`/`max_size` at `CONTROL_VERSION` 9;
   `prevent_close`/`prevent_move`/`prevent_resize` at 10, as one
   `WindowPolicy` carried by `ShellSetWindowPolicy` (tag `0x1E`).

   The entry was right that these could not be shell-side, and the three
   enforcement points are exactly where it said: `request_close`, the
   title-bar drag, and the edge drag.

   **They restrain the user, never the program.** `prevent_close` refuses the
   *request* -- what a close button and a taskbar menu send -- and not
   `destroy_window`, which is how a program exits. A rule that could stop a
   process exiting would be a way to make one unkillable from a text file.

   **`prevent_move` stops the drag without stopping the press.** A pinned
   window still focuses and raises from its title bar; refusing the press
   outright would make it unfocusable by the one part of it a user reliably
   aims at.

   One request rather than three: the flags are one rule's worth of answer,
   and sending them separately could leave a window half-restrained if the
   second frame were refused. An explicit `false` in a rule takes a
   restriction back; silence imposes none.

   **This entry's claim that "the compositor has no per-window constraint
   store at all" was wrong when I read it.** `Window::min_size`/`max_size` and
   `Window::clamp_size` already existed and were already consulted by every
   resize, by maximise, and at creation. What was missing was only the ability
   for a *shell* to set them: they came from the client's `WindowSpec` and
   nowhere else. `RequestBody::ShellSetSizeLimits` (tag `0x1D`) is that.

   **Zeroes on the wire mean "leave this one as it is", not "no limit"**, and
   that distinction is the whole design. The rule vocabulary has `min_size`
   and `max_size` as *optional* fields — a rule either names one or says
   nothing, and there is no way to write "remove the minimum this program
   asked for". Had zero meant "no limit", a rule naming only a maximum would
   silently discard the program's own minimum, and the user would find out
   when the window collapsed under a drag. Mutation-checked: making `None`
   clear instead of skip fails the test.

   The new limits are applied to the window immediately rather than at the
   next resize. A rule that says "at most 400 wide" and leaves a 900-wide
   window alone until somebody drags its edge has not been applied.

   The limits are asked for before the size, so a rule setting both clamps on
   the way in rather than being corrected afterwards.
5. `target_monitor` last, behind multi-monitor support.

**Severity while open:** low but *dishonest*, which is the part that matters.
Nothing breaks; the user is shown a control that does nothing. If the fix is
going to be deferred past the next Settings pass, the panel should grey out or
mark the twelve rather than let them be written — a rule that is saved and
ignored is worse than a control that says it is not available yet.

**Not a regression.** Until 2026-08-26 the rules engine had no caller at all, so
*seventeen* of seventeen did nothing. This is the state after wiring five of
them up.
