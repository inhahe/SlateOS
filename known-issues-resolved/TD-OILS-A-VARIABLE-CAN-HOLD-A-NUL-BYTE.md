### TD-OILS-A-VARIABLE-CAN-HOLD-A-NUL-BYTE. `printf -v v 'a\0b'` gave `${#v}` 3 — 2026-08-07 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` — the free function `cut_at_nul` and
its call sites (`put_var`, `scalar_write_checked`, `assoc_set`, `assign_elem`'s
indexed arm, and `builtin_mapfile`'s per-record value), plus the NUL skip in
`read_record`/`read_one_line`.

**What:** a bash variable is a C string, so a NUL cannot be in one. `printf -v`
builds the value in a buffer and assigns it with `bind_variable`, which takes a
`char *` — the value ends at the first NUL, silently. The lexer already modelled
this on the *word* side (`escape::ansi_c_unescape` truncates, so `$'a\0b'` is
`a`); the builtin side did not.

This entry's original framing — "truncate wherever a value is stored" — turned
out to be **one of three** rules, and the measurement, not the framing, decided
which applies where:

1. **A store truncates.** Every write is a `char *`, so the value ends at the
   first NUL. That is `printf -v`, an element write, and `mapfile`.
2. **Reading *skips*.** `read` drops the byte and carries on, so
   `printf 'p\0q\n' | read -r r` leaves `r` **two** characters — the original
   "truncate everywhere" reading was simply wrong for `read`. bash spells it out
   at `builtins/read.def:771`:
   ```c
   if (c == '\0' && delim != '\0')
     continue;		/* skip NUL bytes in input */
   ```
   The test sits *after* the delimiter test, so `read -d ''` still stops at one;
   and *before* `nr++` (read.def:813), so a skipped byte counts toward neither
   `-n`/`-N` nor "anything was read" — input that is only NULs reads as if it
   were empty (status 1, name cleared). An *escaped* NUL is kept, `pass_next`
   jumping straight to `add_char` past the test.
3. **A command substitution strips and warns** — every NUL, not just the tail —
   which osh already did (`Shell::strip_capture_nuls`).

The cut happens **before** the value attributes, because in bash it happens as
the bytes become a C string while the attribute is the assignment's own later
work: `declare -i n; printf -v n '1\0002'` stores `1`, not the `1002` the whole
byte string spells, and not an arithmetic error. Hence the cut sits in
`scalar_write_checked` rather than in `scalar_write_store`, and before
`apply_value_attrs` in `mapfile`.

`mapfile`'s cut is earlier still — at the record, not at the store — because the
`-C` callback is handed the value that *will* be stored: for a file holding
`m\0n`, bash's callback prints `m`.

**Repro** (bash 5.2.37 left, osh before the fix right):

| input | bash | osh |
|---|---|---|
| `printf -v v 'a\0b'; echo ${#v}` | `1` | `3` |
| `printf -v w '%b' 'x\0y'; echo ${#w}` | `1` | `3` |
| `printf -v v 'a\0b'; echo "${v@Q}"` | `'a'` | `$'a\000b'` |
| `declare -i n; printf -v n '1\0002'; echo $n` | `1` | arithmetic error |
| `printf 'p\0q\n' \| { read -r r; echo ${#r}; }` | `2` | `3` |
| `mapfile -t a < f` (file holding `m\0n`) | `1` | `3` |
| `declare -A h; printf -v 'h[k]' 'e\0f'; echo ${#h[k]}` | `1` | `3` |

The `@Q` row was a *symptom*, not a second bug — it was quoting a value that
could not exist in bash. Truncating in the quoter would have hidden the wrong
length instead of fixing it, which is why `escape::sh_single_quote` deliberately
does not.

**Covered by:** `userspace/oils/tests/corpus/a-nul-byte-ends-a-stored-value.sh`,
and the unit tests `a_stored_value_ends_at_its_first_nul` and
`reading_skips_a_nul_rather_than_stopping_at_it`.
