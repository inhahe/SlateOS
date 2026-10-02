## §676 — an unknown io_uring opcode answers `NoSuchSyscall` (-10) too, because §656's ambiguity is a property of dispatch, not of syscalls

**Date:** 2026-09-02
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** §656 fixed a case where the kernel gave one answer to two
different questions — "I have never heard of this request" and "I heard you and
the answer is no" — so programs had to guess which they had been told. That fix
was applied to the list of system calls. The kernel has a *second* such list, of
io_uring operations, and it had the identical flaw. This extends the same fix
there. Nothing observable changes today, because nothing in the tree currently
asks the question; the point is that the first program to ask will get a
truthful answer rather than an ambiguous one.

### The same defect, one layer over

`execute_sqe` in `kernel/src/ipc/io_ring.rs` ended its opcode match with
`NotSupported` (-2) — the very code a *registered* `exec_*` returns when it ran
and could not do the thing on this handle. Same code, two facts, opposite
correct responses, exactly as §656's table put it:

| The caller sees in CQE `res` | It might mean | Correct response |
|---|---|---|
| `-2` | this kernel has no such opcode | fall back to the synchronous route |
| `-2` | the opcode ran and refused | honour it; do **not** fall back |

The difference from §656 is only *where the value surfaces*: a syscall return
value there, the CQE `res` field here. That is a difference in plumbing, not in
the reasoning, which is the whole argument for this entry.

### Why it was not swept into §656 at the time, and why that was right

§656 had two callers asking for it, an outstanding promise to lane B, and a
boot self-test to pin it. This had none of those. Folding it in would have been
a behavioural change nobody requested, decided by analogy rather than on its own
evidence, and buried in a commit about something else. The known-issues entry
that carried it (`A-IO-URING-UNKNOWN-OPCODE-IS-STILL-AMBIGUOUS`) said so
explicitly and deferred it on that basis. Deferring was correct; leaving it
deferred indefinitely would not have been.

### This one is pre-emptive, and that is worth stating plainly

No caller anywhere in the tree compares a CQE `res` against `-2`. That was
checked when the issue was filed and **re-verified on 2026-09-02** across
`posix/`, `userspace/`, `services/` and `init/` — the only `ENOSYS`-shaped
reasoning in the io_uring consumers is about `io_uring_setup` itself returning
`-1`, which is not a completion result at all.

So this fixes no live misbehaviour. The case for doing it anyway is that the
ambiguity is cheapest to remove *before* the first fallback path is written
around it: §656 exists because lane B had already shipped one latch and was
about to write a second, and a latch is a guess that is wrong for exactly as
long as no answer has arrived yet. Closing this now costs one match arm; closing
it after two callers have compensated for it costs their code as well as ours.

The honest counter-argument, recorded because it is not silly: a change with no
caller is a change with no test of its usefulness, and the tree already has more
speculative generality than it needs. What tips it is that this is *subtractive*
— it removes a wrong answer rather than adding a feature — and that the correct
value already exists and needed no new variant.

### Alternatives considered

**Leave it until a caller needs the distinction.** Rejected on §656's own
evidence: the caller who needs it is, by construction, the caller who has
already been given the wrong answer once and written a workaround around it. The
issue's own trigger ("the next time someone writes a fallback path around a CQE
result") would have fired *after* the damage.

**Invent a new `KernelError` variant for "no such opcode".** Rejected. The fact
being reported — "this dispatch table has no entry point here" — is the same
fact `NoSuchSyscall` already names, and a second variant would make two codes
where callers need one distinction, recreating the ambiguity in mirror image.

**Rename `NoSuchSyscall` to something ABI-neutral (`NoSuchEntryPoint`).**
Tempting, since the name now reads as narrower than the meaning, and rejected on
cost: it touches every use site and every doc reference to buy nothing a comment
cannot. The variant's doc comment was widened instead, and now names both
callers and says outright that the name is historical.

### Consequences

`NoSuchSyscall`'s doc no longer claims it is returned "**only** by
`syscall::dispatch::dispatch`" — that sentence was true when written and became
false with this change, which is the kind of doc drift that makes a later reader
distrust the rest of the comment.

The Linux ABI is unchanged: `NotSupported` and `NoSuchSyscall` both map to
`ENOSYS`, since Linux spends one errno on both. The distinction is visible only
on the native ABI, where the raw code is what the caller sees. This is the same
boundary §656 drew and for the same reason.

Pinned by `test_unknown_opcode_is_distinguishable_from_a_refusal` in
`io_ring::self_test`, which asserts the result is `-10` **and** that it is not
`-2`. The second assertion is not redundant: an assertion on the new value alone
would still pass if someone later collapsed the two codes back together at the
`KernelError` level, which is precisely the regression worth catching. It also
asserts the SQE still *completes* — a ring that silently dropped an unknown
opcode would hang a caller waiting on the completion, a worse failure than the
ambiguity being fixed.
