## §125 — The POSIX `TZ` engine is a shared crate, and osh takes its zone from the *exported* `TZ`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** The libc had no timezone support at all: `localtime_r` was
`gmtime_r`, `mktime` was `timegm`, and `tzset`/`timezone`/`daylight`/`tzname`
were stubs. Separately, `userspace/oils` renders broken-down time *itself* —
`printf '%(FORMAT)T'` and the `\d \D{…} \t \T \@ \A` prompt escapes do their own
`epoch.div_euclid(86_400)` and hardcoded `%z`/`%Z` to `+0000`/`UTC` — because
osh never calls `strftime`. Two independent renderers of the same quantity,
both wrong in the same direction.

### 1. One engine in a shared crate, not a `posix` module

The obvious plan was to write the `TZ` parser inside `posix/src/tz.rs` and be
done. That would have fixed `localtime_r` and left osh alone.

*Against the shared crate:* it is a fourth `no_std` leaf crate
(`netproto`, `netipc`, `netring`, now `tzrules`), it must build for
`x86_64-unknown-none` *and* `x86_64-slateos`, and it has to be added to the
workspace `exclude` list with a comment explaining why — real, if small,
structural cost for what is ~900 lines of pure calendar arithmetic.

*For it:* fixing only the libc would have been **worse than fixing neither.**
Today both are UTC, so `date`, a C program's `localtime`, `printf '%(%T)T'` and
`PS1='\t'` at least agree with each other. Fix one and they disagree — and the
shell is precisely where a user would notice and mistrust the answer. A
disagreement about what time it is between the shell and every C program on the
machine is not a bug you can leave open; it is the kind that makes people
distrust the clock and work around it. Two implementations of one rule diverge
the moment either is touched, so the only durable answer is that there is one
implementation. Chosen.

The same argument had already been made twice in this tree, for `netproto`
(kernel + netstack daemon must parse the same Ethernet frame) and `netipc`
(both ends of the control channel must agree on the schema). `tzrules` is the
third instance of the same shape, and its module doc says so.

### 2. osh resolves the zone from `TZ` *only when it is exported*

This looks like a shortcut — reading a `HashSet` membership instead of doing
what a libc does — but it is bash's actual rule, and copying it is what makes
osh match. `variables.c:sv_tz`:

```c
v = find_variable (name);
if (v && exported_p (v)) array_needs_making = 1;
else if (v == 0)         array_needs_making = 1;
if (array_needs_making) { maybe_make_export_env (); tzset (); }
```

bash renders time through `strftime`, which reads the *process* environment,
and the only thing that puts a shell variable into that environment is the
export attribute. So `TZ=EST5 printf '%(%Z)T'` with no `export` genuinely does
print `UTC` in bash. `Shell::shell_tz` reproduces that exactly.

*Against:* it is surprising, and a user who writes `TZ=EST5` and sees UTC will
think the shell is broken. A "helpful" reading of any assigned `TZ` would look
more correct to that user.

*For:* the surprise is bash's, and diverging from it would break scripts that
rely on the distinction; the common paths (an inherited `TZ`, `export TZ=…`,
and an assignment *prefix* like `TZ=EST5 printf …`) all work, so the surprising
case is rare in practice. It also has a pleasant side effect: a unit-test shell
has never imported an environment, so nothing is in `exported` and the whole
suite renders in UTC on every host — the determinism the test corpus needs
falls out of bash's rule instead of being a carve-out from it, which was the
blocker the original bug report named ("any case that does not pin `TZ` is
nondeterministic across machines, so the corpus cannot simply stop pinning").

### 3. An unparseable `TZ` is UTC, silently

`Tz::parse` understands the POSIX grammar only. A zoneinfo name
(`America/New_York`) fails to parse and falls back to UTC, which is what glibc
does when it can find no matching tzfile — but glibc usually *can* find one,
and we have no tzdata at all, so the fallback fires where glibc would have
succeeded.

*Against:* silence. The user selected a zone and got a different one with no
diagnostic. A warning would at least be honest.

*For:* the libc has nowhere to warn to (`tzset` returns `void` and a libc that
prints to stderr on its own is worse than one that is quiet), and osh warning
where bash does not would be a gratuitous divergence. POSIX also specifies UTC
as the fallback for an unusable `TZ`, so this is the conforming answer even if
it is not the helpful one. Recorded instead as
`known-issues.md TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`, with tests in both
crates that assert the current behaviour so the day it changes is loud.

**How to reverse.** Adding a TZif reader to `tzrules` closes §3 without
touching either consumer — `Tz::parse` stays as the tail rule for times past a
file's last transition, which is exactly how TZif v2+ footers work. §2 is one
function (`Shell::shell_tz`). §1 is the one to keep: merging `tzrules` back
into `posix` would re-create the divergence it exists to prevent.
