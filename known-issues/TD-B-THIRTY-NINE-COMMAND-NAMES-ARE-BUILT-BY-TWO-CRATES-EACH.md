## TD-B-THIRTY-NINE-COMMAND-NAMES-ARE-BUILT-BY-TWO-CRATES-EACH (lane B, 2026-09-10)

**In short:** thirty-nine command names are produced by two different crates in
this tree. `free`, `ps`, `df`, `sed`, `tar`, `wc`, `who`, `uname` and thirty-one
others each exist twice -- once as `userspace/coreutils/src/bin/<name>.rs` and
once as `userspace/<name>/`.

**Derived, not listed:** the set of `coreutils` bin targets intersected with the
`name =` of every other `userspace/*/Cargo.toml`. 84 coreutils binaries, 2762
other userspace crates, 39 names in both.

**It does not cut one way, which is why this is a question and not a chore.**
Two measured examples:

* **`free`** -- **SETTLED 2026-09-12: `userspace/free` is deleted.** The
  prediction below was written from the sources and was, for once, right --
  but it was confirmed by measurement before anything was removed, not
  instead of it. `scripts/free-diff.sh` pins `/proc/meminfo` inside a private
  mount namespace and compares both against procps-ng 4.0.4:
  **coreutils 48 passed / 0 differed; `userspace/free` 0 passed / 48
  differed.** The most lopsided pair yet measured.

  The original reasoning, kept because it is the only one of the five that
  the harness upheld: `coreutils`'s is a 1876-line transcription of procps-ng
  4.0.4, and its own module doc lists the defects of "the implementation this
  replaces": invented flags, wrong header widths, `shared` hardcoded to zero,
  and `used = total - free - buffers - cached` where upstream's is
  `MemTotal - MemAvailable`. `userspace/free` still had every one of them.

  **What the loser knew that the winner did not: nothing.** Its whole unique
  surface was one invented flag, `--json`, which procps-ng does not have and
  which nothing in this tree consumed. It was deliberately not ported --
  adding a flag upstream lacks would break the bug-for-bug property
  `free-diff.sh` asserts, which is the thing that made the pair decidable at
  all. Every *real* option it lacked, coreutils has: `--peta`, `--pebi`,
  `--si`, `--line`, `--committed`, `--version`. It also sat in
  `argv-utf8-baseline.txt` as `argv-as-string`.
* **`ps`** -- **SETTLED 2026-09-12: coreutils' wins, 12 to 0.** The entry
  below is kept because it was right when written and stopped being right four
  hours later, which is the more useful record.

      coreutils ps    12 passed,  0 differed, 15 differ on purpose
      userspace/ps     0 passed, 12 differed, 15 differ on purpose

  Same harness, same cases, opposite results. The reversal is not a change in
  the standalone: it is that coreutils' `ps` was given procps' column set and
  a parser that refuses what it cannot honour, both of which it was missing
  this morning. **A pair verdict is a statement about two implementations on a
  given day, and the losing half of this one was two fixable defects away from
  winning.**

  **The standalone scored 0 XPASS**, which is the sharper half of the result.
  It implements `-l`, `-u`, `-o`, `-p` and `--no-header` -- five real procps
  options coreutils' refuses -- and not one of them produced procps' output.
  Every case declared "not implemented here" for coreutils' *also* differed
  for the implementation that has it. This is `free` again: 23 options
  advertised against 2 is not 21 options that agree.

  **RETIRED 2026-09-12. `userspace/ps` is deleted; coreutils' `ps` is the one
  `ps`.** Final measurement before the delete: **22 passed, 0 differed, 10
  differ on purpose** for coreutils', against **0 passed, 12 differed** for the
  standalone on the same cases.

  **What the loser knew, measured option by option rather than counted.** Its
  nine options split three ways:

  | | |
  |---|---|
  | ported in first | `-p`, `--no-header`, `-u` |
  | real procps options, still missing here | `-l`, `-o`, `-t`, `--sort` |
  | **inventions** | `--reverse`, `--json` |

  `--reverse` and `--json` both draw `error: unknown gnu long option` from
  procps — checked, not assumed, because `free`'s standalone advertised
  `--json` too and it was the one thing that looked like a feature. The four
  real ones are genuine capability and are **not** ported from that crate:
  measured, its implementations of them do not match procps either (0 XPASS),
  so porting the code would import a second wrong rendering. They are written
  fresh against measurements, one at a time, and each is declared by name in
  `ps-diff.sh`. `todo.txt` carries the entry.

  `DIFF_PKG=ps bash scripts/ps-diff.sh` now fails loudly with "has no
  Cargo.toml anywhere under … — is DIFF_PKG right?", which is correct: there
  is no second `ps` to measure.

  **The original entry, from before the fixes:**

  `scripts/ps-diff.sh` pins the process table in a PID
  namespace with its own `/proc` and compares both against procps-ng:

      coreutils ps     0 passed, 26 differed
      userspace/ps     0 passed, 26 differed

  A tally says "tie". The content says otherwise, and this is why a harness
  that only counts is not enough:

      procps      UID          PID    PPID  C STIME TTY          TIME CMD
                  root           1       0  0 13:39 ?        00:00:00 ps -f
      standalone    UID      PID     PPID    C       STIME  TTY   TIME  CMD
                      0        1        0    0       13:36  ?  00:00:00  ps
      coreutils     UID   PID  PPID  STAT   TIME     CMD
                      0     1     0  R      00:00:00 ps -f

  The standalone has **the right column set** and gets the widths, the UID
  resolution (`0` where procps prints `root`) and the `CMD` content wrong.
  coreutils' has **the wrong columns entirely** -- no `C`, no `STIME`, no
  `TTY`, and a `STAT` column procps does not put there. One is a formatting
  problem; the other is a different report. So the old "the standalone plainly
  wins" verdict survives measurement, which is notable given it was reached
  from line counts that were themselves wrong by a factor of five.

  **Do not read "0 vs 0" as "neither is worth keeping."** Both fail against
  procps; only one of them is failing at the last step.

* **`ps` also ignored every option it did not know, and `check-argv-ignored`
  could not see it — FIXED 2026-09-12.** Measured: `ps -X` printed the default listing at exit 0
  where procps exits 1, and `ps --help` prints the process table. The parser
  strips one `-` and then walks the string a character at a time:

      match c { 'e' | 'A' => all_procs = true, 'f' => full = true, _ => {} }

  So `--help` is read as `-h -e -l -p`, the `e` matches, and any long option
  containing `e`, `A` or `f` silently turns that flag on. `--full` would set
  `-f` by accident and `--version` would set `-e`.

  `scripts/check-argv-ignored.py` reports **0 bins ignore argv** with an empty
  baseline, and it is not lying: `ps` *does* read `argv` and *does* honour
  `-e` and `-f`. The gate catches a program that ignores its command line
  **entirely**, which is the defect `uptime` had. It does not catch one that
  honours the options it knows and silently discards the rest — which is the
  same class of hole, one notch finer, and is the shape §1006 is about: a
  program that cannot do what it was asked should say so.

  **The fix:** long options are matched whole, an unknown short option returns
  `error: unsupported SysV option` and an unknown long one
  `error: unknown gnu long option`, both at exit 1 — procps' own messages,
  measured. `--help` prints procps' 170-byte help text, captured with
  `cat -A` rather than retyped, because `uptime`'s help was missing a leading
  blank line and a trailing reference line for exactly the reason that those
  are invisible when you retype instead of measure.

  **A test was holding the defect in place.** It was called
  `parse_unknown_silently_ignored`, and its comment read "Preserves previous
  behaviour — no error, no panic." It preserved `ps -Q` printing the process
  table at exit 0. That is the third time this week a test has certified a bug
  — after `uptime`'s six asserting an unmeasured `up …` format, and
  `ctest-hostname`'s asserting that a personality name was extracted correctly
  when nothing consumed it. **A test and a specification are the same artifact
  right up until someone measures the reference**, and nothing in the test
  itself says which one it is.

  **The gap in `check-argv-ignored.py` is left open deliberately.** Extending
  it from "reads argv at all" to "refuses what it cannot honour" is a much
  harder static question — it would have to know each program's option set —
  and the differential harnesses answer it directly for every program that has
  one. Recorded here rather than filed as a checker change, because the
  cheaper instrument already exists.

* **`uptime`** — **RETIRED 2026-09-12. coreutils 40 passed / 0 differed
  against the standalone's 20 / 20**, same cases, `scripts/uptime-diff.sh`.

  The standalone is the best-performing loser of the seven: it passes half the
  cases rather than none. What it fails is a pair of clusters, and both are the
  kind a from-scratch implementation gets wrong — every uptime at or past a day
  boundary (86400, 86460, 172800, 259200, 604800), and every user count except
  one (0, 2 and 12 all wrong, 1 right). Those are exactly the two places
  procps' own rules are counter-intuitive: `up 1 day, 0 min` rather than
  `1 day, 00:00`, and `0 user` SINGULAR.

  Its three unique options — `-r`, `--raw`, `--json` — are all inventions.
  Measured, not taken from the note that already said so: procps answers
  `invalid option -- 'r'` and `unrecognized option '--raw'` / `'--json'`.
  Nothing to port.

* **`vmstat`** — **RETIRED 2026-10-08, the day the pair came into being.**
  `coreutils/src/bin/vmstat.rs` arrived that day as a port of procps-ng
  4.0.4's `vmstat.c` over its library; `userspace/vmstat` had been written
  from the manual. Same harness, same cases, same fixture machines
  (`scripts/vmstat-diff.sh`, whose worlds change between readings so the
  interval lines are compared too):

      coreutils vmstat    191 passed,   0 differed,  7 differ on purpose
      userspace/vmstat      2 passed, 189 differed,  7 differ on purpose

  The standalone's two passes are `vmstat -Z 2>&-` and `vmstat 0 2>&-`:
  standard error closed, so neither side can print its complaint and both
  exit 1 -- agreement with nothing in it to compare.

  **What the loser knew that the winner did not: nothing.** Its one option
  procps lacks, `--json`, is an invention -- procps answers `unrecognized
  option '--json'`, measured. The rest of its surface was procps' options,
  rendered its own way: `-S k` and `-S K` swapped (procps' `k` is 1000 and
  `K` 1024, measured on the reference: 33626984 k against 32838852 K of one
  `MemTotal`), the first line's context switches divided by seconds of uptime
  where procps divides them by CPU ticks, `cache` without `SReclaimable`, and
  no `-p`, `-y`, `-V` or `-h`. It also sat in `argv-utf8-baseline.txt` as
  `argv-as-string`. Nothing to port.

* **`sysctl`** — **RETIRED 2026-10-08, the same way and the same day.**
  `coreutils/src/bin/sysctl.rs` is procps-ng 4.0.4's `sysctl.c` and the
  `procio.c` it reads and writes through; `userspace/sysctl` was written from
  the manual. `scripts/sysctl-diff.sh`, which runs both over a fixture
  `/proc/sys` and fixture configuration directories and compares what each
  left in the files as well as what each printed:

      coreutils sysctl    138 passed,   0 differed,  2 differ on purpose
      userspace/sysctl      6 passed, 132 differed,  2 differ on purpose

  Its six passes are the three plainest reads (`kernel.hostname`,
  `kernel/hostname`, `-n kernel.hostname`) and three runs with standard
  error closed or full. **What the loser knew that the winner did not:
  nothing procps has.** It searched `/sys/kernel` when `/proc/sys` lacked a
  key, which procps never does; it had `--search` (procps' `-r` is a regular
  expression, not a substring) and `--values-only`, and took a bare `help`;
  its `-q` meant "print no names" where procps' means "say nothing about a
  write", and its `-w` took the key and value as two words. It had no `-N`,
  `-b`, `-e`, `-r`, `-f`, `--system`, `--dry-run` or `--deprecated`. It also
  sat in `argv-utf8-baseline.txt` as `argv-as-string`. Nothing to port.

* **`hexdump`** — **RETIRED 2026-10-08, with `xxd` beside it.**
  `coreutils/src/bin/hexdump.rs` is util-linux 2.39.3's `hexdump.c` and its
  three `hexdump-*.c`; `coreutils/src/bin/xxd.rs` is vim 9.1.0016's `xxd.c`.
  `userspace/hexdump`, written from the manuals, was both -- `xxd` by its
  `argv[0]`, through the manifest alias `xxd = hexdump`. Each harness was run
  over the standalone too, as `xxd` under that name:

      coreutils hexdump     275 passed,   0 differed,  2 differ on purpose
      userspace/hexdump      30 passed, 245 differed,  2 differ on purpose
      coreutils xxd         949 passed,   0 differed,  4 differ on purpose
      userspace/hexdump     163 passed, 786 differed,  4 differ on purpose

  As `hexdump`, its passes are `-C` on a single file, alone or with standard
  output or error closed: `-C` was its *default*, where util-linux's is the
  two-byte hex dump, so a bare `hexdump FILE` differed on every fixture, and
  it had no `-e` or `-f` at all -- 103 of the 245. As `xxd`, it agreed where
  its layout happened to coincide with upstream's (a sixteen-byte file, an
  empty one, some `-c` widths) and where a revert had nothing to write; 248
  of the differences are `-r` cases, 83 are `-R`, 70 are `-i` and 57 are
  `-s`. It took options after its operands, ignored an output operand
  (`xxd -r dump out` wrote to standard output and left `out` alone, so it
  could not patch a file, which is what `-r` is for), and had none of
  `-a -b -C -d -e -E -n -o -R -u -v`. **What the loser knew that the winner
  did not:** `--json`, an invention. Nothing to port.

* **`look`** — **RETIRED 2026-10-08, the same way.** `coreutils/src/bin/look.rs`
  is util-linux 2.39.3's `misc-utils/look.c`; `userspace/look` was written
  from the manual. `scripts/look-diff.sh` searches word lists sorted as a
  dictionary and as bytes, unsorted ones and awkward ones, with every
  option:

      coreutils look       1229 passed,   0 differed,  2 differ on purpose
      userspace/look        595 passed, 634 differed,  2 differ on purpose

  Most of the 634 are one defect: the standalone read its file as UTF-8 and
  refused any file holding a byte that was not -- `look: words: stream did
  not contain valid UTF-8`, status 2 -- so every search of a list with one
  Latin-1 word in it failed outright. The rest: no `-a` and no `$WORDLIST`;
  `-t` cut *before* its character, where upstream keeps it, and cut every
  line the same way, where upstream cuts only the string (read from its
  source -- the UTF-8 refusal hid every `-t` case); an empty file searched
  as empty, silently, where upstream's `mmap` says `Invalid argument`; and a
  directory `Is a directory (os error 21)`, status 2, where upstream's says
  `No such device`, status 1. **What the loser knew that the winner did
  not:** nothing. Nothing to port.

* **`last`** — **RETIRED 2026-10-08, with `lastlog` beside it.**
  `coreutils/src/bin/last.rs` is util-linux 2.39.3's `login-utils/last.c`
  (and `lastb`, by its `argv[0]`); `coreutils/src/bin/lastlog.rs` is
  shadow-utils 4.13's `src/lastlog.c`. `userspace/last`, written from the
  manuals, was all three by its `argv[0]`, through the manifest aliases
  `lastb = last` and `lastlog = last`. Each harness was run over the
  standalone too -- as `lastlog` under that name:

      coreutils last        511 passed,   0 differed,  3 differ on purpose
      userspace/last         12 passed, 497 differed,  3 differ on purpose, 2 hung
      coreutils lastlog     177 passed,   0 differed,  0 differ on purpose
      userspace/last          1 passed, 176 differed,  0 differ on purpose

  As `last`, it listed the file oldest first where upstream reads it
  backwards and lists the newest first; wrote every logout with its date,
  where the short format has only `HH:MM`; listed ghost entries (a login with
  no user) as logins, ran the `-n` limit over the wrong end, and wrote user
  and host names as they are -- so a host holding an escape sequence reached
  the terminal, where upstream's `fputs_careful` writes `*[`. It had no
  `--time-format`, `-s`, `-t` or `-p` and no long option at all; a second
  `-f` replaced the first instead of adding a file; and it read `/dev/zero`
  forever (the hangs: one or two, by how long a run takes to time out).
  Its twelve passes are listings that select nothing -- a name or a tty no
  record has -- and three runs with standard output or error closed. As
  `lastlog`,
  it had no `-R` -- every chrooted case stopped at `unknown option: -R` -- its
  host column was 16 wide where upstream's is 42, and it listed only
  accounts with a record, where upstream lists every account and says
  `**Never logged in**`. **What the loser knew that the winner did not:**
  nothing util-linux or shadow-utils has. Nothing to port.

* **`wall`** — **RETIRED 2026-10-08, with `write` and `mesg` beside it.**
  `coreutils/src/bin/wall.rs`, `write.rs` and `mesg.rs` are util-linux
  2.39.3's `term-utils/wall.c` (with Ubuntu's fix for CVE-2024-28085, which
  escapes a command-line message too), `write.c` and `mesg.c`.
  `userspace/wall`, written from the manuals, was all three by its `argv[0]`,
  though only `wall` reached the image: no alias named the other two.
  `scripts/wall-diff.sh` runs both sides in a user and mount namespace over a
  fixture utmp and fixture terminals, and compares what each wrote into them:

      coreutils wall/write/mesg   240 passed,   0 differed,  3 differ on purpose
      userspace/wall               27 passed, 213 differed,  3 differ on purpose

  It read who was logged in from `/var/run/utmp.txt`, a text file of its own
  that nothing writes, and then wrote to every `/dev/pts/*` and `/dev/tty0`
  to `tty11` that existed, logged in or not; it named the sender from
  `/etc/users.yaml` and refused to send at all when it found nobody there
  (`cannot determine who you are ...; refusing`) -- every `wall` case here. It
  had no `-t`, wrapped nothing, and wrote a message's escape sequences to every
  screen as they were. As `write`, it looked the recipient up in the same text
  file, so every user was `not logged in`; as `mesg`, `y` set only the group's
  write bit where Ubuntu's sets the others' too, `n` exited 0 where upstream
  exits 1, and a `chmod` that failed went unreported. Its passes are `mesg`
  asked a question (`is y`, `is n`) or told `y` on a terminal that already
  had both bits, `write` to a user nobody is, and one `--help` with standard
  output closed. **What the loser knew that the winner
  did not:** refusing an anonymous broadcast. Upstream sends one as
  `<someone>` after a warning, and that stands: the banner then says plainly
  that the sender is unknown. Nothing to port.

* **`pstree`** — **RETIRED 2026-10-08.** `coreutils/src/bin/pstree.rs` is
  psmisc 23.7's `src/pstree.c`, its terminal handling through
  `userspace/terminfo` (ncurses 6.4's, ported for it). `userspace/pstree`,
  written from the manuals, called itself a multi-personality binary --
  `pstree` and "a `pgrep` variant with a tree view" -- though only `pstree`
  reached the image. `scripts/pstree-diff.sh` runs each case in a fresh pid
  namespace over a tree a fixture builds one process at a time, so both sides
  see the same pids:

      coreutils pstree      629 passed,   0 differed,  2 differ on purpose
      userspace/pstree        8 passed, 621 differed,  2 differ on purpose

  It put every child on a line of its own -- `├──` and four columns a level,
  in Unicode even through a pipe -- where upstream runs a chain along one line
  (`python3---alpha---worker`) and draws ASCII off a terminal; it compacted
  leaves but dropped whatever hung under a compacted subtree (`2*[group]`,
  its `leaf` gone), went on compacting under `-p`, which upstream's `-p`
  turns off, and showed no threads at all. It had none of `-s`, `-g`, `-S`,
  `-N`, `-t`, `-T`, `-Z`, `-G`, `-U` or `-C` (`pstree: unknown option: -s`);
  a user operand drew the tree of one process of the user's where upstream
  draws every top-most one; and a user nobody is was `pstree: user
  'nosuchuser' not found` where upstream says `No such user name:
  nosuchuser`. Its eight passes are two processes with no children drawn
  alone -- `pstree 7` and `pstree 9`, with and without `-p`, one line either
  way -- and four runs whose standard output was closed or full, where
  nothing written is seen. **What the loser knew that the winner did
  not:** `-k` (`--show-kernel`), kernel threads hidden unless asked for.
  psmisc has no such option and hides nothing. Nothing to port.

* **`tput`** — **RETIRED 2026-10-08.** `coreutils/src/bin/tput.rs`,
  `clear.rs` and `tset.rs` are ncurses 6.4's `progs/tput.c`, `clear.c` and
  `tset.c` (with `tabs.c` beside them, new), sharing `coreutils::ncurses` --
  `tty_settings.c`, `reset_cmd.c`, `clear_cmd.c` -- over `userspace/terminfo`.
  `userspace/tput`, written from the manuals, was `tput`, `clear`, `reset`
  and `tset` by its `argv[0]`, through the manifest aliases. Measured by
  `scripts/tput-diff.sh` on its `tput`, `clear` and `tset` cases (before
  `tabs` joined it), each side on a pseudo-terminal of its own:

      coreutils tput/clear/tset   957 passed,   0 differed,  9 differ on purpose
      userspace/tput              120 passed, 836 differed,  9 differ on purpose, 1 never ended

  It read no terminfo database: it carried a table of its own for five
  terminals (`xterm`, `vt100`, `linux`, `dumb` and `slateos`), so every
  other name was its idea of an xterm and every crafted entry the harness
  compiles was refused, and even the five were wrong where they mattered --
  `sgr0` `\E[0m` where xterm's is `\E(B\E[m`, `clear` without the
  scrollback's `E3`, a `longname` of its own invention with a newline after
  it. A boolean it did not list, `bw` among them, was an unknown
  capability, and an unknown capability exited 1 where ncurses exits 4.
  As `tset` and `reset` it took no options at all -- `tset -q` was
  `invalid option -- 'q'` -- and sent a hard reset of its own (`\Ec`, the
  screen cleared, the cursor shown) and then `Terminal reset to sane
  state.`, where ncurses sends the terminal's own init or reset strings
  and reports the erase, interrupt and kill characters it changed. As
  `init` it was plain `tput`, so `init -S` sat waiting on the terminal for
  commands -- the case that never ended -- where ncurses refuses `-S` from
  its other names. Its 120 passes are capabilities its table happened to
  hold as the database does, `cols`, `colors`, `bold` and `el` on the
  xterms among them. **What the loser knew that the winner did not:**
  nothing that ships.

* **`logger` had a separate bug, fixed 2026-09-12, independent of B-Q14.**
  Its parser ended in `_ => message_parts.push(arg)`, so an unrecognised
  option **became the message**: `logger -Q` logged the string `-Q` and
  exited 0 where util-linux prints `logger: invalid option -- 'Q'` and
  exits 1. Worse than `ps` discarding one, because the wrong thing is not
  dropped — it is written to the system log and kept. Refusing an option
  you do not implement is right under either answer to B-Q14.

  **The sweep that found it, kept because it is worth re-running and is
  not wired anywhere.** After fixing the same shape in `ps`, every
  coreutils bin was run with an option no utility has:

  ```bash
  cargo build -p coreutils --target x86_64-pc-windows-gnu
  B=target/x86_64-pc-windows-gnu/debug
  for n in $(ls userspace/coreutils/src/bin/*.rs | sed 's|.*/||; s|\.rs$||'); do
    [ -x "$B/$n.exe" ] || continue
    "$B/$n.exe" --no-such-option-xyzzy >/dev/null 2>&1 </dev/null || continue
    echo "$n accepted it"
  done
  ```

  83 tested, 6 accepted. Five are CORRECT and were checked against the
  real binaries rather than assumed — `echo`, `expr`, `printf`, `test` and
  `true` all treat it as text or an operand and exit 0, and ours agree.
  `logger` was the only defect.

  **Deliberately not made into a gate.** It needs all 83 binaries built,
  which is too heavy for `pre-push`, and the only place that already
  builds them is `scripts/boot-test.sh`, which is lane A's file. Writing
  a checker nobody runs is the shape this tree spent 2026-09-12 digging
  out of — `check-diff-preamble-order.py` sat unwired and red-treed
  `main`. A recipe that works is better than a gate that does not run.

  A *pattern* sweep would not have found it: `grep "_ => {}"` matches 25
  files in that directory and most are state machines. Running the
  program answers the question the pattern only approximates.

* **`logger`'s pair question** — **not a measurement question at all, and a
  harness cannot settle it.** `dup-bins-survey` lists it as "no harness — write one", which is
  the wrong instrument here. coreutils' `logger` writes its message to
  **stdout**; `userspace/logger` sends it to the `/dev/log` socket or appends
  to a file, the way util-linux does, with 23 options against 2. A
  differential harness would report that they disagree about everything, which
  is already known and is the *premise* rather than the finding.

  The stdout behaviour is deliberate — the module doc cites `CLAUDE.md`'s "No
  binary logs. Text-based (JSON-lines) structured logging." I think that
  misreads the rule: it governs the **format** a log is written in, not
  **where** the log lives, and writing to stdout does not make a log textual,
  it means there is no log. But it is a user-visible behaviour change either
  way, so it is **`open-questions.md` B-Q14** rather than a judgment call.
  Do not write the harness and do not delete either side until that is
  answered.

So the answer is per command, and for some pairs it is "merge", not "pick" —
and for at least one it is neither, because the two implementations are
answering different questions.

**Nothing collides today**, because `scripts/create-ext4-rootfs.sh` stages
neither -- no `userspace/` binary reaches `/bin` yet. The collision is latent
and arrives whole on the day they are staged, which is the worst time to
discover thirty-nine of them.

**Why this is not a lane-B decision to make alone.** Deleting a working program
is user-visible, and doing it thirty-nine times on my own judgement is a policy
rather than a fix. What lane B can do without asking is the evidence: for each
pair, which is the measured port and which is the invention. `free` and `ps`
above are two of thirty-nine; the rest are unexamined.

**Related but distinct** from the `/proc` entry above. That one is two parsers
of one *file*; this is two implementations of one *command*, and a pair can be
guilty of both -- the two `free`s are.

### The evidence, gathered 2026-09-10

The entry above said lane B could supply the evidence without deciding
anything. Here it is. Four signals per side, all mechanical: lines, `#[test]`
count, **fidelity markers** (mentions of `GNU coreutils`, `procps-ng`,
`util-linux`, "measured against", "transcription", "byte-exact", "upstream"),
and stub markers.

The fidelity count is the one that separates the groups, and it separates them
sharply: on the `coreutils` side it runs to 76 (`df`), 71 (`free`), 69 (`sed`),
67 (`cal`); on the standalone side it is **0 for every one of the 39**. A file
that cites upstream seventy times was written against upstream.

Classified by a stated rule -- `coreutils` fidelity >= 10 means it is the
measured port; otherwise a standalone more than 1.5x longer *and* with more
tests means it is the substantial one; otherwise it needs a human reading.

| | count | commands |
|---|---|---|
| `coreutils` is the measured port | **27** | cal, chown, cmp, comm, cut, dd, df, du, expand, fold, free, head, join, nl, paste, sed, seq, split, stat, strings, tar, tee, tr, tsort, uniq, wc, xargs |
| the standalone is the substantial one | **6** | date, logger, patch, sha256sum, uptime, who |
| needs reading | **6** | diff, env, hostname, kill, ps, uname |

Two spot-checks, because a rule that classifies without being checked is a
different kind of guess:

* `coreutils`'s `date` opens *"No timezone support yet -- always UTC"* against a
  standalone with strftime, RFC 5322/3339, ISO 8601 and parsing. Group 2 is
  right.
* `coreutils`'s `stat` is a real port, but the standalone is **larger** (2840
  against 2118) with twice the tests. Group 1's rule fires on the fidelity
  markers and the evidence is genuinely mixed. Which turned up the complication
  below.

### Eleven of the thirty-nine are not pairs

**`userspace/stat` is one binary that answers to six names** -- `stat`, `ln`,
`mkfifo`, `readlink`, `realpath`, `touch` -- dispatched on `argv[0]`. Eleven of
the thirty-nine standalone crates do this. "Retire the standalone" is therefore
not a per-command decision for those eleven; it removes whatever else rides
along.

Checked rather than assumed: all five of `stat`'s riders **do** have
`coreutils` counterparts, so that one is safe. One command is not:

> **`ncal` exists only inside `userspace/cal`.** `coreutils/src/bin/cal.rs`
> does not contain the string at all. Retiring `userspace/cal` deletes `ncal`
> from the system.

That is the whole of the loss across the eleven, and it is one command -- which
is worth knowing precisely, because "some of them provide other commands too"
would have been enough to stall the decision indefinitely.

### One thing this exercise found about lane B's own recent work

`userspace/ps` had **zero tests** -- verified by running them, not by counting
`#[test]`. It was converted to `procinfo` two ticks ago and no test was added,
while `coreutils`'s `ps` was converted the next tick *and* given four. Same
lane, same week, same kind of change, two standards.

**Given eight**, since the gap was mine and closing it does not prejudge which
`ps` survives. One of the eight is not an assertion about `ps` at all but about
the pair:

> **The two `ps` implementations render the same `tty_nr` differently.**
> `userspace/ps` prints `tty{n}` from the raw number; `coreutils/src/bin/ps.rs`
> prints `pts/{n & 0xff}`. For `tty_nr = 34816` they print `tty34816` and
> `pts/0`. Neither is obviously right -- 34816 is `(136 << 8) | 0`, so `pts/0`
> reads a real Linux device number correctly and `tty34816` names a device that
> does not exist -- but they are two answers to one question, measured, in one
> tree. It is the concrete form of everything above.

### What is still not decided, and by whom

Nothing above chooses. The 27 in group 1 look like deletions and the 6 in
group 2 look like the reverse, but *acting* on 39 commands is a user-visible
policy and stays the operator's. What has changed is that it can now be decided
from a table instead of from thirty-nine readings.
