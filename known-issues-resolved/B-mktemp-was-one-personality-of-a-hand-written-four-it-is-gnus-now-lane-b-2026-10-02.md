## `mktemp` was one personality of a hand-written four; it is GNU's now (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** `userspace/mktemp` was a "multi-personality utility: mktemp / id /
groups / whoami". The other three duplicated coreutils' own `id`, `groups` and
`whoami`, and nothing linked to them; the `mktemp` read argv as `String` (a
template that was not UTF-8 killed it before its first statement) and its
options its own way. It is replaced by `coreutils/src/bin/mktemp.rs`, a port of
GNU coreutils 9.4's `mktemp.c` with gnulib's `gen_tempname_len`,
`last_component` and `file_name_concat`, and the crate is deleted (§1005).
`scripts/mktemp-diff.sh`: 58 cases, none differing from GNU's -- the random
characters turned back into X's, and what was made compared instead (type,
permissions, nothing made by `-u` or by a run whose output failed).

**What the port had to reproduce, because it is observable:** the X's are the
last run before the suffix, the suffix everything after the last `X` unless
`--suffix` says otherwise; `-t` ranks `$TMPDIR` above `-p`'s directory and `-p`
ranks it below; `-t` refuses a template with a slash and `-p` an absolute
one; `-u` makes nothing but still requires the name to be free; and a name
that cannot be printed takes its file with it, with `write error` and its
reason.
