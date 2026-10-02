### TD-OILS-A-MODIFIERS-PATTERN-IS-EXPANDED-AS-A-DOUBLE-QUOTED-STRING. `"${w#${z:-~}}"` does not expand the tilde, and `"${w#${z:-a"\x"}}"` loses the backslash — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_word_pattern` and its
six modifier call sites, `Shell::expand_replacement`, and the two tests that
read `Shell::dquote`: `Shell::push_literal_annotated`/the tilde arm of
`Shell::expand_word_joined_annotated`, and `Shell::operand_rhs_read`.

osh carried the enclosing word's `dquote` straight into a `${x#…}` pattern and
into a `${x/…/…}` replacement. bash does not: it swaps the bit for a *different*
one on the pattern side and clears it outright on the replacement side.

```c
  l = *value ? expand_string_for_pat (value,
        (quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) ? Q_PATQUOTE : quoted,
        …)                                            /* subst.c:5754-5756 */

  rep = expand_string_for_patsub (rep,
          quoted & ~(Q_DOUBLE_QUOTES|Q_HERE_DOCUMENT));   /* subst.c:9183 */
```

`Q_PATQUOTE` is `0x008`, a bit of its own, and the *whole* of bash names it in
three places — subst.c:3020, 10358 and 10365, all about `$*`/`$@`. So a pattern
keeps the joining half of double quoting and loses every other test that asks
`quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)`. Two of those are visible with
nothing but a `${x:-…}` written inside the pattern: the tilde refusal
(subst.c:11190) and `parameter_brace_expand_rhs`'s `SX_STRIPDQ` scan
(subst.c:7724).

**Reproduce** (`HOME=/hh`, `z` unset):

```sh
w=/hh;   echo "[${w#${z:-~}}]"          # bash: []      osh: [/hh]
w=/hh;   echo "[${w/${z:-~}/Z}]"        # bash: [Z]     osh: [/hh]
w=/hh;   echo "[${w/hh/${z:-~}}]"       # bash: [//hh]  osh: [/~]
v='a\xb'; echo "[${v#${z:-a"\x"}}]"     # bash: [b]     osh: [a\xb]
v=axb;   echo "[${v#${z:-a"\x"}}]"      # bash: [axb]   osh: [b]
```

A `"` written *inside* the pattern puts the bit back — that is an ordinary
quoted run and gets `Q_DOUBLE_QUOTES` like any other — so
`"${w#"${z:-~}"}"` is `/hh` in both.

**How the two bugs got in.** Neither test read `dquote` until this week. The
tilde refusal landed with `f808b32d7` (a-tilde-in-a-brace-operand-does-not-expand-under-quotes)
and the `SX_STRIPDQ` scan with `620a16b10`
(a-used-brace-operands-embedded-quotes-lose-their-backslashes); both were green
across the corpus because no case wrote either construct inside a pattern. The
`a-list-in-an-operand-makes-it-a-word-list` case is what finally caught it, and
only indirectly: its `strip` row expands `"${w#${z:-"${f[@]}"  X}}"`, and once
the `SX_STRIPDQ` rewrite was built as *text* the stripped `"` around `${f[@]}`
stopped making it a word list, so the pattern became the istring `  X` and
matched where bash's ` X` does not.

**The fix.** A `Shell::pat_quote` field spelling `Q_PATQUOTE`: `dquote` stays
set (the `$*` joining is genuinely shared) and the two tests ask
`Shell::dquote_bits()`, which is `dquote && !pat_quote`. It is raised by the new
`Shell::expand_modifier_pattern` — the `getpattern` wrapper the six modifier
sites now call, where a `case` arm and a `[[ == ]]` operand keep calling
`Shell::expand_word_pattern` and get no swap — and only where `dquote` was set
to begin with, since `quoted` passes through untouched otherwise. It is cleared
by `Shell::enter_dquote` (the new pair behind the three `" … "` entries) so a
quote written inside the pattern restores `Q_DOUBLE_QUOTES`, and by the
command-substitution body, which is a fresh `expand_word_internal`. The
replacement side needed no flag at all: `Shell::expand_replacement` now clears
`dquote` and `pat_quote` for its own walk.

**Regression cover:**
`userspace/oils/tests/corpus/a-modifiers-pattern-is-quoted-as-a-pattern-not-as-the-word.sh`
— the tilde through every removal/substitution/case-modification spelling and a
bulk operator over an array, the `SX_STRIPDQ` backslash proved both ways (the
stripped pattern would have matched `axb` and does not), the embedded quote that
puts the bit back, the replacement side, and the `$*`/list rows that show what
`Q_PATQUOTE` does keep.
