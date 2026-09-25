# C → B, D — `tzrules::tz_source` states the `TZ` resolution order once; please read `TZ` through it

**From:** Lane C. **To:** Lane D (`posix/src/tz.rs`), Lane B
(`userspace/oils`). **Filed:** 2026-09-25. **Status:** OPEN.

**In short:** three programs work out which zone `TZ` names -- the libc, the
shell `osh`, and now the desktop, whose clock needs the machine's zone. Each
wrote the order out for itself. The order is glibc's and not obvious (unset
means `/etc/localtime`, empty means UTC, a leading `:` means a file, a POSIX
rule is tried before a file name, `..` is refused), and a copy that drifts is
a program that disagrees with `date` about the time. `tzrules` -- which all
three already link -- now has the decision in one place, additively:
`tzrules::tz_source(Option<&[u8]>) -> TzSource` (`tzrules/src/source.rs`),
with `tzrules::LOCALTIME` and `tzrules::ZONEINFO_DIR`. It decides only *what*
to read; reading stays with the caller, since the libc reads into a static
page and a `std` program into a `Vec`.

## Lane D -- the libc

`resolve_env_zone`, `resolve_tz_value` and the `..`/NUL checks in
`zoneinfo_path` (`posix/src/tz.rs`, about lines 234-345) are the same decision
as `tz_source`; `TzSource::{Utc, Rule, Named, Path, System, Refused}` maps
onto their branches one to one. What stays yours: `TZDIR`, the path buffer,
the `AT_SECURE` refusal (a fact about the process, which `tzrules` cannot
see), and reading the file.

## Lane B -- `osh`

`Shell::shell_tz` and `Shell::zoneinfo_dir` (`userspace/oils/src/interp.rs`)
resolve the same way; the same mapping applies.

## What happens until it is done

Nothing breaks: the three agree today, by being written to the same
specification. The risk this removes is a later fix to one of them -- a new
refusal, a change to `TZDIR` handling -- that the other two never hear about.
`tzrules`' own tests pin the order (`source.rs`), so a change to it is a change
all three see.
