# `${x:off:len}` is cut and read by two rules that a *command* tokenizer gets
# wrong in opposite directions, and osh used to have one where a tokenizer's
# answer stood in for both.
#
# **The cut.** Bash does not `strchr` for the `:`; `skiparith` (subst.c) walks
# the text with two counters. One `:` is skipped for every `?` seen, so a
# ternary keeps its own colon — and the count is not capped at one. Inside a
# `( … )` nothing counts at all, `?` and `:` alike, so an unbalanced `(` hides
# the rest of the text outright.
#
# **The read.** Neither bound is tokenized. The characters go to
# `expand_arith_string` and then `evalexp`, so every operator a command
# tokenizer would have claimed — `<`, `>`, `(`, `)`, `;`, `&`, `|` — is the
# arithmetic operator it looks like. The bounds are read exactly as the
# subscript beside them is: verbatim, with each top-level `' … '` given its
# second, arithmetic reading.
#
# A `<( … )` in a bound is therefore *not* performed either: the fragment is
# expanded under `Q_DOUBLE_QUOTES|Q_ARITH`, which is precisely what stops
# `expand_word_internal` reading one (subst.c:11079), so the evaluator meets the
# characters and its error names them.
#
# One shape is deliberately left out: an **unbalanced `(`** in the whole bounds
# text, which bash answers with `bad substitution: no closing `)' in <text>`
# before either bound is evaluated — a different diagnostic from the arithmetic
# one osh raises. See known-issues
# TD-OILS-AN-UNBALANCED-PAREN-IN-A-SLICES-BOUNDS-IS-AN-ARITHMETIC-ERROR-NOT-A-BAD-SUBSTITUTION.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== every operator a tokenizer would have claimed ==="
e 'z=abcdef; echo "${z:1<2}"'
e 'z=abcdef; echo "${z:1>2}"'
e 'z=abcdef; echo "${z:1<=2}"'
e 'z=abcdef; echo "${z:1>=2}"'
e 'z=abcdef; echo "${z:1<<2}"'
e 'z=abcdef; echo "${z:8>>2}"'
e 'z=abcdef; echo "${z:1&2}"'
e 'z=abcdef; echo "${z:3|2}"'
e 'z=abcdef; echo "${z:1&&2}"'
e 'z=abcdef; echo "${z:0||2}"'
e 'z=abcdef; echo "${z:1;2}"'
e 'z=abcdef; echo "${z:(1)<(2)}"'
e 'z=abcdef; echo "${z:1 < (2)}"'
e 'z=abcdef; echo "${z:0:1<2}"'
e 'z=abcdef; echo "${z:0:3>2}"'

echo "=== which colon cuts: one is skipped for each question mark ==="
e 'z=abcdef; echo "${z:1?2:3}"'
e 'z=abcdef; echo "${z:1?2:3:1}"'
e 'z=abcdef; echo "${z:0?2:3:1}"'
e 'z=abcdef; echo "${z:1?2:3:1?1:2}"'
e 'z=abcdef; echo "${z:1?2:3?4:5}"'
e 'z=abcdef; echo "${z:1?1?2:3:4}"'
e 'z=abcdef; echo "${z:2:1?2:3}"'
e 'z=abcdef; echo "${z:1?2:3:1:1}"'

echo "=== …and nothing inside a paren counts, colon or question mark ==="
e 'z=abcdef; echo "${z:(1?2:3)}"'
e 'z=abcdef; echo "${z:(1?2:3):1}"'
e 'z=abcdef; echo "${z:)1}"'

echo "=== a process substitution in a bound is read but never performed ==="
e 'z=abcdef; echo "${z:1<(2)}"'
e 'z=abcdef; echo "${z:0:<(echo 1)}"'
e 'z=abcdef; a=(q w e); echo "${a[1<(2)]}"'
e 'echo $(( 1<(2) ))'

echo "=== an empty bounds text is a bad substitution, uniformly ==="
e 'z=abcdef; echo "${z:}"'
e 'set -- a b c; echo "${@:}"'
e 'set -- a b c; echo "${*:}"'
e 'a=(q w e); echo "${a[@]:}"'
e 'a=(q w e); echo "${a[1]:}"'
e 'unset u; echo "${u:}"'
e 'z=abcdef; echo "${z::}"'
e 'z=abcdef; echo "${z::2}"'
e 'z=abcdef; echo "${z:2:}"'
e 'z=abcdef; echo "${z: }"'
e 'z=abcdef; e=; echo "${z:$e}"'
e 'z=abcdef; echo "${z:$(echo)}"'

echo "=== the ordinary bounds are unmoved ==="
e 'z=abcdef; echo "${z:2}"'
e 'z=abcdef; echo "${z:2:3}"'
e 'z=abcdef; echo "${z: -2}"'
e 'z=abcdef; echo "${z:(-2)}"'
e 'z=abcdef; echo "${z:1+1}"'
e 'z=abcdef; n=2; echo "${z:n}"'
e 'z=abcdef; n=2; echo "${z:$n}"'
e 'z=abcdef; echo "${z:$((1+1))}"'
e 'z=abcdef; a=(q w e r t); echo "${z:a[1]}"'
e 'z=abcdef; echo "${z:1,2}"'
e 'z=abcdef; echo "${z:0:-1}"'
e 'z=abcdef; echo "${z:"1"}"'
e "z=abcdef; echo \"\${z:'1'}\""
e 'z=abcdef; echo "${z:\1}"'
e 'z=abcdef; echo "${z:1#2}"'
e 'z=abcdef; echo "${z:q q}"'
e 'z=abcdef; echo "${z:1 2}"'
e 'z=abcdef; echo "${z:<}"'
e 'z=abcdef; echo "${z:>}"'
e 'z=abcdef; echo "${z:1)}"'
e 'z=abcdef; echo "${z:1:2:3}"'
echo TAIL
