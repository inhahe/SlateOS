### TD-OILS-HISTEXPAND-INHIBITION. history expansion ignored most of the shell syntax that gives `!` another meaning — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `inhibited()` (bash's
`bash_history_inhibit_expansion`) and the `!string` scan in `expand_one`.

**How it surfaced.** Turning the `history`/`histexpand` defaults on for an
interactive shell (TD-OILS-INTERACTIVE-SHELL-VS-INTERACTIVE, above) made osh's
REPL actually *run* history expansion for the first time, and 34 tests went red.
The old code only checked bash's `history_no_expand_chars`, so `echo ${!ref}`
died with `!ref}: event not found`.

**Fix (2026-07-29).** Every rule was **measured** against
`bash --norc --noprofile` (a script with `set -o history; set -H` triggers
expansion too, which is what makes it differentially testable), because the
obvious guesses were wrong in both directions. A `!` is literal when:

* the next character is end-of-line, space, tab, newline, CR or `=`
  (`history_no_expand_chars`);
* it is inside a double-quoted string and the next character is the closing `"`
  (one character further along the quote merely ends the search string, so
  `echo "x !"` is literal but `echo "x !a"b` is a real reference);
* the previous character is `$` (`$!`, including `$$!q`);
* the previous two are `${` **and** a `}` follows on the line;
* the previous character is `[` **and** a `]` follows;
* `extglob` is on, the next character is `(`, a `)` follows it, **and** the `!`
  is at least two characters into the line (bash's own `i > 1`, so `!(x)` at the
  start of a line and `x!(y)` both still expand).

Notably *not* inhibited: `${x#!}`, `${x:-!q}`, `${a!b}`, `${ !q}`, an unclosed
`${!q` or `[!q`, `a[b!c]`, `"!q"` (double quotes protect nothing — the rewrite
runs before quote removal), `x=!q`, and `!(zzz)` with `extglob` **off**.

Two further real bugs came out of the same measurement pass:

* **readline's `history_comment_char` was missing entirely**, so osh expanded `!`
  inside comments — including in the new corpus case's own comments. A `#` that
  is outside single *and* double quotes and sits at index 0 or right after a
  `history_word_delimiters` character (`" \t\n;&()|<>"`) now makes the rest of
  the line literal. This is the only rule that respects double quotes, which is
  why `expand()` tracks them at all.
* **The `!string` search string ended at the wrong characters.** It now ends at a
  `history_word_delimiters` character, at a word-designator/modifier introducer
  (`:^$*%-`), or at the closing `"` when the `!` is inside double quotes — and
  nowhere else, so `echo !zz!b` reports `!zz!b: event not found` rather than
  `!zz`. An empty search string before a *delimiter* matches nothing and is
  reported as a bare `!` (`echo !(zzz)` with extglob off, `echo !;x`), while
  before a *designator* it still means the previous event, which is what makes
  `!:0` and `!$` work.

**Tests.** `tests/corpus/histexpand-inhibit.sh` (new, 132-case corpus, zero
waivers) pins the whole matrix differentially, including bash's line-number lag —
a line that fails expansion does not advance the counter. Unit tests
`shell_syntax_inhibits_expansion`, `a_comment_makes_the_rest_of_the_line_literal`
and `event_search_string_ends_at_a_delimiter` cover the same ground in-process.
