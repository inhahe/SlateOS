# A subscript that reaches the shell as **bytes** — a name operand (`unset
# 'a[…]'`, `printf -v`, `read`, `test -v`) or a nameref target — was never read
# by the parser. Nothing gives it a parse back, so it is exactly the case of
# `a-pattern-of-an-unread-brace-is-read-with-the-brace-s-own-read.sh`, one layer
# out: the *read* and the *quoting* are two separate questions, and bash answers
# them in two different places.
#
# The read: `array_expand_index` hands the text to `expand_arith_string (exp,
# Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)` for an indexed array (arrayfunc.c:1354-
# 1358) and to `expand_subscript_string (sub, 0)` for an associative one
# (arrayfunc.c:1145). Both go through `expand_word_internal`, which only ever
# *parses* a `$( … )` — via `extract_command_subst` at expansion time. When that
# parse fails, `xparse_dolparen` reports it and then
#
#     clear_shell_input_line ();  …  jump_to_top_level (-nc);   /* y.tab.c:6563-6700 */
#
# abandons the whole command. So `unset 'a[$(fi)]'` is not a silent no-op: it
# prints the two-line command-substitution report and the command dies.
#
# The quoting: `expand_word_internal` translates no `$'…'` at all — that is the
# reader's job, and the two readers that do it (`read_token_word`, and
# `extract_heredoc_dolbrace_string` for the one span inside a here-document
# brace) never ran here. So the `$` of a `$'…'` is just a `$` and the `'…'` is
# just a single-quoted string: `$'a\tb'` names the key `$a\tb`, not the one with
# a real tab in it, and names it at *every* depth of the subscript, including
# inside a `${…}` operand written in it. Quote *removal* is a different
# mechanism — `expand_word_internal` does it itself, which is why the quotes
# come off at all — so an associative `m['x y']` still finds the key `x y`.
#
# A backquote is not parsed by the scan: it goes to `string_extract`, which only
# steps over its own extent. Its body is read later, by the backquote's own
# machinery, so its failure is reported on its own terms and the command around
# it still stands — the same split as in the brace-pattern case.
#
# Verified against bash 5.2.37.

echo "=== a failed comsub in a name operand reports and abandons the command"
a=(one two)
( unset 'a[$(fi)]' ) 2>&1; echo "after=$?"
( [[ -v 'a[$(fi)]' ]] ) 2>&1; echo "after=$?"
( printf -v 'a[$(fi)]' X ) 2>&1; echo "after=$?"
( [ -v 'a[$(fi)]' ] ) 2>&1; echo "after=$?"
( read -r 'a[$(fi)]' </dev/null ) 2>&1; echo "after=$?"

echo "=== a backquote is only stepped over, so it reports but does not abandon"
( unset 'a[`fi`]' ) 2>&1; echo "after=$?"

echo "=== the subscript is not translated: an ANSI-C word is not one here"
declare -A m
m[$'a\tb']=TAB
printf -v lit '%s' "\$'a\\tb'"
m[$lit]=LIT
printf '  keys:'; for k in "${!m[@]}"; do printf ' <%s>' "$k"; done; echo
[[ -v "m[\$'a\\tb']" ]];         echo "v-direct=$?"
[[ -v "m[\${z:-\$'a\\tb'}]" ]];  echo "v-nested=$?"
[[ -v 'm[$lit]' ]];              echo "v-byvar=$?"
printf -v "m[\$'a\\tb']" HIT
printf '  after:'; for k in "${!m[@]}"; do printf ' <%s>' "$k"; done; echo

echo "=== but quote removal is the expansion's own, and still happens"
declare -A q=([a]=A [x y]=XY)
[[ -v "q['a']" ]];   echo "sq=$?"
[[ -v 'q["a"]' ]];   echo "dq=$?"
[[ -v "q['x y']" ]]; echo "sq-space=$?"
[[ -v 'q[x y]' ]];   echo "bare-space=$?"

echo "=== an indexed subscript is arithmetic, and reports as arithmetic"
n=(z y x w)
( printf -v 'n[1+]' X ) 2>&1; echo "after=$?"
( unset "n['1']" ) 2>&1; echo "after=$?"

echo "=== a nameref target is the same text, read again at every use"
i=1
declare -n r='n[$i]'
echo "i=1 -> $r"
i=3; echo "i=3 -> $r"
( declare -n bad='n[$(fi)]'; echo "got <$bad>" ) 2>&1; echo "after=$?"

echo done
