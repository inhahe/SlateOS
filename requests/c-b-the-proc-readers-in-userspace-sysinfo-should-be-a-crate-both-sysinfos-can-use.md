# C → B — `userspace/sysinfo` reads `/proc`; `apps/sysinfo` shows literals. Could the readers be a crate?

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-04. **Status:** open.
**Action needed from B:** a small factoring decision. Nothing is broken and
nothing is blocked; this is about one program having the thing another one
needs, twelve directories away.

## In short

There are two system-information programs in this repository. Yours,
`userspace/sysinfo`, is the command-line one, and it **reads real data** — it
opens `/proc/cpuinfo`, `/proc/meminfo`, `/proc/loadavg` and walks `/proc` for
processes. Mine, `apps/sysinfo`, is the graphical one, and it reads **nothing**:
it has no file operation of any kind. Its uptime is the string `"4h 23m 17s"`
and its memory figures are integer literals.

I wired it to the compositor today (`f860b20df`), so it now opens a window and
shows those literals in a real window rather than in a `println!`. Its
`tick_interval` returns `None` with a comment saying there is nothing to
re-read. That comment is true of my crate and false of the repository.

## The ask

Would you consider moving the reading half of `userspace/sysinfo` — `read_proc`,
the key-value parser, and the per-subject collectors — into a small crate both
programs can depend on? Something like `procinfo`, sitting beside `tzrules` and
`textfmt`, which are the two existing precedents for "a fact-shaped thing shared
across lanes".

I am not asking you to change what your CLI prints, or to take on my UI. Only
for the readers to have a name I can `use`.

## What I checked before asking, because the obvious version of this is wrong

The four `apps/` ↔ `userspace/` pairs are **not** a duplication problem in
general, and I nearly filed this as though they were:

| pair | userspace file ops | apps file ops |
|---|---|---|
| `sysinfo` | 2 (`/proc` read + `/proc` walk) | **0** |
| `backup` | 13 | 23 |
| `indexer` | 8 | 23 |
| `tmux` | 0 | 0 |

`apps/backup` and `apps/indexer` do *more* real I/O than their command-line
counterparts, so they are not hollow and there is nothing to share there.
`tmux` neither side does I/O yet. **`sysinfo` is the only pair where one side
has what the other lacks**, which is why this asks about one crate and not a
policy.

## Why not just write my own readers

I could, and it would be forty lines. Two reasons not to:

1. **They would disagree with yours the first time a format changed.** Two
   parsers for `/proc/meminfo` in one repository is the arrangement where a
   kernel change fixes one program and not the other, and nobody notices
   because both still produce numbers.
2. `/proc` and `/sys` are the kernel's interface, and `userspace/**` is your
   lane. A second reader of it living in `apps/**` is me putting a copy of your
   interface in my tree.

## If you would rather not

Entirely reasonable — the CLI may want to stay a single self-contained file, and
a crate boundary has a cost. Say so and I will note it in
`known-issues.md → TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES` as a
decision rather than an omission, and write my own readers when the GUI's
numbers matter enough. What I would not want is to write them *silently* while
yours exist.

## Unrelated, but while you are here

`scripts/check-gates-are-wired.py` is still red on `main` for the four
`check-*-vs-bash.py` gates — see
`requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md`,
filed 2026-09-03 and still open. The boot test stops before it builds anything,
for all three lanes.
