#!/usr/bin/env bash
# Differential test: our `chmod` against GNU coreutils 9.4's.
#
# Each case runs both sides in its own copy of one fixture tree, under the
# same umask, and compares what they print, what they say, their status, and
# the tree they leave: every path's mode and type. The tree is the part that
# matters most -- a `chmod` whose messages agree and whose modes do not is the
# worse failure, since nothing on the screen says so.
#
# ## The two hazards
#
# **`-R` on `/`.** `--preserve-root` is measured only by its refusal of a
# path that *names* the root, and only where both sides refuse before
# walking; there is no case that would walk anything real if ours failed to
# refuse. Every other `-R` is inside the fixture copy.
#
# **Links out of the tree.** Every fixture symlink is relative and resolves
# inside the tree, and the harness checks that before running anything, as
# `chown-diff.sh` does: `chmod -R` does not follow links, but a link it is
# handed as an operand it does follow.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting the distribution's patched build.
set -u

DIFF_PROG='chmod'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "chmod-diff: refusing to run as root: a mistake in the fixtures would" >&2
  echo "  have privilege behind it." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
UMASK=022

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -----------------------------------
proto=$work/proto
mkdir -p "$proto/dir/sub" "$proto/empty"
printf 'a\n' > "$proto/file.txt"
printf 'b\n' > "$proto/exec.sh"
printf 'c\n' > "$proto/dir/inner.txt"
printf 'd\n' > "$proto/dir/sub/deep.txt"
chmod 644 "$proto/file.txt" "$proto/dir/inner.txt" "$proto/dir/sub/deep.txt"
chmod 755 "$proto/exec.sh" "$proto/dir" "$proto/dir/sub" "$proto/empty"
ln -s file.txt "$proto/link-to-file"
ln -s dir "$proto/link-to-dir"
ln -s nowhere "$proto/dangling"
ln -s ../file.txt "$proto/dir/in-link"
printf 'r\n' > "$proto/ref.txt"
chmod 640 "$proto/ref.txt"
printf 'n\n' > "$proto/$(printf 'name\nwith newline')"
chmod 644 "$proto/$(printf 'name\nwith newline')"

escaped=$(cd "$proto" && find . -type l -print | while IFS= read -r l; do
  t=$(readlink -f "$l" 2>/dev/null) || continue
  case "$t" in
    "$proto"/*|"$proto") ;;
    *) printf '%s -> %s\n' "$l" "$t" ;;
  esac
done)
if [ -n "$escaped" ]; then
  echo "chmod-diff: a fixture symlink resolves outside the fixture tree:" >&2
  printf '  %s\n' "$escaped" >&2
  exit 2
fi

# --- comparing a whole tree ------------------------------------------------------
# Mode and type per path, sorted: `%A` is both at once (`-rwxr-xr-x`). A path
# that vanished stops appearing.
snap() {
  ( cd "$1" && find . -mindepth 1 -print0 | LC_ALL=C sort -z \
      | while IFS= read -r -d '' e; do
          printf '%s %s\n' "$(printf '%s' "$e" | od -An -c | tr -s ' \n' ' ')" \
            "$(stat -c '%A' "$e" 2>/dev/null || echo '?')"
        done )
}

run_side() {
  local dir=$1 side=$2; shift 2
  if [ "$TO_FULL" = errclosed ]; then
    # Standard error closed for `chmod` alone, through a shell's `exec 2>&-`:
    # `timeout` and `env` keep theirs.
    ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
        env LC_ALL=C.UTF-8 PATH="$bindir/$side" \
        /bin/sh -c 'exec 2>&-; exec chmod "$@"' sh "$@" )
    return
  fi
  ( umask "$UMASK" && cd "$dir" && diff_run timeout -k 2 20 \
      env LC_ALL=C.UTF-8 PATH="$bindir/$side" chmod "$@" )
}

compare() {
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  case $TO_FULL in
    full)
      run_side "$o_dir" ours "$@" </dev/null >/dev/full 2>"$o_err"; o_rc=$?
      run_side "$g_dir" gnu  "$@" </dev/null >/dev/full 2>"$g_err"; g_rc=$?
      ;;
    closed)
      run_side "$o_dir" ours "$@" </dev/null >&- 2>"$o_err"; o_rc=$?
      run_side "$g_dir" gnu  "$@" </dev/null >&- 2>"$g_err"; g_rc=$?
      ;;
    errfull)
      run_side "$o_dir" ours "$@" </dev/null >"$o_bin" 2>/dev/full; o_rc=$?
      run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin" 2>/dev/full; g_rc=$?
      ;;
    errclosed)
      run_side "$o_dir" ours "$@" </dev/null >"$o_bin"; o_rc=$?
      run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin"; g_rc=$?
      ;;
    *)
      run_side "$o_dir" ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
      run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
      ;;
  esac
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

run_case() {
  local label="chmod $*${TO_FULL:+  [stdout $TO_FULL]} (umask $UMASK)"
  compare "$@"; TO_FULL=
  report "$label"
}

xfail_case() {
  local why=$1; shift
  compare "$@"; TO_FULL=
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS chmod %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail chmod %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- numeric modes ------------------------------------------------------------------
for m in 644 755 0644 600 000 7777 4755 2755 1777 0 7 77 00644 0000644; do
  run_case "$m" file.txt
done
run_case 755 dir
run_case 4755 dir
run_case 2755 dir
run_case 02755 dir
run_case 00755 dir
# Not modes: a digit past 7, too many digits, a sign, nothing.
for m in 8 999 77777777 +644 -644 '' 0x1ff; do
  run_case "$m" file.txt
done

# --- symbolic modes -------------------------------------------------------------------
for m in u+x g-w o=r a+rwx +x -w =r u=g g=u o=u a=- u+s g+s o+t +t a-x a+X \
         u+rwx,g-w,o-rwx ugo+r 'u+x,' ',u+x' u+z a+ 'u=rw,g=r,o=' go-rwx u-rwx+x \
         ug=rx o+w,a-w =,u+x u+X g=s; do
  run_case "$m" file.txt exec.sh dir
done
# Where the umask decides what `+x` and `-w` mean, and the warning when the
# result is not what the mode said.
for u in 022 077 000 027 777; do
  UMASK=$u
  run_case +x file.txt
  run_case -w file.txt
  run_case '=rw' file.txt
  run_case +rwx dir
  UMASK=022
done
UMASK=077 run_case -w exec.sh

# --- what it is given -----------------------------------------------------------------
run_case 600 nosuch.txt
run_case 600 file.txt nosuch.txt exec.sh
run_case 600 link-to-file
run_case 600 dangling
run_case 700 link-to-dir
run_case 600 "$(printf 'name\nwith newline')"
run_case 600
run_case
run_case u+x
run_case -- -w file.txt
run_case -w file.txt
run_case -rwx file.txt
run_case -r,u+w file.txt

# --- recursion -------------------------------------------------------------------------
run_case -R 700 dir
run_case -R u-w dir
run_case -R a+X dir
run_case -R 600 dir
run_case -R go-rwx .
run_case --recursive 755 dir link-to-dir
run_case -R 700 link-to-dir
run_case -R --preserve-root 700 dir
run_case -R --no-preserve-root 700 dir

# --- reporting -------------------------------------------------------------------------
run_case -v 755 file.txt exec.sh
run_case -v 644 file.txt
run_case -c 755 file.txt exec.sh
run_case -c 644 file.txt
run_case --verbose -R 700 dir
run_case --changes -R u+x dir
run_case -v 600 nosuch.txt
run_case -f 600 nosuch.txt
run_case -f -v 600 nosuch.txt file.txt
run_case --silent 600 nosuch.txt
run_case --quiet 600 nosuch.txt
run_case -v 600 "$(printf 'name\nwith newline')"
run_case -v 600 link-to-file
run_case -v 600 dangling

# --- a reference file -------------------------------------------------------------------
run_case --reference=ref.txt file.txt
run_case --reference ref.txt file.txt
run_case --reference=ref.txt -v file.txt exec.sh
run_case --reference=nosuch.txt file.txt
run_case --reference=link-to-file exec.sh
run_case --reference=ref.txt
run_case --reference=ref.txt 644 file.txt

# --- refusals ---------------------------------------------------------------------------
run_case --nosuchoption 600 file.txt
run_case -Z 600 file.txt
run_case --reference
run_case --verb 600 file.txt
run_case --pre 600 file.txt

# --- standard output and error that cannot be written ------------------------------------
TO_FULL=full run_case -v 755 file.txt
TO_FULL=full run_case -c 755 file.txt
TO_FULL=full run_case 755 file.txt
TO_FULL=full run_case -v -R 700 dir
TO_FULL=closed run_case -v 755 file.txt
TO_FULL=closed run_case 755 file.txt
TO_FULL=full run_case --version
TO_FULL=full run_case --help
TO_FULL=closed run_case --version
TO_FULL=errfull run_case 755 file.txt
TO_FULL=errfull run_case 755 nosuch.txt
TO_FULL=errfull run_case 755 nosuch.txt file.txt
TO_FULL=errfull run_case -v 755 nosuch.txt file.txt
TO_FULL=errfull run_case -f 755 nosuch.txt
TO_FULL=errfull run_case -Z 755 file.txt
TO_FULL=errfull run_case zzz file.txt
TO_FULL=errfull run_case 755 dangling
TO_FULL=errfull run_case -R 700 dir
TO_FULL=errfull run_case u+w file.txt
TO_FULL=errfull run_case -w file.txt
# A surprise warning: under umask 077 `-rwx` reaches only the owner's bits.
UMASK=077 TO_FULL=errfull run_case -rwx exec.sh
UMASK=077 TO_FULL=errclosed run_case -rwx exec.sh
TO_FULL=errclosed run_case 755 file.txt
TO_FULL=errclosed run_case 755 nosuch.txt
TO_FULL=errclosed run_case -v 755 nosuch.txt file.txt
TO_FULL=errclosed run_case -f 755 nosuch.txt
TO_FULL=errclosed run_case -Z 755 file.txt
TO_FULL=errclosed run_case -w file.txt

# --- the two whose text is ours -----------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
