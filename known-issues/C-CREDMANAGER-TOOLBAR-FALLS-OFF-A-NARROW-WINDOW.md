### C-CREDMANAGER-TOOLBAR-FALLS-OFF-A-NARROW-WINDOW — 2026-08-26 — LANE C, OPEN (narrowed 2026-09-03: the search box now gives way first, and Ctrl+L is tested)

**What happens.** credmanager's toolbar is a fixed row of six buttons laid out
left to right from the sidebar's right edge: Add, the search box, Sort,
Generator, Lock Vault, Settings. It needs 882 px and neither wraps nor scrolls.
Resize the window narrower than that and the right-hand buttons go off the
edge — **Lock Vault among them** — with nothing on screen to say they exist.
At 700 px wide, only Add, the search box and Sort remain.

**The half that is safe.** Since the conversion above, the toolbar is drawn
inside `Frame::clip(layout.toolbar)`, so a button past the right edge records
no hit box at all. There is no invisible click target hanging off-screen, and
no stale bounds check to go wrong. `a_toolbar_button_past_the_right_edge_
records_no_hit_box` pins that.

**The half that is not.** The button is simply gone. For Settings that is an
annoyance; for **Lock Vault** it is a security control the user cannot reach
without resizing the window, which is not an obvious remedy when the thing you
want is to lock your passwords and walk away. The keyboard has no accelerator
for it either.

**The proper fix**, in rough order of preference:

1. Give the toolbar an overflow: measure the buttons, and when they do not fit,
   collapse the ones that do not into a `»` menu at the right edge. This is
   what every real toolbar does and it needs a popup, which the toolkit does
   not have yet.
2. Failing that, drop the search box's width first — it is the only elastic
   thing in the row and it is 200 px of the 882.
3. At minimum, bind Lock Vault to `Ctrl+L` so it is reachable at any size. This
   is a few lines in `handle_key` and should probably happen regardless of
   which of the above lands.

**Until then**, `App::initial_size` is 1280×800 and the layout is comfortable
there, so this only bites a user who deliberately narrows the window. When the
toolbar learns to overflow, the test named above should start failing — which
is the intent.

### Improved 2026-09-03 — the search box now gives way first, and one claim above was wrong

**Correction first: recommendation 3 was already done, and "the keyboard has no
accelerator for it either" was false when it was written.** `handle_key` has had
a `Key::L if key.modifiers.ctrl => state.vault.lock()` arm all along
(`apps/credmanager/src/main.rs`). What was missing was not the binding but any
*test* of it: nothing drove that keystroke, and the arm sits after an early
return for the lock screen, so "it is in the file" and "a keystroke reaches it"
were different claims with only the first one checked.
`ctrl_l_locks_the_vault_even_when_the_button_is_off_the_edge` now drives it —
through `handle_event`, not `handle_key`, because the press/release guard is at
the dispatch site and calling `handle_key` directly would pass just as loudly if
that guard were wrong. It also asserts the *release* does nothing, which is the
defect `check-key-release-wiring.py` exists to find.

**Recommendation 2 is implemented.** `search_box_width(width)` makes the search
box the one elastic control in the row: it is 200 px when there is room and
shrinks to a floor of 80 px before any button is pushed off. The search box is
the right thing to shrink because it is the only control that is still *itself*
at half size — a button at half width is a button with its label cut in half.

**What that buys, measured rather than estimated:**

| window | before (search fixed at 200) | after |
|---|---|---|
| 1280 px | all six drawn | unchanged — the box is at its full 200 |
| 820 px | `Settings` off the edge | all six drawn; box at 126 |
| 700 px | `Lock Vault` **and** `Settings` off | `Lock Vault` fully drawn at 600..670; `Settings` straddles the edge |
| 500 px | four off | `Lock Vault` and `Settings` off; the rest drawn |

So the row now needs about 762 px rather than 882, and `Lock Vault` — the
security control this entry was really about — survives down to ~682 px instead
of ~802.

**Two existing tests changed, and both changes are the fix working.** This entry
predicted one of them: "When the toolbar learns to overflow, the test named
above should start failing — which is the intent."

- `a_toolbar_button_past_the_right_edge_records_no_hit_box` measured at 700 px,
  where `Settings` used to sit at 802..882, entirely off. It is now at 682..762,
  i.e. *partially* visible, and correctly records a hit box for the part you can
  see. The test moved to 500 px, where the property it is about — a button drawn
  **entirely** past the edge is not clickable — still applies unchanged. It also
  gained an assertion for the new middle case: a button straddling the edge is
  clickable only where it is visible, and its recorded rect stops at the window
  edge.
- The `Ctrl+L` test's own precondition had to move from 700 px to 500 px for the
  same reason: at 700 the button is now reachable, so the test would have been
  asserting nothing.

**Recommendation 1 — a `»` overflow menu — is still the right end state and is
still blocked** on the toolkit having a popup. This does not pretend to be one:
below ~682 px the right-hand buttons still fall off with nothing on screen to
say they exist. What has changed is that the window has to be a good deal
narrower before that starts, and that `Ctrl+L` is now known to work rather than
merely present.
