#!/bin/bash
# Can cp-diff.sh's `contents()` tell two files apart when they differ ONLY in
# NUL bytes?
#
# `contents` is consumed as `o_body=$(contents "$o_dir" | scrub ...)`, and
# command substitution in bash DISCARDS NUL bytes. So a `cat` of a file whose
# content differs only in NUL placement produces the same captured string
# either way, and the harness compares equal -- reporting that our `cp` copied
# the file correctly when it may not have.
#
# This probes BOTH directions, which is the point: the bug must be shown to
# exist with the current body, and shown to be gone with the proposed one.
# A control pair that differs in a NON-NUL byte must be distinguished by both,
# otherwise the "fix" could be something that merely reports everything as
# different.
set -u
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# --- fixtures ---------------------------------------------------------------
# `\000` written here in a file rather than typed through a tool that collapses
# backslashes; this has cost this project eight separate mistakes.
mk() { mkdir -p "$tmp/$1"; printf "$2" > "$tmp/$1/f"; }

mk nul_a   'a\000b'
mk nul_b   'ab'
mk ctrl_a  'ab'
mk ctrl_b  'ac'

# --- the body cp-diff.sh had BEFORE 2026-09-14 ------------------------------
contents_before() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\0' 2>/dev/null | LC_ALL=C sort -z \
      | while IFS= read -r -d '' f; do
      printf '== %s\n' "$f"
      cat -- "$f"
      printf '\n'
    done )
}

# --- the body cp-diff.sh has NOW, kept byte-identical to it -----------------
# A hash of the bytes survives command substitution because it is hex. The
# `cat` stays: it keeps the readable diff for the ordinary text case, which is
# what makes a failure report legible. The hash is what makes it CORRECT.
contents_after() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\0' 2>/dev/null | LC_ALL=C sort -z \
      | while IFS= read -r -d '' f; do
      printf '== %s\n' "$f"
      printf 'sha %s\n' "$( { sha256sum <"$f"; } 2>/dev/null | cut -d' ' -f1 )"
      cat -- "$f"
      printf '\n'
    done )
}

cmp_with() {
  local fn="$1" a="$2" b="$3"
  local x y
  x=$("$fn" "$tmp/$a")
  y=$("$fn" "$tmp/$b")
  if [ "$x" = "$y" ]; then echo SAME; else echo DIFFER; fi
}

echo "=== fixture bytes (od) ==="
for d in nul_a nul_b ctrl_a ctrl_b; do
    printf '%-8s %s\n' "$d" "$(od -An -c < "$tmp/$d/f" | tr -s ' ')"
done

echo
echo "=== the two pairs, under each body ==="
printf '%-34s %s\n' 'NUL pair, old body'  "$(cmp_with contents_before   nul_a  nul_b)"
printf '%-34s %s\n' 'NUL pair, current body' "$(cmp_with contents_after nul_a  nul_b)"
printf '%-34s %s\n' 'CONTROL pair, old'   "$(cmp_with contents_before   ctrl_a ctrl_b)"
printf '%-34s %s\n' 'CONTROL pair, current'  "$(cmp_with contents_after ctrl_a ctrl_b)"

echo
echo "Expected: NUL/old=SAME (the bug), NUL/current=DIFFER (fixed),"
echo "          both CONTROL rows=DIFFER (neither body is blind or paranoid)."
