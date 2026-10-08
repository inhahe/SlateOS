#!/usr/bin/env bash
# Differential test: our `mkdir` against GNU coreutils 9.4's.
#
# Each case runs both sides in their own copy of one fixture tree, under the
# same umask, and compares what they print, what they say, their status, and
# the tree they leave: every path's type and mode. The tree is the part that
# matters most -- `mkdir` exists to change it, and a run whose messages agree
# while a directory came out with other permissions is the failure nothing on
# the screen reports.
#
# ## What it covers
#
#   * **the plain form** -- one directory, several, one that exists, one whose
#     parent is missing, is a file, is unwritable; the order of the messages
#     when some fail and the rest are still made.
#   * **`-p`** -- the ancestors it makes and the modes it gives them, the
#     component it names when the walk fails (upstream's `mkancesdirs` names
#     the prefix it was in, not the operand), `.`, `..`, doubled and trailing
#     slashes, symbolic links on the way.
#   * **`-m`** -- numeric and symbolic modes, under several umasks, and the
#     special bits, including in a set-group-ID parent, where the kernel adds
#     a bit nobody asked for and upstream's `dirchownmod` decides whether it
#     stays.
#   * **`-v`** -- one line per directory made, `-p`'s ancestors included.
#   * **the options** -- every spelling, abbreviations, the ambiguous ones,
#     `-Z` and `--context`.
#   * **the descriptors** -- standard output full and closed, standard error
#     full and closed, with and without something to say.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='mkdir'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "mkdir-diff: refusing to run as root, where the unwritable fixtures are" >&2
  echo "  writable and their cases measure nothing." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
UMASK=022

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -----------------------------------
proto=$work/proto
mkdir -p "$proto/dir/sub" "$proto/ro" "$proto/nox" "$proto/sgid"
printf 'a\n' > "$proto/file.txt"
chmod 755 "$proto/dir" "$proto/dir/sub"
chmod 555 "$proto/ro"
chmod 666 "$proto/nox"
# A set-group-ID directory: the kernel gives every directory made in it the
# same bit, whatever mode `mkdir(2)` was asked for.
chmod 2775 "$proto/sgid"
ln -s dir "$proto/link-to-dir"
ln -s nowhere "$proto/dangling"
ln -s file.txt "$proto/link-to-file"
mkdir "$proto/$(printf 'old\nline')"

# The set-group-ID fixture means something only where the filesystem keeps
# the bit; say so rather than letting its cases pass for the wrong reason.
case $(stat -c '%A' "$proto/sgid") in
  d?????s???) ;;
  *) echo "mkdir-diff: this filesystem does not keep a directory's set-group-ID bit;" >&2
     echo "  the sgid cases would measure nothing." >&2
     exit 2 ;;
esac

# --- comparing a whole tree ------------------------------------------------------
# Mode and type per path, sorted: `%A` is both at once (`drwxr-sr-x`). Each
# name goes through `od -c`, so one holding a newline or a byte that is not
# UTF-8 is compared exactly.
snap() {
  ( cd "$1" && find . -mindepth 1 -print0 | LC_ALL=C sort -z \
      | while IFS= read -r -d '' e; do
          printf '%s %s\n' "$(printf '%s' "$e" | od -An -c | tr -s ' \n' ' ')" \
            "$(stat -c '%A' "$e" 2>/dev/null || echo '?')"
        done )
}

# `run_side DIR SIDE REDIR ARGS...`: one side in its copy of the tree, with
# one descriptor as REDIR says. Standard error closed goes through a shell's
# `exec 2>&-`, so that only `mkdir` runs without it -- `timeout` and `env`
# keep theirs.
run_side() {
  local dir=$1 side=$2 redir=$3 bin=$4 err=$5; shift 5
  case $redir in
    '')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) </dev/null >"$bin" 2>"$err" ;;
    '>/dev/full')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) </dev/null >/dev/full 2>"$err" ;;
    '>&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) </dev/null >&- 2>"$err" ;;
    '<&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) <&- >"$bin" 2>"$err" ;;
    # Both streams into one file: where a diagnostic lands among the `-v`
    # lines is upstream's `error ()` flushing standard output first.
    '2>&1')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) </dev/null >"$bin" 2>&1 ;;
    '2>/dev/full')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" mkdir "$@" ) </dev/null >"$bin" 2>/dev/full ;;
    '2>&-')
      ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
          env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
          /bin/sh -c 'exec 2>&-; exec mkdir "$@"' sh "$@" ) </dev/null >"$bin" ;;
    *) echo "mkdir-diff: no such redirection: $redir" >&2; exit 2 ;;
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

run_case() { compare_with '' "$@"; report "mkdir $* (umask $UMASK)"; }

# `fd_case REDIR ARGS...`: run_case with one descriptor changed.
fd_case() {
  local redir=$1; shift
  compare_with "$redir" "$@"
  report "mkdir $* $redir (umask $UMASK)"
}

xfail_case() {
  local why=$1; shift
  compare_with '' "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS mkdir %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail mkdir %s (%s)\n' "$*" "$why"
  fi
  return 0
}

nl_name=$(printf 'new\nline')
old_nl=$(printf 'old\nline')
bad_utf8=$(printf 'bad\377name')
long_name=$(printf 'x%.0s' $(seq 1 300))

# --- the plain form ------------------------------------------------------------------
run_case new
run_case new other
run_case dir
run_case file.txt
run_case new dir other
run_case nosuch/new
run_case file.txt/new
run_case ro/new
run_case nox/new
run_case link-to-dir
run_case link-to-dir/new
run_case dangling
run_case dangling/new
run_case link-to-file/new
run_case ''
run_case -
run_case -- -new
run_case ./-new
run_case new/
run_case dir/
run_case new//
run_case "$nl_name"
run_case "$old_nl"
run_case "$bad_utf8"
run_case "$long_name"
run_case new new
run_case .
run_case ..
run_case /

# --- -p ---------------------------------------------------------------------------------
run_case -p a/b/c
run_case -p dir
run_case -p dir/sub
run_case -p dir/sub/new/deeper
run_case -p file.txt
run_case -p file.txt/x
run_case -p file.txt/x/y
run_case -p nosuch/x/y nosuch/x/z
run_case -p ro/x
run_case -p ro/x/y
run_case -p nox/x
run_case -p nox/x/y
run_case -p link-to-dir/x/y
run_case -p dangling
run_case -p dangling/x
run_case -p link-to-file/x
run_case -p a//b///c
run_case -p a/b/c/
run_case -p ./a/./b/.
run_case -p a/../b
run_case -p a/..
run_case -p .
run_case -p ..
run_case -p /
run_case -p ''
run_case -p "$nl_name/x"
run_case -p "a/$long_name/b"
run_case -p a a/b a/b/c
run_case -p a/b a
run_case --parents a/b
run_case --p a/b
run_case a/b -p

# --- -v ---------------------------------------------------------------------------------
run_case -v new
run_case -v new other
run_case -v dir new
run_case -v new dir other
run_case -pv a/b/c
run_case -pv dir/x/y
run_case -pv a/b a/c
run_case -pv ro/x/y
run_case -pv a/../b
run_case -pv a/b/c/
run_case -v "$nl_name"
run_case -pv "$nl_name/x"
run_case -v "$bad_utf8"
run_case -v -- -new
run_case --verbose new
run_case --verb new

# --- -m ---------------------------------------------------------------------------------
for m in 700 0700 755 777 000 0 1777 2755 4755 6755 7777 00755 02755 0002755 \
         u=rwx go= u=rwx,go=rx a=rwx a= a=,+w a=,+X =x +t g+s g-s u+s,g+s \
         go-w a-x o+t u=g g=u; do
  run_case -m "$m" new
done
for m in zzz 8 999 77777 u+z '' ,u+x a+ 0x7 -700; do
  run_case -m "$m" new
done
run_case -m zzz
run_case -m 700
run_case -m700 new
run_case --mode=700 new
run_case --mode 700 new
run_case --mod=700 new
run_case --m=700 new
run_case -m 700 -m 755 new
run_case -m 700 dir
run_case -m 700 file.txt new
run_case -p -m 700 a/b/c
run_case -p -m 700 dir
run_case -p -m 2755 a/b
run_case -p -m a=,+w a/b
run_case -pv -m 700 a/b
run_case -v -m 700 new

# The set-group-ID parent: the bit arrives from the kernel, and whether it
# survives is upstream's `dirchownmod` -- a mode that *mentions* the bit
# (five digits, or a symbolic `g-s`) takes it off again; one that does not
# leaves it.
for m in 755 0755 00755 000755 2755 02755 1777 u=rwx,go=rx g-s g+s a=rwx =x go-w; do
  run_case -m "$m" sgid/new
done
run_case sgid/new
run_case -p sgid/a/b
run_case -p -m 700 sgid/a/b
run_case -p -m 00700 sgid/a/b
run_case -v -m g-s sgid/new

# --- the umask --------------------------------------------------------------------------
for u in 077 000 027 002 300 700 777 222; do
  UMASK=$u
  run_case new
  run_case -p a/b
  run_case -m 700 new
  run_case -m a=,+w new
  run_case -m +x new
  run_case -m a+X new
  run_case -m g+s new
  run_case -p -m 755 a/b
  run_case sgid/new
  run_case -m 755 sgid/new
  UMASK=022
done

# --- the options --------------------------------------------------------------------------
run_case
run_case -p
run_case -v
run_case -pv
run_case --
run_case -q new
run_case --nosuch new
run_case --v new
run_case --=x new
run_case --parents=yes new
run_case --verbose=yes new
run_case -m
run_case --mode
run_case new -m
run_case -Z new
run_case -Z
run_case --context new
run_case --context=user_u:object_r:tmp_t:s0 new
run_case --context= new
run_case --cont new
run_case -Z -m 700 new
run_case -pvZ a/b
run_case --nosuch --help

# --- the descriptors ----------------------------------------------------------------------
fd_case '>/dev/full' new
fd_case '>/dev/full' -v new
fd_case '>/dev/full' -pv a/b/c
fd_case '>/dev/full' -v dir new
fd_case '>&-' new
fd_case '>&-' -v new
fd_case '>&-' -pv a/b/c
fd_case '>&-' -p a/b/c
fd_case '<&-' new
fd_case '<&-' -pv a/b
fd_case '2>/dev/full' new
fd_case '2>/dev/full' dir
fd_case '2>/dev/full' dir new
fd_case '2>/dev/full' -v dir new
fd_case '2>/dev/full' -q
fd_case '2>/dev/full' -m zzz new
fd_case '2>&-' new
fd_case '2>&-' dir
fd_case '2>&-' -v dir new
fd_case '2>&-' -p a/b
fd_case '2>&-' -q
fd_case '2>&1' -v new dir other
fd_case '2>&1' -pv a/b ro/x/y c
fd_case '2>&1' -pv nox/x a
fd_case '2>&1' -v --context=ctx new
fd_case '>/dev/full' --help
fd_case '>&-' --help
fd_case '>/dev/full' --version
fd_case '>&-' --version

# --- the two whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version
xfail_case "our help text: --help is acted on before the bad option" --help --nosuch
xfail_case "our version string: the first of the two wins" --version --help

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
