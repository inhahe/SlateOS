### [A] `listen()` fails with `InternalError` about one boot in twenty, reds the run, and was recorded nowhere -- 2026-09-18
**Status:** OPEN (bounded retry landed and boot-verified on ebb683642; the round counter reported ZERO retries, which argues against late completion and points at the other three InternalError sites -- now distinguishable. Cause still not identified)

**In short:** roughly one boot in twenty fails because opening a network
listening socket returns an error, on a socket that was created and bound
successfully a line earlier. It has been happening for at least a day. It
was in no entry of `known-issues.md` or `todo.txt`, because a failure that
happens one run in twenty is invisible to anyone reading the run in front
of them.

**The evidence**, from `20260918T022810Z-fb66a2a2a-rc1.txt`:

```
[netsock]   FAIL: head-of-line setup step listen failed: InternalError
[spawn]   FAIL: net::socket head-of-line witness (InternalError) -- a listener's
          accepted connections are serialising again
WARNING: net::socket head-of-line self-test failed: InternalError
WARNING: persistent userspace netstack (ring 3) startup failed: InternalError
```

**Read the order carefully, because I read it backwards first.** The
`listen` is the *first* line, not the last: `listen()` failed, which failed
the witness, which failed the netstack startup report. My first reading was
that an environmental netstack blip had cascaded into the witness -- the
opposite causation, and it would have sent me to the daemon instead of to
the socket layer.

`kernel/src/net/socket.rs` runs the sequence
`create(2)` -> `bind_stream(srv, PORT)` -> `listen(srv, 2)`, each wrapped in
a `step!` macro that names itself on failure. The first two succeeded. Only
`listen` failed, and only in this one run.

**Rate: 1 of 20 archived serial logs.** Measured, not estimated -- the other
nineteen contain neither line.

**It reds the boot**, which is correct and worth stating because it means
this is already costing runs. `scripts/boot-test.sh:353` greps the serial
log case-insensitively for `self-test failed` and `:8987` turns a hit into
*"Boot test FAILED (marker reached but a self-test failed)"*. So the kernel
reaches `BOOT_OK` and the harness fails the run anyway -- exactly the design
recorded when that witness was wired, working as intended.

**How it was found, which is the reusable part.** `build/scan-guest-output.py`
was written this morning after bash's `getcwd` error turned up four lines
from an `OK`. I ran it on the newest log, found that, and stopped -- the
site where the failure was seen. Lane C's rule the same evening (*a remedy
applied where the failure was seen does not reach the places it was not*)
prompted running it over all twenty. Nineteen of them contain only the
`getcwd` line; one contains this as well.

A one-in-twenty failure is precisely the shape a per-run reader cannot see
and a corpus can. It cost one command over logs that were already on disk.

**Next step, not taken yet.** Establish what `listen()` can return
`InternalError` for at all, and whether the port is still held from an
earlier test in the same boot -- a plausible hypothesis given `PORT` is a
constant and this witness runs after a good deal of other network activity,
but a hypothesis and not a finding. The cause is unknown; only the rate and
the failing call are established.

#### Root-caused the same day: `submit_and_reap` polls the completion queue exactly once

**The hypothesis above is wrong, and the code says so plainly.** A port
still held returns `KernelError::AddrInUse` from `listen()`, which is a
distinct arm -- so whatever happened, it was not port reuse. Following the
`InternalError` instead of the guess:

`net::socket::listen` delegates to `netstack_client::listen`, which is two
statements: `attach_ring()?` and `submit_and_reap(&ring, &sqe)`. Three
`InternalError` sites exist between them, and one is the mechanism:

```rust
if !ring.sq_push(sqe) { return Err(KernelError::ResourceExhausted); }
self.submit_round()?;                  // the control round-trip
self.session_open = true;
let cqe = ring.cq_pop().ok_or(KernelError::InternalError)?;   // <-- here
```

**`cq_pop()` is called exactly once.** There is no retry, no bounded spin
and no yield. If `submit_round()` returns before the daemon's completion is
visible to this CPU, `listen` fails with `InternalError` -- and the function
doc says what it assumes in as many words: *"run one control round-trip, and
reap exactly one completion"*. A single poll against another process's
producer is a race by construction, and a race that lands roughly one boot
in twenty is exactly what a single poll with a usually-sufficient delay in
front of it looks like.

The other two `InternalError`s in the same function are consistency checks
(`user_data` mismatch, a second unexpected completion), and `attach_ring`'s
is a genuinely absent ring. Any of the three would be reported identically
at the call site, which is worth noting on its own: **four distinct
conditions arrive at the rung as one word.**

**So both of my earlier readings were wrong, in different ways.** First I
read the cluster's last line and concluded a netstack-startup blip had
cascaded into the witness -- wrong direction. Then I corrected that to
"`listen` is first, so `listen` is the root" -- right about the order and
still wrong, because the root is a mechanism *inside* the first observable.

**Which sharpens the causal-order rule built into
`build/scan-guest-output.py` this morning.** "Read the first anomaly of a
cluster before the rest" is correct and insufficient: the first anomaly is
the first **observable**, and the root may be an unreported precondition or
a single line inside that observable. The rule gets you to the right
function; it does not get you to the right line, and stopping there is how
I recorded a wrong hypothesis with a right-sounding provenance.

#### Fix landed 2026-09-18, and this boot does NOT verify it

The bounded poll is in (`netstack_client.rs`, 8 rounds, `TimedOut` on
exhaustion) and the boot after it reached `BOOT_OK` with only the three
baselined failures. That is **not** evidence the race is fixed, and the
distinction is the whole point:

| log | head-of-line result |
|---|---|
| after the fix | `NOT CHECKED` -- declines to A-Q15 |
| 2026-09-18 21:24, **before** the fix | `NOT CHECKED` -- declines to A-Q15 |
| 2026-09-18 03:49, before the fix | `NOT CHECKED` -- declines to A-Q15 |
| 2026-09-18 02:28 | `FAIL: head-of-line setup step listen` |

So declining to A-Q15 is the **normal** path and predates the change; the
`listen` failure was the 1-in-20 anomaly. A single green boot is therefore
consistent with "fixed" and with "did not happen to fire", and at a base
rate of one in twenty it would take many boots to tell those apart.

**The fix is claimed structurally, not empirically:** the control path now
polls the way its eight neighbours in the same file do, for the reason their
comments state. That is lane C's rule applied to myself -- they claimed their
environment-race fix structurally after 2,843 tests passed both before and
after it, and declined to read the pass as proof. A flake that reproduces one
run in twenty is not disproved by one run.

What *would* verify it: the `FAIL: head-of-line setup step listen` line not
appearing across ~20 further boots, or an instrumented count of how many
rounds `submit_and_reap` actually needs. The second is cheap and tells you
something the first cannot -- if the answer is always 1, the retry is
insurance; if it is sometimes 2, the race was real and is now absorbed.

#### The count came back zero, which moves the hypothesis rather than closing it

Instrumented and booted. `submit_and_reap` prints only when it needs more
than one round, and on boot `ebb683642` it printed **nothing**.

Verified that the zero means something before reading it as an answer, since
a missing line is exactly what a build without the counter also produces:

| check | result |
|---|---|
| counter commit is an ancestor of the booted commit | yes |
| booted tree contains the code (`git show <commit>:file`) | yes, 2 matches |
| the path was actually exercised | 20 `netsock`/`netstack-client` lines |

**So one round sufficed every time it was called.** That is not the result I
expected, and it argues against my own diagnosis. If the 1-in-20 `listen`
failure were a completion arriving late, the margin would be thin and
`rounds > 1` should appear *often* -- far more often than one boot in twenty,
because a marginal timing is marginal on every call, not on one call in
hundreds. Zero retries across a boot's worth of operations says the single
poll is normally comfortable, which makes late visibility an unlikely
explanation for a rare failure.

**The more likely candidates are now the other three `InternalError` sites**,
which the same change made distinguishable: an absent ring from
`attach_ring`, a `user_data` mismatch, or an unexpected second completion.
Before, all four arrived at the caller as one word; now exhaustion is
`TimedOut` and the rest stay `InternalError`, so the next occurrence names
its own category.

**What this does not establish.** One boot with zero retries is also
consistent with a real race at a low rate -- at 1-in-20 for the *failure*,
the underlying near-miss could still be rarer than one boot's traffic. The
count needs accumulating across boots, which it now does for free: every
future run either prints the line or does not.

**And the retry is worth keeping regardless of which cause wins.** It cost
nothing measurable, it applies the convention the file states eight times,
and if the race is real-but-rarer-than-observed it absorbs it silently. What
it must not do is be recorded as the fix for a failure it may have nothing to
do with -- which is what this section exists to prevent.

**Sharpened: this is not a missing retry, it is an unfollowed convention
stated eight times in the same file.** `netstack_client.rs` already
contains eight bounded poll loops -- `for _ in 0..64`, `..32`, `..32`,
`..16`, `..64`, `..64`, `..16`, `..16` -- and their comments give the
reason in almost identical words:

> *Poll for the reply. Each non-blocking recv **drives the daemon's RX pump
> once**, so a bounded loop is enough...*
>
> *Poll for the looped-back datagram. Each non-blocking recv **drives the
> daemon's UDP pump once**, so a bounded loop suffices...*
>
> *...to writable (**each poll pumps once**). Loopback normally completes
> immediately.*

So the file states the governing fact -- one poll drives the daemon's pump
exactly once, therefore one poll may not be enough -- and applies it in
eight **data**-path functions. The one **control**-path function that every
other call in the module routes through, `submit_and_reap`, polls once.

That is lane C's rule with the populations inverted: a remedy applied
everywhere the failure was *seen* (the data paths, where a missing datagram
is obvious and frequent) and absent from the place it was not (the control
path, where it costs one boot in twenty and arrives as a bare
`InternalError`). And *"Loopback normally completes immediately"* is the
same sentence as the ratio argument: true about the common case, and the
reason nobody noticed the uncommon one.

**The fix, not applied.** Copy the neighbours: a bounded poll loop around
`cq_pop()` with the same shape and rationale as the eight beside it, and a
*distinguishable* error on exhaustion rather than sharing `InternalError`
with three unrelated conditions. `TimedOut` (-6) and `WouldBlock` (-4) both
already exist, so no new variant is needed. Not applied here because it is in the
control path every `netstack_client` call uses, not just `listen`, so it
wants its own boot rather than a ride on one already in flight; and because
the consistency checks below it should get their own error values in the
same change, which is a slightly larger edit than it first appears.
