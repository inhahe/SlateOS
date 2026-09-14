#!/bin/bash
# How does `date -f FILE` behave at its edges?
#
# The happy path is obvious (one date per line) and the harness already covers
# it. What is not obvious, and decides the implementation: what happens to a
# BAD line, whether it stops or continues, what the exit status is, whether `-`
# means stdin, and whether -f collides with -d/-r the way -d and -r collide
# with each other.
set -u
export TZ=UTC
DATE=/usr/bin/date
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf '@0\n@1000000000\n' > "$tmp/good.txt"
printf '@0\nnot a date\n@100\n' > "$tmp/bad-middle.txt"
printf '\n\n' > "$tmp/blank.txt"
printf '@0' > "$tmp/no-trailing-newline.txt"
: > "$tmp/empty.txt"

run() {
    local label="$1"; shift
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    printf '%-30s rc=%-4s %s\n' "$label" "$rc" "$(printf '%s' "$out" | tr '\n' '|')"
}

echo "=== GNU $($DATE --version | head -1) ==="

# Control: the good file works, and a missing one does not.
run 'CONTROL good'        "$DATE" -f "$tmp/good.txt"
run 'CONTROL missing'     "$DATE" -f "$tmp/nosuch.txt"

run 'bad line in middle'  "$DATE" -f "$tmp/bad-middle.txt"
run 'blank lines only'    "$DATE" -f "$tmp/blank.txt"
run 'no trailing newline' "$DATE" -f "$tmp/no-trailing-newline.txt"
run 'empty file'          "$DATE" -f "$tmp/empty.txt"
run 'with a format'       "$DATE" -f "$tmp/good.txt" +%s

# `-` as stdin?
printf '@0\n@5\n' | "$DATE" -f - > "$tmp/stdin.out" 2>&1
printf '%-30s rc=%-4s %s\n' 'dash means stdin' "$?" "$(tr '\n' '|' < "$tmp/stdin.out")"

# Collides with the other date sources?
run '-f with -d'          "$DATE" -f "$tmp/good.txt" -d @0
run '-f with -r'          "$DATE" -f "$tmp/good.txt" -r "$tmp/good.txt"
run '-d with -f'          "$DATE" -d @0 -f "$tmp/good.txt"

echo "=== done ==="
