## 1032. `TZ` is read as glibc reads it, process-wide state included -- except through `..`

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** what a program's clock says depends on how it reads the `TZ`
variable, and glibc's reading has details no standard mentions: an empty `TZ`
is a zone called `Universal`, `TZ=Foo/Bar` is UTC called `Foo`, and a value like
`AAA3BBB` (a daylight-saving name but no dates) borrows New York's history from
a file called `posixrules` -- shifted by an amount that depends on what the
program has already converted. Every time-printing program here now reads `TZ`
with a line-by-line port of glibc 2.39's code, including that history, because
the alternative is printing a different hour from the GNU program being
replaced. The one place it deliberately differs is a `TZ` that reaches a file
through `..`, which is refused, as the C library here refuses it.

### How faithful

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **glibc's `tzset.c`/`tzfile.c`, state and all (chosen)** | the same hour as GNU for every `TZ` value `tz-diff.sh` tries, in every order of conversions | the programs being replaced are the reference, and their answers depend on the state: `TZ=AAA3BBB date -d @1604203200` and `date -d '2020-11-01 02:30'` disagree in glibc because `mktime` re-reads the zone | reproduces glibc behaviour that is arguably a bug (`rule_dstoff` is never computed for a file with transitions), and `Zone` needed interior mutability and lazy reading to do it |
| glibc's order and partial parse, stateless zones | the same for every value except `posixrules` ones, whose fall transitions land where the rules say | simpler; `Zone` stays immutable | differs from GNU by up to two hours near every fall transition under such a `TZ`, and silently |
| Keep `tzrules`' engine (each year's own transitions; refuse a partial rule) | UTC for anything it cannot fully parse, and real summer time before 1970 | the "more correct" answers | not what any GNU program prints; this entry's predecessor, `TD-B-LOCALTIME-RESOLVES-TZ-DIFFERENTLY-FROM-GLIBC`, was filed because the harness found exactly these differences |

`tzrules` stays the TZif decoder (and the libc's engine, which is lane D's to
change); the four raw accessors this needed were added to it, additively.

### `..`

glibc reads `TZ=../zoneinfo/UTC` as a file unless the program is setuid. Here
it never is: `zoneinfo_path` refuses a `..` component, as `posix/src/tz.rs`
does, and the value falls through to the POSIX rule, as a missing file does.
The refusal is not strong on its own -- an absolute path is accepted -- but the
two readers of one `TZ` in this tree must agree about what it means, and
changing the libc's is lane D's decision. `tz-diff.sh` keeps the case as an
xfail. **Revisit** if the libc drops its refusal, or gains a secure-mode test
this crate could share.

**Where:** `userspace/localtime/src/tzset.rs` (the port), `lib.rs` (`Zone`:
lazy, `tzset`, `reread`, `switched`, `localtime`), `mktime.rs`;
`tzrules/src/tzif.rs` (accessors); `userspace/coreutils/src/parse_datetime/mod.rs`
(`mktime_z`, `localtime_rz`). Checked by `scripts/tz-diff.sh`.
