#!/usr/bin/env bash
# Differential test: our `chroot` against GNU coreutils 9.4's.
#
# ## Root, without being root
#
# `chroot (2)` needs `CAP_SYS_CHROOT`, so most cases run under `unshare -r`:
# a user namespace in which the harness's own uid is mapped to 0. That is
# enough to change root and to set the group to 0, and not enough for anything
# else -- `setgroups` is refused outright (`unshare -r` writes `deny` to
# `/proc/self/setgroups`, as an unprivileged mapping must), and every uid and
# gid but 0 is unmapped, so `setuid`/`setgid` to one is `EINVAL`. Both sides
# meet the same refusals, so each failure is compared as faithfully as a
# success would be; the one success path through the credentials is
# `--userspec=:0`.
#
# ## The new root
#
# A fixture tree with a few programs and the libraries they load (copied by
# what `ldd` says), and its own `/etc/passwd` and `/etc/group` that disagree
# with the machine's -- so a case can tell whether a name was looked up
# outside the new root, inside it, or both, which is the part of `chroot.c`
# that is easiest to get subtly wrong.
#
# ## Cases that differ on purpose
#
# The family's two: `--help` omits the GNU project's link block, and
# `--version` names SlateOS.
#
# Run `OURS=$(command -v chroot) ./scripts/chroot-diff.sh` to confirm the
# harness still discriminates: it should report every xfail as XPASS and
# nothing else.
set -u

DIFF_PROG='chroot'
DIFF_GNU_SOURCE=9.4
DIFF_NEED="unshare ldd"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

BOUND=/usr/bin/timeout
[ -x "$BOUND" ] || { echo "chroot-diff: no $BOUND to bound the runs with; skipping"; exit 0; }
if ! unshare -r true 2>/dev/null; then
  echo "chroot-diff: this kernel refuses unprivileged user namespaces; skipping"
  echo "  (every case that changes root would otherwise fail on both sides alike)"
  exit 0
fi

# ---------------------------------------------------------------------------
# The fixture root
# ---------------------------------------------------------------------------
root=$work/root
mkdir -p "$root/bin" "$root/etc" "$root/dir"
for prog in true false sh cat pwd env; do
  src=$(command -v "$prog") || { echo "chroot-diff: no $prog to copy; skipping"; exit 0; }
  cp -L "$src" "$root/bin/$prog"
  # The libraries it loads, at the paths it loads them from.
  ldd "$src" 2>/dev/null | grep -o '/[^ )]*' | while read -r lib; do
    [ -e "$root$lib" ] || cp -L --parents "$lib" "$root" 2>/dev/null
  done
done
printf 'inside\n' >"$root/etc/marker"
# Accounts the machine does not have, and one name it does with another id.
cat >"$root/etc/passwd" <<'EOF'
root:x:0:0:root:/:/bin/sh
fix:x:4321:4322:fixture:/:/bin/sh
nogroup:x:4500:4599:no such group:/:/bin/sh
daemon:x:4700:4322:another daemon:/:/bin/sh
EOF
cat >"$root/etc/group" <<'EOF'
root:x:0:
fixg:x:4322:fix,daemon
other:x:4400:fix
4401:x:4402:fix
EOF
# Another way to name "/": through a symbolic link, and with dots.
ln -s / "$work/slash"
empty=$work/empty
mkdir -p "$empty"

# --- knobs, reset after every case -----------------------------------------

KIND=
# Run under `unshare -r`: root in a namespace of its own.
ASROOT=
# SHELL for the case: unset by default, so the default-shell cases mean it.
CASE_SHELL=
reset_knobs() { KIND=; ASROOT=; CASE_SHELL=; }

classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: chroot }" != "$first" ]; then echo help
  elif [ "${first#chroot \(}" != "$first" ]; then echo version
  else echo other
  fi
}

run_direct() {
  local side=$1 out=$2 err=$3 rcf=$4; shift 4
  local pre=
  [ -n "$ASROOT" ] && pre="unshare -r"
  ( cd "$work" && env -u SHELL ${CASE_SHELL:+SHELL="$CASE_SHELL"} \
      PATH="$bindir/$side:$PATH" "$BOUND" -k 5 60 $pre chroot "$@" \
      </dev/null >"$out" 2>"$err" )
  echo $? >"$rcf"
  return 0
}

judge() {
  local o_out=$1 g_out=$2 o_err=$3 g_err=$4 o_rc=$5 g_rc=$6 label=$7
  local o_show g_show o_e g_e o_r g_r
  if [ -n "$KIND" ]; then
    o_show="class $(classify "$o_out")"; g_show="class $(classify "$g_out")"
  else
    o_show=$(cat "$o_out"); g_show=$(cat "$g_out")
  fi
  o_e=$(cat "$o_err"); g_e=$(cat "$g_err")
  o_r=$(cat "$o_rc"); g_r=$(cat "$g_rc")
  if [ "$o_show" = "$g_show" ] && [ "$o_e" = "$g_e" ] && [ "$o_r" = "$g_r" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: rc=%s out{%s} err{%s}\n  gnu : rc=%s out{%s} err{%s}' \
    "$o_r" "$(printf '%s' "$o_show" | tr '\n' '|')" "$(printf '%s' "$o_e" | tr '\n' '|')" \
    "$g_r" "$(printf '%s' "$g_show" | tr '\n' '|')" "$(printf '%s' "$g_e" | tr '\n' '|')")
  LABEL=$label
}

compare_direct() {
  case_no=$((case_no+1))
  local p=$work/$case_no
  run_direct ours "$p.oo" "$p.oe" "$p.or" "$@"
  run_direct gnu  "$p.go" "$p.ge" "$p.gr" "$@"
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.or" "$p.gr" \
    "${ASROOT:+[root] }${CASE_SHELL:+[SHELL=$CASE_SHELL] }chroot $*"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case()  { compare_direct "$@"; report; }
root_case() { ASROOT=1; compare_direct "$@"; report; }

xfail_case() {
  local why="$1"; shift
  compare_direct "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "chroot-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. The command line
# =============================================================================

run_case
run_case --bogus
run_case -x
run_case --
run_case --skip-chdir
run_case --userspec
run_case --groups
run_case --userspec=x
run_case --u=x
run_case --g=x
run_case --s
KIND=1; run_case --h
KIND=1; run_case --he
KIND=1; run_case --vers
KIND=1; run_case --help --bogus
KIND=1; run_case --version x y
run_case --bogus --help
xfail_case 'help omits the GNU project link block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 2. Without the privilege
# =============================================================================

run_case "$root" true
run_case / true
run_case /nonexistent true
run_case "$root/etc/marker" true          # not a directory
run_case '' true
run_case "$(printf '/no\377pe')" true      # quoted, the byte escaped
run_case "$(printf '/a b')" true

# =============================================================================
# 3. Changing root, and the working directory
# =============================================================================

root_case "$root" true
root_case "$root" false
root_case "$root" sh -c 'exit 3'
root_case "$root" cat /etc/marker
root_case "$root" pwd
root_case "$root/" pwd
root_case "$root/dir/.." cat /etc/marker
root_case / pwd
root_case /nonexistent true
root_case "$root/etc/marker" true
root_case "$empty" true                    # no command there to run
root_case '' true
root_case "$(printf '/no\377pe')" true

# --skip-chdir: only where NEWROOT is the root already, by any name.
root_case --skip-chdir / pwd
root_case --skip-chdir /. pwd
root_case --skip-chdir // pwd
root_case --skip-chdir /tmp/.. pwd
root_case --skip-chdir "$work/slash" pwd
root_case --skip-chdir "$work/slash/." pwd
root_case --skip-chdir "$root" pwd
root_case --skip-chdir /nonexistent pwd
root_case --skip-chdir --skip-chdir / pwd

# The command: found on PATH inside the new root, or not at all.
root_case "$root" nosuchcommand
root_case "$root" /bin/nosuch
root_case "$root" /etc/marker              # found, not executable: 126
root_case "$root" /etc                     # a directory: 126
root_case "$root" ''
root_case "$root" "$(printf 'na\377me')"
root_case "$root" sh -c 'echo "$0" "$@"' a -v --help

# No command: "$SHELL" -i, or /bin/sh -i.
root_case "$root"
CASE_SHELL=/bin/true;  root_case "$root"
CASE_SHELL=/bin/false; root_case "$root"
CASE_SHELL=/nonexistent; root_case "$root"
CASE_SHELL=/etc/marker; root_case "$root"

# =============================================================================
# 4. --userspec
# =============================================================================
# Every uid but 0 is unmapped here, and `setgroups` is refused, so these
# compare which step fails and how it is reported.

root_case --userspec=0:0 "$root" true
root_case --userspec=root "$root" true
root_case --userspec=root: "$root" true    # the colon is dropped
root_case --userspec=root:: "$root" true   # ...only one of them
root_case --userspec=root.root "$root" true
root_case --userspec=fix "$root" true      # the new root's account
root_case --userspec=fix:other "$root" true
root_case --userspec=fix.other "$root" true
root_case --userspec=nosuch "$root" true
root_case --userspec=nosuch: "$root" true
root_case --userspec=:nosuch "$root" true
root_case --userspec=12345 "$root" true    # a uid nobody has
root_case --userspec=12345:12345 "$root" true
root_case --userspec=:0 "$root" true       # the one that succeeds
root_case --userspec=:0 "$root" cat /etc/marker
root_case --userspec=:fixg "$root" true    # a gid the namespace has not got
root_case --userspec=:4322 "$root" true
root_case --userspec=+0 "$root" true
root_case --userspec=+fix "$root" true
root_case --userspec=' 0' "$root" true
root_case --userspec=4294967295 "$root" true
root_case --userspec= "$root" true
root_case --userspec=: "$root" true
root_case --userspec=nogroup "$root" true  # its group is in no group file
root_case --userspec=daemon "$root" true   # a name both roots have
root_case --userspec="$(id -un)" "$root" true   # only outside
root_case --userspec="$(id -un)" / true    # and with NEWROOT the old root
root_case --userspec=0 --groups=fixg "$root" true

# =============================================================================
# 5. --groups
# =============================================================================

root_case --groups= "$root" true           # clear them: refused here
root_case --groups= / true
root_case --groups=fixg "$root" true
root_case --groups=fixg,other "$root" true
root_case --groups=4322 "$root" true
root_case --groups=4401 "$root" true       # a group called 4401
root_case --groups=+4401 "$root" true      # ...and the number, by +
root_case --groups=' 4322' "$root" true
root_case --groups=-1 "$root" true
root_case --groups=4294967295 "$root" true
root_case --groups=4294967296 "$root" true
root_case --groups=0x10 "$root" true
root_case --groups=nosuch "$root" true
root_case --groups=nosuch,nosuch2 "$root" true
root_case --groups=fixg,nosuch,other "$root" true
root_case --groups=, "$root" true
root_case --groups=,, "$root" true
root_case --groups=,fixg, "$root" true
root_case --groups=nosuch "$empty" true
root_case --groups=nosuch / true
root_case --groups="$(id -gn)" "$root" true    # only outside: the fallback
root_case --groups="$(id -gn)",nosuch "$root" true
root_case --groups="$(printf 'gr\377p')" "$root" true

# =============================================================================
# 6. Descriptors closed or full
# =============================================================================

sh_case() {
  case_no=$((case_no+1))
  local p=$work/$case_no
  for side in ours gnu; do
    ( cd "$work" && PATH="$bindir/$side:$PATH" ROOT="$root" "$BOUND" -k 5 60 \
        bash -c "$1"'; echo $? >&9' </dev/null ) \
      >"$p.${side:0:1}o" 2>"$p.${side:0:1}e" 9>"$p.${side:0:1}r"
  done
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.or" "$p.gr" "[bash] $1"
  report
}
sh_case 'chroot 2>&-'
sh_case 'chroot / true 2>&-'
sh_case 'chroot --help >&-'
sh_case 'chroot --help >/dev/full'
sh_case 'unshare -r chroot "$ROOT" nosuch 2>&-'
sh_case 'unshare -r chroot "$ROOT" true >&-'
sh_case 'unshare -r chroot "$ROOT" cat /etc/marker >&-'

# =============================================================================
# Summary
# =============================================================================
total=$((pass+fail+xfail+xpass))
printf 'chroot       %d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
  "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
