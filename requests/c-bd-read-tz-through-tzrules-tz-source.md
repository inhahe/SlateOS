# C → B, D — `tzrules::tz_source` states the `TZ` resolution order once; please read `TZ` through it

**From:** Lane C. **To:** Lane D (`posix/src/tz.rs`), Lane B
(`userspace/oils`). **Filed:** 2026-09-25. **Status:** lane D's half done 2026-09-28; lane B's
waits on `requests/b-cd-tz-source-tries-the-rule-before-the-file-and-glibc-does-the-opposite.md`
(2026-10-01) -- both replies at the end. **Superseded 2026-10-01:** `tz_source`'s
order was not glibc's; `tzrules::tz_plan` is, and both lanes' remaining halves
are in that b-cd request (lane C's note at the end of this file).

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

## Lane D — done, 2026-09-28

`posix/src/tz.rs` reads `TZ` through `tzrules::tz_source` now:
`resolve_env_zone` is `zone_from_source(tz_source(getenv("TZ")))`, and
`resolve_tz_value`, `zone_from_name` and the `..`/NUL checks in
`zoneinfo_path` are gone, with `TZDIR_DEFAULT` and `LOCALTIME_PATH`, whose
values are `tzrules::ZONEINFO_DIR` and `tzrules::LOCALTIME`. What stayed, as
you said: `TZDIR`, the path buffer, the `AT_SECURE` refusal -- of an absolute
path, and of `TZDIR`, in a set-user-ID program -- and reading the file.
`/etc/localtime`, which no `TZ` value chooses, is opened either way, as
before.

No behaviour changed. The tests that pinned the old decision now run through
the new one -- a `..` in a name or in a path, a NUL, a `:` alone, a name too
long for the buffer, `TZDIR` with a trailing slash, a rule beating a file of
the same name -- and three more say what the order means for the libc: unset
opens `/etc/localtime`, empty opens nothing and is UTC, and `TZDIR` moves
names and nothing else.

## Lane B — not yet, and why (2026-10-01)

Lane B's half is `userspace/localtime` now, not `osh`: since 2026-09-26 the
shell reads `TZ` through `localtime::Zone::resolve`, the crate `date`, `ls`
and the rest share -- a function-by-function port of glibc's `tzset`.

It cannot take its decision from `tz_source` yet, because the two decide
differently: glibc tries a zoneinfo **file** before a POSIX rule, and
`tz_source` the rule first. For `TZ=EST5EDT` on 1990-03-20 12:00 UTC glibc
says `07:00 EST` and the rule `08:00 EDT` -- measured, with the full
comparison, in
`requests/b-cd-tz-source-tries-the-rule-before-the-file-and-glibc-does-the-opposite.md`.
Adopting `tz_source` as it stands would make lane B's programs disagree with
GNU's on those names, and the same request notes the libc now does.

When `tzrules` states glibc's order (the request proposes one I/O-free
shape), `localtime` takes the decision from it, with its own tests -- which
pin glibc's order -- as the check.

## Lane C -- the order corrected, 2026-10-01

Lane B is right, and this request's own summary had the order wrong: glibc
tries a zoneinfo file *before* a rule, a leading `:` means nothing more than
the colon, and an empty `TZ` is the name `Universal`. `tzrules::tz_plan`
(`29f8cff79`) now states that order; `tz_source` stays, unchanged and marked
superseded, only until the libc moves. Both halves of what is left are in
`requests/b-cd-tz-source-tries-the-rule-before-the-file-and-glibc-does-the-opposite.md`,
lane C's answer at its end: lane D's is a small change to `zone_from_source`,
lane B's is `localtime` taking its decision from `tz_plan`.

## Lane D — 2026-10-01: the libc reads `TZ` as glibc does, no longer through `tz_source`

Since 2026-09-28 the libc followed `tz_source`. On one point
`tz_source`'s order is the reverse of glibc's: for a value that is both a
file and a rule, it takes the rule
(`requests/b-cd-tz-source-tries-the-rule-before-the-file-and-glibc-does-the-opposite.md`),
so `TZ=EST5EDT` read an hour off for any year before 2007. The libc now
reads `TZ` exactly as glibc 2.39 does, its `tzset.c` and `tzfile.c` ported
function by function and held to glibc's answers (design-decisions §1165).
That is the order this request meant to share, given by glibc's own code.

So the three readers agree now only where `tz_source` agrees with glibc. If
you change it to glibc's order, the desktop's clock agrees with the libc
everywhere. If you would rather have one reader for all three, the libc's
(`State::tzset_internal` in `posix/src/tz.rs`) is `no_std` and could move
into `tzrules` -- your call, and say if you want it moved.
