## TD-B-STANDALONE-PORTS-MATCH-LONG-OPTIONS-WHOLE (lane B, 2026-09-26) — **open**

**In short:** GNU and util-linux programs parse options with glibc's
`getopt_long`, which accepts any unambiguous abbreviation of a long option
(`--pri` for `--priority`), `--opt=value` and `--opt value` alike, options
after operands, and `--`. coreutils has had a faithful port of that parser
since the getopt conversions; the programs OUTSIDE coreutils each parse argv
by hand and match long options by their whole name, so `flock --verb` or
`lsmem --summ` is refused where upstream accepts it -- and each hand-written
loop has its own edge cases around values, `=`, and operands.

**Where:** the standalone ports that already share `usageerror` (the
diagnostic wording) but not a parser -- `blockdev`, `capsh`, `chattr`,
`hostnamectl`, `objdump`, `resolvectl`,
`route`, `sanitize`, `systemctl`, `tput` -- plus hand-parsed programs that do
not use it yet. (`logger` was one; its port uses `getoptlong`, 413e56f1d. So
was `getopt` itself -- now a port of util-linux's, whose script-facing parse
is `getoptlong` with the knobs it gained for it: keep-going, long-only,
distinct entries, `W;`. And `flock`, now a port of util-linux's, whose
old hand parser took an unknown option for the file to lock. And `lsmem`,
now a port of util-linux's printing through the `smartcols` crate, whose old
parser refused `--summ`, and whose old program invented a block size when it
could not read one -- design-decisions §1036. And `prlimit`, `column`,
`lsirq` and `lscpu`, likewise ported onto `getoptlong` and `smartcols`.)

**The proper fix,** now possible: `getoptlong` (extracted from
`coreutils/src/getopt.rs` on 2026-09-26) is the shared parser. Converting a
program means copying upstream's option string and `struct option[]` table
IN ITS ORDER (the order is observable in the ambiguity message), handling
`Opt` items as they arrive, and printing errors through the program's own
diagnostic path. A program whose upstream is not glibc-getopt-based is not a
candidate. `scripts/getopt-ambiguity-check.py` verifies coreutils' tables
against the reference and would need extending to cover these.
