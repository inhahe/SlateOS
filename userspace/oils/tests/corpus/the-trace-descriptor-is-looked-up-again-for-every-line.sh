# `$BASH_XTRACEFD` holds a descriptor *number*, and the shell looks that number
# up again for every trace line it writes.
#
# So the trace follows fd N wherever a later redirect points it — `exec 8> a;
# BASH_XTRACEFD=8; exec 8> b` traces into `b`, not into `a`. The one event that
# ends the diversion is fd N ceasing to exist: after `exec 8>&-` the trace is
# back on stderr, and reopening the same number does not bring it back.
#
# The rest of the name's behaviour is the same "one setting, two spellings"
# shape `$GLOBIGNORE` has, with three details worth writing down —
#
#   * the value is read the way `strtol` reads it, so ` 8`, `+8` and `08` all
#     name fd 8 while `8x` names nothing;
#   * a value that names no *open* descriptor is a complaint and nothing else:
#     the diversion already in force is left exactly as it was;
#   * losing the value — `unset`, an empty assignment, a `local` going out of
#     scope — puts the trace back on stderr **and closes the descriptor**.
#     Moving the name from one descriptor to another closes neither.

echo "=== the trace goes to the descriptor, not to stderr"
exec 8> a.txt
BASH_XTRACEFD=8
set -x
echo one
set +x
exec 8>&-
unset BASH_XTRACEFD
echo "  a:"; sed 's/^/    /' a.txt

echo "=== and follows the number through a later redirect"
exec 8> b.txt; exec 8> c.txt
BASH_XTRACEFD=8
exec 8> d.txt
set -x
echo two
set +x
exec 8>&-
unset BASH_XTRACEFD
echo "  c lines=$(wc -l < c.txt) d lines=$(wc -l < d.txt)"

echo "=== closing the number ends it, and reopening does not undo that"
exec 8> e.txt
BASH_XTRACEFD=8
exec 8>&-
exec 8> f.txt
set -x
echo three
set +x
exec 8>&-
unset BASH_XTRACEFD
echo "  e lines=$(wc -l < e.txt) f lines=$(wc -l < f.txt)"

echo "=== which spellings name a descriptor"
exec 8> g.txt
for v in 8 08 +8 ' 8' 8x -1 '' 99999 notanumber; do
  ( BASH_XTRACEFD="$v"; set -x; echo hi; set +x ) 2>&1 | sed "s/^/  [$v] /"
done
exec 8>&-
unset BASH_XTRACEFD
echo "  g lines=$(wc -l < g.txt)"

echo "=== a value naming no open descriptor leaves the diversion alone"
exec 8> h.txt
BASH_XTRACEFD=8
BASH_XTRACEFD=99
set -x
echo four
set +x
exec 8>&-
unset BASH_XTRACEFD
echo "  h lines=$(wc -l < h.txt)"

echo "=== an empty value resets it, and closes the descriptor"
exec 8> i.txt
BASH_XTRACEFD=8
BASH_XTRACEFD=
echo direct >&8 2>&1; echo "  st=$?"
set -x
echo five
set +x
unset BASH_XTRACEFD
echo "  i=[$(cat i.txt)]"

echo "=== unsetting it closes the descriptor too"
exec 8> j.txt
BASH_XTRACEFD=8
unset BASH_XTRACEFD
echo direct >&8 2>&1; echo "  st=$?"
echo "  j=[$(cat j.txt)]"

echo "=== but moving it from one descriptor to another closes neither"
exec 8> k.txt; exec 9> l.txt
BASH_XTRACEFD=8
BASH_XTRACEFD=9
echo direct8 >&8 2>&1; echo "  st8=$?"
set -x
echo six
set +x
unset BASH_XTRACEFD
exec 8>&- 2>/dev/null; exec 9>&- 2>/dev/null
echo "  k=[$(cat k.txt)] l lines=$(wc -l < l.txt)"

echo "=== a subshell and a function trace into it as well"
exec 8> m.txt
BASH_XTRACEFD=8
( set -x; echo seven; set +x )
f() { set -x; echo eight; set +x; }
f
exec 8>&-
unset BASH_XTRACEFD
echo "  m lines=$(wc -l < m.txt)"

echo "=== a scope that ends is the name losing its value"
exec 8> n.txt
BASH_XTRACEFD=8
g() { local BASH_XTRACEFD=2; set -x; echo nine; set +x; }
g 2>&1
set -x
echo ten
set +x
exec 8>&- 2>/dev/null
unset BASH_XTRACEFD
echo "  n lines=$(wc -l < n.txt)"

echo "=== it is an ordinary variable otherwise"
exec 8> o.txt
BASH_XTRACEFD=8
echo "  v=[$BASH_XTRACEFD]"
declare -p BASH_XTRACEFD
exec 8>&-
unset BASH_XTRACEFD
echo "  after unset v=[${BASH_XTRACEFD-unset}]"
