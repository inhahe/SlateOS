## TD-C-LOGINUSER-INVENTS-A-UID-IT-DOES-NOT-KNOW -- FIXED 2026-09-13

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/desktop/src/login_screen.rs` — `users_from`, the
`account.uid.unwrap_or(0)`. `LoginUser::uid` is `u32`.

**In short:** an account whose entry in the user database does not say what its
numeric id is gets shown on the login screen as **id 0**, which is the id of
the machine's administrator. Nothing today reads that number, so nothing goes
wrong yet; the danger is the day something does.

**Why the account is offered at all.** `loginusers::offered` deliberately keeps
a record with no readable uid, because a hand-edited `users.yaml` that omits
the field describes a person far more often than it describes a service
account, and dropping the machine's only account is a worse failure than
listing one extra. So the case is real and will not be filtered away.

**Why zero is the wrong stand-in.** Zero is not a neutral placeholder here —
it is *root*. `loginusers::Account` keeps the honest `Option<u32>` for exactly
this reason ("the file does not say" and "the file says 0" are different
facts), and this conversion throws that distinction away at the last step. It
is safe **only** because `LoginUser::uid` currently has no reader: the screen
draws the name, and authentication is by name. The moment anything starts a
session, sets ownership, or checks a privilege from this field, an account with
a malformed database entry becomes an account that claims to be root.

**The proper fix** is for `LoginUser::uid` to be `Option<u32>` too, so the
unknown survives to whoever starts the session and that caller resolves the
name against the database itself — which it must do regardless, since the
screen's copy can be stale by the time a password is accepted. Deferred only
because the session hand-off does not exist yet (see
`TD-C-FOUR-SHELL-FEATURES-ARE-BUILT-AND-NEVER-CONSTRUCTED`), and the right
shape for the field is easier to see with one real caller than with none.

**FIXED 2026-09-13, and the trigger above is why it was done now rather than
then.** *"Do not add a reader without changing the type first"* is a rule that
depends on somebody remembering it at the moment they are busy doing something
else. The type change costs three lines and removes the trap instead of
documenting it, so waiting for a caller bought nothing that the entry's own
statement of the right shape -- `Option<u32>` -- did not already provide.

`LoginUser::uid` is `Option<u32>`; `LoginUser::new` takes one; the
`unwrap_or(0)` is gone and the unknown travels to whoever starts the session,
which must resolve the name against the database itself regardless, since this
screen's copy can be stale by the time a password is accepted.

**The test could not have been written before.** With a `u32` there was no
value to assert: the unknown was spent by `unwrap_or(0)` before anything could
look at it. That is the defect rather than an inconvenience of testing it -- a
type that cannot represent "unknown" makes the wrong answer unobservable.
`an_account_with_no_id_is_not_offered_as_root` asserts the silent account has
`None`, and carries a negative control on an account that *does* state its id,
so the `None` means "the file was silent" rather than "this parser reports
None for everything". Proved able to fail: putting `unwrap_or(0)` back inside
the new type makes it report *an account whose file does not say its id must
not claim one*.
