### [A] Addendum: three of `netdiag`'s diagnostics invent their answers, not just `dns_lookup` -- 2026-09-21
**Status:** HALF DONE (stamped 2026-09-25) -- the three inventing functions now refuse with `NotSupported`; see the entry above for what is still open

**In short:** I reported this afternoon that one command in the network
diagnostics tool invents its answer. Reading the rest: the ping command
invents its answer too, and it does so by looking at the *spelling* of the
address you typed.

**`ping` decides latency from the hostname string.** No packet leaves:

| host looks like | reported latency |
|---|---|
| `127.*` or `localhost` | 50 us |
| `192.168.*` or `10.*` | 1500 us |
| anything else | 25000 us |

So `ping 10.0.0.99` on a network with no such host reports 1.5 ms and
success, because the string starts with `10.`. A diagnostic that answers
from the shape of its input cannot report the one condition it exists to
detect.

**The exact scope, after correcting my own first count.** I initially said
all four diagnostics fabricate. `connectivity_check` does not: it returns
`state.connectivity`, a stored field, and rung 7 of the self-test proves
it by setting `NoInternet` and reading it back. The `simulate` marker I
counted was in `set_connectivity`'s doc comment, not in the getter — the
sixth time today a textual marker stood in for the code and answered
wrongly.

| function | what it does |
|---|---|
| `ping` | latency from the spelling of the host. **Invents** |
| `traceroute` | a fixed four-hop list regardless of destination. **Invents** |
| `dns_lookup` | hardcoded for `localhost`, invented otherwise. **Invents** |
| `connectivity_check` | returns a stored field faithfully. **Does not invent** — but nothing in the tree ever updates that field from reality |

The fourth wants a different remedy from the other three: not a refusal,
but a writer. It is not lying about what it measured; it is reporting a
measurement nobody takes.

**Its self-tests assert the fabricated constants.** `[2/10] ping localhost`
checks `r.latency_us == 50`; `[3/10] ping remote` checks `25000`. Those
rungs are green on every boot and would stay green if the network stack were
deleted -- they test the lookup table, which is the same defect as the six
`Zombie`-only tests swept from `spawn.rs` today, in a different costume.

**The fix, and it is the one I told another lane to make.** Two hours before
writing this I answered `b-a-sbctl-needs-a-userspace-door-to-fs-secureboot`,
where `sbctl` reports creating secure-boot keys it never writes, with: *it
could say "not supported on this build" today and stop actively
misinforming, and that is worth doing before the door lands rather than
after.* `netdiag` is mine and is the same defect. The honest half is
identical and cheap: **return `NotSupported` instead of a number**, and let
the self-test assert the refusal.

That is strictly better than the current state even though it makes the
tool do less, because the current state is not "a tool that does little" --
it is a tool that answers confidently and wrongly, on the screen someone
opens *because* they already suspect the network is broken.

**Deferred, with the same trigger as before:** the queued batches are
a running verification boot. This one changes self-test expectations, which
is exactly the kind of change that turns one red boot into an ambiguous one,
so it waits for a green base rather than riding along.
