## §356 — `printf`'s `\uXXXX` encodes as UTF-8, for the same reason §351's quote marks are curly

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — applying the premise the operator settled
in §351/Q38 to a second helper that had branched on the same locale.

**In short:** `printf '\u00e9'` is supposed to print `é`. Ours printed the seven
literal characters `\u00E9` instead, because it was implementing what GNU does
on an ASCII-only system — where the character does not exist so the escape is
written back unchanged. SlateOS is never an ASCII-only system, so that branch
was implementing a machine that cannot exist. It now prints `é`, which is what
every modern system prints.

**Related:** §351 (diagnostics quote with the curly marks), Q38 (osh's string
layer is UTF-8, full stop). This is the same premise a third time.

### Why this is not a new decision so much as a missed one

GNU's `\uXXXX` goes through gnulib's `unicode_to_mb`: encode the code point as
UTF-8, then `iconv` it to the locale's charset, and if that fails hand it to a
callback that writes the escape back. Under `C` the charset is ASCII, so the
conversion succeeds for exactly the code points below U+0080 and fails for
every other one. Measured, GNU printf 9.4:

| | `LC_ALL=C` | `LC_ALL=C.UTF-8` |
|---|---|---|
| `printf '\u00e9'` | `\u00E9` — seven bytes | `\303\251` — the character |
| `printf '\u20ac'` | `\u20AC` | `\342\202\254` |
| `printf '\U0001F600'` | `\U0001F600` | `\360\237\230\200` |
| `printf '\U00110000'` | `\U00110000` | `\U00110000` — no encoding exists |

Our `print_unicode_char` implemented the left-hand column, and said so in its
doc comment: *"In the C locale the charset is ASCII, so the conversion succeeds
for exactly the code points below 0x80."* That was an accurate description of
the wrong locale. §351 had already answered the general question — Q38 settled
that there is no non-UTF-8 locale on this target, so a branch on an ASCII
charset is dead code that looks load-bearing — but §351 was scoped to
diagnostics, and this is stdout, so nothing swept it up.

It was found by the harness migration that followed §351: `printf-diff.sh` had
pinned `LC_ALL=C` on the strength of a header paragraph saying *"`extfloat` and
`coreutils::quote` implement the C locale, which is what the OS's own printf
will run under."* Half of that sentence stopped being true at §351, and moving
the reference to `C.UTF-8` to fix the other half is what exposed this.

### What changes, exactly

- U+0000–U+007F: one byte. **Unchanged** — `\u0001` was and is the byte 0x01,
  and `\u0000` was and is a NUL.
- U+0080–U+10FFFF: the UTF-8 encoding, one to four bytes. **This is the
  change.** Includes the non-characters U+FFFE and U+FFFF, which glibc's iconv
  encodes rather than refusing — measured, not assumed.
- Above U+10FFFF: `\U%08X`, the escape written back. **Unchanged in spelling**,
  though it used to be reached by everything above U+007F and is now reached
  only here.
- Surrogates: still a fatal `invalid universal character name \ud800`, refused
  before the encoder is consulted, which is upstream's order too.

gnulib's failure callback also has a `\u%04X` arm for code points below
U+10000. It is unreachable under a UTF-8 charset — every such code point
encodes — so it is not written out, per §351's "no dead code that looks
load-bearing".

### The argument against, and why it does not hold

The literal-passthrough behaviour is *predictable*: `printf '\u00e9' | wc -c`
answers 7 on every machine, where the new behaviour makes it depend on the
charset. That is a real property and it is the wrong one to want, because it is
predictable in the way a broken clock is: it is the answer for a configuration
this OS does not have, and a script that relied on it would break the moment it
was run on the Linux it was written against.

**Revisit if:** SlateOS ever gains a non-UTF-8 locale — the same trigger §351
carries, and it would restore the same branch in both places.

### Where it lands

`userspace/coreutils/src/bin/printf.rs` → `print_unicode_char`, with
`universal_character_names_encode_as_utf8` and
`a_code_point_beyond_utf8_writes_the_escape_back` pinning both arms.
`scripts/printf-diff.sh` moves to `LC_ALL=C.UTF-8` and
`scripts/printf-cases.py` gains one case per arm of the encoder — before this,
every code point above U+007F came out as its own literal text, so no case
could tell the arms apart.
