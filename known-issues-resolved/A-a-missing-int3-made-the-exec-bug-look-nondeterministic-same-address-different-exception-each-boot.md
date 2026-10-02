### [A] A missing `int3` made the exec bug look nondeterministic: same address, different exception each boot -- 2026-09-21
**Status:** RESOLVED by the same batch that fixed the exec bug (the int3 is now emitted). Recorded because the *symptom* was actively misleading.

**In short:** when the exec syscall failed, the test program ran off the end
of its own code into whatever bytes followed. Those bytes were never set, so
the crash they produced differed from build to build -- the same bug
reporting a different fault each time, which reads like an intermittent
problem rather than a constant one.

**The alignment, which is exact:**

| fact | value |
|---|---|
| where the caller died | `rip = 0x4000000016` = stub base **+ 22** |
| length of the stub | 22 bytes (`B8`+5, `48 BF`+10, `BE`+5, `0F 05`+2) |
| what `build_exec_test_elf`'s doc promised at +22 | `int3 ; unreachable -- exec does not return on success` |
| what it emitted there | nothing. No `0xCC` anywhere in the builder |
| what `emit-int3.py` now writes | `buf[c + 22] = 0xCC;` |

**Why it mattered beyond tidiness.** Across investigations the same failure
presented as **#GP (13)** on one build and **#NM (7)** on another, at the
*same* address. Neither is what running past the end of a function should
produce in any principled way -- they are whatever the uninitialised padding
happened to decode as. A reader comparing two logs sees one address and two
exception numbers, and the natural inference is that the fault is
intermittent and therefore timing- or memory-dependent. It was neither: the
exec failure underneath was perfectly deterministic (`rdx = 0x1B` every
time), and only the *epitaph* varied.

With the `int3` present, a failed exec now traps as **#BP (3)** at a known
offset, every time. The failure becomes one signal instead of a family of
them.

**The general shape, which is worth more than this instance:** a doc comment
promising a trap that the code does not emit is not a documentation defect.
It is a *diagnostic* defect, and it degrades exactly when you need the
diagnosis -- the trap is unreachable on the success path, so its absence is
invisible until something fails, at which point it converts a clean halt
into a random one. `build_exec_test_elf` carried that promise long enough
for the bug to be investigated six times underneath it.
