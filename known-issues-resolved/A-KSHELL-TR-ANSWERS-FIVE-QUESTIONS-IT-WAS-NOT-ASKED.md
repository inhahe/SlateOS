## `A-KSHELL-TR-ANSWERS-FIVE-QUESTIONS-IT-WAS-NOT-ASKED` (lane A, 2026-08-25) — ✅ **FIXED**

**Where.** `kernel/src/kshell.rs` — `cmd_tr` (~108262), `cmd_tr_input`
(~108312), `tr_translate` (~108328), `tr_delete` (~108345), `expand_tr_set`
(~108365).

**What.** The kernel shell's `tr` has five separate ways of producing a
confident, successful, wrong answer. None of them is a refusal; every one of
them exits 0.

| typed | what it does | what it should do |
|---|---|---|
| `cat f \| tr ' ' '_'` | passes the text through **unchanged** | replace spaces with underscores |
| `tr ' ' '_' file` | usage message, exit 1 | the same, on the file |
| `cat f \| tr -s ab` | translates `-`→`a` and `s`→`b` | refuse: `-s` is not implemented |
| `cat f \| tr a b c d` | expands `b c d` as one set | refuse: extra operand |
| `tr -d x binaryfile` | prints **nothing at all** | say the file is not text |
| `tr -d '[:space:]'` | deletes `[`, `:`, `s`, `p`, `a`, `c`, `e`, `]` | delete whitespace |

**Status (2026-08-25): fixed, in four commits, pinned by kshell self-test rungs
46 and 49–51.** Every row of the table above is gone, and so is the repeat
construct the table never listed (`[x*n]` / `[x*]`, which used to translate
using the five literal characters `[`, `x`, `*`, `3`, `]`).

Three deliberate divergences from GNU are recorded rather than hidden, each
argued at its item below:

| | this `tr` | GNU |
|---|---|---|
| `\303` (octal above `\177`) | refused | the byte 0xC3 |
| `tr 'abc' '[:upper:]'` | accepted, paired positionally | refused as misaligned |
| `-t` (truncate SET1) | refused | supported |

The first two are in `design-decisions.md` §276. All three run in the safe
direction: two are *missing* answers where GNU has one, and the third accepts
a script GNU would have rejected — none of them answers an already-working
script differently, which is the property this entry was opened about.

**Why, one at a time.**

1. **A set cannot contain a space.** `tr` is not on
   `command_parses_own_quotes`, so `remove_quotes` runs over its line before it
   arrives: `' ' '_'` becomes `  _` (two spaces and an underscore). `cmd_tr_input`
   then does `args.splitn(2, ' ')`, which yields `("", " _")` — SET1 is the
   *empty* set, so `expand_tr_set("")` is empty, nothing matches, and the input
   is copied out verbatim with exit 0. This is the same fault `cut -d' '` had
   (fixed in `667d34060`), except that `cut` at least failed loudly.
   The file form takes the other branch of the same split and reaches the usage
   message instead, so the two halves of one command disagree about what
   happened.

2. **An unknown option becomes SET1.** There is no flag loop at all — the first
   word is compared to the literal `"-d"` and otherwise *used as a set*. So
   `tr -s ab` is read as "translate `-` and `s` into `a` and `b`". `-s`
   (squeeze), `-c`/`-C` (complement) and `-t` (truncate SET1) are all real `tr`
   options that this one does not implement, and all three are silently
   reinterpreted as data.

3. **Extra operands are absorbed.** `splitn(2, ' ')` in the pipe form puts
   everything after the first word into SET2, so `tr a b c d` expands
   `b c d` — including the spaces — as the replacement set.

4. **An undecodable file becomes an empty one.** Both file paths read
   `core::str::from_utf8(&data).unwrap_or("")`, so a file `tr` cannot decode
   produces no output and exits 0, indistinguishable from a file that really
   was empty. This is the same silent-guess-by-another-door already fixed in
   `tac`, `fold`, `base64`, `awk` and `cut`. Note the *pipe* form does not have
   this bug — `dispatch_with_input` narrows through `shell_bytes_as_str`, which
   reports and exits 1 — so, again, the two halves of one command disagree.

5. **Character classes are literals.** `expand_tr_set` knows ranges (`a-z`) and
   escapes (`\n`, `\t`, `\r`) and nothing else, so POSIX's `[:alpha:]`,
   `[:digit:]`, `[:space:]` … expand to their own punctuation, and the `[x*n]`
   repeat form and octal `\NNN` escapes do likewise. `tr -d '[:space:]'` on a
   line containing a `c` deletes the `c`.

**What the fix looks like.** Three commits, mirroring the shape `cut` already
has in this file (`CutSpec` / `CutParseError` / `parse_cut_args`):

1. **Arguments.** A `TrSpec` and a `TrParseError`, parsed from
   [`split_words`] rather than `splitn`, with `tr` added to
   `command_parses_own_quotes` — which it then qualifies for, by the rule that
   function documents. A real flag loop that names `-s`, `-c`/`-C` and `-t` as
   *unsupported* rather than eating them as data, refuses anything else
   beginning with `-`, and checks the operand count (1–2 words for `-d`, 2–3
   for a translation). `strip_quotes` comes out of `expand_tr_set` at the same
   time, because with the quotes already removed by `split_words` it would
   start eating real data (`tr "''" x`).
2. **Data.** Byte-clean whenever every character of SET1 is ASCII — which is
   provably identical to the character path, since UTF-8 is self-synchronising
   and so no ASCII byte ever occurs inside a multi-byte sequence, and it is
   still correct on input that is not text at all. `tr -d '\r'` over a binary
   file is the case that matters. When SET1 *does* contain a non-ASCII
   character the input genuinely has to be decoded, and a file that will not
   decode is reported (`tr: <file>: not valid text, and SET1 contains
   non-ASCII characters`) with exit 1 rather than treated as empty.
3. **Sets.** `[:alnum:] [:alpha:] [:blank:] [:cntrl:] [:digit:] [:graph:]
   [:lower:] [:print:] [:punct:] [:space:] [:upper:] [:xdigit:]`, the `[x*n]`
   and `[x*]` repeat forms, and octal `\NNN`. The classes are defined over
   ASCII only — that is what GNU does in the C locale, it is the reading that
   keeps them inside the byte-clean fast path above, and a Unicode reading
   would have to answer questions (is `²` a digit?) that `tr`'s positional
   set-to-set correspondence cannot express anyway. An unrecognised `[:name:]`
   is an error, not a literal.

   **Commits 1 and 2 have landed. Commit 3 split in two**, because the escapes
   and the bracket constructs turned out to be independent and the escape half
   was the one with a live wrong answer in it.

   - **3a — escapes: done** (kshell self-test rung 49). `expand_tr_set` now
     returns a `Result`, which threads into the `TrParseError` path commit 1
     built. `\a \b \f \v` and octal `\NNN` were all falling into the
     "escaped character stands for itself" arm, so `tr -d '\a'` deleted every
     letter `a`, and `tr -d '\0'` — the example this file's own byte-table
     documentation gives as a common shape — deleted the digit `0`. `\101`
     was read as the three characters `1`, `0`, `1`.

     **One deliberate divergence from GNU**: an octal escape above `\177` is
     *refused*. Above that point an octal escape names a byte in every other
     `tr`, and this one's sets are code points — which is not an oversight but
     the choice that makes `à-â` a range of three characters rather than of
     six bytes (commit 2). No single value of `\303` is right under both
     readings, so answering U+00C3 would be a silent guess of exactly the kind
     this entry exists to remove. Refusing is a missing answer, which is the
     acceptable one.

   - **3b — classes and equivalence classes: done** (kshell self-test rung
     50). `tr_class_members` gives the twelve POSIX names their ASCII members
     in ascending code-point order, `tr_bracket` reads `[:name:]` and `[=c=]`,
     and a `[` that begins neither stays a literal `[` — which is the only
     reading under which `tr -d '[]'` means anything. **Row 6 of the table
     above is no longer true**: `tr -d '[:space:]'` deletes whitespace.

     The rule that pays for itself here is *not a construct* vs *a malformed
     construct*. `[abc]` is the first, so it is five literal characters;
     `[:alhpa:]` is the second — right shape, bad name — so it is an error.
     Falling back to literals for a typo would quietly delete brackets,
     colons and the letters of the misspelling, which is the same failure
     class one level down.

     Two GNU restrictions are kept (only `[:upper:]`/`[:lower:]` in SET2;
     no `[=c=]` in SET2, since it names a *set* and so names no single
     replacement). **One is deliberately not**: GNU refuses
     `tr 'abc' '[:upper:]'` as a "misaligned construct", and we accept it,
     because a class here is always the same fixed ordered list so the
     misalignment that check guards against cannot arise. See
     `design-decisions.md` §276 — the argument is that accepting more than
     GNU cannot change what an already-working script means, whereas the
     other two directions can.

   - **3c — the repeat constructs: done** (kshell self-test rung 51). `[c*n]`
     is n copies of c in either set; `[c*]` is SET2-only and pads SET2 to
     SET1's length. A count beginning with `0` is octal, as everywhere else,
     so `[x*010]` is eight copies — rung 51's assertion is built so that a
     decimal reading would change the answer.

     Two structural notes worth keeping, because they are the reason this did
     not ride along with 3b. `expand_tr_set`'s second parameter became
     `Option<usize>` rather than a bool: the pad's width is SET1's length
     minus *the rest of* SET2, so the flag distinguishing the two sets has to
     carry that number, and SET1 has to be expanded first. And the pad cannot
     be resolved where it is read — in `tr abcd '[x*]z'` it is two characters
     wide, knowable only after the `z` — so it is carried out of the bracket
     parser and spliced in once the scan finishes.

     `tr_escape` came out of `expand_tr_set`'s scan into its own function at
     the same time, because the repeated character may itself be escaped and
     `[\n*2]` has to mean two newlines. The alternative was a second copy of
     the octal decoder inside the bracket parser, which is how one command
     comes to disagree with itself.

**Not a regression.** All five have been true since the command was written.
They are recorded together because they are one command's worth of the same
failure class, and because fixing any one of them in isolation would leave a
`tr` that is honest about one thing and not the others.

**No caller is affected.** Nothing in `kernel/` invokes `tr` — no self-test
rung, no script — so the stricter parser cannot turn something green red.
