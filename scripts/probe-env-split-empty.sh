#!/bin/bash
# Round 5: does a word that ends up empty become an EMPTY ARGUMENT or no
# argument at all?
#
# This is the difference between `cmd ""` passing argc=2 and argc=1, and the
# implementation needs a "a word has begun" flag that is set by quotes but not
# by whitespace to get it right. Rounds 1-4 never put an empty word on its own:
# every expansion case was wrapped in [brackets], which supply the word.
set -u
ENV=/usr/bin/env
export FOO=bar
export EMPTY=

# printf with an [%s] format repeats the format once per argument, so the
# count of [] groups IS the argument count -- and zero arguments prints the
# format once with an empty substitution. To tell "one empty arg" from "no
# args" the format has to be applied by something that shows argc directly.
n() {
    local label="$1"; shift
    local out rc
    # `sh -c 'echo $#' x "$@"` reports the count of arguments after $0.
    out=$("$@" 2>&1)
    rc=$?
    printf '%-28s rc=%-4s argc=%s\n' "$label" "$rc" "$out"
}

echo "=== GNU $($ENV --version | head -1) ==="

# `\$` so the `$` survives -S's expansion pass and reaches sh; `\_` inside
# DOUBLE quotes so it is a literal space rather than a separator, keeping
# `echo $#` as one argument to -c. The first version of this line used a bare
# `$#` and an unquoted `\_`, and every case came back as the same bare-$ error
# -- which is what the control is for.
C='/bin/sh -c "echo\_\$#" ignored'

# Control: one real argument, then none. These must differ.
n 'CONTROL one arg'     "$ENV" -S"$C a"
n 'CONTROL no args'     "$ENV" -S"$C"

n 'empty double quotes' "$ENV" -S"$C \"\""
n 'empty single quotes' "$ENV" -S"$C ''"
n 'unset braced alone'  "$ENV" -S"$C \${NOPE}"
n 'empty var alone'     "$ENV" -S"$C \${EMPTY}"
n 'set var alone'       "$ENV" -S"$C \${FOO}"
n 'two empty quotes'    "$ENV" -S"$C \"\" \"\""
n 'quote then unset'    "$ENV" -S"$C \"\"\${NOPE}"

echo "=== done ==="
