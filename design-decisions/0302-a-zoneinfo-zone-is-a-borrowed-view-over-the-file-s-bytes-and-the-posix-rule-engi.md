## §302 — A zoneinfo zone is a borrowed view over the file's bytes, and the POSIX rule engine is its *tail*, not its alternative

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B (POSIX & userland)
**Affects:** `tzrules/src/tzif.rs` (the reader), `posix/src/tz.rs` (`Zone`, `ZONE_FILE`), `userspace/oils/src/interp.rs` (`ShellZone`, `Zone<'a>`)

### The problem

`tzrules` already understood the POSIX `TZ` grammar. To make
`TZ=America/New_York` work, both the libc and osh had to learn to read TZif
(RFC 8536) binary zoneinfo files. Three things had to be decided along the way,
each with a real trade-off.

### Decision 1 — the reader borrows the file's bytes rather than owning a table

`TzFile<'a>` holds four slices into the caller's buffer (transition times,
transition types, ttinfo records, designations) and decodes an entry on each
lookup. It does not copy the transition table into a struct.

**For:** `tzrules` is `no_std` with no allocator — it is linked into the libc,
where `localtime()` must not allocate. The alternatives are a fixed inline
array, which either wastes kilobytes in every `Tz` or caps the transition count
at some arbitrary number (`America/New_York` has 236 transitions; some zones
have over 1 000), or requiring an allocator, which the libc cannot provide on
this path.

**Against:** it pushes the lifetime problem onto every consumer — the bytes
must outlive the zone. In the libc that meant a `static mut ZONE_FILE` buffer
and `TzFile<'static>`; in osh it meant an owning `ShellZone` holding an
`Rc<[u8]>` with a `view()` that borrows it. Two different answers to the same
question is a smell, but they are genuinely different situations: the libc has
one process-wide zone and no allocator, osh has an allocator and wants a
snapshot per prompt.

**Also against:** decoding per lookup is a few big-endian loads and a binary
search rather than a single indexed read. Measured against the cost of the
`open`/`read` that precedes it, this is noise.

### Decision 2 — the footer rule governs at and after the last transition

A TZif v2+ file ends with a POSIX `TZ` string. `TzFile::lookup` consults the
existing `Tz` engine for any instant at or past the last recorded transition
(and for *every* instant in a file with no transitions at all), and the
transition table only before that.

**For:** this is what the format means. `zic -b slim` — the default in modern
tzdata — stops emitting transitions as soon as the footer describes them, so a
reader that ignored the footer would freeze every zone at its last recorded
entry and report the wrong offset for every future date. Treating the rule
engine as the *tail* of the file engine also means the two paths share one
implementation of DST arithmetic and can never disagree about a future date,
which is the property the whole shared-crate arrangement exists to guarantee.

**Against:** it means a lookup can take either of two quite different code
paths depending on the instant, so a bug in one is invisible from the other.
Mitigated by testing both sides of the boundary in the same fixture.

### Decision 3 — the v2+ footer is mandatory

`parse` returns `None` for a v2+ file with no `\n<rule>\n` footer, even though
the data block before it is complete and self-consistent.

**For:** RFC 8536 §3.3 requires it, and it is the *only* structural check that
catches a file truncated exactly at the end of its data block — every count
still adds up, every index is in range, and the file looks perfectly valid. A
truncated zoneinfo file that silently becomes a zone with a plausible-looking
history is exactly the failure that is hardest to notice.

**Against:** a hand-written or unusual file with an empty footer is refused
where a more permissive reader would accept it. Judged the right trade: `TZ` is
attacker-shaped input (any libc will open the path you give it), so refusing
the ambiguous case is worth more than accepting the odd one.

### Decision 4 — `standard()`/`daylight()` prefer the tail over the history

The summary accessors that back `tzname[]`, `timezone` and `daylight` report
the footer rule's two halves when there is a footer, and fall back to the most
recent matching type in the table only when there is not.

**For:** it matches glibc, and it matches what a user means. Europe/Moscow has
decades of DST transitions on record and abandoned DST in 2011; São Paulo did
the same in 2019. A "does this zone have DST?" that answers from the history
says yes for both, and `daylight = 1` on a machine in Moscow is simply wrong.

**Against:** it makes the accessors disagree with `lookup()` for a historical
instant — `daylight()` says "no DST half" for a zone that was in DST in 1985.
That is the same thing glibc does, and the POSIX globals are documented as
describing the *current* zone, so the disagreement is in the spec rather than
in us.

### Reversibility

Decision 1 is the structural one: undoing it means giving `TzFile` an owned
table and an allocator, which would change every consumer's type. Decisions 2,
3 and 4 are each a handful of lines in `tzrules/src/tzif.rs` (`lookup`,
`footer_body`, `standard`/`daylight`) with tests naming the behaviour they pin,
so each can be revisited on its own.
