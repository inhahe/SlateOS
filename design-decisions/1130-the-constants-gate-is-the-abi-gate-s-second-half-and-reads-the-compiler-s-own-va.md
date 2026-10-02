## 1130. The constants gate is the ABI gate's second half, and reads the compiler's own values

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** the numbers the C library shares with a C program -- flags,
error codes, item numbers -- must be the ones in musl's headers (§1119), and
after the one-off audit that established it nothing checked them.
`scripts/check-libc-abi.py`, which already held the library's structure
layouts against musl's headers, now holds its constants against them too:
every public constant whose name a musl header defines is compared with that
header's value by the C compiler, on every push that touches the library.
Its first run found three wrong numbers the audit had missed
(`known-issues.md`, `D-POSIX-CONSTANTS-WERE-NOT-MUSLS`).

| Choice | Alternatives | Why this one |
|---|---|---|
| **Values from rustdoc's JSON** (nightly, `--output-format json`), which carries each constant's value as the compiler evaluated it, built for the SlateOS target | a Rust test printing a hand-kept list, as `abi_layout.rs` does for layouts; Python evaluating Rust expressions, as the audit did | nothing to keep in step: every public numeric constant is there, including those computed with `size_of` -- which the audit's evaluator skipped, and which is how `perthread::BLOCK_SIZE` escaped it.  The cost is a nightly toolchain, which the sysroot build needs anyway |
| **The list derived**: every constant whose name musl's headers define | a list of the constants to check | a constant added tomorrow is checked on its first push; `TMP_MAX` arrived with the stdio rewrite, after the audit |
| **musl's own headers judge their names; the kernel's (`linux/...`) only names musl's lack, in a unit of their own** | one unit with every header | in one unit `linux/limits.h` redefines musl's `NGROUPS_MAX` (32 to 65536), and the check compares against the wrong header without a word |
| **Compared in the bits both sides have** -- the Rust type's width and the C expression's, via `sizeof` | the Rust type's width; exact values | musl's `MS_NOUSER` is `(1<<31)`, an `int` that sign-extends on its way to `mount`'s `unsigned long`; it and a Rust `u64` of bit 31 carry the same bits.  `WEOF` and `__WCLONE` the same, which §1119 had to list by hand |
| **Folded into `check-libc-abi.py`** | a gate of its own, `check-libc-constants.py` | the same boundary, oracle, scope and owner, and it runs wherever the layout check runs; a new gate script has to be wired into `scripts/hooks/pre-push` or `scripts/boot-test.sh`, which are other lanes' files.  The cost: `ALLOW_UNCHECKED_ABI` bypasses both halves, and the hook's refusal text speaks of layouts |

**Kept by hand:** `KNOWN_DIFFERENT` (§1119's eight deliberate differences) and
`NOT_CONSTANT_IN_MUSL` (`SIGRTMIN`, `SIGRTMAX`, `MB_CUR_MAX`, calls to this
library's own functions).  Both refuse an entry that stops being true.

**How to reverse.** The constants half is one function, `check_constants`,
called from `main`; lifting it into a script of its own is a move, not a
rewrite.
