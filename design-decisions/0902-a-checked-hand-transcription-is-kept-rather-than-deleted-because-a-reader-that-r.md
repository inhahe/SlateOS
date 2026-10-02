## §902 — a checked hand transcription is kept rather than deleted, because a reader that rebuilds from source has nothing to be wrong against

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** `scripts/check-evdev-elf-asm.py` contains a hand-typed Python
copy of machine code that the kernel builds byte by byte, so the encodings can
be disassembled and read before the kernel ever runs them. Nothing checked
that the copy still matched the kernel, so a byte changed in one and not the
other left the script cheerfully disassembling a program that no longer
existed. The fix was to rebuild the same bytes *from the Rust source itself*
and compare. That makes the hand copy redundant as a source of bytes, and the
obvious follow-up is to delete it — one source of truth, less to maintain.
This decision is to keep it anyway, and to keep it deliberately *unshared*
with the values it is checked against.

### The decision

`scripts/rustemit.py` reads `kernel/src/proc/elf.rs` and replays its emission:
the byte literals, the constants (from `evdev.rs`), the `_IOC` bit layout (from
`evdev::ioc`'s body), and the encodings of the `sentinel` / `jcc` /
`ioctl_call` helpers (from those functions' bodies, with arguments bound). The
hand mirror in `check-evdev-elf-asm.py` stays, and the two are compared byte
for byte before anything is disassembled.

The mirror also keeps its own hardcoded `EV_VERSION`, `KEY_BYTES` and
`EVIOC_NR_*` rather than importing the ones read from `evdev.rs`.

### The alternative, which is what "don't repeat yourself" would say

Delete the mirror; disassemble whatever `rustemit` rebuilds. One construction,
one place to edit, and no possibility of the two disagreeing.

**For it:** a duplicated 200-line transcription of machine code is exactly the
kind of thing that rots, and this one *had* rotted in the sense that nobody
could have told. Keeping it means every future edit to the Rust must be made
twice — which is the maintenance cost that motivated the original defect.

**Against it, and why it lost:** the rebuilt buffer would then be the only
statement of what the program is, and there would be nothing to check it
against. `rustemit` is a parser; parsers are wrong in quiet ways. If it
mis-parsed a statement — dropped a `for` block's successor, say — the
disassembly would be of a program neither the kernel nor anyone else builds,
and it would look exactly as clean as a correct one. That is the *same* defect
one level up: a single unchecked construction presented as evidence.

That is not hypothetical. It happened during this change: a block with no
trailing semicolon swallowed the statement after it, three bytes of
`mov rdi, r8` vanished from the rebuild, and the only reason it was caught is
that the mirror disagreed. Under the deleted-mirror design that run would have
printed a clean disassembly of a 600-byte program missing an instruction.

Two independent constructions that agree is a check. Either one alone is an
assertion. The duplicated maintenance is the price of the check, and it is now
a *loud* price — editing one and not the other fails immediately and points at
the offset, where before it was silent.

### Why the constants are deliberately not shared

Having decided to keep two constructions, the tempting half-measure is to let
them share the constants: import `EV_VERSION` and the `EVIOC_NR_*` from the
values read out of `evdev.rs`, so at least those cannot drift.

That is backwards. Shared constants make the two constructions agree *by
definition* about the values most likely to change — a renumbered ioctl, a
changed `KEY_MAX` — while leaving them independent only about the byte
encodings, which change far less often. It converts the highest-value part of
the comparison into a tautology. Hardcoded separately, a renumbered ioctl now
fails the comparison and names the offset; before this change it altered
nothing observable at all.

The general form: **redundancy is only worth its cost where the two copies can
actually disagree.** Sharing the volatile half and duplicating the stable half
is the worst of both.

### What reversing this looks like

Delete the `build_mirror` function from `check-evdev-elf-asm.py` and
disassemble `rebuild_from_rust`'s output directly; delete the end-to-end group
in `scripts/test-rustemit.py`, which exists to prove the comparison fires. The
signal that this is worth reconsidering is `rustemit.py` acquiring its own
substantial test suite against fixtures rather than against this one subject —
at which point the parser has an independent witness of its own and the mirror
is genuinely a third copy rather than the second.
