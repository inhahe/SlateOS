## A-KSHELL-REDIRECT-MISSED-WHEN-AN-APOSTROPHE-PRECEDES-IT, and the eleven — no, twelve — disagreeing quote scanners behind it — **Status: FIXED 2026-09-03 (all eleven shell scanners now go through `kernel/src/shellquote.rs`; the twelfth, `awk_split_print_args`, was never a shell scanner and was fixed with awk's own escape rule instead. See "Closed 2026-09-03" at the end. `TD-SHELLQUOTE-NO-ANSI-C-QUOTING` remains open, as do TD-KSHELL (b')/(c)/(d).)** — 2026-09-02

**Lane:** A. **Severity:** silent wrong behaviour on an ordinary line; the
output goes to the terminal instead of the file and the shell reports success.

**In short:** `echo "it's fine" > out` does not redirect. It prints
`it's fine > out` to the screen, creates no file, and exits 0. The cause is
that `parse_redirect` decides "am I inside quotes?" with a **single on/off
switch flipped by either quote character**, so an apostrophe inside a
double-quoted string turns quoting *off* and the closing `"` turns it back
*on* — leaving the scanner convinced the `>` is quoted when it is not.

**Repro** (any odd number of quote characters before the operator):

| Line | What happens | What should happen |
|---|---|---|
| `echo "it's fine" > out` | prints `it's fine > out`, no file | writes `it's fine` to `out` |
| `cat < "don't.txt"` | no redirect; `don't.txt` treated as an argument | reads the file |
| `echo 'a > b'` | correct — two quote chars, even | correct |
| `echo "a" > b` | correct — two quote chars, even | correct |

So the bug needs an **odd** count of `'`/`"` bytes to the left of the
operator, which is exactly what an apostrophe inside a double-quoted string
produces. That is why it has survived: every test written with balanced
quotes passes.

**Where it is:** `parse_redirect` (`kernel/src/kshell.rs:6615`) and
`parse_input_redirect` (`:6784`). Both are literally

```rust
if b == b'"' || b == b'\'' {
    in_quote = !in_quote;
} else if !in_quote && b == b'>' {
```

One boolean, both quote characters. A `'` inside `"…"` is an ordinary
character in every shell, and a `"` inside `'…'` likewise; a single toggle
cannot represent that, because it has no idea *which* quote opened the region.

### The root cause is not these two functions

**Eleven places in `kshell.rs` independently decide whether a byte is inside
a quote, and they do not agree with each other.** The two above are simply the
weakest. Ranked by how much of the grammar each one knows:

| # | Site | State it keeps | Knows `'` ≠ `"` | Knows `\` |
|---|---|---|---|---|
| 1 | `unquoted_positions` :1186 | `Option<u8>` | yes | no |
| 2 | `first_unquoted_space` :1229 | `Option<u8>` | yes | no |
| 3 | `split_unquoted` :1255 | `Option<u8>` | yes | no |
| 4 | `expand_braces` :1337 | `Option<u8>` | yes | no |
| 5 | `remove_quotes` :1469 | `Option<char>` | yes | no |
| 6 | `split_words` :3383 | nested loops | yes | no |
| 7 | `expand_vars_bytes` :885 | `in_single_quote: bool` | **only `'`** | no |
| 8 | `parse_redirect` :6615 | `in_quote: bool` | **no** | no |
| 9 | `parse_input_redirect` :6784 | `in_quote: bool` | **no** | no |
| 10 | `awk_split_print_args` :141351 | `in_quote: bool` | n/a — awk has only `"` | no |
| 11 | `tab_complete` :6001 | none — `rfind(' ')` | **no quoting at all** | no |

Ten of the eleven are wrong in at least one way, and they are wrong
*differently*, which is the part that makes this expensive: a line is scanned
several times on its way to a command, by scanners that disagree about where
its words begin and end. `#10` is the only one that is defensible as written,
because awk really does have just one quote character.

This is the "band-aid accumulation" case `CLAUDE.md` names: the same rule
reimplemented until the copies drift. Patching the two redirect scanners would
fix the repro above and leave nine implementations of one grammar.

### The proper fix

**One scanner, used by all eleven.** A single quote/escape state machine over
bytes, exposing what each caller actually asks:

- `is_quoted(s) -> impl Iterator<Item = bool>` (or a `QuoteScan` cursor), so
  "find the first unquoted `X`" is a filter over one shared traversal rather
  than a new loop each time;
- the three context rules stated once — unquoted `\c` → `c`; inside `"…"` the
  backslash is special only before `"`, `` ` ``, `$`, `\`; inside `'…'` there
  are no escapes;
- callers keep their current signatures, so this is a substitution at eleven
  sites and not a redesign of the pipeline.

That scanner is the *same object* `TD-KSHELL-LINE-EDITOR-IS-UTF8` stage (b)
needs (backslash escapes) and stage (d) needs (completion must find a word
start the way the parser does). **These are one piece of work, not three**, and
this entry exists mainly to record that: doing (b) as "add backslash handling
to `remove_quotes` and `split_words`" would be the third band-aid rather than
the fix.

**Verification when it lands:** a rung per row of the repro table, plus a
property over the eleven sites — for a corpus of lines mixing `'`, `"`, `\`
and an operator, every scanner must report the same quoted/unquoted
classification for every byte. Disagreement *is* the bug, so agreement is the
assertion.

### Second symptom, same trigger: `echo "it's $HOME"` does not expand `$HOME`

Verified 2026-09-02: the string `in_double_quote` occurs **zero** times in all
140k lines of `kshell.rs`, and `expand_vars_bytes` never tests for a `"` byte
at all. The double-quoted context is not modelled anywhere in the expansion
stage. Scanner #7 keeps one flag, `in_single_quote`, and flips it on *any*
apostrophe:

```rust
if b == b'\'' && !in_single_quote { in_single_quote = true;  … }
if b == b'\'' &&  in_single_quote { in_single_quote = false; … }
if in_single_quote { /* copy verbatim, expand nothing */ }
```

So an apostrophe inside a double-quoted string opens a region the expander
treats as single-quoted, and expansion stays off until the next apostrophe or
the end of the line:

| Line | Prints | Should print |
|---|---|---|
| `echo "it's $HOME"` | `it's $HOME` | `it's /root` |
| `echo "don't $USER"` | `don't $USER` | `don't root` |
| `echo "it's $HOME" > out` | *both* bugs at once: nothing expanded **and** no redirect | `it's /root` written to `out` |

This matters for the fix beyond being one more bug. It is the **same trigger**
as the redirect failure — an apostrophe inside double quotes — reached through
a completely different scanner, which is the clearest possible evidence that
the defect is the duplication rather than any one copy. It also means the
shared scanner cannot merely be "quote-aware"; it must carry a three-state
context (unquoted / single / double), because two of the eleven sites are
wrong specifically for collapsing that into a boolean.

Bash's actual rule, which the scanner must encode: inside `"…"` an apostrophe
is an ordinary character and `$` still expands; inside `'…'` nothing expands
and a `"` is ordinary. A one-bit model cannot express either half.

The third row is worth a rung of its own — it is the line where both defects
fire together, and a fix that repaired only one of them would still get it
wrong while looking greener.

### Narrowed 2026-09-03 — the scanner exists; six of twelve sites converted

**In short:** the single shared scanner this entry asked for is written
(`kernel/src/shellquote.rs`, commit `6f8564967`) and six of the copies now call
it instead of scanning for themselves (`eaed9854e`). Both redirect bugs in the
repro table are fixed. `echo "it's $HOME"` is **not** yet fixed — that is site
#7, still to convert.

**There were twelve, not eleven.** `parse_here_string` (`<<<`) is a thirteenth
scan of the same grammar and a twelfth hand-rolled copy; it was found only by
doing the substitution, which is itself an argument for the substitution. The
count in the table above is left as written — it is what was known on 2026-09-02
— and this section is the correction.

**Converted (6):**

| # | Site | Now calls |
|---|---|---|
| 1 | `unquoted_positions` | `shellquote::bare_positions` |
| 2 | `first_unquoted_space` | `shellquote::find_bare_space` |
| 3 | `split_unquoted` | `shellquote::split_bare_ranges` |
| 8 | `parse_redirect` | `shellquote::find_bare` — **bug fixed** |
| 9 | `parse_input_redirect` | `shellquote::scan` + `is_bare` — **bug fixed** |
| 12 | `parse_here_string` | `shellquote::scan` + `is_bare` |

**Remaining (6):** #4 `expand_braces`, #5 `remove_quotes`, #6 `split_words`,
#7 `expand_vars_bytes` (the `$HOME` symptom), #10 `awk_split_print_args`,
#11 `tab_complete`.

**One rule was wrong in this entry's own statement of the fix, and real bash
found it.** The section above says the scanner must expose "three context
rules". It must, but a caller that asks only *"which context is this byte
in?"* still gets `"\$HOME"` wrong: the `$` there genuinely **is** in the
double-quoted context, and what suppresses the expansion is the backslash, not
the context. The correct predicate is `ctx != Single && !escaped`. This was
found by porting the Rust scanner to Python and diffing it against real bash
over 32 cases (`printf '%s\n' <word>`, which prints one line per word after
bash has done quote removal) — not by re-reading the Rust, which looked right.
`shellquote` therefore answers the question itself, as `Tok::expands()`, rather
than leaving site #7 to restate a rule that this entry stated incompletely.

**The verification this entry asked for is partly built.** `shellquote`'s
`self_test()` covers all three contexts, backslash handling in each, delimiter
visibility, word splitting including the quoted empty word, `quote_word`
round-tripping arbitrary bytes, and the bare-vs-expands distinction. The
cross-scanner *agreement property* the entry proposes is not yet written and
cannot be until all twelve sites share the scanner — at which point it becomes
trivially true by construction, which is the better outcome than a test.

**One pre-existing bug surfaced while writing the new rung and is deliberately
not fixed here:** redirect paths are never unquoted. `echo hi > "out.txt"`
creates a file literally named `"out.txt"`, quotes included, and always has —
`parse_redirect` returns the raw slice and `resolve_path` does not strip
quotes. Rung 114 asserts the *parser's* contract (split the line) rather than
papering over this, because unquoting belongs to the executor
(`execute_redirect` / `execute_input_redirect`) and is its own commit.

### Two limits of the shared scanner, found while converting the rest

**In short:** the shared scanner is not a complete model of shell quoting, and
one of the twelve "copies" turns out not to be a shell scanner at all. Neither
is a regression — both describe ground that was never covered — but both would
otherwise look like oversights to whoever reads the conversion next.

**`shellquote` does not model `$'…'` (ANSI-C quoting).** To the scanner, the
`$` is an ordinary byte and the `'` opens an ordinary single-quoted region, so
in `$'a\'b'` the backslash-escaped apostrophe — which bash reads as *data*
inside the construct — is read as *closing* the region, and everything after it
is misfiled as unquoted.

- *Why it is not a regression:* every one of the twelve hand-rolled scanners had
  exactly the same blind spot, so a converted site is no worse than it was. The
  one place that does understand `$'…'` is `expand_vars_bytes`, which has its
  own arm for it (rung 113) and drives the scanner past the body with
  `QuoteScan::skip_to`, so the live expander is correct today.
- *Why it is not fixed now:* `$'…'` is not a third quoting context but a fourth,
  with its own escape alphabet (`\n`, `\t`, `\x41`, `\u00e9`, …). Adding it
  means `Ctx` grows a variant and every `matches!(ctx, …)` in the scanner and
  its callers has to be re-decided. That is a real change to the shared type
  and belongs in its own commit with its own bash cross-check, not smuggled
  into a substitution.
- *What it costs meanwhile:* only sites that see a `$'…'` **and** ask about
  quoting are affected, and the expander — the one that actually handles the
  construct — is not among them, because it skips the body rather than scanning
  it. Tracked as **TD-SHELLQUOTE-NO-ANSI-C-QUOTING**.
- *The rules are now measured, 2026-09-03.* `scripts/check-ansic-quoting-vs-bash.py`
  pins them against real bash — 30 cases, all green — so the implementation can
  be written against evidence rather than recollection. The parts that decide
  the shape of the Rust:
  - The alphabet is **C's, not the shell's**: `\n \t \r \a \b \f \v \e \\ \' \"`,
    plus `\nnn` octal, `\xHH` hex, `\cX` control, and `\uHHHH`/`\UHHHHHHHH`
    which emit **UTF-8** (so `\u00e9` is two bytes, `\U0001F600` is four).
  - **`\'` works inside the quotes.** `'…'` cannot express an apostrophe at
    all, which is the whole reason the construct exists — and it means the new
    variant cannot reuse `Ctx::Single`'s "no escapes whatsoever" rule.
  - **An unrecognised escape keeps both bytes** (`\z` → `\z`). The *unquoted*
    context does the opposite (`\z` → `z`), so this cannot share that path
    either. The new variant is genuinely a fourth, not a blend of two.
  - `\0` truncates the word.
  - **Nothing expands.** `$`, backticks, `~`, `{a,b}` and blanks are as inert
    as inside `'…'`; only escapes are special. So `Tok::expands()` and
    `Tok::is_bare()` must both answer *false* inside it.
  - It is a **word** construct: `x$'a\tb'y` is one word, and `$''` is an empty
    word rather than no word.
  - The `$` must be **bare**. Inside `"…"`, escaped, or quoted off, the
    construct does not exist and the `$` is literal — so entering the context
    is itself a question for the scanner, not a lexical prefix match.
  - An unterminated `$'…` is a **syntax error** in bash, unlike an unterminated
    `'…` mid-typing, which the scanner deliberately treats as running to
    end-of-input for tab completion's sake. The two readings will have to
    coexist.
- *The token model cannot express it as it stands, 2026-09-03.* This is the
  part that decides how big the change is, and it is not visible from the
  bash rules — it comes from reading `shellquote.rs`:
  - `strip_quotes` is `scan(bytes).filter(Tok::is_literal).map(|t| t.byte)`,
    and `Tok::is_literal` is just `!structural`. So the whole model is a
    **keep/drop filter over input bytes**: every output byte is an input byte,
    and the only decision per byte is whether it survives.
  - `$'…'` does not merely delete bytes, it **produces** them. `$'a\tb'` must
    yield a `0x09` that appears nowhere in the input (which holds `\` and
    `t`); `$'\u00e9'` must turn six input bytes into two output bytes;
    `$'\U0001F600'` ten into four. No assignment of `structural` to the input
    bytes can express any of those.
  - Therefore the fourth variant is **not** a drop-in. Either `strip_quotes`
    grows a decode path it dispatches to when it enters the region, or `Tok`
    grows a way to carry an emission distinct from `byte`. That is a change to
    the shared type or the shared function, and should be decided deliberately
    rather than discovered halfway through.
  - **Adding the variant silently breaks `Tok::expands()`.** It reads
    `!matches!(self.ctx, Ctx::Single) && !self.escaped`, so a new `Ctx::AnsiC`
    would report *true* — but nothing expands inside `$'…'` (measured: `$` and
    backticks are inert). It must become `matches!(… Ctx::Single | Ctx::AnsiC)`.
    `Tok::is_bare()` is already correct by construction, since it tests
    `matches!(self.ctx, Ctx::Unquoted)` positively — which is the argument for
    writing such tests positively in the first place.

**Site #10, `awk_split_print_args`, is deliberately excluded from the
conversion.** It splits an *awk* `print` argument list, and awk is a different
language: `'` is not a quote character there at all. Substituting the shell
scanner would fix one case and break another — awk's `\"` escape would start
being honoured (good), while a bare apostrophe in `print "it" 's'` would start
opening a quoted region that awk says does not exist (bad). The site should
stay a separate scanner; what it should *not* stay is wrong.

- *The real bug there:* the loop toggles `in_quote` on every `"`, with no
  concept of the backslash, so `print "a\",b"` splits at the comma inside the
  string. The fix is four lines of awk's own escape rule, not this scanner.
  Tracked as **A-KSHELL-AWK-PRINT-SPLITS-INSIDE-AN-ESCAPED-QUOTE**.
- *Consequence for the count:* the "twelve scanners" figure includes one that
  is not a shell scanner, so the shared-scanner goal is eleven sites, not
  twelve. The agreement property proposed above becomes true by construction
  over those eleven; site #10 is outside it on purpose.

### Closed 2026-09-03: eleven of eleven, plus the two bugs found on the way

**In short:** every shell quote scanner in kshell now goes through
`kernel/src/shellquote.rs`. The awk splitter — the one site that was never a
shell scanner — was fixed with awk's own rule instead. Two further bugs turned
up while converting the last sites and are fixed in the same batch: tab
completion did not know what a word was, and a redirection target was never
unquoted or resolved.

**`A-KSHELL-AWK-PRINT-SPLITS-INSIDE-AN-ESCAPED-QUOTE` — FIXED.**
`awk_split_print_args` now honours awk's string escape: inside `"…"` a
backslash makes the next byte literal, and both bytes are kept verbatim for
`awk_eval_expr`, which already knew how to unwrap `\"`. Before, the `"` of
`print "a\"b", c` closed the string and the comma after it — which is inside
the string — split the argument, so the two halves were printed as separate
arguments. It is still *not* the shared scanner, and rung 117 carries the
control that says why: `print "it's", x` must keep splitting, which the shell
scanner would stop doing.

**`A-KSHELL-TAB-COMPLETION-DOES-NOT-KNOW-WHAT-A-WORD-IS` — FIXED (found
2026-09-03 while converting site #11).** `tab_complete` found the start of the
word with `rfind(' ')` and the end of the command word with `find(' ')`, so:

| Typed | Was looked up | Now |
|---|---|---|
| `cat "My Fi` | name `Fi` in the **working directory** | name `My Fi` in the working directory |
| `cat "/tmp/zz a` | name `a` in the working directory | name `zz a` in `/tmp` |
| `cat My\ Fi` | name `Fi` in the working directory | name `My Fi` in the working directory |
| `'my prog` | a *filename* completion | a *command* completion |

Three separate defects, one cause: a quoted or escaped blank ended a word for
this stage and for no other. Finding the boundary was not sufficient on its
own — the word still carried its quotes, and no file is named `"My`, so
`remove_quotes` is applied before the lookup. A unique file match now also
closes a quote the user left open, because a completion that leaves the line
unparseable is not a completion.

- *Still open:* the text **inserted** is the raw filename, so completing
  `My Doc.txt` still produces two words. `shellquote::quote_word` exists for
  it; it is TD-KSHELL (d), separate because it also has to decide what to do
  about a quote the user has already opened.

**`A-KSHELL-REDIRECT-TARGET-IS-NEITHER-UNQUOTED-NOR-RESOLVED` — FIXED (found
2026-09-03).** The raw text after `>` went straight to `Vfs::write_file`, which
resolves nothing:

- `echo hi > "my file"` created a file whose name began with a quote character.
- `echo hi > f` in `/tmp` wrote `/f`, not `/tmp/f`.
- The *input* side did call `resolve_path` (but did not unquote), so a single
  `cmd < a > b` read from one directory and wrote to another — a divergence
  that reads as a filesystem fault rather than a shell one.

Both sides now call one `redirect_path`, which trims, unquotes and resolves.
`redirect_write` is the single chokepoint for all four output call sites, for
the same reason it already was for the append read-modify-write. Rung 118
covers it end to end, including that `>>` reaches the same file `>` created.

**Count closed out.** Sites #1, #2, #3, #8, #9, #12 went in `eaed9854e`; #4,
#5, #6, #7 in `450c60109`; #11 in this batch. #10 is awk's and stays its own
scanner. `TD-SHELLQUOTE-NO-ANSI-C-QUOTING` remains open and is the only known
gap in the shared model.
