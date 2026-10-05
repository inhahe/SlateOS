### [A] A checker narrowed its own scope by delegating to a tool nobody runs -- 296 unresolved doc links remain in the kernel -- 2026-09-21
**Status:** PARTLY FIXED (112 defects fixed and a gate added; 296 unresolved links remain behind a ratchet, and the cross-lane half is requested from lane B)

**In short:** the kernel's documentation contains about 6,000 cross-references
to other functions. Nothing had ever checked that any of them point at
anything. 430 problems had built up, including 64 that make the documentation
tool silently *delete* the sentence it is in.

**The interesting part is not the count, it is how the hole was argued into
existence.** `scripts/check-doc-links.py` is a careful, well-reasoned tool. It
gates exactly one class -- a link naming something that exists nowhere -- and
its docstring says plainly why one class is enough:

> The other three do not need one: rustdoc reports them the moment anyone
> runs `cargo doc`.

That is a correct argument resting on a premise nobody checked. **Every
mention of `cargo doc` in this repository is inside that docstring.** Nothing
has ever run it. The narrow scope was justified by a backstop that does not
exist, and three classes of defect accumulated in the gap between the
delegation and its target.

This is worth naming as its own shape, because it is not the same as a
missing check and not the same as a wrong one: **a check that is correctly
scoped, and whose correctness depends on a second check that was never
built.** Neither tool is wrong on its own. The defect lives in the handoff,
which is exactly the place no single tool's tests can see. dd-953 collects
ways a check reports something true about the wrong thing; this is a check
reporting something true about the right thing while something else it named
goes unmeasured.

**There is a second, independent reason the kernel was uncovered.** The
checker's `ROOTS` are lane B's trees, with a `--roots` flag added so other
lanes can point it at their own -- lane C did this for `gui/` and `apps/`.
Lane A never did. And when I tried, it refused:

    refusing to report a verdict -- a whole-tree scan saw only 1 crate(s),
    below the floor of 5
           inspected: 1 crate(s), 807 file(s), 146494 doc line(s),
                      6078 link(s), 5845 judged

The floor exists so an empty scan cannot read as a clean one, which is right.
But it counts **crates**, and crate count is a proxy for "did this scan see
anything" that has stopped being one: the same refusal reports 6,078 links in
the breath where it calls the scan too small to judge. Lane A's whole tree is
one crate, so no `--roots` argument can ever clear it. Requested from lane B
in `requests/a-b-your-doc-link-gate-refuses-a-verdict-on-a-single-crate-
root.md` with three possible fixes.

**Measured, first rustdoc run in the project's history (94s):**

| class | count | what it does to a reader |
|---|---|---|
| unclosed HTML tag | 64 | **deletes text.** Markdown reads `Vec<u8>` in prose as a tag and can swallow what follows |
| unresolved link | 344 | renders as literal bracketed text |
| redundant explicit link target | 8 | cosmetic |
| both a function and a crate/macro | 7 | ambiguous, resolves arbitrarily |
| links to a private item | 3 | a public doc pointing somewhere unreachable |

**Fixed (112):** 64 unclosed tags; 20 `KernelError::X` links in the two files
lacking the import; 14 that named the right module but a private item, so the
path could never resolve; 14 naming functions that no longer exist at all.
That last group is lane B's own headline class -- nine renamed-away names,
every successor confirmed from git rather than guessed.

**Two of those nine could not be fixed by substituting the new name**, and
this is the part worth carrying forward. `awk_pattern_eval` became
`awk_compile_pattern`, whose signature takes no record -- and the sentence
pointing at it read *"which is what lets awk_validate_program ask with a dummy
record"*, describing a technique that had been **deleted**, not renamed.
Substituting would have turned a dead link into a live falsehood, which is
worse than the dead link. Same for `awk_compare`, cited as where a shape once
drifted: pointing it at today's `awk_parse_cmp` would attribute an old defect
to current code. **A finding in this class is actionable without being
mechanical,** and a sweep that assumes rename-and-move-on degrades the docs
while clearing the gate.

**Remaining (296 unresolved links), and why they are not simply fixed.** Most
are prose that merely looks like link syntax -- `argv[0]`, `buf[i]`, `[path]`
-- which rustdoc cannot tell from a real link. A sample classified by whether
the target names any definition in the crate splits about half and half, but
that test is itself unreliable in both directions: `path`, `link` and `reset`
are common English words that happen to also be identifiers, and a genuinely
renamed function looks exactly like prose. They need reading, not a script.

**Gate added** (`check_kernel_docs` in `scripts/boot-test.sh`): unclosed tags
ceiling 0 and it means zero, unresolved links ratcheted at 296. Both counts
print every run so silence and success do not look alike. Verified by making
it fail, not only by watching it pass -- an injected `Vec<injected>` is caught
and located, and a ceiling one below actual exits 1.
