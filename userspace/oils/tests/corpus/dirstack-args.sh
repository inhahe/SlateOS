# Argument grammar of `cd`/`pushd`/`popd`/`dirs`, measured against bash 5.2.
# The three stack builtins each parse their words differently, and every
# difference below is observable:
#   * `cd` rejects a second operand with a bare "too many arguments" (no usage
#     line, no chdir); an empty operand is a *logical* no-op but a real
#     chdir("") under -P. `cd -` echoes on stdout, so a redirection silences it.
#   * none of them cluster option letters — `dirs -cp` is read as an index and
#     reported as an "invalid number", not an unknown option.
#   * a bad index is "invalid number" + usage (status 2); an out-of-range one is
#     "directory stack empty" when nothing is saved, else "index out of range"
#     — printed *with* its sign by pushd/popd but bare by dirs. Status 1.
#   * `--` ends option processing. For `dirs`/`popd` (which take no operand) it
#     discards everything after it; for `pushd` the next word becomes a literal
#     directory, so `pushd -- +1` tries to chdir to "+1".
#   * a recognised `+N`/`-N` ends pushd's scan entirely (`pushd +1 foo` ignores
#     foo), but popd/dirs keep scanning and reject the trailing word.
#   * a directory operand ends pushd's option processing, so a trailing `-n`
#     or `--` counts as a second operand.
#   * `pushd -n` prints nothing when there is no directory operand (both the
#     no-argument swap and the `+N` rotation are silent), while `popd -n` does
#     print the stack.
#
# Every stack-mutating builtin is run in the *current* shell (output captured
# through a file, never a pipeline) so the case measures argument parsing only.
mkdir -p root/a root/b
cd root
ROOT=$PWD
O=$ROOT/out.txt
# The tests run in a randomly-named temp directory, so fold the absolute
# prefix away before comparing any printed stack.
show() { sed "s|$ROOT|@|g" "$O"; }
reset() { cd "$ROOT"; dirs -c; }

cd a b
echo "cd2=$? d=${PWD##*/}"
cd ""
echo "cdempty=$? d=${PWD##*/}"
cd -P ""
echo "cdemptyP=$? d=${PWD##*/}"
cd -- a
echo "cddd=$? d=${PWD##*/}"
cd - >"$O"
echo "cddash=$? d=${PWD##*/}"; show
cd a
cd - >/dev/null
echo "cdquiet=$? d=${PWD##*/}"

echo "=== bad options"
reset
dirs -cp; echo "rc=$?"
dirs foo; echo "rc=$?"
dirs -z; echo "rc=$?"
pushd -z; echo "rc=$?"
popd -z; echo "rc=$?"
pushd a b; echo "rc=$?"
popd a; echo "rc=$?"

echo "=== empty stack"
dirs +5; echo "rc=$?"
pushd +5; echo "rc=$?"
popd +0; echo "rc=$?"
popd -0; echo "rc=$?"
pushd; echo "rc=$?"

echo "=== out of range"
pushd a >/dev/null
dirs +5; echo "rc=$?"
dirs -5; echo "rc=$?"
pushd +5; echo "rc=$?"
popd -5; echo "rc=$?"

echo "=== index 0 is always in range"
dirs +0 >"$O"; show
dirs -0 >"$O"; show
popd +0 >"$O"
echo "popd0=$? d=${PWD##*/}"; show

echo "=== end of options"
reset
pushd a >/dev/null
dirs -- -p >"$O"; echo "rc=$?"; show
dirs -- foo >"$O"; echo "rc=$?"; show
dirs +1 -- >"$O"; echo "rc=$?"; show
dirs -p -- >"$O"; echo "rc=$?"; show
popd -- foo >"$O"; echo "rc=$? d=${PWD##*/}"; show
pushd -- +1; echo "rc=$?"
pushd -- a >"$O"; echo "rc=$? d=${PWD##*/}"; show
pushd a --; echo "rc=$?"
pushd a -n; echo "rc=$?"

echo "=== trailing words"
reset
pushd a >/dev/null
pushd +1 foo >"$O"; echo "rc=$? d=${PWD##*/}"; show
popd +1 foo; echo "rc=$?"
dirs +1 foo; echo "rc=$?"

echo "=== -n"
reset
pushd -n; echo "rc=$?"
pushd -n -n; echo "rc=$?"
pushd a >/dev/null
pushd -n; echo "rc=$? d=${PWD##*/}"
pushd -n ../b >"$O"; echo "rc=$? d=${PWD##*/}"; show
pushd -n +1 >"$O"; echo "rc=$? d=${PWD##*/}"; show
dirs >"$O"; show
popd -n >"$O"; echo "rc=$? d=${PWD##*/}"; show
