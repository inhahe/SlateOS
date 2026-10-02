#!/usr/bin/env bash
# Differential test: our `install` against GNU coreutils 9.4's.
#
# ## Why the tree is compared, as `chown-diff.sh` does
#
# `install`'s result is the file it leaves behind, not what it prints: most
# invocations print nothing. Each case runs in two disposable copies of one
# fixture tree, and afterwards every path's type, mode, owner, group, size,
# content and link target is compared -- with stdout, stderr and the exit
# status. A port that printed the right words and left a 0600 file where GNU
# leaves 0755 would pass any comparison of the output alone.
#
# ## Times
#
# An installed file's mtime is the moment it was made, which differs between
# the two sides by however long the first one took. So a time is not compared
# as a number: every fixture is stamped with one fixed instant, and a path's
# mtime is reported as `stamp` when it still has that instant and `new`
# otherwise. That keeps `-p` (which must carry the stamp across) and `-C`
# (which must leave an unchanged destination untouched) both measurable.
#
# ## What an unprivileged run can do
#
# As `chown-diff.sh` measured: `-o` with the invoking user is a permitted
# no-op, `-g` with a second group the user belongs to is a real change, and
# `-o root` is a refusal with its diagnostic -- which `install` reports after
# the copy is already made, so the tree after a refusal is worth comparing
# too. The harness refuses to run as root, where every refusal would become
# a change.
#
# ## `strip`
#
# The PATH each side runs under holds only the program being compared, so a
# bare `-s` finds no `strip` -- the same failure on both sides, which is a
# case. The others name their strip program by absolute path: `/bin/true`,
# `/bin/false`, and a script that records its arguments, so that what the
# program was handed (`./-dash`, for a name that would read as an option) is
# compared too.
#
# ## The reference
#
# GNU coreutils, so `DIFF_GNU_SOURCE` builds 9.4 from source (§726).
#
# Usage: bash scripts/install-diff.sh        (VERBOSE=1 to list every case)
DIFF_PROG="install"
DIFF_GNU_SOURCE=9.4
# Upstream builds it as `src/ginstall`; see diff-wsl.sh.
DIFF_GNU_NAME="ginstall"
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "install-diff: refusing to run as root: '-o root' would be a change," >&2
  echo "  not a refusal, and every ownership case would mean something else." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

ME=$(id -un)
ALTGROUP=$(id -Gn | tr ' ' '\n' | grep -v "^$(id -gn)$" | head -1)

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

STAMP='2020-01-02 03:04:05'

# --- the tree, built once and copied per case --------------------------------
proto=$work/proto
mkdir -p "$proto/dest/sub" "$proto/srcdir"
printf 'alpha\n' > "$proto/a.txt"
printf 'bravo\n' > "$proto/b.txt"
printf '#!/bin/sh\necho hi\n' > "$proto/exe"; chmod 755 "$proto/exe"
printf 'read only\n' > "$proto/ro.txt"; chmod 444 "$proto/ro.txt"
printf 'alpha\n' > "$proto/same.txt"; chmod 755 "$proto/same.txt"
printf 'old\n' > "$proto/dst.txt"
printf 'older\n' > "$proto/numbered.txt"
printf 'one\n' > "$proto/numbered.txt.~1~"
printf 'dash\n' > "$proto/-dash"
printf 'inner\n' > "$proto/srcdir/inner.txt"
printf 'in dest\n' > "$proto/dest/a.txt"
printf 'locked\n' > "$proto/locked.txt"; chmod 444 "$proto/locked.txt"
printf 'nl\n' > "$proto/$(printf 'new\nline')"
printf 'byte\n' > "$proto/$(printf 'b\377te')"
ln -s a.txt "$proto/link"
ln "$proto/a.txt" "$proto/hard.txt"
ln -s nowhere "$proto/dangling"
ln -s dest "$proto/link-to-dest"
# A strip program that says what it was given, by appending to a log the
# tree comparison will then see.
cat > "$proto/fakestrip" <<'SH'
#!/bin/sh
printf '%s\n' "$@" >> stripped.log
SH
chmod 755 "$proto/fakestrip"
find "$proto" -exec touch -h -d "$STAMP" {} +
stamp_epoch=$(stat -c %Y "$proto/a.txt")

# --- comparing a whole tree ---------------------------------------------------
snap() {
  # NUL-separated: two fixtures have a newline or a non-UTF-8 byte in their
  # names, and `install` is asked to copy both.
  ( cd "$1" && find . -mindepth 1 -print0 | LC_ALL=C sort -z \
      | while IFS= read -r -d '' e; do
          local t m sum when
          t=$(stat -c '%F %a %U:%G %s' "$e" 2>/dev/null || echo '?')
          if [ -L "$e" ]; then
            sum="-> $(readlink "$e")"
          elif [ -f "$e" ]; then
            sum=$(md5sum < "$e" | cut -c1-8)
          else
            sum=-
          fi
          m=$(stat -c %Y "$e" 2>/dev/null)
          if [ "$m" = "$stamp_epoch" ]; then when=stamp; else when=new; fi
          printf '%s %s %s %s\n' "$(printf '%s' "$e" | od -An -c | tr -s ' \n' ' ')" "$t" "$sum" "$when"
        done )
}

# `CASE_UMASK` is the mask a case runs under; `install` clears it before it
# creates anything, so a 077 must not reach a single mode.
run_side() {
  local dir=$1 side=$2; shift 2
  ( cd "$dir" && umask "${CASE_UMASK:-022}" && diff_run timeout -k 2 20 \
      env LC_ALL=C.UTF-8 PATH="$bindir/$side" ${CASE_ENV:+"$CASE_ENV"} install "$@" )
}

compare() {
  local o_dir g_dir o_out g_out o_err g_err o_rc g_rc
  o_dir=$(mktemp -d); g_dir=$(mktemp -d)
  cp -a "$proto/." "$o_dir/"; cp -a "$proto/." "$g_dir/"
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ -n "$TO_FULL" ]; then
    run_side "$o_dir" ours "$@" </dev/null >/dev/full 2>"$o_err"; o_rc=$?
    run_side "$g_dir" gnu  "$@" </dev/null >/dev/full 2>"$g_err"; g_rc=$?
  else
    run_side "$o_dir" ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side "$g_dir" gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg o_tree g_tree
  o_msg=$(od -An -c <"$o_err"); g_msg=$(od -An -c <"$g_err")
  o_tree=$(snap "$o_dir"); g_tree=$(snap "$g_dir")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  rm -rf "$o_dir" "$g_dir"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out {%s} err {%s}\n  gnu  (rc=%s): out {%s} err {%s}\n  tree diff:\n%s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr -s ' \n' ' ')" \
    "$(diff <(printf '%s\n' "$o_tree") <(printf '%s\n' "$g_tree") | sed 's/^/    /' | head -12)")
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
  local label="${CASE_UMASK:+umask $CASE_UMASK; }${CASE_ENV:+$CASE_ENV }install $*${TO_FULL:+  [>/dev/full]}"
  compare "$@"; TO_FULL=
  report "$label"
}

# `CASE_ENV=NAME=VALUE run_env_case ARGS...`: one case under one variable.
run_env_case() {
  run_case "$@"; CASE_ENV=
}

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS install %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail install %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the three ways to name a destination ---------------------------------------
run_case a.txt out.txt
run_case a.txt dest
run_case a.txt dest/
run_case a.txt b.txt dest
run_case -t dest a.txt b.txt
run_case --target-directory=dest a.txt
run_case -T a.txt new.txt
run_case -T a.txt dest
run_case --no-target-directory a.txt dest/sub
run_case a.txt b.txt exe dest/sub
run_case a.txt link-to-dest

# --- operands missing, extra, or wrong ---------------------------------------------
run_case
run_case a.txt
run_case -t dest
run_case -T a.txt b.txt c.txt
run_case -T -t dest a.txt
run_case -t dest -t dest a.txt
run_case a.txt b.txt nosuchdir
run_case a.txt b.txt a.txt
run_case -t nosuchdir a.txt
run_case -t a.txt b.txt
run_case nosuch out.txt
run_case nosuch b.txt dest
run_case srcdir out
run_case srcdir dest
run_case dangling out.txt
run_case a.txt a.txt
run_case a.txt ./a.txt
run_case a.txt dest/sub/../../a.txt

# --- what a source can be -------------------------------------------------------------
run_case link out.txt
run_case /dev/null empty
run_case exe out
run_case ro.txt out.txt
run_case -- -dash out.txt
run_case "$(printf 'new\nline')" out.txt
run_case "$(printf 'b\377te')" out.txt
run_case a.txt "$(printf 'x\377y')"

# --- what a destination can be --------------------------------------------------------
run_case a.txt locked.txt
run_case a.txt link
run_case a.txt dangling
run_case b.txt dst.txt
run_case a.txt srcdir

# --- modes ------------------------------------------------------------------------------
run_case -m 644 a.txt out.txt
run_case -m 0600 a.txt out.txt
run_case --mode=u=rwx,go= a.txt out.txt
run_case -m +x ro.txt out.txt
run_case -m a-w a.txt out.txt
run_case -m 4755 exe out
run_case -m 2755 exe out
run_case -m 1777 a.txt out.txt
run_case -m 7777 a.txt out.txt
run_case -m bad a.txt out.txt
run_case -m '' a.txt out.txt
run_case -m 999 a.txt out.txt
run_case -m u+s,g+s exe out
run_case -m 644 -t dest a.txt b.txt

# --- -d: directories ------------------------------------------------------------------------
run_case -d newdir
run_case -d a/b/c
run_case -d newdir other
run_case -d dest
run_case -d -m 700 x/y/z
run_case -d -m 1777 tmpdir
run_case -d -m 2755 sgdir
run_case -d -m 700 dest
run_case -d a.txt
run_case -d a.txt/sub
run_case -d ''
run_case -d
run_case -dv a/b
run_case --directory --verbose newdir
run_case -d -s newdir
run_case -d -t dest newdir
run_case -d -m bad newdir
run_case -d nosuch/../made

# --- -D: leading directories --------------------------------------------------------------------
run_case -D a.txt x/y/z.txt
run_case -D a.txt dest/sub/new/f.txt
run_case -D -t new/dir a.txt b.txt
run_case -Dv a.txt x/y/z.txt
run_case -Dv -t p/q a.txt
run_case -D a.txt a.txt/sub/f
run_case -D a.txt x/
run_case -D -T a.txt q/r/s
run_case -D -m 600 a.txt deep/er/f

# --- backups ---------------------------------------------------------------------------------------
run_case -b b.txt dst.txt
run_case --backup b.txt dst.txt
run_case --backup=simple b.txt dst.txt
run_case --backup=numbered b.txt dst.txt
run_case --backup=numbered b.txt numbered.txt
run_case --backup=existing b.txt dst.txt
run_case --backup=existing b.txt numbered.txt
run_case --backup=nil b.txt numbered.txt
run_case --backup=never b.txt dst.txt
run_case --backup=t b.txt dst.txt
run_case --backup=none b.txt dst.txt
run_case --backup=off b.txt dst.txt
run_case --backup=bogus b.txt dst.txt
run_case --backup=n b.txt dst.txt
run_case -S .bak b.txt dst.txt
run_case --suffix=.old b.txt dst.txt
run_case -S '' b.txt dst.txt
run_case -S / b.txt dst.txt
run_case -b b.txt new.txt
run_case -bv b.txt dst.txt
run_case -b -t dest a.txt
CASE_ENV=VERSION_CONTROL=numbered run_env_case -b b.txt dst.txt
CASE_ENV=VERSION_CONTROL=bogus run_env_case -b b.txt dst.txt
CASE_ENV=VERSION_CONTROL=never run_env_case --backup=numbered b.txt dst.txt
CASE_ENV=SIMPLE_BACKUP_SUFFIX=.save run_env_case -b b.txt dst.txt
CASE_ENV=SIMPLE_BACKUP_SUFFIX=.save run_env_case -S .x b.txt dst.txt

# --- -C: compare first ---------------------------------------------------------------------------
run_case -C a.txt same.txt
run_case -C a.txt dst.txt
run_case -C a.txt new.txt
run_case -C -m 755 a.txt same.txt
run_case -C -m 644 a.txt same.txt
run_case --compare a.txt same.txt
run_case -C -m 4755 a.txt same.txt
run_case -C -p a.txt out.txt
run_case -C -s a.txt out.txt
run_case -C -o "$ME" a.txt same.txt
run_case -C -v a.txt same.txt
run_case -C -v a.txt dst.txt
run_case -C link same.txt
run_case -C /dev/null empty

# --- -p: timestamps -------------------------------------------------------------------------------
run_case -p a.txt out.txt
run_case --preserve-timestamps exe out
run_case -p -m 600 a.txt out.txt
run_case -p /dev/null out
run_case -p -t dest a.txt b.txt

# --- ownership ----------------------------------------------------------------------------------------
run_case -o "$ME" a.txt out.txt
run_case -o root a.txt out.txt
run_case -o 0 a.txt out.txt
run_case -o nosuchuser a.txt out.txt
run_case -o 99999999999999999999 a.txt out.txt
run_case -g nosuchgroup a.txt out.txt
run_case -o root -d newdir
run_case -o root -D a.txt x/y/z
if [ -n "$ALTGROUP" ]; then
  run_case -g "$ALTGROUP" a.txt out.txt
  run_case -g "$ALTGROUP" -d newdir/sub
  run_case -o "$ME" -g "$ALTGROUP" -m 640 a.txt out.txt
fi

# --- strip --------------------------------------------------------------------------------------------
run_case -s a.txt out.txt
run_case -s --strip-program=/bin/true a.txt out.txt
run_case -s --strip-program=/bin/false a.txt out.txt
run_case -s --strip-program=./fakestrip a.txt out.txt
run_case -s --strip-program=./fakestrip a.txt -t dest
run_case -s --strip-program=./fakestrip -- a.txt -dash
run_case -s --strip-program=/nonexistent a.txt out.txt
run_case --strip-program=/bin/true a.txt out.txt
run_case -s --strip-program=/bin/true -p a.txt out.txt

# --- verbose, and a full stdout -------------------------------------------------------------------------
run_case -v a.txt out.txt
run_case -v a.txt b.txt dest
run_case --verbose -D a.txt n/m/o
TO_FULL=1; run_case -v a.txt out.txt
TO_FULL=1; run_case a.txt out.txt
TO_FULL=1; run_case -dv a/b

# --- SELinux, on a kernel without it ----------------------------------------------------------------------
run_case -Z a.txt out.txt
run_case --preserve-context a.txt out.txt
run_case --context=system_u:object_r:bin_t:s0 a.txt out.txt
run_case --context a.txt out.txt

# --- options that do little or nothing -------------------------------------------------------------------
run_case -c a.txt out.txt
run_case -c -c a.txt out.txt

# --- option spelling ------------------------------------------------------------------------------------------
run_case --dir newdir
run_case --comp a.txt same.txt
run_case --pres a.txt out.txt
run_case --preserve-t a.txt out.txt
run_case --strip-p=/bin/true -s a.txt out.txt
run_case --suf=.x -b b.txt dst.txt
run_case --nosuchoption a.txt out.txt
run_case -q a.txt out.txt
run_case -m
run_case -t
run_case -S
run_case --mode
run_case --backup= b.txt dst.txt

# --- the umask, which install clears before it makes anything ---------------------------------------
CASE_UMASK=077 run_case a.txt out.txt
CASE_UMASK=077 run_case -d x/y/z
CASE_UMASK=077 run_case -D a.txt p/q/r.txt
CASE_UMASK=077 run_case -m 664 a.txt out.txt
CASE_UMASK=027 run_case -d -m 2775 shared
CASE_UMASK=

# --- one file under two names, and one destination named twice ------------------------------------------
run_case a.txt hard.txt
run_case hard.txt a.txt
run_case -b a.txt a.txt
run_case -b link a.txt
run_case -b a.txt hard.txt
run_case -t dest dest/a.txt
run_case a.txt dest/a.txt dest
run_case a.txt b.txt a.txt dest
run_case --backup=numbered a.txt b.txt a.txt dest
run_case -b a.txt b.txt a.txt dest
run_case link a.txt dest

# --- the two whose text is ours ---------------------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
