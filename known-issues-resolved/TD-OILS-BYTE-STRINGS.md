### TD-OILS-BYTE-STRINGS. A byte that is not valid UTF-8 cannot survive osh: a script holding one is refused outright, and data holding one is replaced or dropped — ✅ RESOLVED 2026-07-30 (all 10 steps done; `scaffold_lossy_string` deleted)

**Where:** the representation itself — osh stored a shell word, a variable value
and a captured stream as a Rust `String`, which cannot hold invalid UTF-8. The
observable boundaries were:
- `userspace/oils/src/main.rs:517` (`Plan::Script` → `fs::read_to_string`) and
  `interp.rs:3199` / `3962` / `4059` / `21523` / `25789` (`.`/`source`, and the
  other file readers) — these **refused** the file. Fixed at step 8: the script
  readers hand raw bytes to a byte-typed parser.
- `interp.rs` `substitute_file` (~13203), `finish_comsub` (~13278),
  `builtin_mapfile` (~21455) and `read_record` (~26401) — these were
  `String::from_utf8_lossy`, so the byte became U+FFFD. Fixed at steps 5–6.
- `read_one_line` (~26337) used `BufRead::read_line` with `.ok()?`, which turned
  the UTF-8 error into a *fake EOF*. Fixed 2026-07-30.

All of the above are closed. What is left is the *diagnostic* layer (step 10) —
see **Remaining** below.

**What.** Against bash 5.2.37, with `d.bin` = `a\xa9b\ncr\n` (`\xa9` is a lone
continuation-less high byte):

| | bash | osh, as first diagnosed | osh today (measured after step 8) |
|---|---|---|---|
| `osh d.bin` where the *script* holds `\xa9` | runs it, `echo` writes the raw byte | `osh: d.bin: stream did not contain valid UTF-8`, nothing runs | **fixed** — runs, writes `a9` |
| `. ./d.bin` | runs it | same refusal, tagged `osh: line 1: .:` | **fixed** — runs, writes `a9` |
| `v=$(cat d.bin)` | `a \xa9 b \n c` | `a \xef\xbf\xbd b \n c` (U+FFFD) | **fixed** — byte-exact |
| `v=$(<d.bin)` | same as bash's above | U+FFFD | **fixed** — byte-exact |
| `mapfile -t a < d.bin` | element 0 = `a\xa9b` | element 0 = `a\xef\xbf\xbdb` | **fixed** — `a\xa9b` |
| `IFS= read -r l < d.bin` | status 0, `l=a\xa9b` | *was* status 1 with `l` empty; then U+FFFD, status 0 | **fixed** — status 0, `l=a\xa9b` |
| `printf %s $'\xa9'` | one byte `a9` | two bytes `c2 a9` (U+00A9) | **fixed** — one byte `a9` |

Every row now matches bash. What remains undone is not in this table: it is the
*diagnostic* layer, where a path or a `$0` that is not text is still shown
approximately (never *acted on* approximately) — step 10.

The `read` row was the damaging one: `read_line` consumes the bytes and *then*
reports `InvalidData`, which `.ok()?` mapped to `None` — the same value a real
EOF returns. So `while IFS= read -r l; do …; done < file` stopped at the first
non-UTF-8 line and reported success, silently processing a prefix of the file.
That is now fully fixed: the readers are byte-native, so the loop neither
truncates nor invents an EOF nor alters the line.

The last row is the same defect seen from the writing side and was already noted
as a "known remaining gap" under the ANSI-C escape work (~line 965); it is
folded in here so the family has one home. Its *shape* changed twice on the way
out, which is worth recording because it shows how a half-converted stack
behaves. Step 2 made `escape.rs` byte-native, so `ansi_c_unescape` yielded the
single byte `a9` correctly — but the lexer that received it was still
`String`-typed, so the byte was immediately re-encoded lossily and emerged as
U+FFFD rather than the old U+00A9. Both were wrong; the corruption had merely
moved from the escape layer to the lexer. Step 8 byte-typed the lexer and the
byte now survives the whole path, so `$'\xa9'` and `$(printf '\xa9')` are
finally interchangeable.

**Why it matters beyond bash fidelity.** CLAUDE.md rule 7 is explicit: OS-boundary
data — paths, environment values, pipe contents — is bytes, and
`from_utf8_lossy` is silent data corruption. SlateOS paths allow every byte but
`/` and NUL, so a shell that cannot carry `a\xffb` cannot name every file its own
filesystem permits. `find … -print0 | while read -r -d ''` over a directory
holding a Latin-1-named file corrupts the name and then operates on the wrong
path — the failure mode is a wrong-file write, not a diagnostic.

**Proper fix.** Move the shell's string type from `String` to a byte string.
Concretely: introduce a `ShellStr`/`ShellString` newtype over `Vec<u8>`/`[u8]`
with the handful of operations osh actually needs (the UTF-8-aware character
counting that `${#x}`/`${x:o:l}` rely on stays, operating on the byte string and
treating an invalid sequence as one character per byte, which is what bash in a
UTF-8 locale does), then convert outward from the boundaries: the file readers
(`read_to_string` → `read`), `finish_comsub`/`substitute_file`/`mapfile`/`read`
(drop the `from_utf8_lossy`), the lexer's input, `WordPart::Literal`, and
`Shell::vars`/`arrays`/`assoc`. Everything that today writes bytes out
(`emit_stderr` and friends already take `&[u8]`) is unaffected.

This is the single largest refactor left in oils and touches nearly every
function in `interp.rs`, so it wants to be its own task rather than a rider on
another change. Two things make it tractable: the write side is already
byte-oriented, and the escape layer already masks `\xHH`/`\nnn` to a byte
(`escape::ansi_c_unescape`), so only the materialisation step needs changing
once a word can hold the byte. Do **not** attempt it piecemeal — a half-converted
`vars` map with a `String`-typed lexer is worse than either end state.

**Progress (branch `oils-byte-strings`).** Done so far, each its own green
commit (build + test + clippy on `x86_64-pc-windows-gnu` *and* `x86_64-slateos`):

1. `bytes.rs` — the foundation: `Str = Vec<u8>` / `BStr<'a> = &'a [u8]`, the
   `bfmt!` concatenating macro (there is deliberately no format-string macro:
   `Display for bstr::BStr` is lossy, so a `{}` that silently accepted a
   `String` would reintroduce the bug), `char_count`/`char_slice`, and the
   `#[deprecated] scaffold_lossy_string` seam marker.
2. `escape.rs` + `printf` — byte-native. `\xa9`/`\377` now emit **one** byte.
3. `bytes::Ch` — the character of a byte string: `Ch::U(char)` for a decoded
   scalar, `Ch::B(u8)` for a byte that begins no valid sequence. Needed because
   `?`, `${#v}` and `${v^^}` are character-defined; a raw `u8` would let `?`
   match a third of a two-byte character, and a plain `char` could not tell a
   decode failure from a real U+FFFD in the data.
4. The byte-to-`OsStr`/`Path` boundary. On the real target the target JSON says
   `"target-family": ["unix"]`, so `os_to_bytes` is the identity — this is what
   makes the whole refactor worth doing. The Windows dev host stays lossy
   (its names are UTF-16; there are no bytes to recover).
5. The glob/pattern engine and the character-defined parameter operators
   (`${v#p}`, `${v:o:l}`, `${v/p/r}`, `${v^^}`) run on `Ch` end-to-end. This
   removed the two `to_string_lossy` calls on directory entries that made the
   shell glob up a name it could then not open.
6. **The value layer — done 2026-07-30** (two commits: the lib, then the test
   suite). `Shell::vars`/`arrays`/`assoc`/`positional`/`cwd` are `Str`, and
   `expand_word`/`expand_to_string`/`expand_dynamic`/`expand_double_quoted`
   return `Str`. This also took the file readers that were listed as a separate
   step, because they are the same edit once the store is byte-typed: it removed
   the **third** `from_utf8_lossy` corruption bug (`builtin_mapfile` mangled
   every record that was not text, so an array of filenames read from `find`
   named different files than the ones on disk) on top of `substitute_file`
   (`$(< file)`) and `finish_comsub` (`$(cmd)`) fixed in step 5. `read`,
   `getopts`, `mapfile`, `compgen`, `select` and the `test` file primaries are
   now byte-correct end to end. Where a value must be *text* to mean anything,
   the conversion routes through `bytes::as_str` and answers `None` honestly
   rather than approximating: a nameref's target name, `${!ref}`, a tilde
   prefix, `OPTIND`/`OPTERR`. Two splits deliberately differ in kind: `$CDPATH`
   and `$PATH` split on the *byte* `:` (a path list is bytes), while `IFS` and
   `getopts`' optstring walk *characters* (a multi-byte IFS entry delimits as a
   whole; a byte that is part of no character delimits nothing and matches no
   option).

7. **The builtin argv surface — done 2026-07-30.** Every `args: &[String]`
   builtin signature is now `&[Str]`, which deleted the largest scaffold
   cluster: `run_builtin`, `exec_declare_with_arrays`'s `argv_text` and the
   `exec cmd` re-widening. Two subsystems that were `String`-typed all the way
   down came with it, and each removed a live corruption bug rather than just a
   type:
   * **Completion** (`compgen`/`complete`/`compopt`). `CompKey`/`CompSpec` and
     both candidate generators are byte-typed. `compgen_paths` and
     `compgen_path_commands` had been running every directory entry through
     `to_string_lossy`, so `compgen -f` could offer — and a script could then
     act on — a *different* filename than the one on disk; `compgen -G` ran its
     glob results through `scaffold_lossy_string`. `-W` now splits on IFS
     **bytes** like every other IFS site, rather than only on IFS's decodable
     characters.
   * **`test`/`[`.** The recursive expression parser, both primary evaluators,
     the operator tables and every diagnostic are byte-typed, so `[ -f "$file" ]`
     stats the file it was given and `[ "$a" = "$b" ]` compares the bytes it was
     given (byte-wise, which is what bash's C-locale `strcmp` does).

   Seams authored along the way are all the same shape: an operand that must be
   *text* to mean anything is narrowed with `bytes::as_str` and rejected with a
   diagnostic rather than approximated — running an *altered* command is the one
   outcome worse than running none. Narrowed: variable/function/array/alias and
   shell-option names (`declare`, `readonly`, `unset`, `shopt`, `set`,
   `getopts`' `name`, `mapfile`'s array), numeric counts, fd specs, and the
   `test` primaries that name a namespace (`-v`/`-o`/`-R`, which answer *false*
   — nothing that is not text is set, enabled or a nameref). Staying bytes:
   `export`/`readonly`/`declare` values, `read -p`'s prompt and `-d` delimiter,
   `mapfile` records, positional parameters, every path. Two seams remain only
   **because the parser is still `str`-typed**, and close with step 8: a
   compound array literal (`declare -a x=(…)`) and the `mapfile -C` callback
   line are shell *source*.

8. **The syntax layer — done 2026-07-30** (two commits: the lexer/parser/interp
   conversion, then a regression test for a bug it exposed). `lexer.rs`,
   `ast.rs` and `parser.rs` are byte-native — `WordPart::Literal(Str)`,
   `SingleQuoted { text: Str }`, the heredoc delimiter and body, the alias
   table — dragging `brace.rs` and `unparse.rs` with them, and `parse` /
   `run_source` / `eval_string` now take `BStr`. This is the step that made a
   **script** able to hold a byte that is not text: the top three rows of the
   table above all flipped here, because `main.rs`'s script reader and
   `builtin_source` went from `read_to_string` (refuse the file) to `read`
   (run it). `$'\xa9'` also finished its journey — step 2 had made the escape
   layer produce the right byte only for the lexer to re-encode it lossily.

   Eleven `bytes::as_str` seams were **deleted** rather than moved, because a
   consumer that re-parses shell source no longer needs that source to be text:
   `prompt_expand` (PS1/PS4 re-lex), `eval`, `jobs -x`, both `trap` handler
   call sites plus the whole `trap_source` helper, the `declare -a x=(…)`
   compound literal, the `mapfile -C` callback line, `source`'s file read, and
   the `type` builtin's function-body lookup.

   The conversion exposed a **live bug in `unset`**, now fixed with a
   regression test. A subscript is an ordinary shell word and may hold any
   byte, but `builtin_unset` applied its *identifier* gate to the whole operand
   before splitting `name[sub]` apart — so `unset 'm[<non-text>]'` was silently
   dropped and the element stayed. The gate now runs on the base name only,
   after a raw-byte split. Three sibling sites have the same gate-ordering
   shape but currently *refuse with a diagnostic* rather than corrupting, and
   are tracked separately as `TD-OILS-ELEMENT-TARGET-SUBSCRIPT-TEXT`.

**Remaining, in order.** Step 6 was the keystone — expansion and the variable
store are each other's main producer and consumer, so converting either alone
would have needed a scaffold at every site the other now satisfies for free.
Step 8 was the last one that could still *alter data*; everything left is the
layer that only ever *displays* it. What remains can land one layer per commit:

9. `arith.rs`, `histexpand.rs`, `ere.rs`. `main.rs` was pulled forward and is
   already done (see `TD-OILS-ARGV-PANICS-ON-NON-UTF8`).

   * **`arith.rs` — done 2026-07-30.** The evaluator took a `&str`, so
     `eval_bytes` gated the whole expression on the source decoding as UTF-8.
     Arithmetic syntax is entirely ASCII, so that gate almost never mattered —
     except for the one thing in an arithmetic expression that is *not* syntax:
     an **associative subscript**, which is a literal key. `declare -A m;
     k=$(printf '\xa9'); m[$k]=7` then `(( m[$k] ))` failed with `syntax
     error: invalid arithmetic operator` where bash answers `7`, and
     `(( m[$k] = 9 ))` failed the same way *while silently dropping the store*
     — the worse half, since the shell reported a syntax problem rather than
     the write it had not done. Both now match bash, with the regression test
     `an_associative_subscript_may_hold_any_byte`.

     The lexer reads **bytes** rather than `char`s, which is not merely
     equivalent but strictly closer to bash: bash's `isspace`/`isdigit` are
     byte-wise in the C locale, whereas the `char` lexer skipped a Unicode NBSP
     as whitespace where bash rejects it. `read_op` returns a `&'static str`
     from a longest-match-first table, so the operator tables (`binop_bp`,
     `is_assign_op`, `assign_base`) keep matching on text without ever
     converting a byte of input. Variable *names* stay `&str` — identifier
     syntax is ASCII — while values, subscripts and error tokens are bytes.
     `ArithError`'s `Display` is gone, replaced by `body() -> Str`, because the
     error token is a slice of the source; the shell now prints an offending
     byte as itself rather than as U+FFFD. `eval_bytes` was deleted, not
     rewired: there is one entry point again. This closed the three `VarLookup`
     seams in `interp.rs` (formerly `:24931`/`:24940`/`:24967`).

     A latent bug was fixed *before* the conversion, in its own commit:
     `bytes::trim_start`/`trim_end` used `u8::is_ascii_whitespace`, which omits
     the **vertical tab**, while documenting C's `isspace`, which includes it.
     `str_to_val`'s Unicode `str::trim` masked the difference; converting it to
     `bytes::trim` would have silently broken `v=$'\v5'; echo $((v))` (bash: 5).
     New `bytes::is_space` with a pinning test.

   * **`histexpand.rs` — done 2026-07-30.** History expansion is now byte-native
     end to end, which closed all three of its seams *by deletion* rather than by
     moving them: the `HistCtx` text accessors and the `expand_history_lines`
     pass-through (both interim seams authored at step 8) and the `history -p`
     "argument is not valid text" refusal. None of them was a
     `scaffold_lossy_string` call, so the gate count below is unchanged at 7 —
     but each was a real loss of function, not merely of display:

     A recorded line is whatever bytes were typed, and a SlateOS path admits
     every byte but `/` and NUL, so `cat a\xffb` is an ordinary command. Before
     this, `expand_history_lines` refused any *line* that did not decode and
     committed it unexpanded — so typing `!!` after that `cat` ran the literal
     two characters `!!` instead of the command. Worse, the *entry* was equally
     unreachable: `!c`, `!?\xff?` and `!-1` could not name it, and `history -p`
     answered `argument is not valid text`. The regression test
     `history_holds_bytes_that_are_not_text` pins all of it — the three event
     forms, both searches, word designators (`a\xffb` is one word, since no byte
     of it is a readline delimiter), `%`, and the `:q`/`:x`/`:s`/`:e` modifiers
     — comparing bytes throughout.

     Like `arith.rs`, the scanner reads **bytes**, and history-expansion syntax
     is entirely ASCII, so a byte scan can neither match inside a multi-byte
     character nor split one. `words`/`word_spans`/`search_match`/`modify_path`
     return borrowed or owned bytes; `Expansion`'s four payloads are `Str`; the
     unknown-modifier and bad-word-specifier messages quote the offending byte
     back as itself. Three general helpers were added to `bytes.rs` for it —
     `rfind`, `contains` and `replacen` — the last deliberately diverging from
     `str::replacen` on an empty pattern (it replaces nothing, because every
     caller reaches it through a shell construct where an empty pattern means
     "match nothing", and splicing at every boundary would insert text the user
     never asked for).

     The ~900 lines of bash-measured expectations in the test module stayed
     readable rather than being rewritten as byte literals: a text-typed mirror
     of `Expansion` plus `words`/`search_match`/`quote_breaks`/`expand` adapters
     are *defined inside* `mod tests`, where a local item shadows the `use
     super::*` glob. They `expect` on a decode failure — a case that wrote its
     expectation as a Rust string literal has a bug if it sees a non-text byte —
     while the non-text cases call `super::…` directly.

   * **`ere.rs` — done 2026-07-30.** Closes `TD-OILS-ERE-TEXT-ONLY`. Both
     `cond_regex` seams went away *by deletion*: the one that refused a pattern
     that was not text (which made `[[ $f =~ … ]]` exit 2 as though the regex
     were malformed) and the one that refused such a subject (which silently
     matched nothing). `ere::Regex::new`/`new_flags`/`is_match`/`captures` now
     take and return bytes.

     Unlike `arith.rs` and `histexpand.rs`, the scanner is **not** byte-wise: it
     reads `bytes::Ch`, exactly as the glob engine does, because ERE has
     character-defined constructs. That is the only reading under which `.`
     matches an undecodable byte as **one** character rather than as a third of
     an `é`, `{3}` counts it once, `[^a-z]` matches it, no POSIX class does
     (`PosixClass::matches` gates on `Ch::as_char`), and case-folding under
     `nocasematch` folds such a byte only to itself — so two *different*
     undecodable bytes never fold together and none ever folds into a letter.
     Bracket ranges rely on `Ch`'s derived `Ord`, which orders every decoded
     scalar below every undecodable byte. ERE *syntax* is entirely ASCII, so
     every metacharacter test goes through `Ch::as_ascii` and no encoding
     question arises in the parser. `captures` reassembles a group from the
     decoded characters rather than slicing `text`, which keeps a group boundary
     from ever landing inside a character.

     `EreError` became `Str` (two of its messages quote a slice of the pattern
     back) and its `Display` impl was **deleted** rather than made lossy: bash
     prints nothing for an uncompilable `=~` right-hand side — it just makes
     `[[` exit 2 — so the shell discards the error, and the only other reader is
     a test asserting the pattern was rejected at all.

     Two regression tests pin it: `ere::tests::matches_a_subject_and_a_pattern_that_are_not_text`
     for the engine, and `interp::tests::cond_regex_matches_a_value_that_is_not_text`
     end-to-end. The latter reads its capture through `run_raw`, not `run` —
     `run` decodes lossily and would have hidden the very byte under test.
10. The diagnostic layer. Split in two, because the *data* it labels and the
    *formatting* of the label are separable:

   * **10a — the source-label island. Done 2026-07-30.** `Shell::name` (`$0`),
     `SourceFrame::path`, `func_sources` and `fn_source_stack` all hold a path
     or a shell word, so all four became `Str`, along with the five accessors
     over them (`frame_source`, `current_source`, `error_source`,
     `merged_frames`, `bash_source_at`). That makes `$0`, `BASH_ARGV0`
     (including its `+=` form), `BASH_SOURCE`, `FUNCNAME`'s parallel source
     array and `caller`'s source field byte-exact: a script at `/tmp/a\xffb.sh`
     now *names* the file you can open, where before every one of those read
     back a U+FFFD. Four seams removed (`set_name`, both `SourceFrame.path`
     pushes, `BASH_ARGV0`); one added, `error_source_shown`, which is the single
     narrowing the three still-`String` prefix builders share. Pinned by
     `interp::tests::the_shell_name_and_source_labels_hold_bytes_that_are_not_text`.

   * **10b — the prefix builders and their call sites. Done 2026-07-30.** All
     four builders (`err_prefix`, `err_prefix_at`, `read_error_prefix`,
     `syntax_error_prefix`) now return `Str` and assemble with `bfmt!`, so
     `error_source_shown` is gone and the prefix carries `$0` / a script path as
     bytes end to end. The ~285 call sites did *not* need 285 hand edits: about
     110 already fed the prefix into a `bfmt!`, and `PushBytes for Vec<u8>` made
     those compile untouched. The rest went through a new method rather than the
     `berrln!` macro this entry originally sketched —

     ```rust
     fn perrln(&self, msg: &(impl bytes::PushBytes + ?Sized)) {
         self.berrln(&bfmt![self.err_prefix(), msg]);
     }
     ```

     — which reads better than a macro, needs no hygiene, and lets a `&str`
     literal, a `format!` of purely-textual parts and a `bfmt!` of shell data all
     reach the same call. It also *collapsed* the two spellings the same message
     had: 84 `self.errln(&format!("{}…", self.err_prefix()))` and 39
     `self.emit_stderr(format!("{}…\n", self.err_prefix()).as_bytes())` are now
     both `self.perrln(…)`, with `perrln` appending the newline itself. Also
     converted here, because they were the last consumers of `interp.rs`'s
     `shown()`: `format_parse_error` and `wrap_parse_message` (now returning
     `Str`, so a syntax error echoes the offending source line *verbatim* rather
     than through a decode that would rewrite the very text being blamed), the
     two here-document reader warnings (a `<<a\xffb` delimiter is quoted back as
     written), and `arith_cmd`, which became `Option<Cow<'static, [u8]>>`.
     `interp.rs`'s `shown()` is deleted. Pinned by
     `interp::tests::diagnostics_carry_bytes_that_are_not_text`.

   * **10c — the parser's own text. Done 2026-07-30.** `LexError::msg`,
     `ParseError::msg`, `token_display`/`token_display_at`, `cond_near`,
     `cond_error_near` and `unterminated_heredoc` are all `Str`; `wrap_parse_message`
     takes bytes. Both `Display` impls were **deleted** rather than made lossy —
     the shell formats these itself through `format_parse_error`, and the only
     other readers were three tests. That removes `parser.rs`'s `shown()`, the
     last seam.

     The visible effect is that every diagnostic which quotes a shell construct
     back now quotes the *bytes the user wrote*: `f() a\xffb` reports `syntax
     error near unexpected token \`a\xffb'`, `for a\xffb in x` reports
     `` `a\xffb': not a valid identifier ``, `[[ -z x a\xffb ]]` names `a\xffb`
     on its second line, and a here-document delimiter is quoted as the same
     byte string the reader compares each body line against. Before this a
     `\xff` became U+FFFD, so the message named something the user had not
     typed — and, for the here-document case, a delimiter that could never have
     ended the body.

     Classification stayed correct because every test in it is over the fixed
     *English* part of a message, which no quoted word can perturb:
     `ParseError::is_incomplete` and `wrap_parse_message` became byte-wise
     prefix/suffix tests, and `format_parse_error` splits on `b'\n'`.
     `cond_error_near`, which scans back for a trailing `;`/`|`/`&`, reads the
     last *byte* — all three delimiters are ASCII, so a byte scan can neither
     match inside a multi-byte character nor split one.

     The ~40 parse-error cases in `parser.rs`'s test module kept their Rust
     string literals through an `emsg(&ParseError) -> String` adapter that
     `expect`s on a decode failure (the same shape as `interp.rs`'s
     `parse_error`); the byte cases call `super::parse` on a byte literal
     directly. Pinned by `parser::tests::diagnostics_quote_the_source_bytes_back`,
     with the here-document warning added to
     `interp::tests::diagnostics_carry_bytes_that_are_not_text`.

11. **`scaffold_lossy_string` deleted — 2026-07-30.** The gate below is met: the
    function is gone from `bytes.rs` and no lossy decode survives anywhere in
    osh's production code. The one remaining `from_utf8_lossy` in the crate is
    `bytes::bytes_to_os`'s `#[cfg(not(unix))]` arm — the Windows *development*
    host, whose filesystem names are UTF-16 rather than bytes, so there is no
    byte sequence to recover. SlateOS itself takes the `#[cfg(unix)]` arm, which
    is exact. Everything else is `#[cfg(test)]` harness code.

**Gate.** The branch does not merge until `scaffold_lossy_string` is deleted and
a grep for it outside `bytes.rs` returns nothing. That grep count *is* the
tracker — because each seam needs `#[allow(deprecated)]` to keep the build
warning-free, the deprecation warning itself never accumulates. Count after
step 5: **17**. After step 6: **14**. After the `main.rs` argv fix
(TD-OILS-ARGV-PANICS-ON-NON-UTF8): **15** — that one *added* a seam, at
`Shell::set_name`, by pulling `$0` a layer earlier than step 10. After step 7:
**11** (10 in `interp.rs`, 1 in `lexer.rs`) — that step removed five and added
one, `builtin_source`'s `SourceFrame.path`, which is the same step-10 seam
`Shell::set_name` is (`$0` and `BASH_SOURCE` both feed `err_prefix`). After
step 8: **10** — 8 in `interp.rs` (`:3005` `set_name`, `:3280` and `:22883`
`SourceFrame.path`, `:7300` `BASH_ARGV0`, `:24931`/`:24940`/`:24967` the
arithmetic `VarLookup`, `:26541` the `shown` helper), 1 in `parser.rs` (its own
`shown`) and 1 in `lexer.rs:111` (the unterminated-heredoc delimiter). After
`arith.rs` (step 9, first of three files): **7** — the three arithmetic
`VarLookup` seams are gone, and **every seam that remains is a step-10
diagnostic seam**: `interp.rs:3005` `set_name`, `:3280` and `:22883`
`SourceFrame.path`, `:7300` `BASH_ARGV0`, `:26528` `shown`, `parser.rs:63`
`shown`, `lexer.rs:111` the unterminated-heredoc delimiter. None of them can
alter what a command *does*. After `histexpand.rs` (step 9, second of three
files): still **7** — its three seams were removed by deletion, not converted
into scaffolded ones, so the count is flat while the loss of function is gone.
After `ere.rs` (step 9, third of three files — **step 9 complete**): still
**7**, for the same reason; its two `cond_regex` seams were deleted outright.
Everything that reaches step 10 is now purely diagnostic. After step 10a:
**4** — `interp.rs:9873` `error_source_shown`, `interp.rs:26485` and
`parser.rs:63` the two `shown` helpers, and `lexer.rs:111` the
unterminated-here-document delimiter. After step 10b: **2** — `parser.rs:63`
`shown` and `lexer.rs:111` the unterminated-here-document delimiter, both of
which are step 10c and both of which live in the parser rather than the
interpreter. `interp.rs` is now seam-free. After step 10c: **0**, and
`scaffold_lossy_string` itself is deleted. **The gate is met.**

**Interaction.** `TD-OILS-UNICODE-ESC`, `TD-OILS-PRINTF-QUOTE-CHAR` and
`TD-OILS-STRLEN-CHARS` are *not* part of this: those are host-locale artifacts
where osh already matches UTF-8-locale bash, and the byte-string move must
preserve their current character-wise answers.

**Resolved 2026-07-30.** All ten steps are done and the gate is met: osh's
representation is `Str = Vec<u8>` end to end, from the script reader through the
lexer, parser, expander, arithmetic, history expansion, ERE engine and every
builtin, out to the diagnostics. A SlateOS path admits every byte but `/` and
NUL, so a file named `a\xffb` is now one osh can name, glob, open, stat, delete
and *report errors about* — where before the shell would variously refuse the
script, replace the byte with U+FFFD (and so act on a **different file**), or
fake an EOF. `bytes.rs` carries the vocabulary the conversion needed: the `Str`
/ `BStr` aliases, the `bfmt!` concatenating macro and its `PushBytes` trait, the
`StrBuf` extension (deliberately without a `push`, so a leftover `s.push('x')`
fails to compile), the `Ch` scanned-character type whose derived `Ord` puts
every decoded scalar below every undecodable byte, and `as_str` — the one
honest, *fallible* bytes-to-text conversion, now the only one in the crate.

What remains genuinely text is text *by construction* and is documented as such
at each site: variable/alias/reserved-word names and `HashMap<String, _>` keys
are `[A-Za-z_][A-Za-z0-9_]*`, so bytes that are not text are not names and the
honest answer is a rejection rather than an approximation. Both narrowings that
were tracked separately are now resolved: `TD-OILS-ELEMENT-TARGET-SUBSCRIPT-TEXT`
was not a *name* narrowing at all but a subscript wrongly caught up in one, and
`TD-OILS-NONUTF8-ENV-NAME` was a genuine one whose fix is not to widen the name
type but to stop an unnameable *inherited* entry from being lost — it is set
aside verbatim and passed through to children without ever becoming a variable.
