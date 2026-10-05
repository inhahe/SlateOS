## B-PATCH-OPTION-GAP (lane B, 2026-09-12) — partly closed, and the rest is named

`patch-diff.sh`: 3 passed / 62 differed this morning, **42 / 23** now.

**Implemented properly:**

| option | what it does |
|---|---|
| `-r FILE` / `--reject-file=FILE` | names the reject file instead of `<target>.rej` |
| `--no-backup-if-mismatch` | suppresses `<target>.orig` on a failed hunk, and **only** that — the reject is still written, because a reject is the failure report rather than a backup |
| `-d DIR` / `--directory=DIR` | chdir before the patch file is opened |

**`-l/--ignore-whitespace`, `-N/--forward`, `-F/--fuzz` and `-f/--force` are
all IMPLEMENTED** as of 2026-09-15. Only `-Z/--set-utc` is still accepted and
inert.

**The claim this paragraph used to make about `-f` was false**, and it is the
clearest example in this file of how an inert option gets justified. It said
`-f` "differs from the default only where GNU would otherwise prompt". What it
actually does, measured on an already-applied patch:

| | without `-f` | with `-f` |
|---|---|---|
| message | `Reversed (or previously applied) patch detected!` | `Hunk #1 FAILED at 1.` |
| count | `1 out of 1 hunk ignored` | `1 out of 1 hunk FAILED` |
| exit status | **0** | **1** |

A script reading the status gets the opposite answer. The prompt was the least
of it. The original claim was not careless — it was measured, on cases that
could not tell the two behaviours apart, which is the failure this whole entry
keeps circling.

`-Z` is the last one, and the honest statement about it is narrower than the
others were: it sets mtimes from the patch header, no observable difference
turned up on the cases here, **and `patch-diff.sh` snapshots mode, content and
size but not mtime — so it could not have seen one.** That is a reason to be
careful about calling it harmless rather than a reason to call it done.

The wording this replaces said that accepting an inert option was "correct
today and incomplete rather than wrong". **That argument should not be made
again without one qualification, and `-l` is why: `--help` ADVERTISED it.** Of
the four places that option was written down, three said inert and the only one
a user reads said it worked. An inert option is defensible exactly as long as
nothing promises otherwise.

**`-F` was not a gap at all — it was a DIVERGENCE**, and the sharpest thing in
this entry. The constant governing hunk placement was called `max_fuzz` and
meant *slide distance*: how far a hunk may move from the line its header names,
while matching every line exactly. GNU's fuzz is a different mechanism
entirely — ignore up to N **context** lines at each end of the hunk, default 2,
never excusing a removed line. Two mechanisms, one name, and the name belonged
to the one that did not exist. So this build refused hunks GNU applies at fuzz
1, and `patch-diff.sh` was green throughout, because not one of its cases
perturbed a context line: a harness agreeing with the reference on every case
that cannot distinguish them.

A hunk that applies with fuzz also leaves a `<target>.orig`, because
`--backup-if-mismatch` is GNU's default and a fuzzy apply counts as a mismatch.
An offset does too. A `-l` loose match does **not** — measured, and worth
stating because it is the one inexact-looking case that writes no backup.

Eight differential cases cover fuzz now, including the control that matters:
a perturbed REMOVED line must still be refused at `-F 3`. Still open in the
same family: an **offset** is neither reported (`Hunk #1 succeeded at 3 (offset
1 line).`) nor does it write the `.orig` GNU writes for it.

**Not implemented at all**, and each still costs its cases: `-o/--output`,
`-l/--ignore-whitespace`, `-E/--remove-empty-files`, `-v` (which prints the
version, not verbose output), `--verbose` (which prints a long narrative), and
the context (`-c`) and normal (`-n`) patch formats the parser cannot read.

**A test caught a change I did not think of.** `parse_unknown_flag_errors` used
`-Z` as its unknown flag, so accepting `-Z/--set-utc` turned it into an
assertion that a *recognised* option is rejected. It failed immediately, which
is exactly right: a test whose fixture quietly becomes valid input stops testing
anything, and this one said so rather than passing on.
