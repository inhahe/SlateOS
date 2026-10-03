## §285 — The one kernel path that wrote to user memory through a physical alias was replaced with the audited primitive, and the audit was turned into a build gate

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel can reach a process's memory two ways: through the
address the process itself uses, or through a second, kernel-private address for
the same physical memory. The first way is policed by the CPU — a mistake there
faults immediately and loudly. The second way is policed by nothing, and a
mistake there quietly rewrites memory that two processes are sharing, so they
disagree about their own contents from then on with no error anywhere. One place
in the kernel took the second way without the checks that make it safe. It has
been changed to call the routine that does have them, and a script now refuses
to build a tree that grows another one.

### What prompted it

`known-issues.md` → `W-KERNEL-COW-WRITE` left an explicit open item: audit every
kernel path that writes through a user pointer without validating first. This
entry records the audit, its one finding, and — more durably — the reason the
enumeration can be trusted.

### Why the same-address-space half of the question was already answered

SMAP is enabled and its enforcement re-verified every boot. A ring-0 access to a
user virtual address faults unless it is bracketed by `stac()`/`clac()`.
That converts "which paths touch user memory?" from an open-ended reading task
into a search for one token, because a path that omits the bracket does not
survive its first execution. All six sites are in `mm/user.rs` and all six
validate for **write** — including the futex atomics, which demand write
permission even to load, so that one rule covers every operation rather than
each op carrying its own.

The residual risk SMAP cannot see is a site that opens a window correctly but
validated the wrong direction. Keeping all six in one file is what makes that
checkable by reading one file.

### The finding, and why it was in a class the question did not name

The other route to a user page is the HHDM alias of its physical frame. Nothing
guards it: the alias is a kernel address in a writable mapping, so SMAP does not
apply (it guards user addresses, and this is not one) and the user mapping's
write-protect bit does not apply (it is a property of the mapping being
bypassed). A write through the alias to a present-but-read-only page therefore
*succeeds* where the same write through the user address would have faulted.

When that page is copy-on-write — which, after `fork`, is every page in the
address space — the write lands in the frame the other process is still using.
This is worse than the fault `W-KERNEL-COW-WRITE` tracks, and the comparison is
the point: that one is a ring-0 #PF with a diagnostic attached, while this one
produces no fault, no log line and no failing test.

`proc/linux_stack.rs::write_user_image` — which installs argc/argv/envp/auxv
onto a new Linux process's initial stack — was a hand-rolled second copy of
`mm::user::copy_to_user_as`, and had lost three of its checks: `WRITABLE`, the
CoW break plus demand-population, and the null/wrap/`USER_SPACE_END` bound.

### The judgment call: it was not a live bug

Its only caller runs after `setup_user_stack`, which maps every stack frame
eagerly, present + writable, from fresh memory, into an address space the
calling thread solely owns. Every precondition held.

It was still worth fixing rather than annotating, and the reason is the
distinction the alternative missed: the preconditions held **in the caller**,
while the function's own contract only *asserted* them. That is a property of
today's call graph, not of the code. A future demand-paged stack — a natural
change, since eight eagerly allocated frames per process is not free — would
have silently converted a caller-side decision into cross-process corruption
here, and the reviewer of that change would have had no reason to look at this
file. Deleting the duplicate removes the coupling entirely rather than
documenting it.

**Alternative rejected:** keep the loop and strengthen its `# Safety` comment.
Cheaper, and it would have been honest about the invariant — but it leaves two
implementations of one operation, which is the condition that produced the
divergence in the first place. `copy_to_user_as` gained its CoW break in §249;
this copy did not, precisely because nothing connected them. The same argument
as §284's `uname` literals, in a subsystem where the failure is silent instead
of merely confusing.

### Why a script and not a resolution to be careful

`scripts/check-user-access-sites.py`, gated in `boot-test.sh` before the build,
enforces both halves: `stac()` stays confined to `mm/user.rs`, and no file
outside an approved list writes through the HHDM alias of a page it resolved
with `page_table::translate`.

Two things about its design were deliberate:

- **It keys on the *binding*, not on proximity.** A first version flagged any
  file where `translate` and a `*mut` appeared near each other, and produced
  four reports, all of which were fine. The discriminator that matters is
  whether the write goes through *the frame `translate` returned*. `pcb.rs`'s
  fault handler, for instance, uses `translate` as a negative presence guard —
  it skips subpages already present, so it writes only to a proven-absent page —
  and the address it writes comes from a frame it allocated itself. Requiring
  the write to name the binding separates those two cases; counting lines
  between them cannot. `self_test_*` bodies are exempt for a related reason:
  they build and tear down the address spaces they stamp.
- **It was validated against the pre-fix tree.** The script was run with the old
  `write_user_image` restored, and it fired on exactly that line. A checker
  confirmed only by passing on a clean tree is the same mistake as the sysfs
  self-test in §284, which asserted its own generator's literal and so certified
  the divergence it existed to catch.

**Cost accepted:** the HHDM check is a heuristic with false negatives — an alias
laundered through a helper or a struct field is not seen — and it can produce
false positives that need a human to clear. Both are stated in the script's
docstring, and clearing a false positive is an edit to a named list with a
comment, which leaves the judgment visible to the next reader instead of in a
commit message.
