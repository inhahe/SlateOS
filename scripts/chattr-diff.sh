#!/usr/bin/env bash
# Differential test: our chattr and lsattr against Ubuntu 24.04's, side by
# side in WSL.
#
# Ours is `userspace/chattr`, e2fsprogs 1.47.0's `misc/chattr.c` and
# `misc/lsattr.c` ported -- one binary, `lsattr` by the name it is called.
#
# ## What is compared
#
# For each case: standard output, standard error, the exit status, and the
# attributes every file in the working directory is left with (read back by
# Ubuntu's `lsattr`, so the two sides are judged by one reader). Each side
# runs in a fresh directory on WSL's ext4 root, as the same unprivileged
# user -- so `+i` and `+a` are refused on both, as they are without root.
#
# `DIFF_FAIL_FAST=1` stops at the first difference; `VERBOSE=1` shows the
# agreements too.
set -u

DIFF_PROG='chattr'
DIFF_PKG='chattr'
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# lsattr: ours is chattr by another name; Ubuntu's its own program.
ln -sfn chattr "$bindir/ours/lsattr"
ln -sfn "$(command -v lsattr)" "$bindir/gnu/lsattr"

cur=$DIFF_TMP/cur
work=$DIFF_TMP/work
pass=0; fail=0

# A working directory every case starts from: files, a directory with
# hidden and nested entries, a symbolic link, a FIFO.
fixture() {
  mkdir -p d/sub d/.hidden
  printf 'x' > f; printf 'y' > g; printf 'z' > d/a; printf 'w' > d/sub/b
  ln -s f link; ln -s missing dangling; mkfifo fifo
}

run_side() {
  local side=$1 out=$2 err=$3 tree=$4; shift 4
  # A case may have left a directory unreadable.
  [ -d "$work" ] && chmod -R u+rwX "$work" 2>/dev/null
  rm -rf "$work"
  mkdir -p "$work"
  ln -sfn "$bindir/$side" "$cur"
  (
    cd "$work" || exit 99
    fixture
    if [ -n "${SETUP:-}" ]; then eval "$SETUP"; fi
    env -i PATH="$cur:/usr/bin:/bin" HOME="$work" /usr/bin/timeout -k 2 10 "$@" </dev/null >"$out" 2>"$err"
  )
  local rc=$?
  ( cd "$work" && /usr/bin/lsattr -aR . 2>&1 | LC_ALL=C sort ) > "$tree"
  return "$rc"
}

compare() {
  local o_rc g_rc
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_tree=$DIFF_TMP/o.tree g_tree=$DIFF_TMP/g.tree
  run_side ours "$o_out" "$o_err" "$o_tree" "$@"; o_rc=$?
  run_side gnu "$g_out" "$g_err" "$g_tree" "$@"; g_rc=$?
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ] \
      && cmp -s "$o_tree" "$g_tree"; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(
    printf '  ours rc=%s out={%s} err={%s}\n' "$o_rc" "$(head -c 400 "$o_out" | tr '\n' '|')" "$(head -c 300 "$o_err" | tr '\n' '|')"
    printf '  gnu  rc=%s out={%s} err={%s}\n' "$g_rc" "$(head -c 400 "$g_out" | tr '\n' '|')" "$(head -c 300 "$g_err" | tr '\n' '|')"
    if ! cmp -s "$o_tree" "$g_tree"; then
      diff "$o_tree" "$g_tree" | head -8 | sed 's/^/  tree: /'
    fi
  )
  SETUP=''
}

# Both programs run in one working directory, ours first: for what differs
# between any two directories made alike, as a file's generation does.
compare_same_files() {
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  [ -d "$work" ] && chmod -R u+rwX "$work" 2>/dev/null
  rm -rf "$work"
  mkdir -p "$work"
  (
    cd "$work" || exit 99
    fixture
    env -i PATH="$bindir/ours:/usr/bin:/bin" HOME="$work" /usr/bin/timeout -k 2 10 "$@" </dev/null >"$o_out" 2>"$o_err"
    echo $? > "$DIFF_TMP/o.rc"
    env -i PATH="$bindir/gnu:/usr/bin:/bin" HOME="$work" /usr/bin/timeout -k 2 10 "$@" </dev/null >"$g_out" 2>"$g_err"
    echo $? > "$DIFF_TMP/g.rc"
  )
  local o_rc g_rc
  o_rc=$(cat "$DIFF_TMP/o.rc"); g_rc=$(cat "$DIFF_TMP/g.rc")
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(
    printf '  ours rc=%s out={%s} err={%s}\n' "$o_rc" "$(head -c 400 "$o_out" | tr '\n' '|')" "$(head -c 300 "$o_err" | tr '\n' '|')"
    printf '  gnu  rc=%s out={%s} err={%s}\n' "$g_rc" "$(head -c 400 "$g_out" | tr '\n' '|')" "$(head -c 300 "$g_err" | tr '\n' '|')"
  )
}

run_case() {
  local label
  label="$(printf '%q ' "$@")${SETUP:+ [setup: $SETUP]}"
  ${COMPARE:-compare} "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then
      echo "chattr-diff: stopped at the first difference"
      exit 1
    fi
  fi
}

# --- lsattr ------------------------------------------------------------------------
for args in '' '.' 'f' 'f g' 'd' '-d d' '-a d' '-R d' '-aR d' '-Ra .' '-l f' '-ld d' '-lR d' \
            '-p f' '-V f' '-V' 'missing' 'link' 'dangling' 'fifo' '/dev/null' \
            'f missing g' '-x' '-x f' '--' '-- f' 'f -d' '-d d f' '--help' '--version' '-' \
            'd/' 'd//' './d' '-R d/' '-a' '-aR' '-l' '/proc/version' 'd/sub/b'; do
  # shellcheck disable=SC2086
  run_case lsattr $args
done
# A file's generation is random, and ext4 refuses to set one where metadata
# checksums are on (ENOTTY), so the -v cases read the same files.
for args in '-v f' '-vp f' '-lv f' '-v d' '-dv d' '-v f g'; do
  # shellcheck disable=SC2086
  COMPARE=compare_same_files run_case lsattr $args
done
SETUP='chmod 000 d'; run_case lsattr d
SETUP='chmod 000 d'; run_case lsattr -R .
SETUP='chmod 000 d/sub'; run_case lsattr -R d
SETUP='touch "$(printf "sp ace")"'; run_case lsattr
SETUP='mkdir e'; run_case lsattr e
SETUP='mkdir e'; run_case lsattr -a e

# --- chattr --------------------------------------------------------------------------
# Not `-e f`, nor an `=` without `e`: whether ext4 lets a file give up its
# extents varies, and Ubuntu's chattr is refused or not from run to run.
for args in '' 'f' '+d' '+d f' '-d f' '=e f' '+ f' '- f' '+dA f' '+d -d f' '+d =e f' \
            '=d +A f' '+z f' '-z f' '=z f' '+E f' '+i f' '-i f' '+a f' '-a f' '+ia f' '+di f' \
            '-V +d f' '-V -d f' '-V =de f' '-V' '-R +d d' '-R -d d' '-RV +d d' '-Rf +d d' \
            '+d d' '+D f' '+D d' '+DV f' '+d missing' '-f +d missing' '+d link' '-f +d link' \
            '+d dangling' '+d fifo' '+d /dev/null' '+d f missing g' '-- +d f' '+d -- f' '-v' \
            '-v 5 f' '-v 0x10 f' '-v 010 f' '-v x f' '-v 5x f' "-v '' f" '-p 1 f' '-p x f' '-p' \
            '-fv 5 f' '-pf 1 f' '+c f' '+s f' '+u f' '+S f' '+j f' '+t f' '+T d' '+C f' '+x f' \
            '+F d' '+P d' '+m f' '+e f' '=de f g' '-RV =de .' '+Ad -dA f' '-+d f' '-R' \
            '-x +d f' '+d' '-Vf -i f' '=ie f' '-f =ie f' '+d,A f' '--help'; do
  # shellcheck disable=SC2086
  eval "run_case chattr $args"
done
SETUP='chmod 000 d'; run_case chattr -R +d d
SETUP='chmod 000 d/sub'; run_case chattr -R +d d
SETUP='chmod 000 f'; run_case chattr +d f
SETUP='chattr +d f'; run_case chattr -d f
SETUP='chattr +d f'; run_case chattr -V =Ae f
SETUP='chattr +dA f g'; run_case chattr -V -A f g

printf 'chattr-diff: %d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
