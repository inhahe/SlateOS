## 33. Bare-ELF ABI auto-classification (Q9) — Hybrid (option D): default unmarked bare ELF → Linux, note-walk as a positive Linux signal, stamp native binaries with an explicit SlateOS marker

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q9; the operator chose
option **D**, which Claude recommended). The operator's words: *"Q9: Let's do
with D."*

**The decision.** Resolve the bare-static-ELF ambiguity (a `SYSV` static binary
carrying only generic GNU-toolchain artifacts is genuinely indistinguishable
between "Linux binary" and "SlateOS-native binary built with a GNU/LLVM
toolchain") with the **hybrid** approach:
1. **Flip the default for unmarked bare ELFs to Linux ABI.** Any ELF with no
   positive native marker is treated as Linux — every real-world Linux static
   binary (`tcc -nostdlib -static`, static musl, hand-rolled asm) "just works".
2. **Add `NT_GNU_ABI_TAG` note-walking** as an additional *positive* Linux signal,
   on top of the existing `EI_OSABI == ELFOSABI_GNU` / Linux `PT_INTERP` /
   `PT_GNU_PROPERTY` markers.
3. **Stamp SlateOS-native binaries with an explicit marker** — a SlateOS
   `EI_OSABI` value in the architecture range 64–255 and/or a `.note.slateos`
   `PT_NOTE`. Native is the side we fully control and can always mark; Linux is
   the open-world default.
4. **Keep `spawn_process_with_abi(elf, options, AbiMode)`** as the override for
   callers that already know the ABI.

**Rationale (both sides).** *For D:* native binaries are produced exclusively by
our own toolchain, so marking them is cheap and unambiguous; Linux binaries
arrive from the outside world unmarked, so the default should be the side we
can't mark — makes "a Makefile builds a tool with tcc then `exec`s it" work
transparently (central to the Path-Z toolchain goal). *Against / cost:* a
user-visible policy flip; the native toolchain must emit the marker, and existing
bare native test ELFs (`build_test_elf`) need it added, or a truly unmarked
native binary would be mis-run as Linux.

**Where it bites.** `kernel/src/proc/elf.rs::detect_linux_abi` (flip default + add
`NT_GNU_ABI_TAG` note-walk + recognise the native marker);
`kernel/src/proc/spawn.rs::spawn_process_inner` and the `exec` path around
`new_abi_mode`; `build_test_elf` and the native toolchain (emit the marker).
**Sequencing:** decided but not the immediate priority — Q12 selected the page
cache (§36) as the next initiative; Q9 is unblocked and can land when the
native-binary marker is wired into the toolchain.
