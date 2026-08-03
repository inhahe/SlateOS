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
# Deliberately absent:
#
#   * the dump listings `-p`, `-v`, `-P`, `-V`. They print readline's *default
#     keymaps*, which osh does not carry yet — only the function-name list. So
#     is `-q` for a name that exists, which prints where that function is bound.
#     `-s`, `-S` and `-X` are here, because a pristine readline has no macros
#     and no `-x` bindings and so prints nothing, which osh matches exactly.
#     See known-issues TD-OILS-NO-BIND-BUILTIN.
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
# A `-u` that names a function readline knows really does unbind it in bash —
# every later `-p`, `-P` and `-q yank` in this script would then disagree with
# osh, whose tables are read-only — so this one probe runs in a subshell, for
# the same reason the `-x` probes below do.
( bind -u yank >/dev/null; echo "  -u known  rc=$?" )

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

# From here on an accepted `-x` really does install a binding in bash, which a
# later `bind -X` would then list — so every probe below runs in a subshell,
# and the listings above were taken before any of them ran.
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

echo "=== it is a builtin like any other"
type -t bind
command -v bind
help -s bind

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
