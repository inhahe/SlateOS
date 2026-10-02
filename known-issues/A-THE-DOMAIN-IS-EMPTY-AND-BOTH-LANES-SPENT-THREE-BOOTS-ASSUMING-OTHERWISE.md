### A-THE-DOMAIN-IS-EMPTY-AND-BOTH-LANES-SPENT-THREE-BOOTS-ASSUMING-OTHERWISE (lane A, 2026-09-12) — OBSERVED, and it closes ctest-hostname check 13

**In short:** the system's network domain name is empty, and two lanes spent three boot
cycles reasoning about it on the assumption that it was `localdomain`. Nobody had printed
it. One line of output settled what a day of inference could not.

**The measurement**, from the ring-0 procfs rung added for exactly this purpose:

```
[procfs]   domainname node = 1 byte(s) "\n", store = ""
```

**What it settles.** `ctest-hostname` failed at check 13 for three boots. Lane B
diagnosed it correctly the first time — `read_kernel_name` returned `None` for both
*could not read* and *read and is empty*, so an empty domain reported as `EIO`. They then
**retracted that correct diagnosis**, on the grounds that the domain is never empty here.
That premise was mine: I had read `init_defaults()` setting `domain: "localdomain"`, seen
`gen_sys` call it before reading, and written to them that the node serves `localdomain`.
It does not. The node serves one byte.

So a value neither lane had observed produced: one false claim, one correct diagnosis
withdrawn on the strength of it, and three boots. The failure was not analysis — both
chains of reasoning were valid — it was that the premise was an inference wearing the
clothes of a fact. `design-decisions.md` §932 is the general rule; this is the instance
that cost the most.

**Consequence for the `(none)` proposal: do not make it.** See below.

**Before anyone makes the kernel serve `(none)` for an unset domain: this lane has
already decided the opposite question, deliberately, and the two decisions collide.**

`kernel/src/syscall/linux.rs` carries a self-test whose pass line reads:

> `uname pure-read contract — no "" -> "localdomain" / "unknown" -> "localhost"
> substitution (v6.6 kernel/sys.c::SYSCALL_DEFINE1(newuname): memcpy(&tmp, utsname(),
> sizeof(tmp)))`

It calls `set_domain("")`, asserts `uname`'s `domainname[0]` is `0x00` rather than `0x6c`
(the `l` of a fabricated `localdomain`), and restores. Its own comment says the pre-batch
behaviour "fabricated 11 bytes that no [caller asked for]". So **`uname` deliberately does
not substitute a plausible value for an unset domain**, and that is pinned by a rung that
would fail if anyone reintroduced it.

That is the same argument as `localhost`, `???` and `localdomain` — reached independently
in this lane, months earlier, and already enforced.

**The collision.** Lane B and I agreed that procfs serving `localdomain` for an unset
domain is a fabrication of exactly that kind, and that `(none)` — what Linux reports — is
the honest answer. But if procfs starts serving `(none)` while `uname` continues to report
empty, then **one value has two sources that disagree**, which is the defect that produced
the third hostname store in `sysfs.rs` and is the reason the getter syscall was declined.
Making procfs "honest" in isolation buys a display improvement and pays for it with a
divergence.

**Three coherent options, and the choice is not obvious:**

1. **Change neither.** Both paths report what the store holds. Consistent, and "unset" is
   reported as empty rather than as a name — which is *already* the distinguishable answer
   the `(none)` change was meant to provide. Linux compatibility is the only loss.
2. **Change both.** procfs and `uname` both substitute `(none)`. Matches Linux exactly,
   and requires deleting a rung that exists specifically to prevent substitution — so the
   deletion has to argue against that rung's stated reasoning, not merely around it.
3. **Change procfs only.** Two sources, one value, disagreeing. Should not be done.

**My reading, offered rather than acted on:** option 1 is already implemented and already
satisfies the underlying goal, because empty *is* distinguishable from configured. The
`(none)` proposal was aimed at `localdomain` being indistinguishable from a real
configuration, and the fix for that is not to substitute a different constant — it is to
stop substituting, which `uname` already does. If the observed value turns out to be
`localdomain`, the question is why the two paths differ, not which constant to prefer.

Deliberately not acted on: the observation boot has not reported yet, and the whole reason
that boot exists is that both lanes had inferred this value rather than seen it.

**Open, low value, and deliberately stopped.** `init_defaults()` sets `localdomain`, yet
the store reads empty. Narrowed by inspection, then abandoned on purpose:

- `nameservice::STATE` is created in exactly one place, `init_defaults()`, with
  `domain: "localdomain"`. Nothing at boot calls it; only procfs's `gen_sys` and kshell do.
- The `uname` pure-read rung is **not** the cause I first guessed. It calls
  `init_defaults()` itself (`syscall/linux.rs:69341`) before capturing `saved_dom`, so it
  captures `localdomain`, and every one of its failure paths restores. It passed, so it
  restored faithfully.
- That leaves something between that rung and this print, of which `ctest-hostname`'s own
  save-and-restore around checks 13-21 is the only candidate I have identified.

**Not pursued further, because nothing is broken.** An empty domain is a legitimate state —
no NIS domain configured — `uname` and procfs agree on it, and the pure-read contract is
satisfied. Settling *why* costs another 35-minute boot with an extra print, to explain a
state that is correct. Recorded so the next person does not re-derive the three bullets
above, and with the instrument already in the tree: the rung prints the value every boot,
so anyone who wants the answer can bisect by adding one more print rather than starting
from the question.
