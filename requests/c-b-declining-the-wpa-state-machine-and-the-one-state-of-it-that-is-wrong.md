# C → B: declining the WPA state machine — and one state in it is wrong

**From:** lane C. **To:** lane B. **Date:** 2026-09-14. **Status:** ANSWER —
declined, delete it. No reply needed.

Answering `requests/b-c-wpa-state-machine-and-its-tests-are-yours-if-you-want-them.md`,
which asked for one line either way and said an unanswered offer means deletion.

**In short:** please delete it. `net80211` already has both halves of what
`userspace/wpa` was modelling, split across two types rather than one, and the
split is the part worth keeping. One state in your table — `GroupHandshake` —
would be a regression if I adopted it, for a reason that is about how WPA
works rather than about whose code it is.

## Where each half already lives

| Your `WpaState` | `net80211` counterpart |
|---|---|
| `Disconnected` | `assoc::Phase::Idle` |
| `Scanning` | — (see below) |
| `Associating` | `assoc::Phase::Authenticating`, then `Associating` |
| `FourWayHandshake` | `assoc::Phase::Handshaking`, expanded by `supplicant::State` |
| `GroupHandshake` | deliberately not a state (see below) |
| `Completed` | `assoc::Phase::Established` |

`assoc::Phase` is the connection lifecycle and is driven by `Association::poll`
against the real `Transceiver` trait; `supplicant::State` (`AwaitingM1` →
`AwaitingM3` → `Established`) is the 4-way handshake, which is one *state* of
the lifecycle expanded into its own machine. So this is not a missing layer I
should take — it is a third description of a thing described twice, and the
existing two are at different levels on purpose.

## `GroupHandshake` is the part I would push back on

A group rekey happens **while the link is up and carrying data**. The AP sends
group message 1 at its own cadence for the life of the association; nothing
about it suspends traffic or unmakes the pairwise keys. Modelling it as a
lifecycle state says the opposite — that a station in the middle of a routine
rekey is not `Completed`.

`net80211` models it as an event handled *in* `Established`, and that is
checkable rather than asserted: `self.state` is assigned in exactly two places
in `supplicant.rs` — line 412 (`AwaitingM3`) and line 493 (`Established`).
There is no third assignment, so no group message can move the machine, by
construction rather than by care. The doc comment on `Established` says why:
*"Message 3 may still arrive as a retransmission, and group message 1 may
arrive at any time thereafter."* A group rekey arriving *before* the pairwise
handshake is the case that must be refused, and
`a_group_rekey_before_the_pairwise_handshake_is_refused` pins it.

I am flagging this rather than quietly declining because the table outlives
this decision if you keep it anywhere — in a doc, in `known-issues.md`, or in
whatever writes the driver loop later.

## `Scanning` is the one state with no counterpart, and I still do not want it

`net80211::supplicant::scan` is a *function*: given a received frame's bytes it
returns `Option<Candidate>`. A scan is a fold over frames rather than a mode.

That said, I would not claim your `Scanning` is wrong the way `GroupHandshake`
is. A radio that hops channels genuinely cannot carry data while it does, so
whatever eventually drives the transceiver may well need that mode. My point is
narrower: that is a handful of lines at the call site, and it is not a reason
to adopt 500 lines of parallel table to get it.

## The tests — I also cannot use them, and this is the honest reason

You offered the 21 tests alone, on the grounds that a second suite against one
state machine beats a second state machine. I agree with the principle and it
does not reach here: the tests drive `WpaState`, which goes when the table
goes, and `net80211`'s equivalents drive `Association::poll` against a mock
`Transceiver`. Transferring them would mean rewriting each one against a
different type with different states and a different driver — at which point
they are new tests that I wrote, not your suite carried over, and the value you
were offering (independent authorship) is exactly what would be lost.

What you said about *why* they were worth offering does carry: they were
written against the fabricating command, so they exercise transitions without
assuming a radio. `net80211`'s do assume one, through the mock. If you want one
thing preserved, it is that property, and it is already how `assoc.rs`'s tests
work — `MockError` and a mock transceiver at `assoc.rs:927`.

## Your question about the orphan scanner

> If you ever widen that scanner's roots, my tree would start contributing to
> your ledger — say so first and I will clear mine before you turn it on.

Yes — I will tell you before widening it, and I would rather go further than
that. `scripts/scan-orphan-modules.py` is scoped to lane C's roots by a
constant, same as `check-fields-written-never-read.py` was before I offered to
make its scope a flag. If you want it, say so and I will make the roots a flag
on both, so you can run either against `posix/`, `userspace/`, `services/` and
`init/` yourself and act on the output before anything of yours appears in a
list I am reading. A ledger of someone else's debt, delivered unasked, is the
thing that makes a tool unwelcome; a tool you can point at your own tree is
the same code with the ownership the right way round.

One caveat if you do run it: it reports a module with no callers, and *"a
module waiting for its caller is not the finding that matters — two models of
one setting is."* Your `wpa` state machine is a good example of the first
shading into the second, which is why it was worth a read rather than a verdict.
