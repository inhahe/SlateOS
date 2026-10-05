## `C-SETTINGS-BUTTONS-WITH-NOTHING-BEHIND-THEM` — visible half fixed 2026-08-20

**Status.** The seven buttons are now **drawn dimmed** instead of looking
pressable, so the app no longer promises an action it cannot perform. The real
half — actually implementing the seven features — is unchanged and still waits
on the services named in the table below. See "Fixed 2026-08-20" at the end of
this entry, and `design-decisions.md` §478. The original report follows.

**In short:** the Settings app draws seven push buttons that do nothing when
clicked, because there is no state for them to change yet. They are not broken
wiring — the features they would trigger (changing a password, reinstalling the
OS, rolling back a package generation) do not exist. Right now clicking one is
silently ignored, which is honest but unhelpful; a user cannot tell "not
implemented" from "my click missed".

**Where:** `apps/settings/src/main.rs`. The `ButtonId` enum lists the buttons a
click can reach, and has exactly one variant, `CheckForUpdates`. Every other
`button_at(…)` call passes `None` for its click target, so no band is
registered:

| Button | Page | What it would need |
|---|---|---|
| Change Password | User Accounts | an account/credential service |
| Add Account, Remove Account | User Accounts | the same |
| Manage Family Settings | User Accounts | the same |
| Clear Activity History | Permissions | a stored activity log |
| Go Back (to previous version) | Recovery | package-generation rollback (`pkg`) |
| Reset (Fresh Start) | Recovery | a reinstall path |

The Snapshots page is the same shape: it lists four package generations and
marks the current one, and none of the rows is selectable. So is the account
picture grid on User Accounts, which is drawn and inert. (The picture grid was
fixed on 2026-08-20 — see
`C-SETTINGS-ACCOUNT-PICTURE-GRID-WAS-DRAWN-BUT-NOT-SELECTABLE` below.)

**Why it is `None` rather than a target that does nothing:** a registered band
swallows the click. A button that consumes a click and produces no effect is
worse than one that ignores it, because the pointer-level feedback ("nothing
happened here") is the only signal the user gets either way, and swallowing
also blocks anything drawn beneath from ever receiving it.

**How to confirm:** `grep -n 'button_at(' apps/settings/src/main.rs` and count
the calls whose last argument is `None`.

**The proper fix, in two parts.** The visible half is cheap and can be done now:
draw a button with no target in a disabled style (dimmed fill, muted label) so
the UI stops promising an action it cannot perform. The real half is per-button
and each waits on a service that does not exist — `pkg` generation rollback for
Recovery and Snapshots, an accounts service for the four account buttons. Add a
`ButtonId` variant and an `apply_row_hit` arm as each lands; the `None` at the
call site is the single place that has to change.

### Fixed 2026-08-20 — the visible half

Done as described, but with the styling *derived* from the click target rather
than requested alongside it. `PageSink::button_at` already took
`what: Option<RowHit>`; that one value now picks the painter as well as
registering the band, so `render_button` runs for `Some` and a new
`render_disabled_button` for `None`. **There is no way to ask for the live
colour while passing `None`** — the combination that was the bug is not
expressible. Same reasoning as §475/§476/§477 and recorded as §478.

`render_disabled_button` takes no colour argument. The colour a live button
carries says what *kind* of action it is (accent for ordinary, red for
destructive), and a button that cannot act has no kind; a greyed-out "Remove
Account" painted red would be an alarm about something that cannot happen.
Width and height come from the same `button_width`/`BUTTON_HEIGHT`, so nothing
on the page moves depending on whether a feature exists yet.

One button was not going through `button_at` at all: "Change Password" was
painted by a raw `render_button` inside a `row(…)` closure, which is why it
could never have had a band. A new `PageSink::button_row` routes a
button-in-a-row through `button_at`, so there is now one path.

The `None` band is still `None`, deliberately — the argument in the original
report stands. A band that swallowed the click would take the "nothing happened
here" feedback away *and* block anything drawn beneath. Dimming is what tells
the user why nothing happened; not registering is what keeps the click honest.

Three tests (168 green, clippy clean, rustfmt clean). They read the paint out
of the render tree and the clickability out of the page's click bands — two
independent places — and assert the two agree, in both directions: a
live-looking button with no band is the original complaint, and a dimmed button
that *is* clickable is a working feature the user has been told not to try.
`the_buttons_with_nothing_behind_them_are_the_ones_on_record` pins the census at
exactly these seven, so wiring one up without striking it off here fails, and so
does adding an eighth.

Mutation-tested with five mutations; all five caught. One was missed at first:
giving the disabled button back its full-brightness label was invisible, because
the tests read only the fill. A dimmed fill under a live label is half a
disabled button and reads on screen as a live one. The liveness test now checks
both commands.

**What is left.** Only the real half, per-button, each waiting on a service:

| Button | Waits on |
|---|---|
| Change Password, Add/Remove Account, Manage Family Settings | an accounts/credential service |
| Clear Activity History | a stored activity log |
| Go Back | `pkg` generation rollback |
| Reset (Fresh Start) | a reinstall path |

As each lands: add a `ButtonId` variant, an `apply_row_hit` arm, and change the
`None` at the call site to `Some(RowHit::Press(…))`. The button becomes live
automatically — no styling change is needed, because there is no styling
argument. Strike the label from the census test in the same commit.

The Snapshots page's generation rows and the User Accounts picture grid are the
same shape (drawn and inert) but are not buttons and were **not** covered by
this fix; they needed a selectable treatment of their own. The picture grid got
one the same day — see
`C-SETTINGS-ACCOUNT-PICTURE-GRID-WAS-DRAWN-BUT-NOT-SELECTABLE`. The Snapshots
rows are still inert and still wait on `pkg` generation rollback.
