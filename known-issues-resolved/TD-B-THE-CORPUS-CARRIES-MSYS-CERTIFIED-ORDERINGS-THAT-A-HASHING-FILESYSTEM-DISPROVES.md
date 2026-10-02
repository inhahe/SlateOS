## `TD-B-THE-CORPUS-CARRIES-MSYS-CERTIFIED-ORDERINGS-THAT-A-HASHING-FILESYSTEM-DISPROVES` (lane B, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** the osh test corpus records what real bash does, and until now it
was recorded against an MSYS (Git-for-Windows) bash sitting on NTFS. NTFS hands
directory entries back already in alphabetical order, so any bash behaviour that
is really "whatever order the directory came out in" *looked* like "sorted", and
was written down as such. On Linux, where ext4 hands entries back in the order a
hash function put them, those notes are simply false — and osh, built to satisfy
them, was answering `compgen -G` in an order bash never uses. Two such notes have
now been found and corrected; this entry exists so the next one is recognised
for what it is rather than debugged as a fresh mystery.

**Where:** `userspace/oils/tests/corpus/*.sh` (the prose *and* the cases),
`userspace/oils/src/interp.rs`.

### The two found so far

| Corpus claim | Written as | Actually |
|---|---|---|
| `compgen -G PAT` order | "the expansion comes back reversed" | reverse of the order the **directory was read**, which equals reverse-sorted only on a filesystem that sorts |
| `TIMEFORMAT="%P"` for `time :` | `75.00`, as a fact | a ratio of two sub-millisecond spans; eight consecutive runs of one bash gave 100.00, 100.00, 150.00, 66.66, 75.00, 75.00, 150.00, 100.00 |

The second was fixed in `80f1eefda`; the first is this entry's subject.

### `compgen -G`, measured

bash's `gen_globpat_matches` (`pcomplete.c`) calls the glob matcher directly
rather than expanding a *word*, so the sort that pathname expansion applies
never runs. What comes back is the matcher's own list, which it builds by
prepending each name as `readdir` produces it — hence backwards. bash 5.2.21 on
ext4, in a directory whose `ls -f` reads `gc1 gm1 gk1 gb1 gq1 gd1 gz1 ga1`:

```
$ compgen -G 'g*1'     ->  ga1 gz1 gd1 gq1 gb1 gk1 gm1 gc1     # reverse readdir
$ echo g*1             ->  ga1 gb1 gc1 gd1 gk1 gm1 gq1 gz1     # sorted
$ compgen -f g         ->  gc1 gm1 gk1 gb1 gq1 gd1 gz1 ga1     # plain readdir
```

A pattern spanning two directories is reversed *whole* — `d?/?` answers all of
`d2`'s matches before any of `d1`'s — which is what reversing the flat list
gives, and is why the fix reverses once at the end rather than per level.

Three neighbouring facts were measured at the same time and are now recorded in
the corpus case, because two of them were also wrong there:

- **Source order is actions, then `-G`, then `-W`**, whatever order the options
  were written in. The corpus said the two filesystem actions were "held back to
  the very end, behind even" the wordlist; measured, `compgen -d -f -W gw g`
  answers files, then directories, then `gw`. `-f`/`-d` are simply the last two
  entries in the *action* table, and the wordlist is not an action.
- **`compgen` does not de-duplicate.** `compgen -W ga1 -G 'g*1'` answers
  `ga1 gb1 gc1 ga1`.
- **`-G` honours `dotglob` and `nocaseglob`** and is not narrowed by the word.

### The one thing that cannot be reproduced

To bash, `.` and `..` are two `readdir` entries like any other, so `compgen -f .`
scatters them through the answer wherever the filesystem put them — measured
`.. .mhid .bhid .zhid .ahid .`, matching that directory's `ls -f` positions
exactly. `std::fs::read_dir` drops both entries on **every** platform, so osh
must name them from outside the loop and has no way to learn the position they
should have had. They are offered first, and the three affected corpus lines are
compared `| sort`ed with a comment saying why. The *set* is the shell fact; the
position is the filesystem's.

Getting this last part right would mean a raw directory reader — `getdents64` on
Linux, `FindFirstFileW` on Windows, and a third for SlateOS — carried solely to
place two entries. Not worth it, and recorded here so the decision is not
re-litigated silently.

### Fix

`glob_expand_field` is now a thin wrapper over `glob_expand_field_ordered`,
which takes a `sorted` flag; `glob_expand_field_unsorted` is the other face of
it and is what `compgen -G` calls before reversing. Sorting a glob is the word
expansion's doing, not the glob's, and bash keeps the two apart for the same
reason. The globstar branch, which needs `sort`+`dedup` to drop the duplicate a
`**` produces by matching a directory both as itself and as a descendant, keeps
first occurrence through a `HashSet` when unsorted.

Two unit tests: one pins the relationship (`-G` output is exactly the unsorted
expansion reversed, and holds the same names as the sorted one), and the
pre-existing `compgen_globpat_is_offered_whole_and_reversed` — which asserted a
literal reverse-*sorted* order, passed on NTFS, and failed the moment it was run
against ext4 — was rewritten to compare the order only against itself, and
renamed `…_and_in_reverse_directory_order` so its name stops asserting the thing
that was wrong.

**The lesson worth keeping:** a Windows-only test run is not a check on this
class of claim, because NTFS agrees with `sort` by construction. Any corpus note
about *order* has to be re-measured on ext4 before it is believed.
