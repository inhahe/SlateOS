#!/bin/bash
# Round 4: the complete -S escape table, and the exact diagnostic wording.
#
# Rounds 1-2 measured \t \n \_ \c \# \$ and found that an unknown escape is an
# ERROR ("invalid sequence '\q' in -S"). That makes the set closed: every
# escape I do not implement becomes a refusal GNU does not give. So the table
# has to be enumerated rather than sampled -- omitting \r here would ship a
# wrong error message, not a missing feature.
set -u
ENV=/usr/bin/env
export FOO=bar

# Show the argument bytes unambiguously: od gives the escape's actual value,
# which is the point -- `\t` printing as whitespace tells us nothing about
# whether it is a tab or a space.
show() {
    local label="$1"; shift
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    if [ "$rc" -eq 0 ]; then
        printf '%-24s rc=%-4s %s\n' "$label" "$rc" \
            "$(printf '%s' "$out" | od -An -c | tr -s ' ' | tr -d '\n')"
    else
        printf '%-24s rc=%-4s %s\n' "$label" "$rc" "$(printf '%s' "$out" | head -1)"
    fi
}

echo "=== GNU $($ENV --version | head -1) ==="

# Control: a known-good escape and a known-bad one, so a run where every case
# reported the same thing would be visible immediately.
show 'CONTROL good \t'  "$ENV" -S'/usr/bin/printf [%s] a\tb'
show 'CONTROL bad  \q'  "$ENV" -S'/usr/bin/printf [%s] a\qb'

for e in a b e f r v 0 1 x s w t n c _ '#' '$' '\' '"' "'" '`' '!' '&' ';' '|' '<' '>' '(' ')' '{' '}' '[' ']' '*' '?' '~' '%' '@' '^' '+' '=' ':' ',' '.' '/' '-' 'A' 'N' 'T'; do
    show "backslash-[$e]" "$ENV" -S"/usr/bin/printf [%s] a\\${e}b"
done

# A trailing backslash, and one inside double quotes.
show 'trailing backslash'  "$ENV" -S'/usr/bin/printf [%s] ab\'
show 'dq trailing bsl'     "$ENV" -S'/usr/bin/printf [%s] "ab\"'

echo "=== done ==="
