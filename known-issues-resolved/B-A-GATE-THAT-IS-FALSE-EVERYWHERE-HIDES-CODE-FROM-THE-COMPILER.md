## `B-A-GATE-THAT-IS-FALSE-EVERYWHERE-HIDES-CODE-FROM-THE-COMPILER` (lane B, 2026-08-26)

**Status:** ✅ FIXED 2026-08-26 (`045f603e1`) for the two instances found. Logged
as its own entry because the *shape* will recur and the search for it is cheap.

**In short:** A `#[cfg(...)]` guard whose condition is never true on any target
does not merely fail to guard — it deletes the code from every build, so the
compiler never type-checks it, `cargo clippy` never lints it, and no test can
reach it. It looks like working, carefully-guarded code in review. It is
unverified text.

**How it arose here.** `#[cfg(target_os = "slateos")]` reads as "only on our OS"
and is in fact false on our OS too, because our target spec must declare
`os = "linux"` (see design-decisions.md §619). Two regions had sat behind it:
one did not compile at all (a function whose name did not match its callers), and
one issued a syscall number that is unassigned in our table. Neither defect was
detectable by any tool until the gate was corrected.

**How to find it.** The gates worth suspecting are the ones naming a value the
target spec does not actually carry. A direct check:

```bash
# every distinct cfg predicate in the tree, with counts
grep -rhoE '#!?\[cfg\w*\([^]]*\)\]' --include=*.rs . | sort | uniq -c | sort -rn
```

then, for any predicate that claims to select our OS, confirm it against
`toolchain/x86_64-slateos.json` rather than against its own wording. A predicate
with **zero** sites compiled in *any* of the three targets is the signature.

**Residual state: zero.** `grep -rn 'target_os = "slateos"' --include=*.rs .` now
matches nothing anywhere in the tree — not in `userspace/**`, and not in lane A's
`kernel/**`/`bench/**` or lane C's zones either. So this is not "fixed where we
looked"; the predicate is gone. Any future reappearance is a regression, and the
grep above is the whole test.

**Proper fix, generally.** Prefer a gate the target spec provably carries
(`target_vendor = "slateos"`), and where `posix` exports the C symbol prefer
`cfg(unix)` and the symbol — because that arm is *live on a dev host*, which
means the compiler and the test suite both see it every day.
