# A here-document body is a double-quoted *context* but not a double-quoted
# *word*, and the difference is which reader collected it. bash's reader takes
# the body with `read_secondary_line` and hands the lines to
# `make_here_document` (make_cmd.c:621) as text — `read_token_word` never runs
# over it, so `parse_matched_pair` never does either. Two things follow, and
# both are measurable:
#
#   * **Nothing in it is translated at parse time.** A `$'…'` inside a `${ … }`
#     in the body is still `$'…'` when `expand_word_internal` reaches it, and
#     there it is not a quote at all: the ANSI-C branch is taken only when
#     `(quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) == 0`. So it stays literal —
#     where the *same* word written in a real double-quoted string is translated
#     by the parser and spliced bare (see
#     `a-brace-body-in-a-quoted-command-substitution-stands-in-the-quote.sh`).
#
#   * **The delimiter stack is empty for it.** A `$( … )` in the body is parsed
#     by `command_substitute` at expansion time, from a fresh reader, so the
#     `${ … }` inside *that* is not `P_DQUOTE` and re-quotes — even though the
#     substitution is written in what is nominally a quoted run.
#
# The same two hold for every other text the shell expands as a double-quoted
# run without a parser having read it as a word: `PS4` before an `xtrace` line
# and `${x@P}`. There the string arrived as a *value*, so again nothing in it
# was translated and a `$'…'` in a `${ … }` prints back as written.
#
# A here-string is the opposite case and is here as the control: `<<<` takes an
# ordinary WORD, so `read_token_word` does read it and the usual rules apply.
#
# Verified against bash 5.2.37.

unset x y z

echo "=== the body: what a \$'…' in it does, by where it sits"
r() { printf '%-28s -> ' "$1"; ( eval "$2" ) 2>&1 | cat -A; }
r 'a bare one'              $'cat <<E\n$\'a\\x2Cb\'\nE'
r 'in a ${ } word'          $'cat <<E\n${x:-$\'a\\x2Cb\'}\nE'
r 'in a nested ${ } word'   $'cat <<E\n${y:-${x:-$\'a\\x2Cb\'}}\nE'
r 'past a ${ } operator'    $'cat <<E\n${x#$\'a\\x2Cb\'}\nE'
r 'a plain quote, for size' $'cat <<E\n${x:-\'q\'}\nE'
r 'a double quote'          $'cat <<E\n${x:-"q"}\nE'
r 'a quoted delimiter'      $'cat <<\'E\'\n${x:-$\'a\\x2Cb\'}\nE'
r 'a here-string is a word' $'cat <<< "${x:-$\'a\\x2Cb\'}"'

echo "=== and the same body's substitutions are parsed from an empty stack"
# A real tab, so the answer is visible as splitting: re-quoted keeps it inside a
# `'…'` and `echo` prints one field, spliced bare it is a live tab and two.
r 'a $( ) in the body'      $'cat <<E\n$(echo ${x:-$\'a\\tb\'})\nE'
r 'with its own quotes'     $'cat <<E\n$(echo "$(echo ${x:-$\'a\\tb\'})")\nE'
r 'a backquote in the body' $'cat <<E\n`echo ${x:-$\'a\\tb\'}`\nE'
r 'the word case, to compare' $'echo "$(echo ${x:-$\'a\\tb\'})"'

echo "=== the printbacks say the same thing about the parse"
f1() { cat <<E
${x:-$'a\x2Cb'}
E
}
f2() { cat <<E
$(echo ${x:-$'a\x2Cb'})
E
}
f3() { cat <<< "${x:-$'a\x2Cb'}"; }
declare -f f1 f2 f3

echo "=== a value expanded as a double-quoted run was not read by a parser either"
v='${y:-$'"'"'a\x2Cb'"'"'}'
printf '@P      : %s\n' "${v@P}"
w='${y:-x$'"'"'a\x2Cb'"'"'y}'
printf '@P word : %s\n' "${w@P}"
u='${y:-${z:-$'"'"'a\x2Cb'"'"'}}'
printf '@P nest : %s\n' "${u@P}"
( PS4="$v+ "; set -x; : mark; set +x ) 2>&1

echo "=== a body that will not parse is a *runtime* failure, not a parse error"
# The body was never parsed when the script was read, so a syntax error in it
# cannot be a syntax error in the script: `extract_command_subst` finds it at
# expansion time, and what it runs is `xparse_dolparen` (parse.y:4248) — through
# `parse_string`, whose failure is `jump_to_top_level (DISCARD)`. That abandons
# the enclosing command with `$?` at 1 and lets the script carry on. Compare the
# word case on the last row, which never reaches this because the *parser*
# rejected it while reading the script.
e() { printf '%-28s -> ' "$1"; ( eval "$2"; echo "rc=$?" ) 2>&1 | sed 's/^.*: command substitution:/cs:/'; }
e 'a bad $( ) in the body'   $'cat <<E\n$(fi)\nE'
e 'a bad backquote'          $'cat <<E\n`fi`\nE'
e 'and the script goes on'   $'cat <<E\n$(fi)\nE\necho next'

echo "=== a prompt expansion reports it and runs it anyway"
# `expand_prompt_string` (subst.c:4459) raises `no_longjmp_on_fatal_error` around
# the whole expansion, which `extract_command_subst` (subst.c:1289) turns into
# `SX_NOLONGJMP`. All that flag reaches is the one guard around the raise in
# `xparse_dolparen` (parse.y:4330) — so the diagnostic is printed, nothing is
# abandoned, and `command_substitute` parses the body a second time in the child
# and reports it a second time. The second report is one line *lower*, because
# the child goes through `parse_and_execute`, which does `line_number--`
# (evalstring.c:329), and the first went through `parse_string`, which does not.
#
# (Nothing may follow the substitution in the string on these rows: a failed
# extent-parse consumes the *whole* remainder rather than stopping at the `)`,
# which osh does not yet model — see known-issues.md,
# `TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING`.)
b='$(fi)'
printf '@P bad  : [%s]\n' "${b@P}"
echo "after rc=$?"
( PS4="$b"; set -x; : mark; set +x ) 2>&1
echo "prompt rc=$?"

echo done
