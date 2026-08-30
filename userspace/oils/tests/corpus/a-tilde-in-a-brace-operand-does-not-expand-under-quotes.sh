# `expand_word_internal`'s `case '~'` refuses on three counts, and the third is
# about the *expansion's* quoting rather than the word's:
#
#     if ((word->flags & W_NOTILDE) ||
#         (sindex > 0 && (internal_tilde == 0)) ||
#         (quoted & (Q_DOUBLE_QUOTES|Q_HERE_DOCUMENT)))
#       { … goto add_character; }                  /* subst.c:11182-11192 */
#
# A word written inside `" … "` never gets that far — its text is the contents
# of a quoted run — so the third test looks redundant. What makes it visible is
# a **brace operand**: `parameter_brace_expand_rhs` hands its own `quoted`
# straight down,
#
#     l = expand_string_for_rhs (temp, quoted, op, pflags, …);  /* subst.c:7737 */
#
# so the quotes around the *substitution* reach the operand's own leading `~`,
# which is written outside any quotes of its own.
#
# `~` is therefore one of the few things that expands differently for being in
# a `${x:-…}` inside double quotes than for being in the same `${x:-…}` bare —
# and it is the enclosing word's quoting that decides, not the operand's.
#
# Verified against bash 5.2.37.

HOME=/hh
unset z
f() { printf '%s [%s]\n' "$1" "$2"; }

echo "=== 1. the operand of every operator that has one"
f 'q :-'   "${z:-~}"
f 'u :-'   ${z:-~}
f 'q -'    "${z-~}"
f 'q :='   "${z:=~}"; f 'q := z' "$z"; unset z
f 'q ='    "${z=~}";  f 'q = z'  "$z";  unset z
w=WW
f 'q :+'   "${w:+~}"
f 'u :+'   ${w:+~}

echo "=== 2. the other two tests still stand"
# `sindex > 0` — a tilde that is not the operand's first character.
f 'q mid'  "${z:-a~}"
f 'u mid'  ${z:-a~}
# The tilde-prefix runs to the first `/`, quoted or not.
f 'q slash' "${z:-~/a}"
f 'u slash' ${z:-~/a}
# A prefix that names nobody is left alone either way.
f 'q nouser' "${z:-~nosuchuser99}"
f 'u nouser' ${z:-~nosuchuser99}

echo "=== 3. the enclosing quoting decides, not the operand's own"
# Quotes *in* the operand are taken out before it is expanded — see
# a-used-brace-operands-embedded-quotes-lose-their-backslashes.sh — so they
# cannot be what suppresses the tilde. The `u inner` row is the proof: there
# the operand's own quotes are all there is, and the tilde still stands.
f 'q inner' "${z:-"~"}"
f 'u inner' ${z:-"~"}
v="${z:-~}"; f 'assign q' "$v"
v=${z:-~};  f 'assign u' "$v"
# `Q_HERE_DOCUMENT` is the same arm of the same test.
cat <<EOF
heredoc [${z:-~}]
EOF

echo "=== 4. a nested operand asks the same question of the same quotes"
f 'nest q' "${z:-${y:-~}}"
f 'nest u' ${z:-${y:-~}}

echo "=== 5. and an ordinary word is untouched by any of it"
f 'word u' ~
f 'word q' "~"
u=~;   f 'plain assign'  "$u"
u="~"; f 'quoted assign' "$u"
u=a:~; f 'colon assign'  "$u"

echo done
