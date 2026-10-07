#!/usr/bin/env bash
# Differential test: our `iconv` against Ubuntu 24.04's (glibc 2.39's
# `iconv_prog.c`).
#
# Both run over the same `iconv(3)` here -- ours, built for Linux, calls
# glibc's -- so a difference is the program's: its options, which files it
# reads and in what order, what it writes and when, and its messages. The
# library is SlateOS's on the target, and lane D measures that against glibc.
#
# What is compared: the status, standard output, standard error, and any
# output file a case names. Each case runs in a scratch directory of its own,
# made afresh for each side.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS and `--help` ends without Ubuntu's bug-reporting
# paragraph (deliberate difference 1 in `userspace/iconv/src/main.rs`). A
# charmap file given as `-f` (difference 2) is not asked: Ubuntu's charmaps
# are gzipped, and its `iconv` cannot read those either. `-l` over glibc is
# glibc's list exactly (difference 3), so it is compared like any other case.
set -u

DIFF_PROG='iconv'
DIFF_PKG='iconv'
DIFF_REF='/usr/bin/iconv'
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- knobs ---------------------------------------------------------------------
# `INPUT` is a `printf %b` string for standard input. `FILES` is shell run in
# the case directory first, to make input files. `SHOW` names output files to
# compare, as found afterwards (a missing one is shown as such).
# `REDIR` is a redirection for iconv itself: `>&-`, `>/dev/full`, `<&-`.
INPUT=; FILES=; SHOW=; ENVS=(); REDIR=
reset_knobs() { INPUT=; FILES=; SHOW=; ENVS=(); REDIR=; }

case_dir=$DIFF_TMP/case
run_side() {
  local side=$1; shift
  rm -rf "$case_dir"; mkdir -p "$case_dir"
  ( cd "$case_dir" && eval "$FILES" ) >/dev/null 2>&1
  # `eval` only for the redirection, which a variable cannot hold otherwise;
  # the arguments stay in "$@".
  ( cd "$case_dir" || exit 125
    printf '%b' "$INPUT" | eval "env LC_ALL=C.UTF-8 \"\${ENVS[@]}\" \
      PATH=\"\$bindir/\$side:\$PATH\" timeout -k 2 60 iconv \"\$@\" $REDIR" \
  ) >"$DIFF_TMP/$side.out" 2>"$DIFF_TMP/$side.err"
  printf '%s' "$?" >"$DIFF_TMP/$side.rc"
  local f
  : >"$DIFF_TMP/$side.files"
  for f in $SHOW; do
    if [ -e "$case_dir/$f" ]; then
      printf '%s: %s\n' "$f" "$(od -An -tx1 "$case_dir/$f" | tr -s ' \n' ' ')" >>"$DIFF_TMP/$side.files"
    else
      printf '%s: <missing>\n' "$f" >>"$DIFF_TMP/$side.files"
    fi
  done
}

compare() {
  run_one_case "$@"
  reset_knobs
}

run_one_case() {
  run_side ours "$@"
  run_side gnu "$@"
  local s same=yes
  for s in rc out err files; do
    cmp -s "$DIFF_TMP/ours.$s" "$DIFF_TMP/gnu.$s" || same=no
  done
  AGREED=$same
  REPORT=$(for side in ours gnu; do
    printf '  %-4s rc=%s out{%s} err{%s} files{%s}\n' "$side" "$(cat "$DIFF_TMP/$side.rc")" \
      "$(od -An -c "$DIFF_TMP/$side.out" | head -4 | tr -s ' \n' ' ')" \
      "$(tr '\n' '|' <"$DIFF_TMP/$side.err")" "$(tr '\n' '|' <"$DIFF_TMP/$side.files")"
  done)
}

check() {
  local name="iconv $* $REDIR"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK    %s\n' "$name"
  else
    fail=$((fail + 1))
    printf 'DIFF  %s\n%s\n' "$name" "$REPORT"
  fi
  return 0
}

xcheck() {
  local why=$1; shift
  local name="iconv $* $REDIR"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$name" "$why"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n%s\n' "$name" "$why" "$REPORT"
  fi
  return 0
}

# =============================================================================
# --- conversions ----------------------------------------------------------------
INPUT='caf\351\n'; check -f ISO-8859-1 -t UTF-8
INPUT='caf\303\251\n'; check -f UTF-8 -t ISO-8859-1
INPUT='caf\303\251\n'; check --from-code=UTF-8 --to-code=UTF-16
INPUT='caf\303\251\n'; check -f UTF-8 -t UTF-16LE
INPUT='caf\303\251\n'; check -f UTF-8 -t UTF-32BE
INPUT='\377\376a\000b\000'; check -f UTF-16 -t UTF-8
INPUT='h\303\251llo \342\202\254\n'; check -f UTF-8 -t ISO-2022-JP
INPUT='h\303\251llo \342\202\254\n'; check -f UTF-8 -t UTF-7
INPUT='\033$B$3$s\033(B\n'; check -f ISO-2022-JP -t UTF-8
INPUT=''; check -f UTF-8 -t UTF-16
INPUT='abc'; check -f UTF-8 -t ASCII
# The locale's set when a name is empty: C.UTF-8's is UTF-8.
INPUT='caf\303\251\n'; check -t ISO-8859-1
INPUT='caf\351\n'; check -f ISO-8859-1
INPUT='x\n'; check
# //TRANSLIT and //IGNORE, and -c.
INPUT='\342\202\254 \303\251\n'; check -f UTF-8 -t ASCII//TRANSLIT
INPUT='\342\202\254 \303\251\n'; check -f UTF-8 -t ASCII//IGNORE
INPUT='\342\202\254 \303\251\n'; check -c -f UTF-8 -t ASCII
INPUT='\342\202\254 \303\251\n'; check -c -f UTF-8 -t ASCII//TRANSLIT
INPUT='a\377b\n'; check -c -f UTF-8 -t UTF-16LE
# A large input, many output blocks.
FILES='seq 1 50000 > big'; check -f UTF-8 -t UTF-16 big

# --- what goes wrong ------------------------------------------------------------
INPUT='a\377b'; check -f UTF-8 -t ASCII
INPUT='ab\342\202\254cd'; check -f UTF-8 -t ISO-8859-1
INPUT='a\303'; check -f UTF-8 -t UTF-16LE
INPUT='a\000'; check -f UTF-16LE -t UTF-8
INPUT='x'; check -f NOPE -t UTF-8
INPUT='x'; check -f UTF-8 -t NOPE
INPUT='x'; check -f NOPE -t NOPE2
INPUT='x'; check -f '' -t NOPE
INPUT='x'; check -f NOPE
INPUT='x'; check -f UTF-8 -t UTF-8//BOGUS
# A name with a slash is a name like any other when no charmap is there.
INPUT='x'; check -f ./nocharmap -t UTF-8

# --- files ---------------------------------------------------------------------------
FILES='printf "one\n" > a; printf "two\n" > b'; check -f UTF-8 -t UTF-16LE a b
FILES='printf "one\n" > a'; check -f UTF-8 -t ASCII a nosuch a
FILES='printf "a\377\n" > bad; printf "ok\n" > good'; check -f UTF-8 -t ASCII bad good
FILES='mkdir dir'; check -f UTF-8 -t ASCII dir
FILES='printf "one\n" > a'; check --verbose -f UTF-8 -t ASCII a nosuch
INPUT='in\n'; check -f UTF-8 -t ASCII -
INPUT='in\n'; FILES='printf "one\n" > a'; check -f UTF-8 -t ASCII - a -
FILES=': > empty'; check -f UTF-8 -t ASCII empty

# --- output ----------------------------------------------------------------------------
INPUT='caf\303\251\n'; SHOW='out'; check -f UTF-8 -t ISO-8859-1 -o out
INPUT='caf\303\251\n'; SHOW='out'; check -f UTF-8 -t ISO-8859-1 --output=out
INPUT='caf\303\251\n'; check -f UTF-8 -t ISO-8859-1 -o -
INPUT=''; SHOW='out'; check -f UTF-8 -t ISO-8859-1 -o out
INPUT='x\n'; SHOW='out'; FILES='printf "old contents\n" > out'; check -f UTF-8 -t UTF-8 -o out
INPUT='x\n'; check -f UTF-8 -t UTF-8 -o nodir/out
INPUT='a\377'; SHOW='out'; check -f UTF-8 -t ASCII -o out
# `error()` flushes `stdout` and not the `-o` file, whose `a` waits for the
# `fclose` at the end -- where the full disk is reported.
INPUT='a\377'; check -f UTF-8 -t ASCII -o /dev/full
INPUT='ab'; check -f UTF-8 -t ASCII -o /dev/full
# Names that are not text are printed as the bytes they are.
INPUT='x'; check -f "$(printf 'B\377D')" -t UTF-8
INPUT='x'; check -f UTF-8 -t "$(printf 'B\377D')"
check -f UTF-8 -t ASCII "$(printf 'no\377such')"

# --- standard descriptors that cannot be used ---------------------------------------
# A small output fails at the close at the end; a large one also stops
# mid-run, with a second message.
for redir in '>&-' '>/dev/full' '2>&-' '<&-'; do
  INPUT='caf\303\251\n'; REDIR=$redir; check -f UTF-8 -t ISO-8859-1
done
FILES='seq 1 50000 > big'; REDIR='>/dev/full'; check -f UTF-8 -t UTF-16 big
FILES='seq 1 50000 > big'; REDIR='>&-'; check -f UTF-8 -t UTF-16 big
# Nothing written, nothing to fail.
INPUT=''; REDIR='>&-'; check -f UTF-8 -t UTF-16
REDIR='>&-'; check -l
REDIR='>&-'; check --help

# --- the command line ----------------------------------------------------------------
check --usage
check -x
check --bogus
check -f
check --ver
check --from
check --list --bogus
check -l
check --list
INPUT='x\n'; check -s -f UTF-8 -t ASCII
check --program-name=conv -x
check --program-name=/a/conv --usage

# --- differences on purpose ---------------------------------------------------------
xcheck 'names SlateOS (difference 1)' --version
xcheck 'names SlateOS (difference 1)' -V
xcheck 'no bug-report paragraph (difference 1)' --help
xcheck 'no bug-report paragraph (difference 1)' '-?'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ]
