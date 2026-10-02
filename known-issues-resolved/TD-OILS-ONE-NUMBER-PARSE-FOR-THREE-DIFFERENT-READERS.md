### TD-OILS-ONE-NUMBER-PARSE-FOR-THREE-DIFFERENT-READERS. `shift`, `read -n/-N/-u`, `read -t` and `ulimit` each parsed their count with the same Rust `str::parse`, but bash reads them with three different functions with three different grammars — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_shift`, `read`'s
option arms, and `ulimit`'s operand parse.

**What:** a numeric operand is not one thing in bash. Which helper a builtin
reaches for decides what it accepts *and* what it says when it refuses:

| helper | callers | grammar |
|---|---|---|
| `legal_number` (= `strtol`) | `shift`, `read -n`, `-N`, `-u` | leading blanks stepped over, trailing blanks allowed, leading `+`/`-` |
| `all_digits` | `ulimit` | digits only — no sign, no blank, either side |
| `uconvert` | `read -t` | `[+-]? DIGIT* ( '.' DIGIT* )?`, fully consumed |

**Measured rule** (bash 5.2.37). Consequences osh got wrong:

```sh
shift " 1"      # shifts once      osh: numeric argument required
shift "1 "      # shifts once      osh: numeric argument required
shift "  +1"    # shifts once      osh: numeric argument required
shift " -1"     # shift count out of range   osh: numeric argument required
ulimit -n +5    # +5: invalid number         osh: accepted
ulimit -c ''    # zero — an empty word passes `all_digits` vacuously
ulimit -c 012   # 12 — read base ten, not as octal
read -t .       # accepted; so are ``, `5.`, `.5`, `00.5`, `-0`
read -t 1e2     # refused — `uconvert` has no exponent
```

`ulimit`'s overflow saturates at `LONG_MAX` rather than failing, and a leading
`-` never reaches the operand at all (`ulimit -n -5` is an unknown *option*:
rc 2 and the synopsis, not a number complaint).

The refusals name the base the word reached for. That is bash's
`sh_invalidnum` (`builtins/common.c`), which looks at the **first two bytes
only** with the octal arm tested first: `0` before a digit → `invalid octal
number`; `0` before a lowercase `x` → `invalid hex number`; else `invalid
number`. So `012x` is octal, `0x3` hex, `0X3` plain, and ` 0x3` plain. It is
used by `read -n/-N` and by `ulimit`, but **not** by `shift`, which has its own
`numeric argument required`.

**Fixed** by routing each caller to the reader bash routes it to: `shift` and
`read -n/-N/-u` to the existing `legal_number`, `ulimit` to an inline
`all_digits` gate with a saturating base-ten fold, and `read -t` to a new
`read_timeout_seconds` spelling `uconvert`'s grammar. `sh_invalidnum_kind` is
spelled once and shared. Pinned by the unit tests
`shift_steps_over_the_blanks_strtol_steps_over`,
`read_reads_its_counts_the_way_strtol_reads_them`,
`read_timeout_is_uconverts_grammar_not_a_floats`,
`ulimit_wants_digits_and_names_the_base_it_refused`, and by the corpus case
`a-builtins-count-is-read-the-way-strtol-reads-one.sh`.

**Standing lesson:** when a builtin's error message differs from bash's, the
question is not "what does bash accept here" but "*which helper* did bash
reach for". The messages are the helper's, not the builtin's — which is why
one wrong answer in `ulimit` was really one wrong answer repeated in four
builtins that should never have shared a parse.
