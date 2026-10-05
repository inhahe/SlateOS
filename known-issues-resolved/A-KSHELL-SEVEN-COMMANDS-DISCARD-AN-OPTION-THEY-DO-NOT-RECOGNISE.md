## `A-KSHELL-SEVEN-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25

**In short:** seven shell commands used to *throw away* any word starting with
`-` that they did not recognise, and then carry on as if it had never been
typed. So a misspelled flag did not produce an error — it produced a
*different command*, run successfully. `batch delete --dry-runn a b` deleted
`a` and `b` for real, because the typo matched no flag (so it was not a dry
run) and was then filtered out of the file list (so it was not an error
either). `find . -mtime -1` walked the whole tree with no filter at all and
printed every file, exit 0. Every one of them now stops and says which word it
did not understand.

**Where.** All in `kernel/src/kshell.rs`:

| Command | The discard | What it silently became |
|---|---|---|
| `find` | the `if`/`else if` predicate chain simply ended, with no `else` | an **unfiltered** walk: `find . -mtime -1` printed every file under `.`, exit 0 |
| `find -size` | `num_str.parse::<i64>().unwrap_or(0)` in `parse_size_predicate` | `-size +abc` ≡ `-size +0` — matches nearly every file |
| `locate` | `s => if !s.starts_with('-') { pattern = Some(s) }` | unknown option dropped; `--ext` with no value ≡ *every* extension; `--size abc` ≡ **no size filter** |
| `dedup` | same catch-all; `parse_size_value(v).unwrap_or(default)` | `--min-size 1O` (letter O) ≡ the default bound — on a command whose other half is `--delete` |
| `backup` | each flag tested independently with `contains` / `any` | `--dry-runn` ≡ not a dry run **and** not an error: the backup ran |
| `undelete scan` | catch-all + `if i + 1 < parts.len()` on every value | any filter that failed to parse was simply not applied: the listing silently widened |
| `batch` | whole-line flag scan **plus** `.filter(\|f\| !f.starts_with('-'))` in each operand list | the typo fell between the two and vanished: **files were deleted** |
| `mapfile` | prefix loop `break`s on an unknown word | the word fell through into the *array name* slot: `mapfile -u 0 ARR` read a file called `0 ARR` |
| `oci run` | `tok if tok.starts_with('-') => { i += 1 }`, deliberately | flag dropped **and its value became the container's command**: `oci run img --menory 512m` ran `512m` |

**Why this is the worse half.** The `--` gap this sweep also closed is a
*refusal*: the command fails loudly and nothing is misread. This one is a
*silent guess*, and the guess is almost always in the direction of doing
**more**: the dropped word was nearly always a filter or a restriction, so
dropping it widens the search, enlarges the deletion set, or removes the
containment the operator asked for. A wrong answer reported as success is
strictly worse than a missing one, and here the wrong answer is the one that
touches the disk.

**On `oci run`'s comment, because it was not an oversight.** It read: *"Unknown
option: skip it (and don't consume a value, since we don't know its arity)."*
The reasoning is correct about the arity and wrong about the conclusion.
Skipping is not the neutral act it looks like — every flag in that parser
exists to *narrow* what the container may do (`--read-only`, `--memory`,
`--user`, `--tmpfs`), so dropping one runs the container with more authority
than was asked for, and prints a container ID as though the request had been
honoured in full. And not knowing the arity is precisely the reason to stop:
we cannot even say how much of the rest of the line we have misread. Its
`parse` is now `-> Option<Self>` and the caller abandons the run.

**How it was found.** Not by looking for it. The entry above named `cut` and
`fold` because that is the shape of *where it was noticed*; §299 of
`design-decisions.md` says a gate keyed on the shape of the last bug defines
its own blind spot, so the fix began with a survey of every option parser in
the file rather than with those two functions. The survey turned up the missing
`--` in seven more commands — and this, which nothing in the original report
suggested was there.

**And the twin next door.** `find`'s `-maxdepth` `.unwrap_or(16)` had already
been fixed, with a three-paragraph comment about why guessing at a search's
bounds is unsafe. `parse_size_predicate`, three lines below it, still carried
the identical `.unwrap_or(0)`. The fix had been applied to the *site* and not
to the *category*, which is exactly how a twin survives its sibling's fix.

**Tested by.** Self-test rung 63, which probes each command with a bogus
option and asserts a non-zero exit *and*, for the two that write to the disk,
that nothing was written: `batch delete --dry-runn` leaves its file in place
and `backup create … --dry-runn` creates no backup directory. It also checks
that `--` makes a file literally named `-empty` findable, and that a valid
`-size` still selects — so the refusals cannot pass by having broken the
options altogether.

**Not a regression.** True of each command since it was written.

**Sequel.** Nine was not the count. A checker written for the rule the next
day found **fourteen more** — see
`A-KSHELL-FOURTEEN-MORE-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE`.
