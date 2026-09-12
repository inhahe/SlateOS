#!/usr/bin/env bash
# Differential test: our `stat` against GNU `stat`.
#
# ## Why both sides read the SAME files, unlike `patch-diff.sh` and `chown-diff.sh`
#
# `stat` does not mutate anything, so there is no reason to give each side its
# own copy of the fixtures -- and a strong reason not to. Two copies have two
# different inode numbers and possibly two different block counts, so `%i`, `%d`
# and `%b` could never be compared. Reading one tree from both sides makes those
# fields comparable, and they are exactly the fields an implementation is most
# likely to get wrong, because they are the ones it cannot guess.
#
# The rule underneath: **give the subject a private copy only when it writes.**
#
# ## Which specifiers are pinned, and which are left out
#
# Timestamps are pinned with `touch -d`, so `%y`, `%Y`, `%w` and `%W` are a
# function of the fixture rather than of when the harness ran.
#
# `%x`/`%X` -- access time -- are included anyway and are *expected* to be
# stable: `stat` reads metadata, not content, so it does not itself update
# atime. If they ever become the only failing cases, that is the thing to
# suspect rather than a defect.
#
# ## The multicall hazard, checked before this was written
#
# `userspace/stat` is not a `stat`: it dispatches on `argv[0]` and also serves
# `touch`, `mkfifo`, `readlink`, `realpath` and `ln`. Retiring the crate would
# delete all six, so all six were confirmed present in `coreutils/src/bin`
# first. A pair's row in `dup-bins-survey.py` names one program; the crate
# behind it may be six.
#
# ## Why `od -An -c`
#
# `--printf` differs from `-c` by exactly one thing: it interprets backslash
# escapes and does *not* append a newline. `-t` is a single space-separated
# line whose field count is the whole point. Both are invisible to a comparison
# that reads lines or trims.
set -u

DIFF_PROG='stat'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures/dir/sub"
cd "$fixtures" >/dev/null || exit 1

printf 'hello\n'            > file.txt
printf ''                   > empty.txt
printf 'x%.0s' $(seq 1 5000) > big.txt
ln -s file.txt                link
ln -s nowhere                 dangling
ln -s dir                     linkdir
ln file.txt                   hardlink.txt
mkfifo fifo 2>/dev/null || true
printf 'q\n'                > 'name with spaces.txt'
printf 'q\n'                > "$(printf 'quote\047name.txt')"
chmod 0644 file.txt
chmod 0755 dir
chmod 0600 empty.txt
chmod 4755 dir/sub 2>/dev/null || true

# One fixed instant for every path, so %y/%Y and the rest are a function of the
# fixture. Done last, because creating a file updates its parent's mtime.
touch -h -d '2020-01-02 03:04:05 UTC' file.txt empty.txt big.txt link dangling \
      linkdir hardlink.txt 'name with spaces.txt' "$(printf 'quote\047name.txt')" \
      dir/sub dir . 2>/dev/null || true

run_side() {
  local side=$1; shift
  diff_run timeout -k 2 20 env TZ=UTC LC_ALL=C.UTF-8 PATH="$bindir/$side" stat "$@"
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report "stat $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS stat %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail stat %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default report, for every kind of thing ---------------------------------
run_case file.txt
run_case empty.txt
run_case big.txt
run_case dir
run_case link
run_case dangling
run_case linkdir
run_case hardlink.txt
run_case fifo
run_case 'name with spaces.txt'

# --- every format specifier, one at a time ----------------------------------------
for f in %a %A %b %B %d %D %f %F %g %G %h %i %m %n %N %o %s %t %T %u %U %w %W %x %X %y %Y %z %Z; do
  run_case -c "$f" file.txt
done

# --- the specifiers that differ on a symlink and a directory ------------------------
for f in %F %N %s %a %h; do
  run_case -c "$f" link
  run_case -c "$f" dir
done

# --- format handling ------------------------------------------------------------------
run_case -c '%n %s %F' file.txt
run_case --format='%n %s' file.txt
run_case -c 'literal' file.txt
run_case -c '' file.txt
run_case -c '%' file.txt
run_case -c '%%' file.txt
run_case -c '%Q' file.txt
run_case -c '%n' file.txt empty.txt
run_case --printf='%n %s' file.txt
run_case --printf='%n\n' file.txt
run_case --printf='%n\t%s\n' file.txt
run_case --printf='a\\b' file.txt
run_case --printf='%n' file.txt empty.txt

# --- terse ----------------------------------------------------------------------------
run_case -t file.txt
run_case --terse file.txt
run_case -t dir
run_case -t link
run_case -t -L link

# --- following links --------------------------------------------------------------------
run_case -L link
run_case --dereference link
run_case -L dangling
run_case -L linkdir
run_case -c %F link
run_case -L -c %F link
run_case -c %N dangling

# --- filesystem mode -----------------------------------------------------------------------
run_case -f .
run_case --file-system .
run_case -f file.txt
for f in %a %b %c %d %f %i %l %n %s %S %t %T; do
  run_case -f -c "$f" .
done
run_case -f -t .

# --- several operands, and failures among them ------------------------------------------------
run_case file.txt dir link
run_case file.txt nosuch.txt empty.txt
run_case nosuch.txt
run_case nosuch.txt nosuch2.txt
run_case -c %n file.txt nosuch.txt

# --- refusals ------------------------------------------------------------------------------------
run_case
run_case -Q file.txt
run_case --nosuchoption file.txt
run_case -c
run_case -f -c
run_case --printf

# --- long-option abbreviation -----------------------------------------------------------------------
run_case --form='%n' file.txt
run_case --deref link
run_case --ters file.txt
run_case --file-sys .

# --- the two whose text is ours ------------------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
