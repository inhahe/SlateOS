# `bind` binds keys for a line editor. Neither shell here has one — bash says
# so itself, once per invocation, before it does anything else — and yet almost
# every part of the builtin still works, because readline's tables exist whether
# or not a terminal is driving them.
#
# What this case pins is that the work does *not* happen in the order the
# options were written. bash parses the whole option string first, then runs
# fixed phases, and the first failing phase returns:
#
#   1. a bad option or a missing argument — the offending letter, then the
#      synopsis on its own unprefixed line, at status 2, and nothing else runs;
#   2. `-m`'s keymap name — so `bind -l -m nosuchmap` prints not one of the 174
#      function names, even though `-l` came first;
#   3. the listings, `-l` among them;
#   4. `-f`'s file — so `bind -f /nosuch -l` prints all 174 *and then* fails;
#   5. `-u`, then `-q`, each rejecting an unknown function name;
#   6. `-r`, then `-x`, which is the one option bash parses itself.
#
# Phases 1 to 5 return the moment they fail, but `-q` and `-x` do not: they
# *assign* the status rather than or-ing into it, so a later phase that succeeds
# wipes out an earlier one's failure — `bind -q nosuchfn -x '"x": echo'` is 0
# even though the `-q` complained. That is the sharpest thing here, and it is
# not something a reimplementation would arrive at by accident.
#
# `-x` wants `"KEYSEQ": COMMAND`, and each way to get it wrong has its own
# wording — note that only the unterminated-quote one puts the spec last. A
# backslash inside the quotes escapes the next byte, so `"a\"b"` is one
# sequence. `-r` by contrast never validates anything.
#
# Each option that takes an argument keeps only its *last* one: `bind -q a -q b`
# complains about `b` alone, never `a`. Letters bundle, so `-lz` dies on `z`
# before `l` can list, and an argument may be attached (`-mvi`) or separate.
#
# An operand is not the shell's to refuse: readline reports it in its own voice,
# with no `bind:` prefix, one line per operand, and the status stays 0. A lone
# `-` is an operand, not an empty bundle. One that *has* a `:` terminator is
# accepted silently — there is simply nowhere for the binding to go.
#
# `INPUTRC=/dev/null` is essential: a system `/etc/inputrc` binds extra keys and
# would change the listings out from under the comparison.
#
# The tables are live, so the order of this script is part of what it tests: the
# listings of the untouched keymaps are taken first, then every way of changing
# a binding runs, then the listings are taken again. A shell that answered from
# constants would pass the first half and fail the second.
#
# Deliberately absent: `-f`, which reads an inputrc. osh reads none, so the only
# outcome it can produce honestly is the one bash produces for a file that is
# not there — which is what the `-f /nosuch/file` probes below check. See
# known-issues TD-OILS-NO-BIND-BUILTIN.
#
# Stderr is collected and replayed at the end so the warning — which would
# otherwise interleave unpredictably with the listings — can be compared in one
# fixed place.
export INPUTRC=/dev/null
exec 4>&2 2>err

echo "=== -l is readline's function list"
echo "  count     $(bind -l | wc -l)"
echo "  first     $(bind -l | head -1)"
echo "  last      $(bind -l | tail -1)"
bind -l >/dev/null; echo "  rc=$?"

echo "=== a bad option or missing argument is a usage error at status 2"
bind -z >/dev/null; echo "  -z        rc=$?"
echo "  -lz       lines=$(bind -lz 2>/dev/null | wc -l)"
bind -lz >/dev/null; echo "  -lz       rc=$?"
bind -m >/dev/null; echo "  -m alone  rc=$?"
bind -f >/dev/null; echo "  -f alone  rc=$?"
bind -q >/dev/null; echo "  -q alone  rc=$?"

echo "=== the keymap is checked before anything is listed"
echo "  -l -m bad lines=$(bind -l -m nosuchmap 2>/dev/null | wc -l)"
bind -l -m nosuchmap >/dev/null; echo "  -l -m bad rc=$?"
bind -m nosuchmap >/dev/null; echo "  -m bad    rc=$?"
bind -mnosuch >/dev/null; echo "  -mnosuch  rc=$?"
echo "  -m vi     lines=$(bind -m vi -l 2>/dev/null | wc -l)"
echo "  -mvi      lines=$(bind -mvi -l 2>/dev/null | wc -l)"
bind -m emacs -l >/dev/null; echo "  -m emacs  rc=$?"

echo "=== -f runs after the listing, not before it"
echo "  -f -l     lines=$(bind -f /nosuch/file -l 2>/dev/null | wc -l)"
bind -f /nosuch/file -l >/dev/null; echo "  -f -l     rc=$?"
bind -f /nosuch/file >/dev/null; echo "  -f        rc=$?"

echo "=== -u then -q, and only the last argument of each survives"
bind -u nosuchfn >/dev/null; echo "  -u        rc=$?"
bind -q nosuchfn >/dev/null; echo "  -q        rc=$?"
bind -q other -q nosuchfn >/dev/null; echo "  -q -q     rc=$?"
bind -u nosuchfn -f /nosuch/file >/dev/null; echo "  -u -f     rc=$?"
# A `-u` that names a function readline knows really does unbind it — every
# later `-p`, `-P` and `-q yank` would then see a table with no `yank` in it.
# That is a real effect worth testing, but it would swamp everything else here,
# so it is held back to the mutation section at the end.

echo "=== the listings, and the fixed order the sections come out in"
bind -p
bind -P
bind -v
bind -V
echo "  -pv == -vp $( [ "$(bind -pv 2>/dev/null)" = "$(bind -vp 2>/dev/null)" ] && echo yes || echo no )"
echo "  combined  lines=$(bind -lpvsPVSX 2>/dev/null | wc -l)"

echo "=== every keymap, and the aliases among them"
for m in emacs emacs-standard emacs-meta emacs-ctlx vi vi-move vi-command vi-insert; do
  echo "  $m p=$(bind -m $m -p 2>/dev/null | wc -l) P=$(bind -m $m -P 2>/dev/null | wc -l)"
done
echo "=== -m shows through the keymap variable, under its canonical name"
for m in emacs-standard vi-command vi-move vi-insert; do
  bind -m $m -v 2>/dev/null | grep '^set keymap '
done

echo "=== -q answers a known name on stdout, and readline gives up after five"
bind -q accept-line
bind -q digit-argument
bind -q self-insert
bind -m vi -q yank
bind -q alias-expand-line; echo "  unbound   rc=$?"
bind -q yank >/dev/null; echo "  bound     rc=$?"

echo "=== a pristine readline has no macros and no -x bindings"
echo "  -X        lines=$(bind -X 2>/dev/null | wc -l)"
echo "  -s        lines=$(bind -s 2>/dev/null | wc -l)"
echo "  -S        lines=$(bind -S 2>/dev/null | wc -l)"
bind -- >/dev/null; echo "  --        rc=$?"

# An accepted `-x` really does install a binding, which a later `bind -X` would
# then list — so each probe below runs in its own subshell, which is what makes
# every line here answerable on its own rather than on whatever the line above
# it left behind. That the isolation *works* is itself a claim: the tables are
# a copy in the child, and the `-X`/`-p` dumps after this section prove none of
# it leaked back. The parent does its own mutating further down.
echo "=== -x parses its own spec, and -r validates nothing"
( bind -x '"x": echo hi' >/dev/null; echo "  ok        rc=$?" )
( bind -x '"\C-t": echo hi' >/dev/null; echo "  keyseq    rc=$?" )
( bind -x '"a\"b": echo' >/dev/null; echo "  escaped   rc=$?" )
( bind -x '"x":' >/dev/null; echo "  no cmd    rc=$?" )
( bind -x ' "x": echo' >/dev/null; echo "  leading   rc=$?" )
( bind -x 'x:echo hi' >/dev/null; echo "  unquoted  rc=$?" )
( bind -x 'x' >/dev/null; echo "  bare      rc=$?" )
( bind -x '"x"' >/dev/null; echo "  no colon  rc=$?" )
( bind -x '"x" echo' >/dev/null; echo "  no colon2 rc=$?" )
( bind -x '"unterminated: echo' >/dev/null; echo "  unclosed  rc=$?" )
( bind -x badA -x badB >/dev/null; echo "  -x -x     rc=$?" )
( bind -r 'not a keyseq' >/dev/null; echo "  -r junk   rc=$?" )
( bind -r '' >/dev/null; echo "  -r empty  rc=$?" )
( bind -r '\C-t' >/dev/null; echo "  -r        rc=$?" )

echo "=== a later phase assigns the status over an earlier failure"
( bind -q nosuchfn >/dev/null; echo "  -q         rc=$?" )
( bind -q nosuchfn -x '"x": echo' >/dev/null; echo "  -q -x ok   rc=$?" )
( bind -x bad -q nosuchfn >/dev/null; echo "  -q -x bad  rc=$?" )
( bind -q nosuchfn ccc >/dev/null; echo "  -q operand rc=$?" )
( bind -x bad ccc >/dev/null; echo "  -x operand rc=$?" )
( bind -l -x bad >/dev/null; echo "  -l -x bad  rc=$?" )
( bind -q nosuchfn -r x >/dev/null; echo "  -q -r      rc=$?" )

echo "=== an operand is readline's to refuse, and is not a failure"
( bind aaa bbb >/dev/null; echo "  two       rc=$?" )
( bind - >/dev/null; echo "  dash      rc=$?" )
( bind '"\C-t": yank' >/dev/null; echo "  bound     rc=$?" )

echo "=== none of that escaped the subshell it happened in"
echo "  -X        lines=$(bind -X | wc -l)"
echo "  -s        lines=$(bind -s | wc -l)"
bind -q transpose-chars

# Everything from here changes the tables of *this* shell, and every listing
# after a change is taken again — which is the half of this file that a shell
# answering from constants cannot pass.
echo "=== the parent binds, and the listings follow it"
bind '"\C-x\C-t": yank'; echo "  bind      rc=$?"
bind -q yank
bind -p | grep 'C-x.C-t'
bind -P | grep '^yank '
# A second sequence for the same function, and readline reports both.
bind -m vi-insert '"\C-t": yank'; echo "  -m bind   rc=$?"
bind -m vi-insert -q yank
bind -q yank

echo "=== a macro and a command binding show up in their own listings"
bind '"\C-x\C-m": "hello"'; echo "  macro     rc=$?"
bind -s
bind -S
bind -x '"\C-x\C-v": echo hi'; echo "  -x        rc=$?"
bind -X
# A macro is not a function, so the function dumpers skip it and the macro
# dumpers skip every function — the same key is in exactly one of the two.
echo "  -p macro  $(bind -p | grep -c 'C-x.C-m')"
echo "  -s count  $(bind -s | wc -l)"

echo "=== set through an operand, and -v follows that too"
bind 'set mark-modified-lines on'; echo "  on        rc=$?"
bind -v | grep 'mark-modified-lines'
bind 'set mark-modified-lines whatever'; echo "  whatever  rc=$?"
bind -v | grep 'mark-modified-lines'
bind 'set comment-begin ;;'; echo "  string    rc=$?"
bind -v | grep 'comment-begin'
bind 'set bell-style visible'
bind -v | grep 'bell-style'
bind 'set keyseq-timeout 250'
bind -v | grep 'keyseq-timeout'
bind 'set nosuchvariable on'; echo "  unknown   rc=$?"
bind 'set editing-mode vi'; echo "  vi        rc=$?"
bind -v | grep -E '^set (keymap|editing-mode) '
bind 'set editing-mode emacs'
bind -v | grep -E '^set (keymap|editing-mode) '

echo "=== convert-meta decides how a meta key is spelled back"
bind '"\M-\C-k": yank'; echo "  meta      rc=$?"
bind -q yank
bind 'set convert-meta off'
bind -q yank
bind 'set convert-meta on'
bind -q yank

echo "=== every way of taking a binding away again"
# An operand whose target is not a function readline knows is an unbind, and
# readline says nothing about it.
bind '"\C-x\C-t": nosuchfunction'; echo "  operand   rc=$?"
bind -q yank
bind -r '\C-x\C-m'; echo "  -r macro  rc=$?"
echo "  -s count  $(bind -s | wc -l)"
bind -r '\C-x\C-v'; echo "  -r -x     rc=$?"
echo "  -X count  $(bind -X | wc -l)"
bind -u yank; echo "  -u        rc=$?"
bind -q yank; echo "  gone      rc=$?"
echo "  -p yank   $(bind -p | grep -c ': yank$')"
# `-u` is per-keymap: the vi-insert binding made above is untouched by an
# unbind that ran against emacs.
bind -m vi-insert -q yank; echo "  vi-insert rc=$?"

echo "=== it is a builtin like any other"
type -t bind
command -v bind
help -s bind

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
