### [A] A guard written to protect the operator's files reported them safe while unable to see them, and a `head` killed the push it was watching -- 2026-09-21
**Status:** FIXED 2026-09-21 (both defects; main merged at b8607ee03 with the operator's three files intact)

**In short:** the script that exists to stop a merge from committing the
operator's unsaved work could not read their directory at all, and said it
was clean. Separately, trimming a command's output to twelve lines did not
just hide the rest -- it killed the command, so a merge that looked like it
had run had not.

**Defect one: the guard's verdict did not depend on the guard working.**

```sh
G="git -C $OS_TREE"      # OS_TREE is "E:/visual studio projects/os"
$G status --porcelain | while read -r code path; do ... done > "$BEFORE"
if [ -s "$BEFORE" ]; then ... else echo "integration tree clean"; fi
```

`$G` word-splits on the spaces, so every call was `git -C E:/visual` and
failed with *"fatal: cannot change to 'E:/visual'"*. The loop then read
nothing, `$BEFORE` was empty, and the `else` branch printed **integration
tree clean** -- over a directory holding three uncommitted operator files,
one of them the project's only copy of its `LICENSE`.

An empty result and an unreadable tree are the same shape to a `while`
loop, and the script chose the reassuring reading. This is the
unconditional-label defect *inside the guard*, about the thing the guard
guards -- and it had never run before today, so nothing had disagreed with
it.

Fixed with a function (`g() { git -C "$OS_TREE" "$@"; }`) so quoting
survives, and by reading the status into a variable first so a non-zero rc
aborts instead of falling through to "clean".

**Defect two, and it is not a reporting problem: `head -12` killed the
push.** The first invocation was piped through `head -12` to keep the
transcript short. `head` exits after twelve lines, closing the pipe, and the
script took SIGPIPE **in the middle of `git push origin main`**. The local
merge had already happened, so afterwards:

| ref | value |
|---|---|
| `os` tree HEAD | `b8607ee03` -- merged |
| `origin/main` | `8e298978c` -- **not moved** |

which reads exactly like "the push failed" and was in fact "the observer
killed the subject". Lane C's *the filter kept the wrong end* has a sharper
form here: **the filter terminated the thing it was filtering.** A truncation
on a pipeline is not a read-only operation, and every `| head` I have written
over a long-running command today was a loaded gun that happened not to fire.

The tell was available and I nearly missed it: the merge had *fast-forwarded
locally* while the remote had not moved. Only a push that started and did not
finish produces that pair.

**What this cost and did not cost.** Nothing: the second run completed, main
is at `b8607ee03`, and the operator's `README.md`, `LICENSE` and
`open-questions-answers.txt` are byte-identical and still uncommitted,
verified by hash before and after. But defect one would have cost the
operator's licence file the first time a conflict forced a `git add`, and it
would have done so while printing a line saying it had checked.
