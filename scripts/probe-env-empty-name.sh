#!/bin/bash
# Does GNU env treat `=novalue` as an assignment with an EMPTY NAME, or as a
# command name?
#
# env.rs asserts the latter in a doc comment -- "`=foo` is not an assignment,
# because there is no such variable, so GNU treats it as the command" -- and
# scripts/env-diff.sh measured GNU doing the opposite. One of them is wrong and
# the comment is the one with no measurement behind it.
set -u
ENV=/usr/bin/env

t() { printf '%-24s rc=%-4s %s\n' "$1" "$2" "$3"; }

run() {
    local label="$1"; shift
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    t "$label" "$rc" "$(printf '%s' "$out" | tr '\n' '|' | cut -c1-90)"
}

echo "=== GNU $($ENV --version | head -1) ==="

# Control: a word that is definitely NOT an assignment must be taken as a
# command and fail. If this passes, the probe is not distinguishing anything.
run 'CONTROL not-assign'  "$ENV" -i definitely_not_a_command_xyz

# Is the empty-name entry actually placed in the environment?
run 'empty name alone'    "$ENV" -i =novalue
run 'empty name + cmd'    "$ENV" -i =novalue /usr/bin/env
run 'bare ='              "$ENV" -i =
run 'empty name, =s left' "$ENV" -i =a=b /usr/bin/env

# And does the child actually inherit it? (execve keeps it or drops it.)
run 'child sees it'       "$ENV" -i =novalue /bin/sh -c 'env | grep -c "^=novalue$"'

echo "=== done ==="
