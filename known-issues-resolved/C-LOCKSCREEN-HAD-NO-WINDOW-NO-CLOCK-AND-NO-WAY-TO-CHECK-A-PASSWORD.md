## `C-LOCKSCREEN-HAD-NO-WINDOW-NO-CLOCK-AND-NO-WAY-TO-CHECK-A-PASSWORD` (lane C, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** the lock screen — the program that guards a machine while its
owner is away — could not be run, could not tell the time, and had nothing that
could check a password. `fn main` was empty, so the whole file was unreachable.
The clock was set once, at construction, to a hard-coded 12:00:00 and never
moved. And the trait it uses to ask "is this the right password?" had exactly
one implementor, a test one; the only thing a real `main` could have passed was
"nobody can answer", which the screen correctly treats as *refuse everyone* —
i.e. a screen that locks a session nothing can ever unlock. All three are fixed.

### What changed

**1. It opens a window.** `impl oswindow::app::App for LockScreen` plus a real
`main`. `render` takes the granted width and height and writes them into the
fields every hit test on this screen reads, rather than keeping the 1920×1080 it
asked for — the first frame goes out before any `Event::Resize`, so a screen
that trusted its own request would draw the password field in one place and
accept clicks for it in another. `resizable()` is `false`: a user who could drag
the corner of a lock screen could drag the desktop out from behind it. An
`Event::Key` that authorises an unlock returns `Response::Exit`, which is what
actually dismisses the screen; `take_unlock_request` is collected after *every*
event, not only after a key, because the flag authorises exactly one unlock and
one left uncollected is one the next person at the keyboard spends.

**2. The clock is a clock.** `set_time_from_utc(i64)` is pure and assertable;
`refresh_clock()` is the single impure wall-clock read, called from the
`Event::Tick` arm and once before the first frame. Same split, for the same
reason, as the desktop's `clock_string_at` / `current_clock_string`.
`tick_interval()` is never `None` — one second while only the clock is moving,
16 ms while the shake animation or the lockout countdown is. One second rather
than one minute even with seconds hidden: a 60-second timer started at an
arbitrary moment turns the minute over up to 59 seconds late, and a lock screen
that is a minute wrong is indistinguishable from a machine that has been sitting
untouched. Zone is an explicit `Tz::utc()` via `lookup().gmtoff`, which is the
tree's convention (`rg 'Tz::utc' apps/ gui/` lists every surface that renders an
instant) and is what makes a real zone a one-line change.

**3. It can check a password, against the system's real stores.** `SystemAuthority`
wraps `authlib::Authenticator` — `/etc/users.yaml`, `/etc/shadow`, and the shared
faillock tally, so a failure here counts against a failure at `su`. The screen's
`AuthOutcome` was already written against `authlib::Outcome`: six variants, same
meanings, documented in terms of each other, so the translation is an exhaustive
`match` with no wildcard. A seventh verdict in `authlib` therefore breaks the
build rather than being silently folded into "rejected".

**4. The user list comes from the same store the authority resolves against**
(`system_users`), so a name offered on the screen is a name that can be answered
for. System accounts are filtered by uid, and the filter is a *range* —
`1000..=60000` — not a floor: `nobody` is conventionally 65534, i.e. **above**
the range, so the obvious `uid >= 1000` keeps the one account the filter most
exists to hide. Locked accounts stay listed; hiding them would make an
administrator's lock look like a deletion.

### The bug this wiring nearly shipped, and the rule that stops it

`LockScreen::submit_password` short-circuits on `!has_password` and returns
`AuthOutcome::NoPassword` **without asking the authority anything at all**, and
`unlocks_for` currently accepts that verdict. So `has_password` is not a display
hint. It is a decision about who gets in.

The natural implementation gets it exactly backwards. `/etc/shadow` is
root-owned and readable by nobody else, and this screen runs as the logged-in
user — so `shadow::lookup(...).is_some_and(|e| !e.password_hash.is_empty())`
answers `false` on every correctly-configured machine, and **the lock screen
opens on the first Enter.** The failure is silent, it is on the default path,
and a test suite built on synthetic `UserInfo::new(..., true)` values cannot
see it, because the field is a *parameter* in every test and a *reading* only
in production.

So `account_has_password` is fail-closed: only a positive reading that the
stored entry is empty returns `false`. Unreadable file, unknown user, missing
store, locked account — all `true`. A passwordless account pays one prompt it
can answer; an unreadable store pays nothing except that the authority, which
runs where it *can* read the file, gets asked. `an_unreadable_store_does_not_mean_there_is_no_password`
drives it end to end through the screen and asserts the Enter is refused.

The store precedence deliberately mirrors `authlib`'s private `resolve` —
native database first, `shadow(5)` second, and it *stops* at the native database
once it finds the user, exactly as `resolve` does — so the account this screen
calls passwordless is the account `authlib` would. The single divergence is the
final `None`, where `resolve` says "unknown user" and this says "assume a
password". The policy is stated once, in `record_has_password`, and shared by
both the by-name lookup and the list walk: two copies of "does this account have
a password?" would be two copies of a rule that decides who gets in.

### Cost

82 → 100 tests, still under a second. New dependencies: `authlib` and `userdb`
(both lane B's, both already in the workspace, both build for
`x86_64-slateos`); `scratchdir` and `posix` as dev-dependencies, the latter so
the end-to-end test computes its `$6$` entry with the hasher `authlib` will
recompute it with rather than pasting a literal that keeps passing after the
hasher has changed. `scripts/check-window-wiring.py` ratcheted 129 → 128.

### What is still owed

- **The screen still reads the credential store itself.** `PasswordAuthority`'s
  doc says the screen should hold *a connection, not a hash*, precisely because
  `apps/lockscreen` runs as the logged-in user and is the process most likely to
  be attacked. `authlib` removes the second copy of the *policy*, which was the
  dangerous half (design-decisions §329 is three hashers that disagreed about
  what a stored entry means), but the process still needs read access to the
  store. The replacement is `logind`'s `AuthenticateSession`, which is written
  and blocked: it answers `UnknownCaller` to everyone because the kernel gives a
  service no way to learn its caller's uid. **Trigger:** when
  `requests/b-a-a-service-cannot-find-out-who-is-calling-it.md` lands, delete
  `SystemAuthority`'s body in favour of a bus connection. Nothing above the
  trait moves.
- **There are now two rate limits.** The screen's own `LockoutTimer` is checked
  before `authlib`'s faillock tally is consulted, so the screen's thresholds
  fire first. That is not a regression — it is the behaviour the screen has
  always had — but it is duplication, and the tally that belongs to the system
  is the one that should win. Deliberately not changed here: a rework that
  quietly alters who can unlock a machine is two changes wearing one commit
  message.
- **`unlocks_for(NoPassword) == true` is still an open question**, unchanged and
  untouched by any of the above. See `open-questions.md`.

### The lesson

**A boolean that gates an authentication short-circuit is not a display hint,
and it must be read fail-closed.** `has_password` looked like presentation — it
decides whether to draw a password field — and it was also the whole
authorisation check for one of the six outcomes. The tell is structural and
worth looking for elsewhere: a field that every test *sets* and only production
*reads* has no test coverage at all, however many tests mention it.
