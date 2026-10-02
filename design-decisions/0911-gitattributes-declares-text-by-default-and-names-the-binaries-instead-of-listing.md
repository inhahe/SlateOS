## §911 — `.gitattributes` declares text by default and names the binaries, instead of listing the text types

**Date:** 2026-09-05. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** a text file can end its lines the Unix way (`\n`) or the Windows
way (`\r\n`). This project wants the Unix form everywhere, because a Windows
line ending inside a shell script breaks it in ways that look like the script
being wrong. `.gitattributes` is the file that tells git which files to hold to
that rule, and until today it did so by **listing the file types that are
text** — `*.sh`, `*.py`, `*.md`, and three more. I inverted it: the default is
now "everything is text", and the file instead **lists the handful of things
that are binary** (images, a disk image, an EFI executable). This closes
`A-27`, which lane A opened on 2026-08-18 and deferred, and answers lane B's
`requests/b-a-your-crlf-files-are-gone-...`, which handed the decision back.

**What changes, observably:** nothing today, and that is measured rather than
hoped — see the neutrality proof below. What changes is what happens on a
machine that is not this one.

**The three reasons the list shape had to go**, in increasing order of how hard
they are to argue with:

1. **It went stale exactly as its critics predicted.** `*.rs` and `*.toml` are
   9 449 of 13 907 tracked files and were declared by nothing, so 27 of them
   sat CRLF in a worktree while `git status` called it clean. §769 covers this
   at length from the *gate's* side.
2. **A list keyed on extensions cannot cover a file that has none — and the
   most important script in this tree has none.** `scripts/hooks/pre-push` is
   bash, it runs on every push, and `*.sh` does not match it. A CR in it turns
   `set -u` into `set -u$'\r'` and the push gate dies looking like a bug in
   itself. No amount of maintaining the list fixes this; the shape is wrong.
   This is the argument that decided it, and it is not one anybody had made:
   A-27, §769 and lane B's request all reason about *types*.
3. **The repository's LF guarantee was resting on a file outside the
   repository.** `core.autocrlf=input` is set **only** at system scope, in
   `C:/Program Files/Git/etc/gitconfig` — installer-owned, untracked, absent
   from a fresh clone anywhere else. Every "the blobs are always LF" claim in
   these documents (including mine) was true by accident of one machine's git
   installation. `text=auto` moves the guarantee into the repository.

**Option A — add `*.rs text eol=lf`, the one line A-27 asked for and lane B
offered.**
*What changes:* Rust files are pinned; the next undeclared type is not.

- *For:* minimal, exactly what was requested, no new reliance on any heuristic.
- *Against:* it answers reason 1 only. `scripts/hooks/pre-push` stays
  undeclared, which is the file it is least acceptable to leave undeclared, and
  the guarantee still lives in `C:/Program Files`.

**Option B — `* text=auto eol=lf`, then name the binaries. (chosen)**
*What changes:* every tracked path is classified, including extensionless ones.

- *For:* answers all three reasons. The inversion is the substance of it: the
  set of binary types here is small and nearly closed — 22 files across `*.png`,
  `*.deb`, `*.fd`, `*.efi`, `*.o` — while the set of text types grows with
  every commit. A list you have to maintain should be the one that stops
  growing.
- *Against:* `text=auto` asks git to classify undeclared files with its NUL-byte
  heuristic, and this tree distrusts that heuristic **on the record**:
  `kernel/ada/.gitattributes` refuses to rely on it for a byte-exact object file
  ("that is an inference, and it is the sort that holds until the day it
  doesn't"), and `known-issues.md` records it misfiring on `known-issues.md`
  itself, where two NUL bytes 4.5 MB in made git call a 4.5 MB document binary.

**Why the objection does not sink Option B.** The heuristic is not *relied* on:
every binary in the tree is declared, and every document known to defeat the
heuristic keeps its explicit `text eol=lf`. The heuristic only decides files in
neither list. And on this machine `autocrlf=input` already runs the identical
heuristic over the identical files, so B adds no new exposure here at all — it
only changes what happens on a machine where today nothing is normalised.

**The neutrality proof, because "it changes nothing" is a claim and not an
argument.** `git add --renormalize .` re-applies every attribute to all 13 907
tracked files and stages whatever the attributes would alter. Run against the
new file it staged **zero** paths — the 18 PNGs, the OVMF image, the EFI
binary, the `.deb` and the Ada `.o` included. Not one stored byte differs. Run
twice: once with the binaries as `-text` and once with the `binary` macro.

**Binaries are spelled `binary`, not `-text`.** The macro expands to
`-diff -merge -text`, and all three matter: `-text` stops conversion, `-diff`
stops a PNG being printed as thousands of junk lines, `-merge` stops git ever
attempting a three-way merge of two disk images, which cannot produce a valid
one. `git check-attr` still reports `eol: lf` on them, inherited from the
catch-all; that is inert, because once `text` is explicitly unset git takes the
binary path whatever `eol` says — verified by the renormalize run above rather
than assumed.

**What this does not buy.** Prevention at *checkout* only, as §769 correctly
argued — every CRLF occurrence in this tree came from a tool rewriting a file
long after checkout, and no attribute intercepts that. The gate of §769 remains
the thing that catches those, and it is scoped to every tracked file with no
list at all. Detection has no list; prevention now has one that lists binaries.
Detection is the property that must never go stale, and it is the one with
nothing to maintain.

**Reversal:** delete the `* text=auto eol=lf` line and the five `binary` lines.
Nothing else depends on them, and by the neutrality proof no blob has to change
back.
