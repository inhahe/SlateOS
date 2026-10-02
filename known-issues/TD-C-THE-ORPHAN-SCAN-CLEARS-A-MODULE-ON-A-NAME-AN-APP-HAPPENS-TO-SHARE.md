## TD-C-THE-ORPHAN-SCAN-CLEARS-A-MODULE-ON-A-NAME-AN-APP-HAPPENS-TO-SHARE

**Date:** 2026-09-14. **Lane:** C.

**In short:** the checker that finds library modules nobody uses can be fooled
into clearing one, by an unrelated application that happens to define a type
with the same name. It cleared a 2 201-line file-type registry that no file in
the tree referred to, and the file explorer went on classifying files with its
own hardcoded extension lists for three days afterwards.

**Where:** `scripts/scan-orphan-modules.py`. Two rules that are each sensible
alone:

* candidates skip `main.rs` -- a binary's root is not a library module, which
  is right;
* a mention is any identifier token equal to one of the module's public item
  names, and names *shared with another module* are dropped from the evidence
  so that a common noun cannot clear anything.

Together they leave a hole: a name owned by a `main.rs` is not "shared with
another module", because a `main.rs` was never collected as an owner -- while
that same file's tokens still count as mentions. So an application that models
the same subject in its own binary clears the library module that models it
properly.

**Reproduced.** `apps/fileassoc/src/main.rs` declares `FileCategory` and
`FileType`. `gui/toolkit/src/filetypes.rs` exports `FileCategory` (among
others). Before 2026-09-14 nothing in the tree named `guitk::filetypes` or
`crate::filetypes`, and the only other occurrence of the string was a method
called `render_filetypes_tab` -- yet the scan did not report the module, and
`--check` was silent. Checked by restoring the pre-change explorer and running
the scan again.

**The fix.** Collect shared-name owners from every Rust file, not only from
candidate modules: being ineligible to *be* an island does not make a file
ineligible to *own* a name. Expect the tightened rule to surface more islands
-- that is the point -- and expect at least `filetypes` to reappear if its new
caller is ever removed.

**The second-order lesson, which is the one worth keeping.** The two rules were
written at different times for different reasons and each is correct. The
defect is in their *interaction*, and no test of either one could find it. What
found it was using the thing the gate had cleared and discovering it had no
users -- which is to say, the gate was checked by accident, by someone doing
unrelated work. A gate nobody checks is a gate whose clearances nobody has
reason to believe.
