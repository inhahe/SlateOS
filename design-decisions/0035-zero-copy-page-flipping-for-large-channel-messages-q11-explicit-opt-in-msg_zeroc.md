## 35. Zero-copy page-flipping for large channel messages (Q11) — explicit opt-in `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region (option B); copy path stays the default

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q11; the operator chose
option **B**, which Claude recommended). The operator's words: *"Q11: Yeah, I
like B."*

**The decision.** Implement "zero-copy page flipping for large messages" as an
**explicit, opt-in** mechanism, not transparent or threshold-automatic:
- A `MSG_ZEROCOPY`-style **send flag**; without it, `send` keeps copy semantics
  (the zero-risk default — nothing existing changes).
- The **receiver pre-registers a page-aligned landing region**; on a zero-copy
  send the kernel moves (page-flips) the sender's pages into it. Move semantics
  (sender loses the pages) are explicit and opt-in — no silent `send` ownership
  change.
- Matches the `io_uring`/`vmsplice` model; 16 KiB page granularity and the
  sub-page-tail length field are visible only to opt-in callers.

**Rationale (both sides).** *For B:* keeps the correct copy path as default,
avoids silently changing `send` ownership at a size threshold (option C's
footgun), explicit/predictable. *Against:* more API surface; only helps adopters.
Accepted because the alternative changes user-visible ownership semantics.

**Compiler involvement (operator's follow-up — "should our compilers auto-choose
the flag, or is that up to the programmer?").** *Decision:* **keep it
programmer-/library-controlled; the compiler does not auto-insert the flag.** It
belongs in the IPC **runtime/library wrapper**, not `fastpy`/`rustc`/the C
compiler, for three reasons:
1. **It is a runtime decision on runtime values** — whether to page-flip depends
   on the runtime message length, buffer page-alignment, and whether the sender
   still needs the pages, none of which the compiler reliably knows statically
   (message size is usually dynamic).
2. **It changes semantics, not just performance** — zero-copy *moves* the
   sender's pages; a compiler silently changing ownership/aliasing would violate
   the language memory model (the same transparent-threshold footgun B avoids).
   Optimizations must be semantics-preserving; this isn't.
3. **The right ergonomic home is the channel library** — the send wrapper can
   offer an *auto-threshold helper* (`if len >= N && region.is_page_aligned() {
   send_zerocopy() } else { send_copy() }`) so most callers get "it just works"
   without the compiler, while a caller who needs the pages after send simply
   doesn't use that helper. For `fastpy`, the high-level channel binding exposes
   both an explicit zero-copy hint and the library-level auto-threshold default;
   the AOT compiler emits ordinary calls into that library and does not reason
   about page flipping itself.

*Net:* document a **library-level auto-threshold helper** as the ergonomic path;
do **not** add compiler analysis. (Recorded at the operator's request as part of
the Q11 resolution.)

**Where it bites.** `kernel/src/ipc/channel.rs` (`Message`, `send`/`recv`,
`MAX_MESSAGE_SIZE`), a new MM page-transfer mechanism (`kernel/src/mm`), the
Linux/native syscall glue marshalling channel messages, and the userspace channel
library (the auto-threshold helper). Benchmark exists:
`kernel/src/bench.rs::bench_ipc_channel_large` /
`bench/baselines.toml [ipc_channel_roundtrip_64k]` (~343 µs/64 KiB today,
copy-bound). **Sequencing:** decided but not the immediate priority — Q12 chose
the page cache (§36); Q11 is unblocked and can be built afterward.
