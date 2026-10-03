## §906 — The end-of-line gate grades the promise rather than the tree, and runs second rather than first

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** A file that git has been told to keep with Unix line endings can be
sitting on disk with Windows ones, and no git command anybody actually types will
say so — `git status` is clean, `git diff` is empty, `git commit` says nothing.
On 2026-09-03 thirteen files in this worktree were in exactly that state,
`scripts/boot-test.sh` among them, and it cost a forty-seven-minute boot test to
find out. `scripts/check-eol.py` is the gate that reads the files directly. Two
things about it were judgement calls rather than obvious, and this entry is about
those two: **which** files it grades, and **where** in the pre-build sweep it
runs.

### Why this needs a gate at all: git is not merely unhelpful here, it is confidently wrong

`text eol=lf` installs a clean filter, and the filter runs *before* any
comparison. A file that is CRLF on disk and LF in the index is therefore not
"modified in a way git chooses to ignore" — it is, to git, byte-identical to the
index.

| Command | Sees the CRLF? |
|---|---|
| `git status` | no — reports a clean tree |
| `git diff`, `git diff --quiet` | no — empty, exit 0 |
| `git add`, `git commit` | no — stages nothing |
| `git diff-files` | **yes** — compares raw bytes |
| `git ls-files --eol` | **yes** — reports raw bytes |

Only the bottom two rows look at the working tree's actual bytes, and nobody runs
them. That is why this failure was *invisible* rather than merely *unreported*:
every instrument on the desk agreed the tree was clean, and every one of them was
answering a question about the index.

The proof that nothing was ever going to point at it: repairing all thirteen
files and staging the result produced a **zero-byte diff**. The bad bytes had
never reached a commit and never would have.

### Decision: the graded set is whatever `.gitattributes` declares — not a suffix list, and not the whole tree

| | *What changes:* | Catches | Misses |
|---|---|---|---|
| Only `*.sh` | the gate fires only where a CR actually breaks something | the one file that broke | the other twelve — that is, the size of the cause |
| Everything declared `eol=lf` (**chosen**) | the gate fires on any declared file whose bytes contradict the declaration | all thirteen | files about which no promise was made |
| Every tracked file | the gate fires on any CR anywhere | everything | nothing — and it is red on day one |

The `.sh`-only scope is the tempting one, because `.sh` is the only extension
where a CR does observable harm today (`bash` folds the CR into the token it
ends, so `set -u` becomes `set -u$'\r'`). It is wrong for a reason that
generalises: **the thirteen files were corrupted by one event, and `.sh` was one
of the thirteen.** A gate scoped to the harm would have reported the symptom and
concealed the extent of the cause. What is worth learning from a CR in a `.md`
file is not that the `.md` file is broken — visibly, it isn't — but that *a tool
in this tree writes text files in the wrong mode*, and the count of affected
files is the evidence for that.

The whole-tree scope fails at the other end. On the order of 180 tracked files
are CRLF in the worktree and carry no `eol` declaration at all — `kernel/src/fs/`
`*.rs`, `kernel/src/crypto.rs`, `kernel/src/syscall/handlers.rs`,
`kernel/build.rs`, `bench/baselines.toml`, the Ada sources. Grading those would
make the gate red on its first run, over files against which no promise was ever
made; and a gate that is red for reasons its own repository considers acceptable
is a gate that gets switched off. Whether those files *should* be declared is a
separate question, and a cross-lane one — see below.

So the scope is the promise itself, read out of git via `git check-attr` rather
than restated here as a list of suffixes. A suffix list would be a second copy of
the policy, free to drift from the first, and the drift would surface as the gate
quietly not covering a newly-declared file type — which is this entry's own
failure mode, one level up.

### Decision: second in the sweep, not first

Prerequisites aside, `check_requests_not_deleted` keeps the head of the sweep and
`check_eol` follows it. The argument is not about which check is cheaper:

| | If it runs late | What is at stake |
|---|---|---|
| `check_requests_not_deleted` | a cross-lane request has already been destroyed | **information that cannot be recovered** |
| `check_eol` | a build has already started against bad bytes | **one wasted cycle** |

The head of the sweep is ordered by what a *late* detection costs, and only one
thing in this tree costs something irreversible. Everything below that is ordered
cheap-and-broad first, and `check_eol` is now the first of those. That is the
whole point of the placement: the equivalent evidence already existed inside
`check_shellcheck`, some forty-five minutes into the sweep and covering only
`.sh`. That is where the CR was found on 2026-09-03, and finding it there is what
threw the run away.

### The floor, because this gate can fail in the exact way it exists to catch

`DISCOVERY_FLOOR = 500`, against ~1,438 declared files in a tree of 13,884
tracked ones. The fragile part of this gate is not "does `\r` appear in these
bytes" — that cannot drift. It is the attribute query: a NUL-separated
`git check-attr -z --stdin` stream walked three fields at a time. If that framing
changes, or the invocation grows a typo, **the declared set becomes empty and the
gate reports a clean tree in a fraction of a second, forever, on every host** —
which is the identical failure it exists to catch elsewhere. The floor converts
that into an exit 2.

### The cost accepted

~37 MB across ~1,438 files: 93 s single-threaded on this host, ~32 s across a
16-thread pool. The cost is per-file antivirus interception rather than
bandwidth, which is both why threads help this much and why the pool is what
makes the gate affordable at all. `open-questions.md` A-Q7 asks about an
exclusion; if it is ever answered, this drops into the noise.

### What this deliberately does not settle

Declaring `*.rs` and `*.toml` as `eol=lf` is the change that would make the
undeclared set empty. It is a whole-tree normalisation touching all three lanes'
files at once, so it wants a `requests/` file and the other two lanes' agreement,
not a unilateral commit from lane A. Until then the undeclared files are out of
scope by construction, and this section is the record of that being a choice
rather than an oversight.

### Reversing this

Delete the `check_eol` call from `scripts/boot-test.sh`; the gate still works
standalone. If it is the *scope* that turns out to be wrong, the change is one
predicate in `declared_lf()` — but note that widening it to the whole tree
without first declaring the `*.rs` files means adopting ~180 pre-existing
findings on day one. The honest order is declaration first, gate second.
