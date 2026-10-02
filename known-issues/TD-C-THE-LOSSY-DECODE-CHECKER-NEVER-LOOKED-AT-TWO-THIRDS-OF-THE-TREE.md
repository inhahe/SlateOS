## TD-C-THE-LOSSY-DECODE-CHECKER-NEVER-LOOKED-AT-TWO-THIRDS-OF-THE-TREE -- 2026-09-16

**In short:** the project has a purpose-built tool for finding exactly the bug
I spent today fixing by hand -- text conversions that quietly replace an
unreadable byte and then use the result as a real value. It has been in
`scripts/` for weeks. It only ever scanned `userspace/`, so it has never once
looked at the graphics or application code, which is where all five of today's
instances were. Pointed at that tree for the first time it reports 74 more --
of which, on reading every one of the seven that looked worst, **one** is a
defect.

**Date:** 2026-09-16. **Lane:** C.

**How it went unnoticed.** Nothing was broken and nothing failed. The checker
ran in the pre-push gate, passed, and reported a number -- a number about
`userspace/`. Its default scope was a bare `os.walk(os.path.join(ROOT,
"userspace"))` written when the tool was built for a coreutils audit, and a
default that was right for the job it was written for became the *whole* scope
once it was wired into a shared gate. A green check about the wrong directory
is indistinguishable, from the outside, from a green check.

This is the same shape as design-decisions 856 -- *a settings page is built
when something obeys it, not when something stores it* -- applied to tooling:
**a checker is covering a tree when it reads that tree, not when it is on the
list of checks that ran.**

**What it reported, first run over `gui/` and `apps/`:**

    VALUE 74   DIAG 7   HOST 3   OK 0   (in 33 file(s), 383 scanned)

**Not all 74 are defects, and the tool cannot tell which.** It classifies by
*what the result is used for*, not by *what the bytes are*. Several hits decode
something that genuinely is text by its own format specification -- `svg.rs`
parsing an SVG document, `rssreader` parsing XML, `worldclock` reading names
out of a compiled zone table. The triage question is the one the tool cannot
answer: **does this byte string come from the filesystem, or from a format that
defines its own encoding?**

**The seven that looked real, and what they were when read:**

| where | what the checker saw | verdict on reading |
|---|---|---|
| `apps/settings/src/main.rs:912` | the wallpaper **path** stored lossily | **REAL.** The saved setting names a different file. Raised as **C-Q24**: the fix needs a policy decision and a change in `yamldoc`, which is not lane C's. |
| `apps/explorer/src/dropzone.rs:661` | a dropped path decoded | **Correct as written** -- and it carries a comment saying so: the string is a drag label, drawn for a human and never used to reach the file. |
| `apps/filesearch/src/main.rs:1216` | the search root decoded | **Correct as written.** It feeds `self.status_message`. |
| `apps/jsonviewer/src/main.rs:2748` | a file name decoded | **Correct as written.** It is the tab's label. |
| `apps/procexplorer/src/main.rs:716,725` | `argv` and `comm` decoded | **Correct as written.** Display columns; `argv` is already joined with spaces, which is lossy in a way no encoding fixes. |
| `apps/safeio/src/lib.rs:353` | a temp name from a lossy stem | **Not a defect.** Fixed, tested, then reverted -- see below. |
| `apps/editor/src/highlight.rs:221` | the extension decoded | **Behaviour-identical.** A name that does not decode matches no language either way. |

**The `safeio` reversal is the one worth reading.** It looked like a certain
match for a bug already fixed in `explorer::fileops::temp_name`: a scratch name
built by `format!` over `to_string_lossy`, where two targets differing only in
undecodable bytes render identically and would contend for one temporary. I
changed it to build the name by pushing bytes, wrote a test with two such
names, and the test passed.

Then I sabotaged the fix -- put the lossy version back -- and **the test passed
again.** The name also carries the process id and a per-call atomic counter, so
it is unique whatever the stem does. There was no collision to prevent. The
change was reverted: a fix whose comment asserts a failure that cannot occur is
worse than the line it replaces, because the next reader believes the comment.

**The methodological finding, which is the point of this entry.** The paragraph
above the table says *"not all 74 are defects and the tool cannot tell which"*
and names the right discriminator. In the first draft of this very entry I then
wrote the table as a defect list anyway, seven rows of them, immediately below
that warning. **Writing the caveat does not protect you from the error the
caveat describes.** What protected me was opening seven files and sabotaging
one fix; nothing else would have.

**What was changed in the tool.** `all_sources()` takes its roots as a
parameter instead of hardcoding one, and `--under DIR` (repeatable) scans any
subtree. The default is unchanged, so lane B's numbers do not move.

**What not to do with the remaining 67.** Do not sweep them. On this sample
roughly six in seven of the worst-looking are correct as written, and a sweep
would remove real distinctions -- the failure recorded in
`TD-C-A-CLEANUP-THAT-REMOVES-A-DISTINCTION`. Each needs the
filesystem-versus-format question answered on its own evidence.
