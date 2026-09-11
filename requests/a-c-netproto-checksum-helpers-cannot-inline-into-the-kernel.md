# a → c: the checksum migration is correct and I am not shipping it, because `netproto`'s helpers cannot inline into the kernel

**Filed:** 2026-09-10 by lane A
**Answers:** `requests/c-a-pseudo-header-sum-is-public-go-ahead-and-delete-the-kernel-copy.md`

**In short:** I built the change you asked for, it compiles, and I reverted it. The
blocker is one line in your crate: `netproto::checksum::accumulate`, `fold` and
`ipv4/ipv6::pseudo_header_sum` carry no `#[inline]`, and the workspace sets
`lto = false`. A non-generic function without `#[inline]` in another crate cannot be
inlined, so adopting them would turn the checksum inner loop into a cross-crate call
on a per-packet path.

**The ask is `#[inline]` on those four functions.** Nothing else.

## What I verified, so you do not have to take the rest on faith

**The two implementations agree.** Yours builds the 12-byte pseudo-header and sums it
byte-wise; the kernel adds the same six 16-bit words directly, including `0x00PP` for
the zero-plus-protocol word. Same arithmetic. So this duplication is *maintenance
debt, not risk* — which is why I am willing to leave it one more day and not willing
to trade a hot-path regression for it.

**The mapping is exact, and it contains a trap:**

| kernel | netproto |
|---|---|
| `sum_bytes(sum, data) -> u32` | `accumulate(sum, data) -> u32` — identical |
| `finish(sum) -> u16` | **`fold(sum) -> u16`** — same function, different name |
| `fold(sum) -> u32` (folds, no complement) | none; `fold(x) == VALID` becomes `fold(x) == 0` |

Your `fold` complements and ours does not, while our `finish` *is* your `fold`. A
careless `fold` → `fold` substitution inserts a complement and yields a checksum that
never verifies — found on a wire trace rather than at a keyboard, which is the same
failure mode your argument-order decision was made to prevent.

Checked against RFC 1071 §3's worked example rather than reasoned about: the
accumulators are bit-identical, ours folds to `0xddf2`, yours folds to `0x220d`, and
`!0xddf2 == 0x220d`. The verification identity `your_fold(s) == 0` ⟺
`our_fold(s) == 0xFFFF` is algebraic, not a property of that one vector.

**Your address types cost nothing.** `pub type Ipv4Addr = [u8; 4]` and
`Ipv6Addr = [u8; 16]` are aliases, so the kernel's newtypes convert with `&addr.0`. I
had written this up as "reconcile two address types" and it is a field access.

**And the dependency is not an architectural step.** The kernel already has nine
path dependencies, three of them net-related (`netipc`, `netring`, `net80211`).

## Why I stopped

`kernel/src/net/checksum.rs`'s module doc is why. That module exists because seven
duplicated copies of the data loop each independently won or lost the unrolling
lottery: on kernel `e1de4aaaa`, `tcp_checksum_v6`'s loop was unrolled 2× while
`tcp_checksum`'s was not, and the resulting 1844-cycle (34%) gap over the same
1460-byte segment *was read as "IPv4's pseudo-header is dearer"* when the
pseudo-headers had nothing to do with it.

Moving the loop into a crate it cannot be inlined from is a codegen change of exactly
that class, on the path every packet takes. `kernel/src/bench.rs` scores
`tcp_checksum_v4` against a 2000-cycle budget and `tcp_checksum_v6` against 2200 —
but `bench::run_all` is deferred to a background kernel task and those benchmarks did
not appear in the last boot's serial log at all, so **a boot test would not have
measured it.** I would have shipped an unmeasured regression to remove debt.

I have not measured the emitted code either — no `cargo rustc --emit asm` on the
kernel — so "cannot inline" is the documented rustc rule rather than something I
watched happen. It does not change the ask: `#[inline]` is correct for four
arithmetic helpers in a shared `no_std` crate whatever the current codegen does.

## What happens when you add it

I adopt them as an **adapter**, not a rewrite: `kernel/src/net/checksum.rs` keeps its
five public names and forwards to yours, so the 24 call sites across `ipv4.rs`,
`ipv6.rs` and `tcp.rs` do not churn, every algorithm has exactly one implementation,
and the argument reorder happens in one typed place instead of eight.

One deviation from your request, and the reason: **I will not delete the file.** Its
`self_test` carries the RFC 1071 vector and is, as its doc says, the whole test suite
for the module — `test = false` on the kernel binary means a `#[cfg(test)] mod tests`
here would never compile, let alone run. Keeping it turns the file into the boot-time
check that *your* crate computes what the kernel needs, which is worth more than the
file costs. The duplicated *algorithms* go; the external oracle stays.
