#!/bin/bash
# Which environment does -S's ${VAR} expansion see?
#
# todo.txt has claimed since 2026-08-22 that "the substitution must see -i, -u
# and the NAME=VALUE operands already applied", calling it "the subtle part".
# That is a design claim with no measurement behind it, and it decides whether
# expanding against the process environment (what the implementation does) is
# right or wrong. So: measure it.
set -u
ENV=/usr/bin/env
export PROBEVAR=fromparent

run() {
    local label="$1"; shift
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    printf '%-34s rc=%-4s %s\n' "$label" "$rc" "$(printf '%s' "$out" | tr '\n' '|')"
}

echo "=== GNU $($ENV --version | head -1) ==="

# Control: the variable is visible at all, and a name that is unset is not.
run 'CONTROL set'        "$ENV" -S'/usr/bin/printf [%s] ${PROBEVAR}'
run 'CONTROL unset'      "$ENV" -S'/usr/bin/printf [%s] ${NOSUCHVAR}'

# -i clears the environment. If expansion happens AFTER -i is applied, the
# variable is gone and the argument vanishes. If it happens against the
# process environment, it still expands.
run '-i before -S'       "$ENV" -i -S'/usr/bin/printf [%s] ${PROBEVAR}'

# -u removes one name. Same question, narrower.
run '-u before -S'       "$ENV" -u PROBEVAR -S'/usr/bin/printf [%s] ${PROBEVAR}'

# An assignment INSIDE the split string, referenced later in the same string.
run 'assign inside, used after' \
    "$ENV" -S'NEWVAR=made /usr/bin/printf [%s] ${NEWVAR}'

# -i inside the split string, with the reference after it.
run '-i inside, used after' \
    "$ENV" -S'-i /usr/bin/printf [%s] ${PROBEVAR}'

echo "=== done ==="
