## §496 — A remembered "allow" expires after eight hours; a remembered "deny" never does

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When the desktop asks "may this program use your microphone?",
there is a *Remember this* checkbox. Ticking it and clicking Allow used to mean
"never ask me again for the rest of this session" — the timestamp the dialog
recorded alongside the decision was written and never looked at. Now a
remembered *yes* stops answering after eight hours and the user is asked again,
while a remembered *no* keeps answering forever. The two directions are treated
differently on purpose.

**Where:** `gui/desktop/src/security_dialog.rs` — `RememberedDecision::is_live`
and `REMEMBERED_ALLOW_LIFETIME_MS`.

### Why the asymmetry

An old refusal cannot become dangerous. The worst a stale "no" does is keep
refusing something the user already refused, and the user has an obvious way
out: start the program again and answer differently. Expiring denials buys
nothing and costs a re-prompt for a question already settled.

An old grant gets *more* dangerous with age, along two axes the user cannot
see. The program may have been updated since — the binary that has the
microphone right is not the binary that was granted it. And the user simply
forgets: a permission given in the morning is invisible by evening, because
there is no revocation UI in which to notice it. Nothing else in the shell can
take a right away, which is what makes the expiry load-bearing rather than
merely tidy: it is currently the *only* path by which a granted capability is
ever given back.

So the risk-free direction is remembered indefinitely and the risky one is not.
This matches how a user reasons about the checkbox — "remember" plainly means
"don't pester me", not "grant permanently" — and it fails in the direction of
asking too often rather than granting too long.

### Why eight hours specifically

It is a policy, not a constraint, and it is chosen to be about one working
session: a permission granted in the morning is asked for again the next day.
The tradeoff is "asked too often" against "granted longer than intended", and
the honest position is that any value in the 4–24h range would be defensible.
What is *not* defensible is infinity, which is what the code did before, because
infinity has no way back — with no revocation UI, a permission granted once was
granted for the life of the session no matter what the user later thought of it.

If a revocation UI is ever built (a "programs you have allowed" list with a
Forget button), this constant becomes much less important and could reasonably
be lengthened. Until then it is the whole mechanism.

### Alternatives considered

- **Expire both directions equally.** Symmetric and simpler to explain, and
  rejected because the symmetry is fake: the two decisions have opposite risk
  profiles under aging. It costs the user re-prompts for questions they have
  already answered no to, in exchange for no safety at all.
- **Expire neither** (the previous behaviour). Rejected: see above — a grant
  with no expiry and no revocation is unconditional.
- **Expire on program restart rather than on a clock.** Arguably the better
  rule, and not available: the dialog is keyed on process name and rights, and
  has no notion of a program's identity across restarts. Worth revisiting when
  the shell can be told that a pid exited.

### The clock going backwards

`is_live` uses `saturating_sub`, so a `now_ms` earlier than the recorded stamp
reads as *not yet expired* rather than as an age of nearly `u64::MAX`. The
alternative underflows — which in release would wrap to an enormous age and
silently expire every remembered decision at once, and in debug panics inside a
security dialog. The chosen direction costs at most one prompt's worth of
trust; the other costs the whole feature at the moment the clock hiccups. This
is a monotonic clock that should never go backwards anyway, so the branch exists
to make the failure boring rather than because it is expected.

Both halves of the asymmetry and the backwards-clock branch are covered by
tests proved real by reintroduction: making `is_live` always return true fails
the grant-expiry test, dropping the denial exemption fails the refusal test, and
replacing `saturating_sub` with `-` panics the backwards-clock test with
`attempt to subtract with overflow`.
