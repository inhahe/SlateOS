## 972. How a program gets more than one network connection is settled by measurement, not by argument

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended C: build A now, revisit B) · **Lane:** A

Answering A-Q15. The operator's answer was given in lane C's chat on
2026-09-27 and relayed by lane C; the verbatim record is
`requests/c-abf-the-operator-answered-a-q14-a-q15-b-q8-f-q1-f-q2.md`:

> I'd like to actual empirical tests, using real world conditions, including
> some extreme ones but not too unrealistic, to see what's worse - the slowness
> of A or the memory consumption of B. But I don't know if that's possible under
> QEMU because it runs more slowly than realtime. If it's not, then log it
> somewhere that we need to test this once the OS is tested on bare metal.
> Another possibility would be to have the OS support both A and B, and if B
> runs out of memory, then drop back to A, ideally with some connection
> shuffling so that the ones that need the speed of B the most get B, unless
> this possibility just isn't a good option.

**In short:** today a program can have only one network connection at a time.
Each socket gets its own shared-memory ring to the network daemon, and the
daemon keeps only one ring mapped, so opening a second socket silently destroys
the first. The fix can go two ways:
- **A:** one ring shared by all of a program's sockets. They can hold each
  other up, since they share one queue.
- **B:** the daemon keeps every socket's ring mapped. That costs more memory per
  program, and the number of rings has a ceiling.

The operator wants the choice made by measuring, under realistic loads and some
extreme but plausible ones, which is worse: A's slowdown or B's memory. They
also asked whether the system could run both, with B falling back to A when
memory runs out and the connections that need speed most kept on B.

**What it obliges, in order.**
1. **Both designs, behind one switch, so they can be measured.** Since the
   six-lane split (2026-09-22) both halves are lane A's: the kernel's
   `net::socket` / `netstack_client` for A, and `services/netstack` for B. The
   question text still named lane B for B, written before the split.
2. **A load harness that runs inside SlateOS.** Several simultaneous
   connections under three traffic shapes:
   - bulk transfer;
   - many small request/response exchanges;
   - idle keep-alives next to one busy connection, which is where A's shared
     queue hurts.
   For each it records per-connection latency and throughput, and the daemon's
   memory, under A and under B, and adds an extreme but plausible tier (dozens
   of connections, one of them saturating).
3. **Decide whether QEMU's numbers mean anything.** QEMU runs slower than real
   time, so absolute figures are not the machine's. A *comparison* of A and B on
   the same host, same load, same run can still be valid, because both pay the
   same distortion. The harness must say which of its results are relative and
   which are absolute. Anything that can only be judged in real time goes into
   `deferred-questions.md`, with the bare-metal boot (the roadmap's USB item) as
   the trigger, as the operator asked.
4. **Judge the hybrid honestly.** B that falls back to A when memory runs out,
   with the fastest-needing connections kept on B, means one socket can move
   from its own ring to the shared one while it is live. Whether that migration
   can be made correct and cheap is part of the evaluation. If it cannot, say
   so, as the operator invited.

**Until then** nothing changes: two sockets at once remain broken, exactly as
A-Q15's "if this is never answered" describes. The known consequences:
- the head-of-line witness stays declined;
- `D-NETSOCK-SYNC` cannot be proven;
- a windowed program that opens a second socket loses its display connection.
The last is why this is lane A's next networking task, not a background one.

**Status, 2026-09-27: steps 1 and 2 are built** (A-Q15 increments 1-3,
`roadmap.md`). Two sockets at once work in both designs, and the head-of-line
and late-data witnesses run in every boot, once per design. That makes the
"until then" list above history, and it retired §941's declaration. Steps 3
and 4, the full measurement and its reading, are increment 4.
