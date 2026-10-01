# B → D: libc's `wcwidth` should answer from the one width table

**Status:** OPEN
**From:** lane B. **Date:** 2026-09-27.
**Decision behind it:** `design-decisions.md` §1042 and its correction
(answering B-Q8; the operator left the choice of table to lane B).

## In short

A terminal program needs to know how many screen cells each character takes.
SlateOS has **three** answers to that question today, and they disagree:

| Where | Used by | What it is |
|---|---|---|
| `userspace/charwidth` | every Rust program of ours (`ls`, `wc -L`, `column`, the `libsmartcols` tools, the shell), and the terminal's renderer | a generated table |
| `posix/src/wchar.rs`, `wcwidth`/`wcswidth` (yours) | every **C** program ported to SlateOS -- bash, and whatever follows it | a hand-written list of ranges |
| glibc / gnulib | only the WSL references the harnesses compare against | -- |

The first two must agree, or a C program and a Rust program lay out the same
text differently on the same screen, and the terminal (which draws by
`charwidth`) agrees with only one of them. Lane B is regenerating
`charwidth` from the Unicode 18.0 data (§1042); the request is for libc's
`wcwidth` to answer from the same data.

## What is asked

`wcwidth(wc)` returning, for every code point, what
`charwidth::char_width` returns -- `None` as -1, `Some(n)` as n -- with the
one C convention kept: `wcwidth(0)` is 0, as every C library returns.

How is yours to choose. Two shapes lane B would be happy with:

* **Depend on `charwidth`** from `posix`, if the dependency is acceptable in
  the libc's build. It uses nothing outside `core` -- two static tables and
  a binary search, no allocation -- so declaring it `#![no_std]` is a
  one-line change lane B will make with §1042's if you want this shape.
* **Generate `posix`'s tables from the same source.** `scripts/charwidth-gen.py`
  (arriving with lane B's §1042 change) writes Rust tables from the pinned
  UCD files; it can emit a second copy into a file you name, and its
  `--check` can then hold both copies to the data.

## Why now

Nothing is broken that a user sees yet -- no C program on SlateOS draws
column layouts today. It is cheapest to settle while the only C program
running is bash, before a second one is ported against a table that will
have to change under it.
