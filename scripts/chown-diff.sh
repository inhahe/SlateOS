#!/usr/bin/env bash
# Differential test: our `chown` against GNU `chown`.
#
# ## Why this harness compares the tree, like `patch-diff.sh` and unlike the rest
#
# `chown`'s output is a **side effect**. Most invocations print nothing at all
# and the whole result is a changed inode. A `chown` that printed the right
# words while changing the wrong file, or the wrong number of files, would pass
# any comparison of stdout. So each case runs in two disposable copies of the
# same tree and what is compared afterwards is every path's owner and group.
#
# ## What an unprivileged run can and cannot do, which shapes every case here
#
# Measured on this host rather than assumed:
#
#     chown root f        Operation not permitted    (refused)
#     chown $(id -un) f   rc 0                       (a permitted no-op)
#     chown :users f      rc 0                       (a REAL change -- the
#                                                     invoking user is a member)
#
# So the harness gets all three kinds of case without privilege: refusals with
# their exact diagnostics, permitted no-ops, and genuine ownership changes via
# the group. That last one is what makes `-R`, `-h` and `--from` worth testing
# at all -- without a change that succeeds, every recursive case would be
# indistinguishable from every other.
#
# ## The two hazards, and what is done about them
#
# **Symlinks that leave the tree.** `chown -R -L` follows symlinks into
# directories. A fixture link pointing at `/etc` would make the harness chown
# something outside its own temp tree. Every link below is relative and internal,
# and there is a check after the fixtures are built that refuses to run if any
# of them resolves outside.
#
# **Running as root.** Then `chown root` stops being a refusal and becomes a
# change, so the cases mean something different -- and a bug in the fixture
# construction would have privilege behind it. The harness refuses, as
# `hostname-diff.sh` does, though the blast radius here is a temp directory
# rather than the machine.
#
# ## The reference
#
# GNU coreutils, so §726 applies: `DIFF_GNU_SOURCE` builds 9.4 from source
# rather than trusting Ubuntu's patched `9.4-3ubuntu6.3`.
set -u

DIFF_PROG='chown'
DIFF_GNU_SOURCE=9.4
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "chown-diff: refusing to run as root." >&2
  echo "  Unprivileged, 'chown root f' is a refusal and a comparison. As root" >&2
  echo "  it is a change, so every case below means something different -- and" >&2
  echo "  a mistake in the fixtures would have privilege behind it." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0

ME=$(id -un)
MYUID=$(id -u)
MYGID=$(id -g)
# A group the invoking user belongs to that is NOT the primary one, so a change
# to it is both permitted and observable. Chosen from the real membership list
# rather than hard-coded, and the harness refuses rather than guessing if there
# is no second group -- a case that cannot change anything proves nothing.
ALTGROUP=$(id -Gn | tr ' ' '\n' | grep -v "^$(id -gn)$" | head -1)
if [ -z "$ALTGROUP" ]; then
  echo "chown-diff: $ME belongs to only one group, so no ownership change is" >&2
  echo "  permitted here and every case would be a refusal. Skipping rather" >&2
  echo "  than reporting a green run that measured only diagnostics." >&2
  exit 0
fi

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- the tree, built once and copied per case -----------------------------------
proto=$work/proto
mkdir -p "$proto/dir/sub"
printf 'a\n' > "$proto/file.txt"
printf 'b\n' > "$proto/dir/inner.txt"
printf 'c\n' > "$proto/dir/sub/deep.txt"
# Links, all relative and all inside the tree -- see "The two hazards".
ln -s file.txt "$proto/link-to-file"
ln -s dir "$proto/link-to-dir"
ln -s nowhere "$proto/dangling"

# The check that the comment above promises. A link that escapes would make
# `-R -L` chown something outside the fixture tree, so this refuses rather than
# trusting that the lines above stayed correct.
escaped=$(cd "$proto" && find . -type l -print | while IFS= read -r l; do
  t=$(readlink -f "$l" 2>/dev/null) || continue
  case "$t" in
    "$proto"/*|"$proto") ;;
    *) printf '%s -> %s\n' "$l" "$t" ;;
  esac
done)
if [ -n "$escaped" ]; then
  echo "chown-diff: a fixture symlink resolves outside the fixture tree:" >&2
  printf '  %s\n' "$escaped" >&2
  echo "  Refusing: 'chown -R -L' would follow it and change something real." >&2
  exit 2
fi

# --- comparing a whole tree ------------------------------------------------------
# Owner and group per path, sorted. Not the mode and not the content: `chown`
# changes neither, so including them would only add noise -- but a path that
# VANISHED shows up, because it stops appearing in the listing.
snap() {
  ( cd "$1" && find . -mindepth 1 -print | LC_ALL=C sort \
      | while IFS= read -r e; do
          printf '%s %s\n' "$e" "$(stat -c '%U:%G' "$e" 2>/dev/null || echo '?:?')"
        done )
}

run_side() {
  local dir=$1 side=$2; shift 2
  ( cd "$dir" && diff_run timeout -k 2 20 \
      env LC_ALL=C.UTF-8 PATH="$bindir/$side" chown "$@" )
}

compare() {
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  run_side "$o_dir" ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
  run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg o_tree g_tree
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snap "$o_dir"); g_tree=$(snap "$g_dir")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
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

run_case() { compare "$@"; report "chown $*"; }

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS chown %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail chown %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- permitted no-ops: the owner is already this user -------------------------------
run_case "$ME" file.txt
run_case "$MYUID" file.txt
run_case "$ME:" file.txt
run_case "$ME." file.txt

# --- genuine changes, through the group ----------------------------------------------
run_case ":$ALTGROUP" file.txt
run_case ".$ALTGROUP" file.txt
run_case "$ME:$ALTGROUP" file.txt
run_case ":$ALTGROUP" file.txt dir
run_case ":$ALTGROUP" link-to-file
run_case -h ":$ALTGROUP" link-to-file
run_case --no-dereference ":$ALTGROUP" link-to-file
run_case ":$ALTGROUP" dangling
run_case -h ":$ALTGROUP" dangling

# --- refusals that need no privilege ---------------------------------------------------
run_case root file.txt
run_case root:root file.txt
run_case 0 file.txt
run_case :0 file.txt
run_case nosuchuser file.txt
run_case :nosuchgroup file.txt
run_case "$ME:nosuchgroup" file.txt
run_case root nosuchfile.txt
run_case "$ME" nosuchfile.txt

# --- recursion, and the three link policies -----------------------------------------------
run_case -R ":$ALTGROUP" dir
run_case --recursive ":$ALTGROUP" dir
run_case -R ":$ALTGROUP" .
run_case -R -H ":$ALTGROUP" link-to-dir
run_case -R -L ":$ALTGROUP" link-to-dir
run_case -R -P ":$ALTGROUP" link-to-dir
run_case -R -h ":$ALTGROUP" .
run_case -R root dir

# --- --from, which only acts when the current owner matches ----------------------------------
run_case "--from=$ME" ":$ALTGROUP" file.txt
run_case "--from=root" ":$ALTGROUP" file.txt
run_case "--from=$ME:$(id -gn)" ":$ALTGROUP" file.txt
run_case "--from=nosuchuser" ":$ALTGROUP" file.txt
run_case --from= ":$ALTGROUP" file.txt

# --- --reference ------------------------------------------------------------------------------
run_case --reference=dir/inner.txt file.txt
run_case --reference=nosuchfile.txt file.txt
run_case --reference=file.txt -R dir

# --- reporting -----------------------------------------------------------------------------------
run_case -c ":$ALTGROUP" file.txt
run_case --changes ":$ALTGROUP" file.txt
run_case -c "$ME" file.txt
run_case -v ":$ALTGROUP" file.txt
run_case --verbose "$ME" file.txt
run_case -v root file.txt
run_case -f root file.txt
run_case --silent root file.txt
run_case -f nosuchuser file.txt
run_case -f root nosuchfile.txt
run_case -c -v ":$ALTGROUP" file.txt

# --- dereference --------------------------------------------------------------------------------------
run_case --dereference ":$ALTGROUP" link-to-file
run_case --dereference -h ":$ALTGROUP" link-to-file

# --- operand shapes and refusals --------------------------------------------------------------------------
run_case
run_case "$ME"
run_case : file.txt
run_case "" file.txt
run_case "$MYUID:$MYGID" file.txt
run_case ":" file.txt
run_case -- "$ME" file.txt
run_case -Q "$ME" file.txt
run_case --nosuchoption "$ME" file.txt

# --- long-option abbreviation ---------------------------------------------------------------------------------
run_case --recur ":$ALTGROUP" dir
run_case --no-deref ":$ALTGROUP" link-to-file
run_case --ref=dir/inner.txt file.txt
run_case --chang ":$ALTGROUP" file.txt

# --- the two whose text is ours -----------------------------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
