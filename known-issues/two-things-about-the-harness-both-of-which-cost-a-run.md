## Two things about the harness, both of which cost a run

**`env` prints its own environment, and its environment contains `PATH`.** Every
other harness here reaches its subject through `$bindir/ours/NAME` or
`$bindir/gnu/NAME` — two *different* directories, which are the whole of `PATH`
for one invocation. For this subject that would hand the two sides different
`PATH` values by construction, so every case that dumps the environment differs
on the harness's own scaffolding. "Normalising `PATH` away" would have hidden a
difference in the variable most worth comparing. Instead there is **one**
directory whose single entry is re-pointed between runs: identical `PATH`,
identical `argv[0]`, nothing filtered. *A harness may not put its own identity
into the subject's input* — visible here only because `env` is the program whose
job is to show you that input.

**The first run reported "59 passed, 0 differed" having compared nothing.** The
fixed environment was built by a helper and expanded unquoted, so
`WITH_SPACE=a b` split in two and the outer `env` read `b` as the command to
run, with `env <case args>` as its arguments. Every case failed identically on
both sides, which reads as agreement.

**The only thing in the output that said so was the `xfail` pair.** `--help` and
`--version` are *required* to differ, because that text is ours; they came back
`XPASS`. A harness needs at least one case that must fail for the same reason a
gate needs a refusal probe — without it, "everything agreed" and "nothing ran"
print the same line.

**14 -> 13 (2026-09-11): `hostname`, on a thin margin and one asymmetry.**
`scripts/hostname-diff.sh`, written today, 61 cases against net-tools 3.23.

**coreutils 7 passed, 50 differed. The standalone 3 passed, 54 differed.**

**Say the honest thing first: neither half is usable, and four cases is not a
verdict.** This is nothing like `uname`'s 71 to 43. It is recorded as a
retirement because §1005 makes `coreutils` the home and nothing here shows it to
be the *worse* half — not because the measurement settles the programs.

**The one asymmetry that is worth more than the count.** `hostname -s newname`:

    net-tools   usage message, rc=255 -- a display flag and a set operand
                cannot be combined
    standalone  prints `Logoplex3`, rc=0 -- the operand is silently ignored

A user typing that to set the name gets a success, a printed hostname, and an
unchanged system. `coreutils` refuses it. That is the difference between the two
halves that has consequences, and it is the shape this tree keeps finding: a
step that reports success is not evidence it did anything.

**What BOTH halves get wrong, which is the more useful output of the harness**
— filed as `B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS`:

| | coreutils | standalone | |
|---|---|---|---|
| domain resolution | 13 | 14 | `-d` answers `attlocal.net` where net-tools answers `localdomain`; `-f` likewise. net-tools resolves through nsswitch, so `/etc/hosts` (`127.0.1.1 Logoplex3.localdomain`) wins; ours takes the search domain from the resolver and never consults `/etc/hosts`. Both exit 0, so a script asking for the FQDN gets a confident wrong answer. |
| missing options | 19 | 16 | `-a`/`--alias`, `-y`/`--yp`/`--nis`, `-A`/`--all-fqdns`, `-b`/`--boot` are absent from both. |
| `-F FILE` | 16 | 23 | reading a name from a file disagrees on every fixture — empty, blank, spaced, two-line, commented. |
| `(os error 13)` | ⊂ | ⊂ | `cannot write /proc/sys/kernel/hostname: Permission denied (os error 13) (are you root?)` where net-tools says `you must be root to change the host name`. |

**A note on the harness, because this subject is unlike every other one here:
`hostname` WRITES TO THE MACHINE.** `hostname foo` sets the system's name and
there is no dry-run flag. As an ordinary user every such call is refused, which
is why the operand cases above are comparisons at all. Run as root they would
stop being comparisons and become edits — twice each, on a host two other lanes
are using — so the harness checks `id -u` and refuses outright. That guard is
specific to this one subject, not a rule for the family.

**15 -> 14 (2026-09-11): `uname`, with a new harness.**
`scripts/uname-diff.sh`, written today: 109 cases, and close to exhaustive
rather than representative, because `uname` has no input and nine flags so its
whole behaviour is a function of `argv` and the host.

**coreutils 71 passed, 10 differed. The standalone 43 passed, 38 differed.**
28 cases differ on purpose on both sides.

**Those 28 matter to the numbers and are worth explaining.** `uname -o` prints
`SlateOS` where GNU prints `GNU/Linux`, which is the *correct* answer — this is
not GNU/Linux — and the operating-system field rides along in `-a` and in every
pairing that includes `-o`. Counted as failures they were 36 and 64; marked as
intended divergence they are 10 and 38. The cases are still run and still
compared, so if that string ever changed to match GNU the harness would report
an XPASS rather than going quiet.

The standalone's 38 include the two families below plus 21 more, and it also
gets `-p` and `-i` wrong on their own.

**What the surviving half still gets wrong — 10 cases, two families, one
cause.** Both are recorded as `B-COREUTILS-UNAME-PARSES-ITS-OWN-OPTIONS`:
`uname.rs` hand-rolls its option parsing instead of using `coreutils::getopt`,
so it re-implements — differently — what the shared module already gets right.

**16 -> 15 (2026-09-11): `sha256sum`, found by fixing the survey's own column.**
This pair was listed as "no harness -- write one" for weeks. It has had one all
along: `digest-diff.sh` covers `md5sum` and `sha256sum` together through
`DIFF_BINS`, and `interleave-diff.sh` covers it too. The column looked for a
`sha256sum-diff.sh` and found none — an enumeration with one entry per instance,
missing the next instance silently, for the fourth time in this tree.

`PROG=sha256sum bash scripts/digest-diff.sh`, subject confirmed by `--version`
on every run because of
`TD-B-A-FAMILY-HARNESS-CANNOT-BE-AIMED-AT-ONE-HALF-OF-A-PAIR`:

**coreutils 113 passed, 0 differed. The standalone 39 passed, 74 differed.**

The standalone's `--check` mode is the bulk of it: on a file with no valid
checksum lines it answers `WARNING: 1 line(s) are improperly formatted` plus
`no file was verified`, where GNU says `a: no properly formatted checksum lines
found` — GNU names the file, which is the whole point of the message when
several are being checked.

Worth noting what this pair is NOT: the survey had it at 210 lines against 1538
until `dup-bins-survey.py` was taught to count shared modules, and that reading
made `coreutils` look like a stub. It is 5067 with `digest.rs` counted — a port
of upstream's `digest.c` shared with `md5sum` — and it passes 113 of 113.

**17 -> 16 (2026-09-11): `tar`, whose archives GNU cannot read back the same.**
`DIFF_PKG=tar bash scripts/tar-diff.sh`:

**coreutils 245 passed, 0 differed. The standalone 18 passed, 227 differed.**

This is the last of the sixteen pairs that had a harness, and the worst kind of
failure for an archiver: the archives it writes are not the archives it thinks
it wrote.

| | cases | defect |
|---|---|---|
| **GNU reads our archive differently** | 181 | `tar -tvf` on an archive the standalone created lists `-rwxr-xr-x 0/0` where GNU's own archive lists `-rwxr-xr-x inhahe/inhahe`: the `uname`/`gname` header fields are **left empty**, so every archive loses its owner names and GNU falls back to numeric ids. For a fifo, GNU listing our archive prints **nothing at all** — the entry is not in there. |
| **every archive differs in `mode`** | 27 | byte 102 of block 0, which is the header's mode field, on a plain file, an empty file, a directory, a whole tree. Not an edge case: **every archive it creates has the wrong permission bits in it.** |
| **fifos are skipped on extract** | 13 | `tar: x/p: unsupported type flag '6', skipping` — type flag 6 is a FIFO. GNU extracts it; the standalone silently omits it **and exits 0**, so a restore quietly loses every named pipe in the archive. |
| name field wrong | 4 | a dangling symlink and a fifo are written with a different `name` field than GNU writes. |
| **PANIC on a non-UTF-8 name** | 2 | `tar -cf X <name that is not UTF-8>` aborts with `thread 'main' panicked at library/std/src/env.rs`, **exit 134** — the same `std::env::args()` unwrap that decided `xargs`. `design.txt` says a path may hold every byte except `/` and NUL, and `tar` is the program whose entire job is preserving names. A `-C` argument that is not UTF-8 panics too. |

**Every harness-backed pair is now settled.** Sixteen had one; all sixteen went
to coreutils, and not one was close. The remaining 16 pairs have no harness, so
nothing further should be deleted until one is written — the survey's counts
are a ranking, not evidence, and this file records five occasions when they were
wrong.

**18 -> 17 (2026-09-11): `sed`, which cannot parse a bracket expression.**
`DIFF_PKG=sed bash scripts/sed-diff.sh`:

**coreutils 449 passed, 0 differed. The standalone 103 passed, 346 differed.**

| | cases | defect |
|---|---|---|
| **a delimiter inside `[...]`** | ⊂135 | `sed 's/[/]/:/g'` — the classic way to replace a slash — is `unterminated character class`. A `/` inside a bracket expression is not a delimiter, and getting that wrong breaks every expression that matches a path. `s/[^/]*$/LAST/` fails the same way. |
| refuses what GNU accepts | 135 | the above, plus `y/ab/XY/`, where the escapes are never decoded so the two halves are measured as 8 against 2 and rejected for unequal length. |
| **empty-match replacement** | ⊂68 | `s/a*/-/g` on `foo bar` gives `--------` — **the line replaced wholesale** — where GNU interleaves. Identical to the `gsub` defect in the standalone `awk` retired an hour earlier, which is some evidence about where both came from. |
| **the Nth-match flag** | ⊂68 | `s/o/0/2g` replaces from the *first* match; the `2` means start at the second. |
| exits 0 where GNU refuses | 51 | `sed 's/a'` — an unterminated `s` command — **runs**, and deletes the `a`. So does a trailing backslash, and `\c` recursive escaping. |
| wrong exit status | ⊂80 | GNU distinguishes 1 (usage), 2 (cannot read an input), 4 (cannot open a script or `w` target). The standalone answers 1 for all of them, so a caller cannot tell a bad script from a missing file. `2q5` — quit with status 5 — is `expected command`. |
| **stops at the first missing file** | ⊂10 | `sed s/a/A/ abc.txt nosuch.txt def.txt` prints `abc`'s output and stops; GNU reports the missing file and **still processes `def.txt`**. Operands after a bad one are silently dropped. |
| `w` unimplemented | ⊂80 | `sed -n w FILE` is `unknown command: 'w'`. |
| refuses non-UTF-8 | 2 | |

**19 -> 18 (2026-09-11): `awk`, which never finishes.**
`DIFF_PKG=awk bash scripts/awk-diff.sh`. **coreutils: 171 passed, 0 differed,
13 differ on purpose.** The standalone's run has no pass count, because it
never finished:

    awk 'NR == 1 {getline; print "got", $0} {print "main", $0}'

fed three lines, GNU prints `got b` / `main b` / `main c` and exits 0. The
standalone **hangs forever** — killed at a 5-second bound with no output, twice,
having also been left running for 35 minutes by an earlier attempt. `getline` is
one of awk's basic constructs, and a hang is worse than a crash: no diagnostic,
no exit status, and a process nobody notices.

In the cases it reached before that, **21 differed**, and they are not edge
cases:

| construct | GNU | standalone |
|---|---|---|
| range pattern `/b/,/c/` | `b` `c` | **parse error** — `unexpected token in expression: Comma` |
| `{NF = 2; print}` | `alice 30` | `alice 30 red` — assigning `NF` does not rebuild the record |
| array as a function parameter | `set` | *(empty)* — **arrays are not passed by reference**, so no awk program that fills an array in a function works |
| `gsub(/x*/, "-")` on `Alpha1` | `-A-l-p-h-a-1-` | `-------` — **the line is replaced wholesale**; empty-match handling destroys the data |
| bare `length` | `6 6 2` | ` 6 2` — `length` with no argument yields nothing |
| `BEGIN {FS = ""}` on `a` | `1 a` | `3 ` — a one-character line reported as three fields |
| `length("héllo")` | `5` | `8` |
| `atan2(1, 1)` | `0.7854` | `undefined function: atan2` |
| `ORS = "\0"` | NUL separators | the two characters backslash-zero |

Also wrong: `RS = ""` paragraph mode, `CONVFMT`, `OFMT`, strnum comparison,
`split` with a regex, `sub` with an escaped `&`, the arithmetic operators, and
`substr`/`index`/`toupper` on non-ASCII.

*A note on the run, since it cost an hour.* No `*-diff.sh` harness bounds an
individual case, and `run-timeout.py` — which exists for exactly this — does not
help here either: the harness re-execs into WSL, so the hung processes are Linux
side and outside the Windows Job Object that runner relies on. Killing the
Windows-side job left `awk` running in WSL for 35 minutes. Recorded as
`TD-B-DIFF-HARNESSES-HAVE-NO-PER-CASE-BOUND-AND-ORPHAN-ACROSS-WSL`; the fix is
a bound inside the harness, on the far side of the boundary, rather than around
it.

**WHY THE SURVEY KEPT SAYING "STANDALONE AHEAD", AND IT WAS NOT BAD LUCK
(2026-09-11, fixed).** Every entry above that records an inverted verdict has
the same cause, and it is structural rather than statistical.

`dup-bins-survey.py` counted **one file** of `coreutils` — `src/bin/<name>.rs`
— against the standalone's **entire crate**. `coreutils` deliberately factors
shared behaviour into modules; the standalone crates duplicate it inline. So
the comparison read a factored implementation's *leaf* against a copy-pasted
one's *whole*.

`sha256sum` shows it plainly. The bin was counted at 210 lines against 1538 —
a rout. It is 210 lines **because** `--check`, the option table, the three
checksum-file formats, the name escaping and the exit statuses live in
`digest.rs`, 1363 lines, a port of upstream's `digest.c` shared with `md5sum`.
Counted properly the row is **5067 against 1538**, and the option tally goes
from 0/14 to 4/4.

The survey's own docstring had always said the scrape "cannot see an option
that is parsed by a shared helper". What nobody drew is that **the blindness is
one-sided**, and therefore a bias with a direction rather than noise. Every
`tee`, `dd`, `date`, `ps`, `uptime` style verdict was produced by it.

Fixed by following `coreutils::<mod>` from the bin and `crate::<mod>` from each
module reached, to a fixed point. **The corrected table is nearly the opposite
of the old one**: `coreutils` is the larger half on 17 of the 19 remaining
pairs — `date` 257→1913, `uptime` 231→1887, `ps` 351→2007, `uname` 681→2337,
`hostname` 1075→2731, `env` 767→4261, `free` 1901→5395.

**Two pairs are now isolated and deserve the attention the other seventeen were
absorbing: `diff` (414 vs 1541) and `patch` (892 vs 2183).** Their numbers did
not move at all, because those two coreutils bins reach no shared modules. They
are genuinely the thinner half, they are the pairs where deleting the
standalone could destroy the better program, and neither has a harness. Do not
touch either without writing one.

**22 -> 21 (2026-09-11): `df`, which passes NOTHING and colours a pipe.**
`DIFF_PKG=df bash scripts/df-diff.sh`:

**coreutils 174 passed, 0 differed. The standalone 0 passed, 174 differed** —
every case in the suite, which no other pair has managed.

Three defects, each enough on its own:

| | defect |
|---|---|
| **most filesystems are missing** | GNU lists 35 mounts on this host; the standalone lists **five**. Everything mounted `none`, `rootfs` or `tmpfs` is absent — `/dev`, `/dev/shm`, `/run`, `/run/lock`, `/run/user`, every bind mount. `df` exists to answer "where has my disk gone", and it is not reporting most of the places it could have gone. |
| **ANSI colour written to a pipe** | the `Use%` column is wrapped in ESC-bracket-digits-m **on non-terminal output** — confirmed directly, 2 ESC bytes in `df / \| od -An -c`. Colour has to be gated on the output being a terminal; ungated, every pipeline that reads `df` gets control sequences in the middle of the field it is parsing. |
| duplicated rows | `/dev/sdd` is listed three times, for `/`, `/mnt/wslg/distro` and `/snap`, where GNU lists the device once against `/`. |

The column widths differ too, but that is the least of it.

**23 -> 22 (2026-09-11): `xargs`, which PANICS on a non-UTF-8 argument.**
`DIFF_PKG=xargs bash scripts/xargs-diff.sh`:

**coreutils 334 passed, 0 differed. The standalone 133 passed, 201 differed.**

This one contains the most serious single defect the whole §1005 campaign has
found.

| | cases | defect |
|---|---|---|
| **PANIC on a non-UTF-8 argv** | **24** | `printf 'a b\n' | xargs argv café` — with `café` in Latin-1 — aborts with `thread 'main' panicked at library/std/src/env.rs:878: called Result::unwrap()`, **exit 134**. It is `std::env::args()` unwrapping, and it is a crash rather than an error. `xargs`'s entire job is handing arbitrary bytes to another program; GNU passes the byte through and exits 0. This is CLAUDE.md self-review item 7 and the `unwrap_used` lint in one place, in the program least entitled to assume its input is text. |
| **`\v` and `\f` are not whitespace** | **40** | GNU splits arguments on vertical tab and form feed; the standalone keeps them inside the argument, so `\013a b` yields the argument `\va` instead of `a`, and `\013\014 a` yields two arguments where GNU yields one. Exit 0 both ways — **the executed command silently receives different arguments**. |
| `-E` / `--eof` unimplemented | 69 | the logical-EOF marker, the option that stops `xargs` at a sentinel line. |
| message shape | 42 | `unterminated single quote` for GNU's `unmatched single quote; by default quotes are special to xargs unless you use the -0 option` — GNU's sentence names the fix, and the exit status differs too (125 against 1). |
| accepts what GNU refuses | 11 | a quote left open across a newline is accepted as a literal; and `-s 25` with a 30-byte argument **runs anyway** where GNU refuses with `argument line too long` — the one option whose whole purpose is to impose a limit. |
| `(os error 2)` | 15 | |

**The panic and the `-s` overrun are the two that matter beyond this pair.**
A tool that aborts on a byte it cannot decode is worse than one that errors,
because exit 134 is indistinguishable from the child having been killed; and a
size limit that is not enforced silently hands the kernel the `E2BIG` the
option exists to prevent.

**24 -> 23 (2026-09-11): `split`, whose `-C` is not implemented but exits 0.**
`DIFF_PKG=split bash scripts/split-diff.sh`:

**coreutils 207 passed, 0 differed. The standalone 65 passed, 142 differed.**

| | cases | defect |
|---|---|---|
| **`-b` size suffixes** | **46** | `-b 1b`, `-b 1KB`, `-b 1KiB` are all `invalid number of bytes`. Suffixed sizes are the normal way to call `split`, and this is the same defect family that decided `dd` — a size operand that accepts only a bare integer. |
| **`-C` produces the wrong files, exit 0** | **22** | `split -C 2` means *at most 2 bytes of whole lines per file*. GNU writes `xaa`..`xak` accordingly. The standalone writes one file per input record regardless of the number — `-C 2`, `-C 3` and `-C 4` all produce the identical five files. The option is accepted, ignored, and the run succeeds. |
| refuses what GNU accepts | 20 | `split -6` and `-1` (numeric shorthand for `-l`), and `-x` (hex suffixes). |
| message shape | 38 | `invalid number of -n: '0'` for GNU's `invalid number of chunks: '0'`. |
| accepts what GNU refuses | 11 | `--numeric-suffixes=abc` and `=-1` are `invalid start value for numerical suffix` in GNU and are silently accepted here; `--numeric-suffixes=98` should exhaust the suffix space after `x99` and instead keeps going. |
| `(os error 2)`, and a wrong diagnosis | 5 | `--additional-suffix=a/b` is reported as `No such file or directory` where GNU says `invalid suffix 'a/b', contains directory separator`. The message sends you to look for a missing file when the argument is the problem. |

`-C` is the entry worth keeping: an option that is parsed, accepted, silently
ignored, and then exits 0 is indistinguishable from a working one until someone
looks at the file sizes.

**25 -> 24 (2026-09-11): `cmp`, which names the wrong file as truncated.**
`DIFF_PKG=cmp bash scripts/cmp-diff.sh`:

**coreutils 141 passed, 0 differed. The standalone 54 passed, 87 differed.**

| | cases | defect |
|---|---|---|
| **the EOF message names the wrong file** | ⊂61 | `cmp a short`, where `short` is the shorter file: GNU says `EOF on short after byte 4, line 1`; the standalone says `EOF on **a** after byte 4, in line 2`. It names the file that did *not* end, and gets the line number wrong. The whole content of that diagnostic is *which* file ran out, and it is the opposite of the truth. |
| message shape | 61 | the above, plus `in line N` for GNU's `line N` throughout. |
| option unrecognised | 13 | `-c` (`--print-chars`) and long-option abbreviations like `--verb`. |
| refuses what GNU accepts | 7 | `-i 1T` (a size suffix on `--ignore-initial`); and a repeated `-n 3 -n 10`, where GNU takes the last and exits 0 while the standalone reports a difference. |
| `(os error 2)` | 6 | |
| accepts what GNU refuses | 1 | `-i 9223372036854775808` overflows silently and exits 0; GNU refuses it as an invalid value. |

*Harness note:* the standalone run reported `2 NO LONGER differ (update the
harness)`. Those two expected-difference entries are correct for the surviving
coreutils half and were deliberately left alone — the harness documents the
program the tree ships, not the one being deleted.

**26 -> 25 (2026-09-11): `tsort`, which splits tokens on a carriage
return.** `DIFF_PKG=tsort bash scripts/tsort-diff.sh`:

**coreutils 87 passed, 0 differed. The standalone 18 passed, 69 differed.**

| | cases | defect |
|---|---|---|
| loop diagnostic shape | 39 | GNU prints a header naming the file and then one line per node — `tsort: cyc2.txt: input contains a loop:` / `tsort: a` / `tsort: b`. The standalone prints a full sentence per node (`tsort: a: input contains a loop`) and **never names the file**, so with several operands you cannot tell which one has the cycle. |
| **`
` splits a token** | **5** | on `a
b x`, GNU reads two tokens (`a
b` and `x`) and succeeds; the standalone splits at the CR, counts three, and refuses with `input contains an odd number of tokens`. Legitimate input rejected — and a CRLF file is the ordinary way to meet a CR. |
| accepts what GNU refuses | 3 | `tsort -h` prints a usage message and exits 0; GNU rejects `-h` as an invalid option. An accidental `-h` therefore looks like a successful sort that produced no edges. |
| `(os error 2)` | 3 | including `tsort ''`, where GNU quotes the empty operand (`tsort: '': No such file…`) and the standalone renders it as nothing at all: `tsort: : No such file or directory (os error 2)`. |
| **refuses non-UTF-8** | 2 | fifth consecutive standalone. |
| a different order | 17 | both exit 0 and the orders differ (`a b c d` against GNU's `a c b d`). Stated carefully: for these inputs both orders are **legal** topological sorts, so this family is not evidence of a bug — it is evidence that the two implementations are not interchangeable, which is the question §1005 asks. The coreutils half matches GNU byte for byte on all 87. |

*Also worth recording:* the standalone identifies itself as `tsort (Slate OS
coreutils) 0.1.0` — it claims to be the very build it is the duplicate of. The
two halves are told apart by a space and a version number. That mattered here,
because `--version` is what was used to prove each run had the right subject.

**27 -> 26 (2026-09-11): `paste`, which ignores an empty delimiter.**
`DIFF_PKG=paste bash scripts/paste-diff.sh`:

**coreutils 202 passed, 0 differed. The standalone 144 passed, 58 differed.**

| | cases | defect |
|---|---|---|
| **`-d ''` silently becomes TAB** | **14** | an explicitly empty delimiter means *join with nothing* — GNU emits `a1b1`. The standalone falls back to its default and emits `a1	b1`, and appends a trailing TAB where a file has run out (`a3	`). Exit 0 either way. Asking for no separator and getting the default one back is the worst possible answer, because the request was explicit. |
| `(os error 2)`, and output before the error | 18 | `paste a.txt nosuch.txt`: the standalone prints all of `a.txt` and *then* reports the missing file; GNU opens every operand first and prints nothing. Output already written cannot be taken back, so a failed run leaves a partial file behind. |
| long-option abbreviation | 13 | `--delim=,` and `--d ,` are `unrecognized option`. |
| accepts what GNU refuses | 7 | a delimiter list ending in an unescaped backslash (`-d ''`, `-d 'a'`) is `delimiter list ends with an unescaped backslash` in GNU; the standalone accepts it and ignores it. |
| **refuses non-UTF-8** | 4 | `paste bad.txt` → `stream did not contain valid UTF-8`. Fourth consecutive standalone with this. |
| message shape | 2 | `option '-d' requires an argument` where GNU says `option requires an argument -- 'd'`. |

**28 -> 27 (2026-09-11): `wc`, which does not align its columns.**
`DIFF_PKG=wc bash scripts/wc-diff.sh`:

**coreutils 116 passed, 0 differed. The standalone 43 passed, 73 differed.**

| | cases | defect |
|---|---|---|
| **no column alignment** | **34** | GNU right-aligns every count in a fixed-width field — `      2       3       6`. The standalone prints `2 3 6`. That is `wc`'s entire output format, it is wrong on every single invocation, and it is invisible to any comparison that normalises whitespace. |
| long-option abbreviation | 24 | `wc --lin` and `wc --w` are `unrecognized option`; GNU accepts any unambiguous prefix. |
| `(os error 21)`, and a row that vanishes | 11 | `wc adir` — GNU prints the diagnostic **and** a `0 0 0 adir` row, and still prints a `total` line for the other operands. The standalone prints the diagnostic only, so the failed file disappears from the table and the total silently omits it. |
| accepts what GNU refuses | 3 | a zero-length name in `--files0-from` (`invalid zero-length file name`), and `--files0-from` combined with a file operand (`file operands cannot be combined with --files0-from`). |
| missing second line | 1 | `Try 'wc --help' for more information.` |

The alignment family is the one to note: 34 cases where both sides exit 0, the
numbers are identical, and the bytes are not. `wc` output is read in columns.

**29 -> 28 (2026-09-11): `join`, and all three "close" pairs have now come
apart.** `DIFF_PKG=join bash scripts/join-diff.sh`, subject confirmed by
`--version`:

**coreutils 305 passed, 0 differed. The standalone 126 passed, 179 differed.**

| | cases | defect |
|---|---|---|
| **attached short-option arguments** | **64** | `-a1`, `-a2`, `-v1`, `-t:`, `-j1` are all `unrecognized option`. These are not exotic spellings — `join -a1 x y` is how the option is normally written, and GNU accepts the attached and detached forms alike. One getopt gap costs a third of the suite. |
| both refuse, different message | 59 | including several where the *diagnosis* is wrong: `join -o 1.1 a b c` is reported as `expected 2 file operands, got 3` where GNU says `invalid file number in field spec`. |
| accepts what GNU refuses | 17 | `-e X -e Y` (`conflicting empty-field replacement strings`) silently takes the last; `-j 1 -1 2` (`incompatible join fields 0, 1`) silently proceeds and prints a join nobody asked for. |
| refuses what GNU accepts | 17 | `-o '1.1 2.2'` — a blank-separated output-field list, which is the standard `-o` syntax — is `invalid field number`. |
| **both exit 0, output differs** | **12** | the silent ones. `-o 1.2 -o 2.2`: GNU **accumulates** repeated `-o` and prints `2 x`; the standalone keeps only the last and prints `x`. `-o auto` on ragged records emits a different number of fields than GNU. `-e X -o auto -a 1` omits the missing field instead of filling it with `X`, shifting every column after it. And on a line with trailing blanks, GNU keeps the empty final field the blanks produce (`a 1 2  x`) where the standalone drops it (`a 1 2 x`). |
| `(os error N)` | 8 | `join: nosuch.txt: No such file or directory (os error 2)`. |
| **refuses non-UTF-8** | 2 | `join bad1.txt bad2.txt` → `read error: stream did not contain valid UTF-8`, exit 1, no output. GNU joins the bytes and succeeds. |

**Three for three on the non-UTF-8 refusal** — `expand`, `comm`, `join`, same
wording, same whole-file refusal, on three programs whose job is to move bytes
around rather than to read them. It is now safe to predict for the remaining 28
and to stop being surprised by it.

**All three of the survey's "close — read both" pairs have now been measured,
and all three were landslides**: `expand` 126 differences, `comm` 93, `join`
179. "Close" was a statement about the two *line counts*, and the line counts
were close — 873 vs 712, 1120 vs 700, 2102 vs 1173. What the survey cannot see
is that the smaller half is smaller *because a third of the options are
missing*. That makes "close" the least informative verdict it prints, not the
most balanced one: it is the one that most reliably conceals a landslide.

**30 -> 29 (2026-09-11): `comm`, the second "close" pair to come apart.**
`DIFF_PKG=comm bash scripts/comm-diff.sh`, subject confirmed by `--version`:

**coreutils 197 passed, 0 differed. The standalone 104 passed, 93 differed.**

| | cases | defect |
|---|---|---|
| `--total` missing | 49 | GNU's summary line (`1	1	2	total`) is unimplemented, so half the suite dies at `unrecognized option '--total'`. |
| missing second line | 25 | on unsorted input GNU prints `comm: file 1 is not in sorted order` **and** `comm: input is not in sorted order`; only the first is printed. |
| **empty `--output-delimiter`** | **4** | `--output-delimiter=` means **NUL** to GNU, which emits `\0` separators. The standalone emits *nothing*, so columns 1, 2 and 3 become indistinguishable — the output stops carrying the answer. Both exit 0. |
| accepts what GNU refuses | 4 | a repeated `--output-delimiter` is `comm: multiple output delimiters specified` in GNU; the standalone takes the last one and exits 0. |
| `(os error N)` | 7 | `comm: nosuch.txt: No such file or directory (os error 2)`. One of these also reports the *wrong* failure — it opens the files before validating the options, so a doubled delimiter on two missing files is reported as the missing file. |
| **refuses non-UTF-8** | 2 | `comm bad1.txt bad2.txt` → `read error: stream did not contain valid UTF-8`, exit 1, no output. GNU compares the bytes and succeeds. |
| **false disorder alarm** | 2 | `comm dis.txt dis.txt` — the same unsorted file twice — exits 1 with two order complaints. GNU exits 0: comparing a file against itself makes every comparison equal, so no disorder is ever observed. The output bytes are identical; only the verdict differs. |

**The non-UTF-8 refusal has now appeared in two consecutive standalones**, with
the same wording, on programs that have no business decoding their input:
`expand` places whitespace and `comm` compares lines. That is worth recording as
a property of the standalone family rather than as two coincidences — it
predicts the same defect in the remaining 29 and it is exactly the defect
CLAUDE.md item 7 names. The coreutils half passes these cases.

**31 -> 30 (2026-09-11): `expand`, the pair the broken knob was hiding.**
This is the pair that read as a **dead heat** — 216 passed both ways — until
`DIFF_PKG` was made to cross the WSL boundary (entry below). With the knob
working, `DIFF_PKG=expand bash scripts/expand-diff.sh`, each run's subject
confirmed by binary hash and `--version` rather than assumed:

**coreutils 216 passed, 0 differed. The standalone 90 passed, 126 differed.**

The survey had called this one "close — read both", and on its own terms it was
right: 873 lines against 712, two option names either side. Every one of the
126 is invisible to a line count.

| | cases | defect |
|---|---|---|
| `-t` specs wrongly **rejected** | 49 | `-t '1 3 5'` and `-t '1,3 5'` — blank-separated stop lists, which POSIX and GNU both accept — are refused as `invalid tab stop specification`. So is an empty `-t ''`. |
| **wrong column, exit 0** | **29** | With an explicit multi-stop list the text lands one column right of where GNU puts it: `-t 1,3,5` on a leading tab emits **two** spaces where GNU emits **one**. Both exit 0 and the output looks plausible. Placing text in a column is the entire job of this program. |
| `-N` shorthand rejected | 19 | `expand -4`, and `-1` … `-9`, `-16` — the historic spelling, still accepted by GNU — die with `invalid option -- '4'`. |
| long options | 11 | `--tab` (an unambiguous abbreviation of `--tabs`) is unrecognised; so are `--initial=4`, `--help=x`, `--version=x`. |
| **refuses non-UTF-8 input** | 2 | `expand badbytes.txt` → `stream did not contain valid UTF-8`, exit 1, **no output at all**. GNU passes the bytes through untouched. Same for a byte off a pipe. |
| accepts what GNU refuses | 5 | `-t 4 -t 2`, `-t 4 -t 4`, `-t +4,6`, `-t +2,+4`, `-t +2 -t +4` all exit 0. GNU refuses each: `tab sizes must be ascending`, `'+' specifier only allowed with the last value`. |
| `(os error N)` in diagnostics | 9 | `expand: nosuch.txt: No such file or directory (os error 2)`; `expand: .: Is a directory (os error 21)`. Rust's `io::Error` Display leaking into a user-facing sentence. |
| missing second line | 2 | `expand -it` and `expand --tabs` omit `Try 'expand --help' for more information.` |

**The non-UTF-8 refusal is the one that would have shipped a data bug.**
`expand` is a whitespace tool: it has no business decoding the bytes between the
tabs, and a Latin-1 file, a file with one stray byte, or anything binary-ish
comes back as an error with no output. That is CLAUDE.md self-review item 7 —
"never force UTF-8 on … pipe data" — as a whole-file refusal rather than as
`from_utf8_lossy` corruption. It cannot be seen by reading the option list,
which is why the survey scored this pair as close.

**The wrong-column family is the one that would have shipped quietly.** 29 cases
where both sides exit 0, neither prints anything, and the columns do not line
up. A harness comparing `od -An -c` byte for byte is the only reason they were
seen at all; any comparison that normalised whitespace would have called them
equal, and whitespace is the output.

**THE KNOB THAT SELECTS THE SUBJECT WAS NOT REACHING THE SUBJECT
(2026-09-11, fixed).** `DIFF_PKG=<pkg>` — the override every entry above uses
to point a coreutils harness at the standalone half instead — was being
**dropped at the WSL boundary**, and the run came back green anyway.

    DIFF_PKG=no-such-package-at-all ./scripts/expand-diff.sh
    216 passed, 0 differed, 2 differ on purpose

216 passing cases for a package that does not exist. `diff-wsl.sh` re-execs
every harness inside WSL, and environment variables do not cross that boundary
by themselves, so the re-exec rebuilt the environment from a written-out list
of four names — `OURS VERBOSE DIFF_GNU_DIR DIFF_GNU_CACHE`. `DIFF_PKG` was not
on it. The far side then applied its own default of `coreutils` and measured
the half nobody asked about.

**Why this is the worst possible failure for the §1005 work.** Both runs of a
pair measured the same binary, so every pair scores a **perfect tie** no matter
how far apart the halves really are — and a tie reads as "the two are
equivalent, delete either", which is the one conclusion that can delete the
better half. `expand` is the proof: it reported *216 / 216, a dead heat* both
ways, and with the knob fixed it reports **216 passed / 0 differed** for
coreutils against **90 passed / 126 differed** for the standalone.

**The seven retirements already made are NOT affected, and that is checked
rather than assumed.** A contaminated pair of runs measures one binary twice
and therefore *must* print the same numbers twice; the harnesses are
deterministic. Every retirement above recorded two different numbers — `dd`
339/0 against 8/331, `tee` 71/0 against 34/37, `uniq` 273/0 against 134/139 —
and no single subject can produce both halves of any of those. The asymmetry
itself is the evidence the swap happened.

**The general defect, which this tree has now hit four times:** an enumeration
that needs one entry per instance misses the next instance BY CONSTRUCTION, and
misses it silently. The fix does not add `DIFF_PKG` to the list; it removes the
list. Every `DIFF_`-prefixed name found in the environment now crosses, so a
knob added later crosses without anyone remembering. Taking the names from the
*environment* rather than from the shell is also exactly the right cut: a knob
a harness writes into itself is set again on the far side and need not travel,
while an operator's override exists only on this side and is lost if it does
not.

Guarded by `scripts/test-diff-forward.sh`, which is built on the two-probe
rule, because for a forwarded variable "the value crossed" and "the value was
dropped and the default did the same thing" look identical in green. So it sets
`DIFF_PKG` to values that *must* refuse — a package that does not exist, and a
package that exists with no such binary — and requires both to fail. It scores
4/4 against the fix and 2/4 against the old code, failing exactly the two
refusal probes.

*(Written POSIX, not bash: `diff-wsl.sh` declares `shell=sh` and two harnesses
are `#!/bin/sh`, so the obvious `${!DIFF_@}` — which works when tried, because
it is tried under bash — would have broken them. shellcheck said so.)*

**32 -> 31 (2026-09-11): `dd`, and the survey's second inversion.** Ranked
**"standalone ahead"** on 21 option names the standalone mentions and coreutils
does not. `DIFF_PKG=dd bash scripts/dd-diff.sh`:

**coreutils 339 passed, 0 differed. The standalone 8 passed, 331 differed.**

It passes eight of 339. Three defect families, sorted by what they cost:

| | cases | defect |
|---|---|---|
| **`bs=` operands** | **102** | `bs=1k`, `bs=1KB`, `bs=1x2`, `bs=2x3x4` all **fail with exit 1** where GNU succeeds. Size suffixes and multiplier products are core `dd` syntax — `dd bs=1M` is the commonest invocation of this program and it errors. |
| summary line | 195 | prints `11 bytes (11 B, 11 B) copied` where GNU prints `11 bytes copied`. GNU only adds the parenthesised sizes at 1000 bytes and up, and never prints the same rendering twice. |
| summary line | ⊂195 | `10000 bytes` renders as `(10 kB, 10 KiB)`; GNU says `(10 kB, 9.8 KiB)`. 10000 bytes is 9.77 KiB, so the IEC divisor is 1000 instead of 1024. |

**What it gets right is worth stating too.** In all 195 summary-only cases the
copied bytes and the exit code are identical — checked by splitting each case's
stdout from its stderr rather than assumed. `dd` copies correctly and *reports*
wrongly, in the line that is its entire feedback.

**Two inversions out of two tested.** `tee` and `dd` were the survey's only
"standalone ahead" verdicts put to a harness, and both were wrong — 37/71 and
331/339 against. The heuristic counts option names MENTIONED in the source; a
program can mention `bs` and not implement its suffixes. Ten such verdicts
remain untested (`date`, `diff`, `env`, `free`, `hostname`, `kill`, `logger`,
`patch`, `ps`, `sha256sum`, `uname`) and none of them should be acted on
without a differential — acting on that verdict deletes the better half.

**Still open — the proper fix.** One name, one program. For each of the
remaining 41: pick the implementation that is under test and maintained, make
sure nothing in the other is worth keeping (the standalone ones are older but
not uniformly worse — `stat` is 2845 lines against a 343-line stub), fold in
anything that is, and delete the loser so the name has exactly one producer.
Three names — `sha1sum`, `sha512sum` and `w` — have *no* `coreutils` bin at all
and exist only as extra personalities of the standalone `sha256sum` and `who`,
so they are gained by the move rather than merely relocated; deleting those two
crates without porting them first would remove three working commands. Verify
with a build that asks for everything at once and emits no `output filename
collision` warning:

```
cargo build --workspace --target x86_64-pc-windows-gnu 2>&1 | grep -c collision   # must be 0
```

**Why it has not shipped a wrong binary yet.** `scripts/create-ext4-rootfs.sh`
stages the fastpy-compiled ELFs, not these; nothing in the image build reads
`target/<triple>/debug/<name>.exe`. That is luck, not design — the moment
anything does, it inherits the coin flip.
