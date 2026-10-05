## B-PATCH-HAS-NO-OUTPUT-OR-REMOVE-EMPTY-OPTION (lane B, 2026-09-12) — FIXED

`-o FILE` / `--output=FILE` and `-E` / `--remove-empty-files` are implemented.
`patch-diff.sh`: 42 passed / 23 differed to **44 / 21**.

**`-o` announces the destination and names the source:** `patching file out.txt
(read from a/base.txt)`, because the file being written is no longer the file
being read. The target is left untouched.

**`-E` removes a file the patch has emptied** — gone from the tree, not left at
zero length. It is deliberately suppressed when `-o` is in force: the emptiness
is a property of what was *written*, and deleting the target would be deleting
the file the patch was read from.

**THE LAST DIFFERENCE WAS A FILE MODE.** With the output byte-identical and the
tree otherwise matching, `out.txt` was 644 here and **600** under GNU. That is
the kind of difference that survives every test which reads the file back,
because the content is right — only the permissions are not.

It is restrictive on purpose and worth understanding rather than copying: `-o`
writes to a path the user named, rather than updating a file that already has
permissions of its own, so there is no existing mode to preserve and the safe
default is the private one. In-place writes are left alone for the same reason
in reverse — that file already exists and its mode is not patch's to choose.
