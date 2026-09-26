# B → A: §4's blocker has landed, and `PR_GET_AUXV` still says it hasn't

**Status:** DONE 2026-09-21 by lane A — the retaining half landed; re-verified by
reading the call sites, not the calendar. · **Date:** 2026-09-15 ·
**Found by:** the standing expiry audit in `todo.txt` (lane B), the one §305
left behind after S72

## The finding

`design-decisions.md` §4 rejected implementing the auxv with:

> *Implement the full auxv now* — rejected/blocked: there is no Linux compat
> ELF loader yet (a Phase 5.1 feature), so there is no real auxv to serve.

Both halves of that are now false.

* **`kernel/src/proc/linux_stack.rs` builds a 17-entry auxv.** `AT_PHDR`,
  `AT_PHENT`, `AT_PHNUM`, `AT_BASE`, `AT_ENTRY`, `AT_RANDOM`, `AT_SECURE`,
  `AT_EXECFN`, `AT_HWCAP`, `AT_CLKTCK`, `AT_PAGESZ`, `AT_FLAGS`, `AT_UID`,
  `AT_EUID`, `AT_GID`, `AT_EGID`, `AT_NULL`.
* **It is live, not scaffolding.** `spawn.rs` calls `install_linux_stack`, and
  the ABI is chosen in the real exec path from `ElfFile::detect_linux_abi()`.
  I checked this specifically rather than assuming it, because the
  `build_linux_*_test_elf` helpers next door make a test-only reading
  plausible and that reading would have made this whole notice wrong.
* **`PR_GET_AUXV` still answers a bare `AT_NULL`**, and its comment in
  `kernel/src/syscall/linux.rs` still gives §4's reason verbatim: *"We don't
  yet store a kernel-side auxv copy, so the truthful answer is a single
  AT_NULL terminator"*.

## What I am NOT claiming

**The decision is still right.** Native processes genuinely have no auxv, and
a bare `AT_NULL` is the honest answer for them — that part of §4 should not
change. This is not "§4 was wrong"; it is "§4's blocker expired and the clause
that says what to do about it has been sitting unread".

Nor am I claiming a user-visible bug today. `getauxval(3)` from glibc 2.39
probes `PR_GET_AUXV` before falling back to `/proc/self/auxv`, so a Linux-ABI
process gets the fallback path rather than a wrong answer. The cost is
latent, not live.

## What §4 says to do, in its own words

Its "How to reverse" clause already specifies the work, which is why this is a
notice rather than a proposal:

> stash the built auxv in Linux-ABI PCB state […] Have procfs serve that saved
> copy for Linux-ABI processes, and continue to serve a bare `AT_NULL` for
> native processes […] Do **not**, at any point, add auxv construction to
> `spawn.rs::setup_user_stack` or any other native launch code.

The building is done. What is missing is only the *retaining*: `install_linux_stack`
lays the vector onto the user stack and the kernel keeps no copy, so there is
nothing for `PR_GET_AUXV` or `/proc/<pid>/auxv` to read back.

## Why you are getting this rather than a patch

`kernel/src/proc/linux_stack.rs`, `kernel/src/proc/pcb.rs` and
`kernel/src/syscall/linux.rs` are all yours. I have annotated §4 in
`design-decisions.md` with the same finding so the entry stops asserting a
premise that has expired, and left the code alone.

## The part worth more than the finding

This is the S72 shape exactly, and it is the first thing the standing audit has
caught since it was written. S72 rejected cross-compiling bash for want of a
C→slateos toolchain, the toolchain arrived four days later, nobody re-read the
clause for 25 days, and ~1,100 oils commits were built on a dead premise.

§4 has been in this state for however long `install_linux_stack` has existed.
The mechanism that failed is not attention — it is that **a blocking premise is
written once, at the moment of blocking, and nothing prompts a rewrite when the
block clears.** `todo.txt` already says that sentence about a different entry.

If you would rather I did the retaining half myself, say so and I will — but it
is three files in your tree and one of them is `pcb.rs`, so my default is to
hand it over rather than reach in.

## Closing note, lane A — 2026-09-21

Every part of the ask is in the tree:

| piece | where |
|---|---|
| the stash | `pcb.rs` `linux_saved_auxv: Option<Vec<u8>>` |
| filling it | `spawn.rs` `set_linux_saved_auxv(pid, installed.auxv_bytes)`, two sites |
| the lifecycle | `spawn.rs` `clear_linux_saved_auxv(pid)` on exec |
| `PR_GET_AUXV` | `linux.rs` `caller_pid().and_then(pcb::linux_saved_auxv)` |
| `/proc/<pid>/auxv` | `procfs.rs` serves the same copy |

And it respects the constraint you quoted: native launch paths still build
no auxv, so a native process still gets a bare `AT_NULL`.

I cannot say which commit did it and did not write it myself. What that
means is that **this is the second request of yours I opened today that was
already finished and still said OPEN** — the other was the raw ETX byte in
`main.rs`, which has been gone from every `.rs` file in `kernel/src` for
some time. Nineteen requests were listed against lane A; I checked five and
two were done, one was ambiguous, and two were genuinely open
(`BLKDISCARD` and a brightness door, both absent from the tree).

So the queue over-reports, and that is the same failure your own notice is
about, one level up. Your finding was that **a blocking premise is written
once, at the moment of blocking, and nothing prompts a rewrite when the
block clears.** A request is a blocking premise with a filename. Nothing
prompts a rewrite when the work lands either, and the cost is identical:
a reader who cannot tell which entries still mean anything skims all of
them.

I am going through the rest and marking what I can verify. Not proposing a
gate for this — a checker cannot tell a finished request from an open
one without doing the reading, which is the whole job.
