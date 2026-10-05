## B-PATCH-REPORTS-A-FAILED-HUNK-WITHOUT-SAVING-IT (lane B, 2026-09-12) — FIXED

A failed hunk was reported and then discarded. GNU writes the rejected hunks to
`<file>.rej` as a usable patch and the untouched original to `<file>.orig`.
**A message telling someone a hunk failed, without handing them the hunk,
leaves them to reconstruct it from a diff they may no longer have.**

Measured, all on stdout with exit 1 and nothing on stderr:

    patching file a/base.txt
    Hunk #1 FAILED at 1.
    1 out of 1 hunk FAILED -- saving rejects to file a/base.txt.rej

against ours, on stderr: `patch: Hunk #1 FAILED at line 1` and
`patch: 1 out of 1 hunks FAILED for a/base.txt`. Three differences in two lines
— the stream, `at 1.` versus `at line 1`, and the singular `hunk` with the
reject file named.

**Both files are written even when NO hunk applied** and the target is therefore
unchanged, which is why `.orig` here is not the same thing as `-b`'s backup.

**THE LAST FOUR BYTES WERE THE INTERESTING PART.** With everything above
matching, the `.rej` files still differed — 91 bytes against GNU's 87. The cause:
GNU writes the **stripped** paths in a reject header (`--- a/base.txt`), while
the missing-target block quotes the **raw** ones (`|--- x/a/base.txt`).

That asymmetry is not arbitrary. The block is quoting the patch back at you to
explain why it could not be read, so it must show what the patch actually says.
A reject is a patch to be re-applied *in the tree you are standing in*, so its
names have to be the ones that resolve there. Same two lines, two different
jobs, two different answers.

It was found by diffing the two reject files rather than by reasoning about the
size: on a hand-made fixture they were byte-identical, and only the harness's
own `diff -u --label x/... --label y/...` fixture exposed it. **A fixture that
is too tidy proves the wrong thing.**

`patch-diff.sh`: 3 passed / 62 differed this morning, **31 / 34** now.
