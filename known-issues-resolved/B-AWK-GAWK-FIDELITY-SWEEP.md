## B-AWK-GAWK-FIDELITY-SWEEP -- 56 ways our awk and gawk --posix part, three of them losing data (lane B, 2026-10-01) — **FIXED** 2026-10-01

**Status:** FIXED 2026-10-01 — every row of the worklist below is fixed or recorded as a deliberate difference (`main.rs`'s table, an xfail each in `scripts/awk-diff.sh`); a newly found difference gets an entry of its own. **Fixed:** 5, 16, 28 (the parser half: the print list, `next` in BEGIN/END, constant zero divisors); 1, 2, 3, 6, 11, 12, 13, 17, 18, 25, 26, 29, 31 (compiling to instructions, §1056: recursion, `exit`/`next` through calls, one evaluation of a read-modify-write target, gawk's `^`, `getline` and directory operands, the reworded fatals, extra arguments, multi-line placement, stdout flushed before a warning); 4, 14, 15, 19, 20, 21, 22, 23, 27, 30, 33 (gawk's output errors, `close`/`fflush`/`system`, `NR`/`FNR` as longs, `arg_assign` for operands and `-v`, `substr`, the math warnings, the null and failed redirections); 7, 8, 9, 10, 24 (`printf` as gawk's `format_tree`, numbers as strings as its `format_val`); 32, 34 (gawk --posix's newline and separator grammar; `func` an ordinary name); 37, 40, 41, 42, 43, 44, 45, 51 (arrays as gawk lays them out, and the values they hand back: `array.rs`, and gawk's lazily-typed nodes in `value.rs`). 46, 55 (numbers as `strtod` reads them; program constants decimal only); 52, 53 (a field and `$0` hold what was assigned; `OFS` assignment rebuilds first). 38, 47, 48, 49, 50, 54, 56 are deliberate differences, each a row of `main.rs`'s table with its reason and an xfail in the harness. All 2026-10-01.

**In short:** four probe batches of `awk` against `gawk --posix` (about 230
programs, `target/drafts/loc-probe*.sh`) found real bugs well beyond the
diagnostic placement they were written for. Three can lose or corrupt a
user's result: a recursive function 3000 calls deep crashes the process with
a stack overflow; `exit` inside a function prints an empty `awk: ` line and
exits 2 instead of exiting with its code; and `print > "/dev/full"` loses the
output and exits 0. The everyday `printf("%s\n", x)` form is a syntax error.
This entry is the worklist; each item is closed by the commit that fixes it.

**Where.** `userspace/coreutils/src/bin/awk/` (all files), and
`scripts/awk-diff.sh`, which gains a row for every item as it is fixed.

| # | What | Ours | gawk --posix | Severity |
|---|---|---|---|---|
| 1 | recursion 3000 deep | stack overflow, exit 134 | works (30000 too) | crash |
| 2 | `exit` in a function | `awk: cmd. line:1: ` (empty), exit 2 | exits with the code; END runs | wrong control flow |
| 3 | `next`/`nextfile` in a function | ignored, record carries on | skips the record / file | wrong control flow |
| 4 | `print > "/dev/full"`, `close()` of it | silent, exit 0 | `fatal: flush to "/dev/full" failed: No space left on device` | silent data loss |
| 5 | `printf("%s-%s\n", 1, 2)`, `print("a", "b")` | syntax error | prints | rejects valid programs |
| 6 | `a[i++] += 5`, `a[i++]++`, `$(i++) += 0`, `sub(re, s, a[i++])` | subscript evaluated twice | once | wrong answer |
| 7 | number to string: `print 1e30`, `print 2^63` | `1e+30`, `9.22337e+18` | `1000000000000000019884624838656`, `9223372036854775808` (integral values print whole) | wrong output |
| 8 | subnormal: `print 1e-320`, `printf "%g"` | `infe-320` | `9.99989e-321` | wrong output |
| 9 | `print 2^1024`, `log(-1)` | `inf`, `nan` | `+inf`, `-nan`/`+nan` | wording |
| 10 | `printf "%d", 2^64` / of inf, nan | saturates / `0` | `18446744073709551616` / `inf -inf -nan` | wrong output |
| 11 | `x^n` for integer n | `powf` | gawk's `calc_exp` (repeated squaring): `1.1^50` differs in the last digits | precision |
| 12 | plain `getline` reaching an unopenable operand | returns -1 | fatal `cannot open file` | wrong control flow |
| 13 | a directory operand | `read error: Is a directory` | `fatal: cannot open file `/' for reading: Is a directory` | wording |
| 14 | `NR`/`FNR` assigned a fraction, a string, 1e30 | kept as given | C `long`: truncated, `LONG_MIN` out of range | wrong answer |
| 15 | an `FNR=10` operand | `FNR` becomes 10 | undone (`arg_assign` restores its C `FNR`) | quirk |
| 16 | `next`/`nextfile` in BEGIN/END | ignored | parse `error:` (`next' used in BEGIN action`) | accepts invalid |
| 17 | `next` from a function called in BEGIN/END | ignored | fatal `` `next' cannot be called from a `BEGIN' rule`` | accepts invalid |
| 18 | a function called with extra arguments | fatal | warning per call, extras evaluated and dropped | rejects valid |
| 19 | `close()` of a pipe that exited 3 | 3 | 0 (`--posix`) | wrong answer |
| 20 | `system("exit 3")`, killed by signal 9 | 3, 0 | 768, 9 (raw wait status under `--posix`) | wrong answer |
| 21 | `fflush("nope")` | 0 | warning, -1 | wrong answer |
| 22 | `substr("hello", 1.5)` | `ello` | `hello` | wrong answer |
| 23 | `log(-1)`, and the other math domain errors | silent | `warning: log: received negative argument -1` | missing warning |
| 24 | printf conversions: `%k` `%5` `%-]`; `%h %l %L %j %t %z`; `%a` | error; accepted; `%e`-style | printed literally; fatal under `--posix`; hex float | wrong output |
| 25 | `x /= 0`, `x %= 0` messages | `...attempted` / `in `%'` | `in `/='` / `in `%='` | wording |
| 26 | `$(-1)`, `NF = -1` | no `fatal:`; NF clamps to 0 silently | `fatal: attempt to access field -1`; `fatal: NF set to negative value` | wording / silent |
| 27 | redirection to `""`, or that cannot open | `: No such file...`; `getline < ""` is -1 | `fatal: expression for `>' redirection has null string value`; `fatal: cannot redirect to `f': ...` | wording / wrong control flow |
| 28 | `1/0`, `x/-0`, `x/0.0` (constant zero divisor) | runtime fatal, exit 2 | parse `error:` at the `/`, exit 1, parsing continues | wrong status |
| 29 | multi-line expression placement (`if (1 &&\n 1/z)`) | the statement's first line | the operator's line | placement |
| 30 | `var=value` operand diagnostics | placed, with FILENAME/FNR | unplaced (`arg_assign` zeroes the line and FNR) | placement |
| 31 | stdout and a diagnostic interleaved on one fd | diagnostic first | stdout flushed first (`err()` flushes) | ordering |
| 32 | newline after `(`, `[`, `=`, `==`, `?`, `:` | accepted | syntax error (`?`/`:` only under `--posix`) | accepts invalid |
| 33 | `FILENAME` numeric in a diagnostic | `(FILENAME=5 FNR=1)` | `(FNR=1)` (no string value) | wording |
| 34 | `func` as a name: `func = 3`, `func f() {...}` | a keyword, `function` | an ordinary name under `--posix`; the definition is a syntax error | accepts invalid |
| 35 | an interactive `awk` -- standard output a terminal | output held until 8 KiB or exit | flushed after every print (`output_is_tty`), and a redirection to a tty too | **fixed** 2026-10-01 |
| 36 | `-v` checks: no `=`, a name like `1x`, a keyword, a function's name, a newline | `invalid -v assignment`, usage, exit 1, for all | usage; `not a legal variable name`; `cannot use gawk builtin`; the definition's `error: function name ... previously defined`; `POSIX does not allow physical newlines` | **fixed** 2026-10-01 |
| 37 | `for (k in a)` order | Rust's randomly seeded `HashMap`: it can differ between two runs of one program | deterministic (gawk's own array layouts) | **fixed** 2026-10-01 |
| 38 | `FILENAME = 5` then something prints it, then a diagnostic | `(FNR=1)` | `(FILENAME=5 FNR=1)`: printing gave the number a cached string | **deliberate** (`main.rs`): gawk's text cache, see 49 |
| 39 | `cmd \| getline` flushed every output first | yes | no (`gawk_popen` does not) | **fixed** 2026-10-01 |
| 40 | the index `for (k in a)` hands out from an integer array | a strnum, numeric always | a number until its text or type is asked for, then a string for good (`INTIND`): `for (k in a) if (k > max) max = k` over 9, 10, 100 gives 9 | **fixed** 2026-10-01 |
| 41 | an index deleted during `for (k in a)` | skipped | visited: the loop walks the list it took | **fixed** 2026-10-01 |
| 42 | `for (k in a) delete a[k]` | `k` left at the last index | `k` left at the first (one `Op_K_delete_loop`), in exactly that shape | **fixed** 2026-10-01 |
| 43 | `getline a[k] < f` at end of file, `sub(re, s, a[k])` matching nothing | no element made | the element exists (`Op_subscript_lhs`) | **fixed** 2026-10-01 |
| 44 | a string array's index that came from input | a strnum, always | a strnum only while the input is unsettled (its `STRING` flag); after a comparison, and for a `-v` value, a plain string. A field stored in a variable or element is a copy (`UNFIELD`) | **fixed** 2026-10-01 |
| 45 | comparing a NaN | C's rule always | C's when neither side is flagged a string, else `cmp_awknums` (NaN equal to NaN and above everything) | **fixed** 2026-10-01 |
| 46 | `"0x1A"+0`, `"inf"+0`, `"nan"+0`; input `0x1A`, `inf`, `nan` | 0, 0, 0; strings | 26, `+inf`, `+nan`; numbers -- `--posix` takes `strtod` whole, hex and all | **fixed** 2026-10-01 |
| 47 | `TEXTDOMAIN` | unset | `messages`: gawk installs it under `--posix` too | **deliberate** (`main.rs`), with 56 |
| 48 | `ENVIRON` with `AWKPATH`/`AWKLIBPATH` unset | absent | both added, naming gawk's library directories | **deliberate** (`main.rs`): this awk searches no path and loads no extensions |
| 49 | `ARGV[1] = 5` with the 5 never converted to text | opens `5` | skipped: the number has no string (`stlen` 0) | **deliberate** (`main.rs`): whether a number has text is gawk's cache, and keeping it would cost every number a shared cell |
| 50 | `length(arr)` | the count | fatal `length: received array argument` under `--posix` | **deliberate** (`main.rs`): POSIX.1-2024 standardised it (Austin Group 1566) after gawk 5.2.1 |
| 51 | assigning an index from `for (k in a)` to `OFS`, `ORS`, `SUBSEP`, `CONVFMT`, `OFMT` | left a number | made a string at once (`set_OFS` and the rest call `force_string`) | **fixed** 2026-10-01 |
| 52 | `OFS = x` after `$3 = "y"` | `$0` rebuilt with the new `OFS` when read | rebuilt with the old one at the assignment (`set_OFS` rebuilds first): `a b y` | **fixed** 2026-10-01 |
| 53 | a field or `$0` assigned a string; `$0` after `sub()` or a rebuild | read back as input, a strnum | the assigned value's type kept; a rebuilt `$0` a plain string: `$2 = "10.0"; $2 == 10` is false | **fixed** 2026-10-01 |
| 54 | `$5` past `NF` compared with 0 | equal (POSIX's uninitialized value) | unequal: gawk's `Null_field` is the string `""` | **deliberate** (`main.rs`): POSIX's answer |
| 55 | a hexadecimal constant `0x1A` in the program; `011` | 26; 11 | the number 0 and then the variable `x1A`; 11 (`--posix` implies gawk's `--traditional`, whose scanner stops at the `x`) | **fixed** 2026-10-01 |
| 56 | gawk's own variables, `ARGIND` `BINMODE` `ERRNO` `FIELDWIDTHS` `FPAT` `IGNORECASE` `LINT` `PREC` `ROUNDMODE` `RT` `TEXTDOMAIN` | the program's names, unset | installed even under `--posix` (`init_vars` installs every one): `PREC` 53, `ROUNDMODE` `N`, `FPAT` a pattern; arrays and functions of those names refused; `IGNORECASE`/`BINMODE` assignments warn, `LINT = 1` turns lint on | **deliberate** (`main.rs`): POSIX reserves only its own names |

**Proper fix.** Each row, faithfully, against gawk 5.2.1's own source
(`/tmp/gawk-ref` in WSL), with a harness row. Structural ones first. Every
expression node carrying its token's line (29) is a change to `ast.rs`. Deep
recursion (1) decides the rest: the tree walk recurses natively once per awk
call, and the obvious cure -- run it on a thread with a huge stack -- is the
wrong one *here*, because SlateOS commits anonymous mappings when they are made
(`MAP_LAZY` is opt-in; `posix/src/pthread.rs` maps thread stacks without it),
so a 1 GiB stack would cost 1 GiB of memory up front. gawk's own answer is
the right one: compile to instructions and run them in a loop whose frames are
on the heap. That one change also gives gawk's line model exactly (its
`sourceline` is per instruction), resolves an lvalue once by construction (6),
and makes `exit`/`next` from inside a function an ordinary unwind (2, 3, 17).
