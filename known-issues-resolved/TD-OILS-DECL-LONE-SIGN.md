### TD-OILS-DECL-LONE-SIGN. A word that is only `-` or `+` was taken for an empty flag word by six separate ad-hoc scans, so `declare -p -` listed the whole shell — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `-f`/`-F` routing, the `-p`
routing, `local`'s routing, the `-p` operand split, the listing filter and the
compound flag loop; the fix is the shared `Shell::is_decl_flag_word`.

**What:** getopt ends option parsing at a word that is only a sign, so it
becomes an operand — and since no variable can be named `-` or `+`, the usual
answer is a complaint about it, not a listing:

```sh
declare -p -     # bash: declare: -: not found, rc 1
                 # osh:  listed every variable in the shell
declare - -p     # bash: two operand complaints; osh: picked the -p up and listed
declare -pr -    # bash: complaint; osh: a readonly-filtered listing
declare -- -     # the -- ends the options, so the - behind it is still an operand
declare -p v --  # a -- written after an operand is an operand itself
```

**Fixed 2026-08-04.** One predicate, `is_decl_flag_word` (a sign plus at least
one letter — the shape `declare_flag_end` and the main flag loop already tested
for), replaced all six `starts_with` tests. Reading `local`'s leading words the
same way also fixed `p` being honoured only on a minus there: `local +p` is a
listing of the frame's locals and `local +p w` a listing asked about `w`,
exactly as `declare +p` already was. Pinned by the 20-section corpus case
`a-lone-sign-is-an-operand-not-a-flag-word.sh` (since extended to cover the
`--` bug above) and a unit test.

**Standing lesson:** six ad-hoc re-spellings of one predicate meant one missing
case slipped through all six. This is the root cause the two entries above share
— when a second caller needs the same test, name it.
