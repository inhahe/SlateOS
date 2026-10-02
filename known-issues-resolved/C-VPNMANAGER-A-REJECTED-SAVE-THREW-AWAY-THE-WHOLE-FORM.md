## C-VPNMANAGER-A-REJECTED-SAVE-THREW-AWAY-THE-WHOLE-FORM (lane C, 2026-08-26) — FIXED 2026-08-26

**In short:** in the VPN Manager's Add/Edit Profile dialog, pressing Save with
one field left blank closed the dialog and discarded *everything else* that had
been typed into it — and the explanation of what was wrong was returned to a
dialog that no longer existed, so it was never shown. The user's only clue was
that nothing happened.

**Where it lived:** `apps/vpnmanager/src/main.rs`, `VpnManager::confirm_edit`.
It did `self.editing_profile.take()` and set `show_add_dialog = false` *before*
calling `validate`, then returned the `Err(String)` to a caller that had no
surface left to display it on.

**Two separate faults, both fixed:**

1. `confirm_edit` now clones the profile, tries the add-or-update, and only
   clears `editing_profile`/`show_add_dialog` **if it succeeded**. A refused
   Save leaves the dialog exactly as the user left it.
2. There was nowhere readable to *put* the complaint. The status bar is behind
   the dialog's scrim (63% black over the whole window), so writing it there is
   a Save button that appears to do nothing. `VpnManager` gained a
   `dialog_error` field, rendered in red inside the dialog under the fields it
   is about, and cleared as soon as the user edits any dialog field — so the
   complaint does not sit under the corrected value still claiming the field is
   blank.

**Why it went unnoticed:** same reason as the entry above — nothing clicked the
Save button. `test_confirm_edit_add` called `confirm_edit` only on a *valid*
profile.

**Regression test:**
`a_rejected_save_keeps_the_dialog_up_with_everything_typed_into_it` — types a
name, saves without a server, and asserts the dialog is still up, the name is
still in it, no profile was added, and the string
`"Server address is required"` is among the `RenderCommand::Text` the frame
actually draws. Confirmed to fail when the early-close is mutated back in.
