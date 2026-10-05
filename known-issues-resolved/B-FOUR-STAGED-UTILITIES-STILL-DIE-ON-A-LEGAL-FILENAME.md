## B-FOUR-STAGED-UTILITIES-STILL-DIE-ON-A-LEGAL-FILENAME (lane B, 2026-09-14)

**FIXED 2026-09-14 — all four.** `patch` and `diff` were converted first,
and both turned out to have a larger content-side fault sitting behind the
argv one: `patch` refused every file that was not valid UTF-8, and `diff`
reported two different files as identical. `logger` had the same shape —
`BufRead::lines()` failed the whole read on one undecodable byte, so
`cat something-binary | logger` logged nothing. **`ps` was the mildest and
is now done too.**

**That is the pattern worth keeping from all four: the argv detector found
the door, and in three cases out of four the bigger hole was inside.** A
checker that looks at one narrow thing will report that narrow thing; what
it is actually telling you is *where to read*.

### One deliberate divergence, in `ps`

procps picks its refusal sentence by WHERE the bad byte appeared, measured:

| input | procps says |
|---|---|
| `ps -<0xE9>` | `error: garbage option` |
| `ps -u<0xE9>` | `error: user name does not exist` |
| `ps -o<0xE9>` | `error: unknown user-defined format specifier "..."` |
| `ps --sort <0xE9>` | `error: unknown sort specifier` |

This build answers `garbage option` for all four. The first matches exactly
and is pinned in `scripts/ps-diff.sh`; the other three differ in wording,
not in outcome — all refuse, all exit 1. Reaching the three sentences would
mean reparsing the argument as bytes throughout a file already at **60
passed / 0 differed** against procps, to change what a program says about
an input nobody types. Recorded here rather than pinned red, so it is a
decision on the record instead of an omission.

`logger` had the argv panic AND a second fault the argv detector cannot
see: `BufRead::lines()` on stdin yields `Result<String>` and fails the
whole read on one undecodable byte, so `cat something-binary | logger`
logged nothing and reported an I/O error — for input a log is exactly the
right place to put. Same shape as `diff`'s: the argv detector found the
door, and the bigger hole was inside.

**In short:** four of the 72 programs on the image abort with a Rust panic if
any argument is not valid Unicode. On this OS a filename may hold every byte
except `/` and NUL — that is `design.txt`, not an implementation accident — so
`patch -i <name-with-byte-0x80>` does not fail, it dies before reaching its own
first statement.

### Which, and why these four

`scripts/argv-utf8.py` reports **187** findings tree-wide. Crossed against
`scripts/rootfs-bin-manifest.txt`, exactly four are on the image:

| bin | takes a path? | note |
|---|---|---|
| `patch` | **yes** — `-i FILE`, `-o FILE` | the sharpest case; it writes files |
| `diff` | **yes** — two operands | also renders the name into a `--- path` header |
| `ps` | no | dies only on a non-Unicode option value |
| `logger` | no | no file handling at all; dies on a non-Unicode *message* |

The other 183 findings are in `userspace/*` crates that the manifest does not
stage, so nothing on the image reaches them.

### The fix is getopt, not a hand conversion

I started converting `patch` in place and stopped, because the hard part has
already been solved once. `--input=<path>` cannot be handled with
`to_str()`: if the *path* is not Unicode then the whole argument fails to
decode, and the option silently stops matching. It needs a byte-level split,
and `coreutils::getopt` already does exactly that
(`getopt.rs:865`, `bytes.strip_prefix(b"--")`), over `&[OsString]`.

That is also what the detector's own header says, as a measured correlation
rather than a preference: *"of the 35 bins already clean, 24 go through
`getopt`; of the 49 dirty ones, **none** do. A bin that parses options through
the shared module never had a reason to reach for `String`."*

So the work is: route each of the four through `getopt`, hold `patch_file` /
`output_file` / `diff`'s two operands as `OsString` to the syscall, and render
a name only where it is printed. `diff` carries one extra decision — GNU writes
the operand **raw** into the `--- path` header, so that header has to be
emitted as bytes rather than `format!`ed, and quoting it instead would be
inventing a format.

### Why this is written down rather than half-done

The conversion is a real refactor of a file-writing utility, and the one
subtlety in it — the encoding boundary after `--input=` — is the kind that
produces a silently wrong path rather than a compile error. The measurement is
the expensive part and it is now done: four bins, named, with the mechanism
identified and the one non-obvious case called out.
