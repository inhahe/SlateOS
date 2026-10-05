## 270. A self-test may skip, but only on a fact it looked up -- and it must say so in the line a reader believes

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Lots of our self-tests contain a section that cannot run
everywhere -- there is no second CPU to take offline on a one-CPU machine, no
`/tmp` to write to before the filesystem is mounted. Skipping those is fine and
always was. What was not fine is *how* the skips were written. Two habits had
spread through the tree: the test worked out whether to skip by trying the
operation and seeing if it failed, and then it printed `Self-test PASSED`
anyway. Together those mean a suite that quietly runs less and less of itself
as the code under test gets worse, while its output stays identical to a full,
clean run. This entry records the rule that replaces both habits, the helpers
that make the rule cheap to follow, and the build gate that keeps it true.

**The reported half.** A skipped section printed a line of its own and then
fell through to the summary, which said `PASSED` unconditionally. The last line
is the one a reader believes -- and the one a grep in CI matches -- so a
half-run was *byte-indistinguishable* from a full one. This is not a
hypothetical: 26 `pathz` rungs no-op'd unnoticed for weeks behind exactly that
green line (`known-issues.md` -> `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`), and
the fix at the time -- `report_pathz_skips()` in `scripts/boot-test.sh` -- was
written for that one suite only.

**The decided half, which is the larger defect.** Far more common was:

```rust
if Vfs::mkdir(dir).is_ok() {
    /* ..the actual test.. */
} else {
    serial_println!("  skipped (read-only root)");
}
```

Read aloud, that says "skip when this filesystem has no directories." What it
*means* is "skip on **any** failure" -- including the exact regression the
section exists to catch. The property that makes it dangerous is its direction:
**the worse the code under test gets, the more sections switch themselves off,
and the suite goes green precisely when it should be loudest.** A test whose
gate is the code under test is not a test; it is a description of what
currently works.

**The rule adopted.**

> A self-test may skip, but it must skip for a reason it *looked up*, and it
> must say so in the line a reader believes.

"Looked up" means a fact obtained from something other than the operation being
tested: the mount table, `CPUID`, a feature query, a capability lookup. When an
error genuinely must be classified -- some preconditions are only observable by
attempting them -- only `NotSupported`, `ReadOnlyFilesystem` and `NoSuchDevice`
mean "this system cannot." Every other error means the system was asked and
refused, which is a defect and must fail the suite.

**What was built to make it cheap.** Three pieces, all in
`kernel/src/fs/selftest.rs` unless noted:

| Piece | Role |
|---|---|
| `Skips` (`record(section, why)`, `report(tag)`, `suffix()`) | The ledger. `suffix()` renders ` -- N section(s) SKIPPED` onto the summary line, so the believed line stops lying. |
| `classify(KernelResult<T>) -> Setup` (`Ready`/`Unsupported`/`Failed`) | The three-way split above, in one place, so the "which errors mean *cannot*" judgement is made once rather than at 40 call sites. |
| `is_mounted(p)` / `is_mounted_rw(p)` | The commonest looked-up fact. Replaces the open-coded `Vfs::mounts().iter().any(..)` fold that every converted site was growing its own copy of. |
| `scripts/check-selftest-skips.py` | Finds both shapes; gated in `scripts/boot-test.sh` before the build. |

**The alternative that was rejected: keep the checker advisory.** The cheaper
path was to run the script by hand now and then. It was rejected for the reason
the count itself argues: the sweep ended at **0 findings across 802 files**, and
a count at zero with nothing holding it there is a count on its way back up. The
`.is_ok()` gate is not an exotic mistake -- it is the *shortest* way to write a
conditional section, so it is what gets written by default. Only a gate that
fails the build makes the honest form the path of least resistance. Placing it
before the build costs milliseconds against a ten-minute build, and exit 2
(could not run at all) counts as failure, so a check that cannot fire is never
mistaken for one that passed.

**The cost, stated plainly.** The checker is textual. It matches three
spellings of the decision -- `.is_ok()`/`.is_err()`, `if let Ok(..)`/`if let
Err(..)`, and a `match` arm whose pattern is a catch-all `Err(_)`/`Err(e)`
(an arm naming *specific* errors is the approved form and stays exempt) -- and
it examines not only functions whose *name* looks like a self-test but
everything reachable from one by a same-file call, because a large suite is
mostly helpers. Both of those started as narrowings, and each hid real
instances: the name-only rule missed `io_ring`'s two file-handle sections,
which skipped on a failed `/tmp` write and were caught by reading a boot log
rather than by the gate.

What remains invisible is a skip decided from an **integer errno** rather than
a `Result` -- `if write_result.value < 0 { ..SKIPPED.. }`, in a test that drives
the syscall dispatch table directly. Catching that needs dataflow, not a
pattern: `x.value < 0` is ordinary code, and what makes it a defect is only
that the value came from the code under test. The checker also cannot tell a
precondition probe from a genuine assertion about a `Result`, which is why
`ALLOW` exists -- currently empty, and it should stay a list with reasons rather
than a habit. A false positive costs one entry in that map; a false *negative*
costs a section that stops running and never says so, which is the failure this
whole entry is about. That asymmetry is why the rule is enforced on the shape
rather than on a judgement about intent.

**Scope of the sweep that this settled.** 19 files converted across five
commits: `fs/handle`, `fs/certmgr`, `fs/devfs`, `fs/history`, `fs/mime`,
`fs/index`, `fs/procfs`, `fs/sysfs`, `fs/trash`, `fs/vfs`, `fs/zip`, `mm/pcid`,
`mm/compact`, `mm/frame_owner`, `mm/kasan`, `mm/kasan_rt`, `mm/oom`,
`cpu_hotplug`, `smep_smap`, `vmguest`, `watchpoint`, `proc/spawn`,
`syscall/linux`. Three went further than reporting, because the honest form
made the weakness obvious once the skip was gone: `fs/mime` now fails when
`detect()` errors instead of warning, `fs/zip` fails when deflate does not
compress instead of noting it, and `watchpoint` deleted its skip entirely by
introducing a dedicated 8-byte-aligned `AtomicU64` -- turning a precondition the
test had to check into one the type system guarantees.
