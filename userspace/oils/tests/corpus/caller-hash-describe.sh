# `caller` reads the *call stack*, not the `FUNCNAME` variable; `hash -d` is
# forgiving until the hash table exists; and `command -V` describes a function
# exactly as `type` does — bash routes both through one `describe_command()`.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== caller at the top level of a script has a frame of its own"
# The script itself is BASH_SOURCE[0], so the bare form succeeds with the
# literal NULL for the absent BASH_SOURCE[1].
( caller; echo "rc=$?" ) 2>&1 | e
# …but there is no FUNCNAME[1] to name, so the numbered form fails silently.
( caller 0; echo "rc=$?" ) 2>&1 | e
( caller 9; echo "rc=$?" ) 2>&1 | e

echo "=== the operand is validated even outside a function"
( caller zz; echo "rc=$?" ) 2>&1 | e
( caller 1zz; echo "rc=$?" ) 2>&1 | e

echo "=== no options, and -- ends them"
( caller -1; echo "rc=$?" ) 2>&1 | e
( caller -x; echo "rc=$?" ) 2>&1 | e
( caller --; echo "rc=$?" ) 2>&1 | e
# A negative frame parses as a number and then names nothing: silent 1.
( caller -- -1; echo "rc=$?" ) 2>&1 | e

echo "=== inside one function"
f1() { caller; caller 0; caller 1; echo "rc=$?"; }
f1 2>&1 | e

echo "=== inside a nested function"
g1() { caller; caller 0; caller 1; caller 2; echo "rc=$?"; }
h1() { g1; }
h1 2>&1 | e

echo "=== operands past the first are ignored"
f2() { caller 0 9 zz; echo "rc=$?"; }
f2 2>&1 | e

echo "=== a sourced file is a frame even though FUNCNAME stays empty"
printf 'echo "nFUNCNAME=${#FUNCNAME[@]}"\ncaller; echo "rc=$?"\ncaller 0; echo "rc=$?"\ncaller 1; echo "rc=$?"\n' > caller-src.sh
. ./caller-src.sh 2>&1 | e

echo "=== hash -d before anything has ever been hashed"
# bash allocates its hash table lazily, and `phash_remove` succeeds without
# looking while the table is still null.
( hash -d nosuchcmd; echo "rc=$?" ) 2>&1 | e
( hash -d a b c; echo "rc=$?" ) 2>&1 | e

echo "=== …and after"
( hash -p /bin/xx yy; hash -d nosuchcmd; echo "rc=$?" ) 2>&1 | e
# `-r` empties the table but does not deallocate it, so the error stays.
( hash -p /bin/xx yy; hash -r; hash -d nosuchcmd; echo "rc=$?" ) 2>&1 | e
( hash -p /bin/xx yy; hash -d yy; echo "rc=$?"; hash ) 2>&1 | e
# The other flags never got the short-circuit.
( hash -t nosuchcmd; echo "rc=$?" ) 2>&1 | e

echo "=== -r is not terminal: with names it flushes and then re-hashes"
( hash -p /bin/xx yy; hash -r nosuch; echo "rc=$?"; hash ) 2>&1 | e
( hash -p /bin/xx yy; hash -r yy; echo "rc=$?"; hash ) 2>&1 | e
( hash -r; echo "rc=$?" ) 2>&1 | e

echo "=== a name with a slash is a path, so it is never hashed"
( hash /bin/sh; echo "rc=$?"; hash ) 2>&1 | e
( hash a/b; echo "rc=$?" ) 2>&1 | e
( hash -p /x /bin/sh; echo "rc=$?"; hash ) 2>&1 | e
( hash -p /bin/xx yy; hash -d /bin/sh; echo "rc=$?"; hash ) 2>&1 | e
( hash -l /bin/sh; echo "rc=$?" ) 2>&1 | e
# …except under -t, which answers before that check.
( hash -t /bin/sh; echo "rc=$?" ) 2>&1 | e

echo "=== -l selects nothing; it only reshapes"
( hash -p /bin/xx yy; hash -l yy; echo "rc=$?" ) 2>&1 | e
( hash -p /bin/xx yy; hash -p /bin/zz zz; hash -t yy zz ) 2>&1 | e
( hash -p /bin/xx yy; hash -p /bin/zz zz; hash -lt yy zz ) 2>&1 | e
( hash -p /bin/xx yy; hash -lt yy; echo "rc=$?" ) 2>&1 | e
( hash -p /bin/xx yy; hash -l ) 2>&1 | e

echo "=== every -t miss is reported, in place"
( hash -p /bin/xx yy; hash -t yy nosuch zz; echo "rc=$?" ) 2>&1 | e

echo "=== -d and -t need an operand, before -r flushes anything"
( hash -d; echo "rc=$?" ) 2>&1 | e
( hash -t; echo "rc=$?" ) 2>&1 | e
( hash -rd; echo "rc=$?" ) 2>&1 | e
( hash -rt; echo "rc=$?" ) 2>&1 | e
( hash -l; echo "rc=$?" ) 2>&1 | e
( hash -p /x; echo "rc=$?" ) 2>&1 | e

echo "=== -p takes its argument the way getopt does"
( hash -p/x y; echo "rc=$?"; hash ) 2>&1 | e
( hash -rp /x y; echo "rc=$?"; hash ) 2>&1 | e
( hash -p /x a b; hash ) 2>&1 | e
( hash -rp; echo "rc=$?" ) 2>&1 | e
( hash -Z; echo "rc=$?" ) 2>&1 | e

echo "=== command -V describes a function the way type does"
fn() { :; }
( command -V fn; echo "rc=$?" ) 2>&1 | e
( type fn; echo "rc=$?" ) 2>&1 | e
( command -v fn; echo "rc=$?" ) 2>&1 | e

echo "=== a body worth reconstructing"
fb() {
  local x=1
  if [ "$x" = 1 ]; then
    echo one
  fi
}
( command -V fb ) 2>&1 | e
( type -a fb ) 2>&1 | e

echo "=== the other describable shapes are unchanged"
( command -V while; echo "rc=$?" ) 2>&1 | e
( command -V command; echo "rc=$?" ) 2>&1 | e
alias al='echo hi'
( command -V al; echo "rc=$?" ) 2>&1 | e

echo "=== done"
