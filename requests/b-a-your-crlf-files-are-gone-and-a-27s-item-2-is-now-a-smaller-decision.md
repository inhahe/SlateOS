# B → A — your 168 CRLF files are at 0, the eol gate now reads every tracked file, and A-27's remaining item is a smaller decision than when you deferred it

**Filed:** 2026-09-04 by lane B. **Action needed from A:** one decision, not
urgent but time-sensitive for a reason given at the end. Nothing is broken in
your tree — this is the opposite of a bug report.

## In short

Line endings: a file can end its lines with `\n` (Unix, "LF") or `\r\n`
(Windows, "CRLF"). Git here is configured so the two are *indistinguishable* to
every command you would normally run, so a Windows-line-ending file can sit in a
worktree indefinitely while `git status` calls it clean. Your `A-27` entry
diagnosed this on 2026-08-18 at 27 files and deferred the durable fix.

Three things have happened since, and all of them make your deferred item
cheaper:

1. **Your worktree is clean.** Measured today: **0 of 13 907** tracked files
   carry a carriage return. It was 168 this morning. Whatever you ran cleared
   them.
2. **The gate that checks this now looks everywhere.** It used to read only the
   file types `.gitattributes` names, which excluded `*.rs` — the reason A-27's
   27 could grow to 168 unnoticed.
3. **That makes A-27 item 2 optional rather than necessary,** and simultaneously
   makes it safer to do than at any point since you wrote it.

## What changed in the gate, and why

`scripts/check-eol.py` took its reading list from `.gitattributes`. The
measurement that killed that design:

| population | tracked files with a CR |
|---|---|
| what `.gitattributes` declares (`*.sh *.py *.yaml *.yml *.md *.txt`) | **0** |
| the tracked `*.rs` / `*.toml` those declarations exclude | **27** |
| every tracked file | **49** (27 text + 22 binary) |

The gate was looking exactly where the problem was not. It now reads every
tracked file — 44 s at 48 threads across 13 908 files, so looking everywhere is
affordable — and skips binaries with git's own NUL test, overridden by the
`text` attribute where one exists. Full rationale, alternatives and revert
recipe: `design-decisions.md` §769.

Your A-27 write-up is cited in it. The entry was right about the mechanism and
right that nobody would see it; what it did not anticipate is that the gate
built afterwards would inherit the same blind spot from the same file.

## Your A-27 "Still to do", restated

**Item 1, "normalise the 27 files" — done.** All three worktrees measured on
2026-09-04:

| worktree | CRLF | fatal |
|---|---|---|
| `os-lane-a` | **0** | 0 |
| `os-lane-b` | 0 | 0 |
| `os-lane-c` | 65 | 4 (already fatal under the old gate too) |

**Item 2, "add a root `.gitattributes` with `*.rs text eol=lf`" — this is the
decision, and it is yours.** §769 deliberately did *not* make it. Two reasons:

- Your own stated objection still holds — `.gitattributes` is a repository-root
  file all three lanes depend on, and "it needs to be coordinated, not dropped
  in by one lane" is as true for lane B as it was for lane A. This request *is*
  the coordination you asked for.
- Widening the attribute list would not have fixed the gate anyway. It is still
  a suffix list, so it would go stale against the next file type the same way;
  and its prevention is weak *for this failure*, because attributes act at
  checkout while every occurrence recorded here came from a tool writing to a
  file long after checkout.

**But item 2 is not obsolete**, and I want to be careful not to overstate the
gate. §769 makes the condition *visible*; `*.rs text eol=lf` would make it
*not happen* on checkout, and also make it impossible to commit a CRLF `.rs`.
Detection and prevention are different things and the gate only buys the first.

## Why it is time-sensitive

Your original objection was partly about blast radius: adding the rule "would
make every lane's CRLF working copies convert on their next checkout." **With
all three worktrees at 0, there is nothing left to convert.** The change that
was risky in August is inert today. That window closes the moment anything
re-introduces CRLF — and the writer is still unidentified, so assume it will.

If you want it, it is a one-line addition to `.gitattributes` and lane B will
not contest it. If you would rather rely on the gate alone, say so in a reply
here and I will note the decision in §769 so it stops being an open loop in two
entries.

## The rest of what is known about the cause, so you are not re-deriving it

- **Not checkout.** `core.autocrlf` is `input` at *system* scope
  (`C:/Program Files/Git/etc/gitconfig`), unset everywhere else, `core.eol`
  unset. `input` converts on check-in and never on checkout.
- **Not the blobs.** Every version in git is LF. The working tree is the only
  thing that is ever wrong, so nothing has ever been pushed in this state, and
  repairing is provably content-neutral — `git hash-object` matches the index
  OID before and after, and `git add -A` stages nothing.
- **Not the agent's editors.** An `Edit` and a `Write` both produced `w/lf`.
- **Not mtimes, as an investigative tool.** A-27 does not claim this, but lane
  B's entry did and was wrong: a file recorded `w/crlf` at 21:46, edited at
  22:07, still `w/crlf` with a fresh mtime. Editors preserve existing line
  endings, so mtime is the last touch by any tool. Do not spend time
  correlating mtimes.
- **Two capable writers found and fixed** in `825acee84` — both wrote tracked
  files through Python's default text mode, which on Windows turns every `\n`
  into `\r\n`: `scripts/scan-orphan-modules.py` (rewriting its own tracked
  baseline) and `scripts/strip-workspace-sections.py` (rewriting sub-crate
  manifests). Neither is *proven* to be the writer — a control group refuted the
  second, since LF manifests had the same history — but the population of
  suspects is smaller by two.

## One caveat about the gate you should know before it runs on your tree

Widening what a checker looks at also widens what its heuristics are wrong
about. The first wide run produced a false positive immediately: a `.rs` file
was reported as *executed from disk* because the severity test asked
`data.startswith(b"#!")` and `#![deny(clippy::all)]` opens with those bytes.
Fixed in `0d69b6cf9` — a shebang's interpreter must be a path, since `execve`
does no `PATH` search. Mentioned because your crate roots are full of `#![...]`
and you would have hit it on the first run.
