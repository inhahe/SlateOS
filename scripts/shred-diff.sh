#!/usr/bin/env bash
# shred-diff.sh — compare our `shred` against GNU coreutils 9.4's, inside WSL.
#
# ## Why this can compare the bytes, not just the messages
#
# `--random-source=FILE` makes `shred` deterministic: the pass schedule
# (`genpattern`, which draws from `randint_choose`) and the data of every
# random pass come from FILE's bytes, in an order fixed by gnulib's `randint`
# and `randread`. So both sides are handed the *same* source and the *same*
# fixture, and what is compared is everything: standard output, the `-v`
# lines, the exit status, and -- the point of the program -- the shredded
# file's contents afterwards, byte for byte (by SHA-256), plus which names are
# left in the directory once `-u` has renamed and removed things.
#
# Each side gets a fresh copy of the fixture directory and runs in it, so the
# names in the messages are the same relative names on both sides.
#
# ## What stays out
#
# * Progress lines with an offset, which upstream prints only after five
#   seconds of a pass -- timing, not behaviour. The fixtures are small enough
#   that every pass finishes long before that.
# * A FIFO, which `open (O_WRONLY)` blocks on until a reader appears, and
#   `/dev/null` without `-s`, which never reports an end and is shredded
#   forever -- by GNU's too.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='shred'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
SETUP=
STDOUT_TO=

scratch=$DIFF_TMP/scratch
mkdir -p "$scratch"

# The random source both sides read: 4 MiB of fixed pseudo-random bytes. Built
# once, outside both case directories, so it is the same file for both.
rs=$DIFF_TMP/random-source
awk 'BEGIN { srand(20260925); for (i = 0; i < 4194304; i++) printf "%c", int(rand() * 256) }' > "$rs"
printf 'tenbytes!!' > "$DIFF_TMP/short-source"

# The fixture every case starts from, copied fresh into each side's directory.
fixture=$DIFF_TMP/fixture
mkdir -p "$fixture/dir"
printf 'hello\n' > "$fixture/small"
awk 'BEGIN { for (i = 0; i < 5000; i++) printf "%c", 65 + i % 26 }' > "$fixture/f5000"
awk 'BEGIN { for (i = 0; i < 70000; i++) printf "%c", i % 251 }' > "$fixture/f70000"
: > "$fixture/empty"
printf 'x' > "$fixture/abcdef"
printf 'y' > "$fixture/ro"
chmod 400 "$fixture/ro"
printf 'z' > "$fixture/taken"

compare() {
  local side dir out err rc o_out g_out o_err g_err o_rc g_rc o_tree g_tree
  for side in ours gnu; do
    dir=$scratch/$side
    chmod -R u+rwx "$dir" 2>/dev/null
    rm -rf "$dir"; cp -a "$fixture" "$dir"
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    ( cd "$dir" && eval "$SETUP" ) >/dev/null 2>&1
    if [ -n "$STDOUT_TO" ]; then
      # `-` means standard output: the redirection decides what is shredded.
      ( cd "$dir" && eval "timeout -k 2 60 env PATH=\"\$bindir/$side:\$PATH\" shred \"\$@\" $STDOUT_TO" ) 2>"$err"
      rc=$?
      : > "$out"
    else
      ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side:$PATH" shred "$@" ) >"$out" 2>"$err"
      rc=$?
    fi
    # The directory afterwards: every name, and every file's size, mode and
    # hash. The mode is there because `-f` changes it -- upstream's answer to
    # EACCES is `chmod (name, S_IWUSR)`, which leaves a write-only 0200 file --
    # and a file that change made unreadable is reported as such rather than
    # hashed, since reading it would only fail.
    local tree
    tree=$(cd "$dir" && find . -mindepth 1 | LC_ALL=C sort | while IFS= read -r f; do
      if [ -f "$f" ]; then
        if [ -r "$f" ]; then h=$(sha256sum < "$f" | cut -c1-16); else h=unreadable; fi
        printf '%s %s %s\n' "$f" "$(stat -c '%s %a' "$f")" "$h"
      else printf '%s\n' "$f"; fi
    done)
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err"); o_tree=$tree
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err"); g_tree=$tree
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n    tree{%s}\n  gnu  (rc=%s): out{%s} err{%s}\n    tree{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$(printf '%s' "$o_tree" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$(printf '%s' "$g_tree" | tr '\n' '|')")
}

label_of() { printf 'shred %s%s' "$*" "${STDOUT_TO:+ $STDOUT_TO}"; }

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  SETUP=; STDOUT_TO=
  return 0
}

xfail_case() {
  local why=$1; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  SETUP=; STDOUT_TO=
  return 0
}

R="--random-source=$rs"

# --- the schedule: every pass count up to past the table's first lap -----------
for n in 0 1 2 3 4 5 6 7 8 9 10 12 16 20 24 25 26 30 40 60; do
  run_case "$R" -v -n "$n" f5000
done
run_case "$R" f5000
run_case "$R" -v f5000 small f70000
run_case "$R" -v -z f5000
run_case "$R" -v -n 0 -z f5000
run_case "$R" -v -z -n 7 f70000

# --- the size ------------------------------------------------------------------
run_case "$R" -v small                    # under a block: the exact-size round first
run_case "$R" -v -x f5000
run_case "$R" -v -x small
run_case "$R" -v empty
run_case "$R" -v -s 100 f5000
run_case "$R" -v -s 10K f5000
run_case "$R" -v -s 0 f5000
run_case "$R" -v -s 0x100 f5000
run_case "$R" -v -s 3 small
run_case "$R" -v -s 70000 small           # grows the file
run_case "$R" -s 1000 /dev/null

# --- removal -------------------------------------------------------------------
run_case "$R" -u abcdef
run_case "$R" -v -u abcdef
run_case "$R" -v --remove abcdef
run_case "$R" -v --remove=unlink abcdef
run_case "$R" -v --remove=wipe abcdef
run_case "$R" -v --remove=wipesync abcdef
run_case "$R" -v --remove=wip abcdef      # an abbreviation
run_case "$R" -v --remove=nope abcdef
run_case "$R" -v -u dir/../abcdef
SETUP='printf a > 0; printf b > 00; printf c > 000000'
run_case "$R" -v -u abcdef                # the zero names are taken: incname
SETUP='mkdir sub && printf q > sub/name'
run_case "$R" -v -u sub/name

# --- files that cannot be shredded ---------------------------------------------
run_case "$R" nosuch
run_case "$R" -v nosuch f5000
run_case "$R" dir
run_case "$R" ro
run_case "$R" -f -v ro
run_case "$R" -s 1000 /dev/full
run_case "$R" -v -s 1000 /dev/full

# --- standard output -----------------------------------------------------------
STDOUT_TO='> f5000'; run_case "$R" -v -
STDOUT_TO='>> f5000'; run_case "$R" -v -
STDOUT_TO='| cat'; run_case "$R" -v -
STDOUT_TO='>&-'; run_case "$R" -v -

# --- the random source ---------------------------------------------------------
run_case --random-source=nosuch f5000
run_case --random-source="$DIFF_TMP/short-source" -v f5000
run_case "$R" "$R" f5000
run_case "$R" --random-source="$DIFF_TMP/short-source" f5000

# --- options -------------------------------------------------------------------
run_case
run_case -v
run_case "$R" -n x f5000
run_case "$R" -n -1 f5000
run_case "$R" -n 99999999999999999999 f5000
run_case "$R" -s x f5000
run_case "$R" -s 1Q f5000
run_case "$R" -s -5 f5000
run_case "$R" --iter=2 -v f5000
run_case "$R" --e -v f5000
run_case "$R" -Z f5000
run_case "$R" --nope f5000
run_case "$R" --remove=unlink=x f5000
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
