#!/usr/bin/env bash
# Differential test: our `mkfifo` against GNU coreutils 9.4's.
#
# Each case runs both sides in their own copy of one fixture tree, under the
# same umask, and compares what they print, what they say, their status, and
# the tree they leave: every path's type and mode, so a FIFO made with the
# wrong permissions is a difference even when nothing on the screen says so.
#
# ## What it covers
#
#   * **the plain form** -- one FIFO, several, a name that exists as a file, a
#     directory, a FIFO or a dangling link, a missing or unwritable parent,
#     and the run going on after a failure.
#   * **`-m`** -- numeric and symbolic modes under several umasks, a mode with
#     a special bit (refused: a FIFO takes permission bits only), and an
#     invalid one, which upstream reports without naming it.
#   * **the options** -- every spelling, `-Z` and `--context` (design-decisions
#     1064), and the order of `missing operand` against a bad mode.
#   * **the descriptors** -- standard output full and closed (`mkfifo` writes
#     nothing there), standard error full and closed with and without
#     something to say, standard input closed.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='mkfifo'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "mkfifo-diff: refusing to run as root, where the unwritable fixture is" >&2
  echo "  writable and its cases measure nothing." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
UMASK=022

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -----------------------------------
proto=$work/proto
mkdir -p "$proto/dir" "$proto/ro"
printf 'a\n' > "$proto/file.txt"
chmod 555 "$proto/ro"
mkfifo "$proto/oldpipe"
ln -s nowhere "$proto/dangling"
ln -s dir "$proto/link-to-dir"

# --- comparing a whole tree ------------------------------------------------------
# Mode and type per path, sorted: `%A` is both at once (`prw-r--r--`). Each
# name goes through `od -c`, so one holding a newline or a byte that is not
# UTF-8 is compared exactly.
snap() {
  ( cd "$1" && find . -mindepth 1 -print0 | LC_ALL=C sort -z \
      | while IFS= read -r -d '' e; do
          printf '%s %s\n' "$(printf '%s' "$e" | od -An -c | tr -s ' \n' ' ')" \
            "$(stat -c '%A' "$e" 2>/dev/null || echo '?')"
        done )
}

# `run_side DIR SIDE REDIR BIN ERR ARGS...`: one side in its copy of the
# tree, with one descriptor as REDIR says. Standard error closed goes through
# a shell's `exec 2>&-`, so that only `mkfifo` runs without it.
run_side() {
  local dir=$1 side=$2 redir=$3 bin=$4 err=$5; shift 5
  case $redir in
    '')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkfifo "$@" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkfifo "$@" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkfifo "$@" ) </dev/null >&- 2>"$err" ;;
    '<&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkfifo "$@" ) <&- >"$bin" 2>"$err" ;;
    '2>/dev/full')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkfifo "$@" ) </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
          /bin/sh -c 'exec 2>&-; exec mkfifo "$@"' sh "$@" ) </dev/null >"$bin" ;;
    *) echo "mkfifo-diff: no such redirection: $redir" >&2; exit 2 ;;
  esac
}

compare_with() {
  local redir=$1; shift
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc o_bin g_bin
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  run_side "$o_dir" ours "$redir" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side "$g_dir" gnu  "$redir" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg o_tree g_tree
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snap "$o_dir"); g_tree=$(snap "$g_dir")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  chmod -R u+rwx "$o_dir" "$g_dir" 2>/dev/null
  rm -rf "$o_dir" "$g_dir"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s {%s}\n    tree: %s\n  gnu  (rc=%s): %s {%s}\n    tree: %s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$(printf '%s' "$o_tree" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" \
    "$(printf '%s' "$g_tree" | tr '\n' '|')")
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

run_case() { compare_with '' "$@"; report "mkfifo $* (umask $UMASK)"; }

# `fd_case REDIR ARGS...`: run_case with one descriptor changed.
fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "mkfifo $* $redir (umask $UMASK)"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS mkfifo %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail mkfifo %s (%s)\n' "$*" "$why"
  fi
  return 0
}

nl_name=$(printf 'new\nline')
bad_utf8=$(printf 'bad\377name')
long_name=$(printf 'x%.0s' $(seq 1 300))

# --- the plain form ------------------------------------------------------------------
run_case p
run_case p q
run_case file.txt
run_case dir
run_case oldpipe
run_case dangling
run_case link-to-dir
run_case link-to-dir/p
run_case p file.txt q
run_case nosuch/p
run_case file.txt/p
run_case ro/p
run_case ''
run_case -
run_case -- -p
run_case ./-p
run_case p/
run_case "$nl_name"
run_case "$bad_utf8"
run_case "$long_name"
run_case p p

# --- -m ---------------------------------------------------------------------------------
for m in 600 0600 644 666 777 000 0 0777 00644 u=rw go= u=rw,go=r a=rw a= a=,+w a=,+X \
         +x +w -w =r +r o+w g=u u=g go-rwx; do
  run_case -m "$m" p
done
# A FIFO takes permission bits only, and an invalid mode is not named.
for m in 1777 2755 4755 0644x zzz 8 999 '' ,u+x a+ u+s +t g+s o+t; do
  run_case -m "$m" p
done
run_case -m zzz
run_case -m 1777
run_case -m600 p
run_case --mode=600 p
run_case --mode 600 p
run_case --mod=600 p
run_case --m=600 p
run_case -m 600 -m 644 p
run_case -m 600 file.txt p
run_case -m 600 oldpipe
run_case -m 600 dangling

# --- the umask --------------------------------------------------------------------------
for u in 077 000 027 002 777 222; do
  UMASK=$u
  run_case p
  run_case -m 600 p
  run_case -m a=,+w p
  run_case -m +x p
  run_case -m a+X p
  run_case -m go+w p
  UMASK=022
done

# --- the options --------------------------------------------------------------------------
run_case
run_case --
run_case -q p
run_case --nosuch p
run_case --=x p
run_case -m
run_case --mode
run_case p -m
run_case -Z p
run_case -Z
run_case --context p
run_case --context=user_u:object_r:tmp_t:s0 p
run_case --context= p
run_case --cont p
run_case --context=x
run_case --context=x -q
run_case -Z -m 600 p
run_case --nosuch --help

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' p
fd_case '>/dev/full' file.txt p
fd_case '>&-' p
fd_case '>&-' file.txt
fd_case '<&-' p
fd_case '2>/dev/full' p
fd_case '2>/dev/full' file.txt
fd_case '2>/dev/full' file.txt p
fd_case '2>/dev/full' -q
fd_case '2>/dev/full' -m zzz p
fd_case '2>/dev/full' --context=x p
fd_case '2>&-' p
fd_case '2>&-' file.txt
fd_case '2>&-' file.txt p
fd_case '2>&-' -q
fd_case '>/dev/full' --help
fd_case '>&-' --help
fd_case '>/dev/full' --version
fd_case '>&-' --version

# --- the ones whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version
xfail_case "our help text: --help is acted on before the bad option" --help --nosuch
xfail_case "our version string: the first of the two wins" --version --help
xfail_case "our version string: --v is --version, the only option it prefixes" --v

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
