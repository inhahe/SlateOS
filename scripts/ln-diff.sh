#!/usr/bin/env bash
# ln-diff.sh -- compare our `ln` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# `ln` prints nothing on success unless `-v` asked, so most of what it does
# is visible only in the directory afterwards. Every case therefore runs in a
# fresh copy of one fixture per side, and the comparison covers four things:
# standard output, standard error, the exit status, and a listing of the
# fixture once the program has run -- every path, each symlink's text and own
# link count, and each regular file's link count and bytes. A hard link made
# to the wrong file (the symlink's target instead of the symlink, say) agrees
# on everything except the listing.
#
#   * **the operand forms** -- `TARGET LINK_NAME`, one operand (links into
#     `.`), `TARGET... DIRECTORY`, `-t DIRECTORY`, `-T`; and GNU `main`'s rule
#     that a two-operand `ln` tries the link first and falls back to "into the
#     directory" only on `EEXIST`, `ENOTDIR` or `EINVAL` -- not when `-n` was
#     given and the name is a symlink.
#   * **replacing** -- `-f`, `-i` (answers on standard input), `-b`/`-S` and
#     `--backup=` every control word, `$VERSION_CONTROL` and
#     `$SIMPLE_BACKUP_SUFFIX`; the refusals (a directory is never
#     overwritten, a file is never replaced by a link to itself, and a hard
#     link made earlier in the same run is not replaced by a later one).
#   * **`-r`** -- relative symlinks, from relative and absolute operands, with
#     missing components and through symlinks.
#   * **`-L`/`-P`/`-d`** -- hard links to symlinks and directories.
#   * **`-v`** -- `'link' -> 'target'`, `=>` for a hard link, and the
#     `'backup' ~ ` prefix; and names that need quoting.
#
# A replacement is made under a `CuXXXXXX` name and renamed into place. If one
# is ever left behind, the listing shows it as `CuXXXXXX` (its letters are
# random) so that the two sides can still be compared, and it is a failure.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='ln'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# The fixture every case starts from: three files, two empty-ish directories,
# a directory holding a file named like one at the top (so that linking `a`
# into it collides), symlinks to a directory, to a file and to nothing, and
# two directories holding a file of the same name (for `dest_set`).
make_fixture() {
  local d=$1
  mkdir -p "$d"
  ( cd "$d" &&
    printf 'A\n' > a && printf 'B\n' > b && printf 'C\n' > c &&
    mkdir d e sub x y &&
    printf 'D\n' > d/a &&
    printf 'X\n' > x/f && printf 'Y\n' > y/f &&
    ln -s d ld && ln -s a la && ln -s nosuch dang )
}

# Everything in the directory, one line each. Names go through `%q`, so a
# newline or a high byte in a name is visible rather than splitting a line.
listing() {
  ( cd "$1" || exit 1
    find . -mindepth 1 -print0 | LC_ALL=C sort -z | while IFS= read -r -d '' f; do
      n=$f
      case ${f##*/} in Cu??????) n=${f%/*}/CuXXXXXX ;; esac
      if [ -L "$f" ]; then
        printf '%q -> %q (%s)\n' "$n" "$(readlink "$f")" "$(stat -c %h "$f")"
      elif [ -d "$f" ]; then
        printf '%q/\n' "$n"
      else
        printf '%q %s %s\n' "$n" "$(stat -c %h "$f")" "$(od -An -c "$f" | tr -s ' \n' ' ')"
      fi
    done )
}

# Per-case knobs, reset after every case.
#   ANSWERS -- standard input (the `-i` answers); empty means /dev/null-like
#   ENVV    -- extra `NAME=value` words for `env`
#   RUNS    -- how many times to run the command in the same directory
reset_knobs() { ANSWERS=''; ENVV=(); RUNS=1; }
reset_knobs

# `@DIR@` in an argument becomes that side's case directory, for the cases
# that need an absolute name. Both outputs and the listing then have the
# directory replaced by `<DIR>` again, so the two sides compare equal when
# they did the same thing.
compare() {
  local side dir out err rcs rc i a o_state g_state o_out g_out o_err g_err o_rc g_rc
  local -a args
  for side in ours gnu; do
    dir=$DIFF_TMP/case-$side
    chmod -R u+w "$dir" 2>/dev/null; rm -rf "$dir"
    make_fixture "$dir"
    args=()
    for a in "$@"; do args+=("${a//@DIR@/$dir}"); done
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    : >"$out"; : >"$err"
    printf '%s' "$ANSWERS" >"$DIFF_TMP/in-$side"
    rcs=''
    for ((i = 0; i < RUNS; i++)); do
      ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" "${ENVV[@]}" ln "${args[@]}" ) \
        <"$DIFF_TMP/in-$side" >>"$out" 2>>"$err"
      rc=$?
      rcs="$rcs$rc "
    done
    if [ "$side" = ours ]; then
      o_rc=$rcs; o_state=$(listing "$dir" | sed "s|$dir|<DIR>|g")
      o_out=$(sed "s|$dir|<DIR>|g" <"$out" | od -An -c); o_err=$(sed "s|$dir|<DIR>|g" <"$err")
    else
      g_rc=$rcs; g_state=$(listing "$dir" | sed "s|$dir|<DIR>|g")
      g_out=$(sed "s|$dir|<DIR>|g" <"$out" | od -An -c); g_err=$(sed "s|$dir|<DIR>|g" <"$err")
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$o_state" = "$g_state" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}\n  state %s\n  ours state{%s}\n  gnu  state{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$([ "$o_state" = "$g_state" ] && echo agrees || echo DIFFERS)" \
    "$(printf '%s' "$o_state" | tr '\n' '|')" "$(printf '%s' "$g_state" | tr '\n' '|')")
}

label_of() {
  local label="ln $*"
  [ -z "$ANSWERS" ] || label="$label   [in: ${ANSWERS//$'\n'/\\n}]"
  [ ${#ENVV[@]} -eq 0 ] || label="$label   [env: ${ENVV[*]}]"
  [ "$RUNS" = 1 ] || label="$label   [runs: $RUNS]"
  printf '%s' "$label"
}

run_case() {
  local label
  label=$(label_of "$@")
  compare "$@"
  reset_knobs
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

xfail_case() {
  local why=$1; shift
  local label
  label=$(label_of "$@")
  compare "$@"
  reset_knobs
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- hard links -----------------------------------------------------------------
run_case a h
run_case a b
run_case -f a b
run_case -f nosuch b
run_case nosuch z
run_case '' z
run_case a ''
run_case a nosuchdir/z
run_case a/ z
run_case a b/
run_case a d
run_case a d/
run_case -f a d
run_case a e
run_case 'a' 'has space'
run_case -v a "$(printf 'nl\nx')"
run_case -v a "$(printf 'hi\377')"

# --- symbolic links ---------------------------------------------------------------
run_case -s a s
run_case -s a b
run_case -sf a b
run_case -sf c la
run_case -sfn a ld
run_case --no-dereference -sf a ld
run_case -sf a ld
run_case -sn a ld
run_case -s '' z
run_case -s a d/
run_case -s nosuch/ z
run_case -s ../a sub/up
run_case -s a sub
run_case -s @DIR@/a abs
run_case -s a

# --- more than one target ---------------------------------------------------------
run_case a b d
run_case a b nosuch
run_case a b c
run_case a b la
run_case a b ld
run_case -t d a b
run_case -t e a b
run_case -t nosuch a
run_case -t a b
run_case -t d -t e a
run_case --target-directory=e a
run_case --target-directory e a b
run_case -t '' a
run_case -s -t e a b
run_case -sv -t e a b
run_case -f x/f y/f d
RUNS=2 run_case -f x/f y/f e
run_case -sf x/f y/f e
run_case x/f y/f e

# --- -T ---------------------------------------------------------------------------
run_case -T a d
run_case -Tf a d
run_case -Tf a e
run_case -sTf a e
run_case -sT a d
run_case -T a
run_case -T a b c
run_case -T -t d a b
run_case -T a h

# --- -v -------------------------------------------------------------------------
run_case -v a h
run_case -sv a s
run_case -fv a b
run_case -vs a d
run_case -sfnv a ld
run_case -v a b -f

# --- backups ----------------------------------------------------------------------
run_case -bv a b
run_case -bfv a b
run_case -S .old -v a b
run_case -b -S '' -v a b
run_case -b -S x/y -v a b
run_case --backup=numbered -v a b
RUNS=2 run_case --backup=numbered -fv a b
run_case --backup=t -v a b
run_case --backup=nil -v a b
run_case --backup=existing -v a b
run_case --backup=simple -v a b
run_case --backup=never -v a b
run_case --backup=numb -v a b
run_case --backup=none -v a b
run_case --backup=off -fv a b
run_case --backup=bogus a b
run_case --backup= a b
run_case --backup=bogus
# (An array cannot be a command's prefix assignment, so these set it first;
# `run_case` resets it afterwards.)
ENVV=(VERSION_CONTROL=numbered); run_case -bv a b
ENVV=(VERSION_CONTROL=bogus); run_case -bv a b
ENVV=(VERSION_CONTROL=bogus); run_case -v a h
ENVV=(VERSION_CONTROL=never); run_case --backup -v a b
ENVV=(SIMPLE_BACKUP_SUFFIX=.s); run_case -bv a b
ENVV=(SIMPLE_BACKUP_SUFFIX=.s); run_case -S .o -bv a b
ENVV=(SIMPLE_BACKUP_SUFFIX=a/b); run_case -bv a b
run_case -bv a a
run_case -bsfv a la
run_case -sbv a d
run_case -bfv a d

# --- -r -------------------------------------------------------------------------
run_case -s --relative a sub/l
run_case -sr a sub/l
run_case -sr sub/../a sub/l
run_case -srv a sub/l
run_case -sr @DIR@/a sub/l
run_case -sr a @DIR@/sub/l
run_case -sr nosuch/x sub/l
run_case -sr d sub/l
run_case -sr ld sub/l
run_case -sr a a2
run_case -srf a la
run_case -sr a sub
run_case -r a l

# --- symlinks and directories as targets ----------------------------------------
run_case d dh
run_case -d d dh
run_case -F d dh
run_case --directory d dh
run_case dang dh
run_case -L dang dh
run_case -L la h
run_case -P la h
run_case -L -P la h
run_case -P -L la h
run_case la h
run_case -nf a ld
run_case -f a ld
run_case -f la a
run_case -fL la a

# --- the same file ------------------------------------------------------------------
run_case -f a a
run_case -sf a a
run_case -sf a ./a
run_case -bf a a
run_case -f d/a d
run_case -t d d/a
run_case -sf la la

# --- interactive ----------------------------------------------------------------
run_case -i a b
ANSWERS=$'y\n' run_case -i a b
ANSWERS=$'n\n' run_case -i a b
ANSWERS=$'yes\n' run_case -i a b
ANSWERS=$'Y\n' run_case -iv a b
ANSWERS=$'y\n' run_case -is a b
ANSWERS=$'y\n' run_case -i a d
ANSWERS=$'y\n' run_case -i a h
ANSWERS=$'n\n' run_case -if a b
ANSWERS=$'n\n' run_case -fi a b
ANSWERS=$'y\nn\n' run_case -i a b d

# --- the command line -------------------------------------------------------------
run_case
run_case -q
run_case --zzz
run_case --sym a s
run_case --s a s
run_case --no- a b
run_case -S
run_case -t
run_case --help=x
run_case -- a h
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --version names SlateOS' a h --vers

chmod -R u+w "$DIFF_TMP" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
