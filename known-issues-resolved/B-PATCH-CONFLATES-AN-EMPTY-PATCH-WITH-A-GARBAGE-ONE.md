## B-PATCH-CONFLATES-AN-EMPTY-PATCH-WITH-A-GARBAGE-ONE (lane B, 2026-09-12) — FIXED

**Measured against GNU patch 2.7.6**, each stream separately:

| input | GNU stdout | GNU stderr | exit |
|---|---|---|---|
| empty file | — | — | **0** |
| garbage | — | `patch: **** Only garbage was found in the patch input.` | 2 |

Ours answered `patch: no valid patches found in input` and exit 2 to both.
**Nothing to do is not an error** — a script piping a possibly-empty diff into
`patch` is doing something reasonable, and answering 2 turns "there were no
changes" into a build failure. Whitespace-only counts as empty, because GNU's
reader skips blank lines before deciding it found nothing.

**And the unknown-option wording, which has two spellings, not one:**

    patch: unrecognized option '--nosuchoption'
    patch: invalid option -- 'Q'

both followed by `patch: Try 'patch --help' for more information.` Ours said
`unknown option: X` for both, which is this build's own phrase and matches
nothing upstream prints.

**Worth noting beside the `strings` work of the same night:** `patch` prints a
`Try … --help` referral where `strings` shows its whole usage instead, and this
tree had the two backwards in opposite directions. Neither can be inferred from
the other, which is the argument for measuring each program rather than
carrying a house style between them.

`patch-diff.sh`: 3 passed / 62 differed this morning, **20 / 45** now, across
three fixes — the output stream, empty-versus-garbage, and this wording.

**What is left is a feature gap, not a bug list.** 22 of the 45 are options
this build does not implement at all: `-N/--forward`, `-F/--fuzz`, `-f/--force`,
`-r`, `--no-backup-if-mismatch`, `-Z`, `-v/--verbose`, `-o/--output`,
`-d/--directory`, `-E/--remove-empty-files`, `-l/--ignore-whitespace`. The rest
are context-format (`c.patch`) and normal-format (`n.patch`) inputs the parser
does not read. Neither is a defect in what exists; both are work not yet done.
