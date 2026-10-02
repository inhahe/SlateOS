## B-UTMPX-IS-384-BYTES-AGAINST-MUSLS-400 (lane B, 2026-09-09) -- FIXED the same day

**In short:** the login-record structure is 16 bytes smaller than the C
library's and its last two fields are at the wrong offsets, so `who`, `last`
and anything reading `/var/run/utmp` through the C API sees the time and
address fields shifted.

| | ours | musl |
|---|---|---|
| `sizeof(struct utmpx)` | 384 | **400** |
| `ut_tv` | 340 | **344** |
| `ut_addr_v6` | 348 | **360** |

**The cause, measured rather than assumed:** musl's `ut_tv` is a full
`struct timeval` — **16 bytes**, `time_t` plus `suseconds_t`, both 8 — and ours
is a pair of `i32`s modelled on glibc's `__int32_t` variant. That is 8 bytes
short and 8-aligned rather than 4, which is where both the offset shift and the
missing 16 bytes come from; musl also carries `char __unused[20]` at the end.

Everything up to and including `ut_session` is at the correct offset, so the
type, pid, line, id, user and host all read correctly -- which is why nothing
noticed.

**Correction:** this entry first gave the two columns the wrong way round.

**Fixed** by widening `UtmpxTimeval` to two `i64`s, which is a real
`struct timeval`. The trailing `__unused[20]` turned out to be **already
present**, as `_reserved`, documented "reserved for future use" — which is what
it looks like from this side and not what it is. It is now documented as musl's.

Worth recording: the first attempt at this fix *added* a second 20-byte field,
because the struct was read as far as the fields the gate had named and no
further. The gate caught it immediately — 416 against 400 — which is the third
time in two days that reading only as far as the question took me produced a
confident wrong edit.

**Where.** `posix/src/utmpx.rs`.

**Registered in `KNOWN_MISMATCH`.**
