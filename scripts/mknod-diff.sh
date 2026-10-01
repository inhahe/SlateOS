#!/usr/bin/env bash
# mknod-diff.sh — compare our `mknod` against GNU's, inside WSL.
#
# ## What this is checking
#
# `mknod`'s result is a new node, so every case runs in a fresh directory per
# side and the comparison includes a listing of it afterwards -- each name, its
# type and its permission bits -- beside the output, diagnostics and status.
#
#   * **FIFOs** -- created, with the umask applied, and with `-m` giving exactly
#     its mode (upstream creates the node and then `lchmod`s it).
#   * **devices** -- unprivileged, so creating one is `Operation not
#     permitted`; what is compared is everything checked before that: TYPE by
#     its first letter, MAJOR/MINOR as C literals that must fit 32 bits, the
#     pair that packs to `NODEV`.
#   * **the operand count** -- decided by TYPE, with a second line saying why
#     a special file needs numbers or a FIFO must not have them.
#   * **`-m`** -- checked before the operands are counted.
#
# Runs unprivileged, which is what makes the device cases refusals.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS. `-Z` and
# `--context` are refused by name here (as by our `mkfifo`), where GNU on a
# kernel without SELinux ignores `-Z` and warns about `--context`.
set -u

DIFF_PROG='mknod'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "mknod-diff: refusing to run as root, where the device cases would" >&2
  echo "  create real device nodes." >&2
  exit 2
fi

pass=0; fail=0; xfail=0; xpass=0
umask 022

make_fixture() {
  mkdir -p "$1" && printf 'x\n' > "$1/f"
}

listing() {
  ( cd "$1" && find . -mindepth 1 -printf '%p %y %m\n' | LC_ALL=C sort )
}

compare() {
  local side dir out err rc o_out g_out o_err g_err o_rc g_rc o_state g_state
  for side in ours gnu; do
    dir=$DIFF_TMP/case-$side
    rm -rf "$dir"; make_fixture "$dir"
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    ( cd "$dir" && timeout -k 2 30 env PATH="$bindir/$side" mknod "$@" ) >"$out" 2>"$err"
    rc=$?
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err"); o_state=$(listing "$dir")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err"); g_state=$(listing "$dir")
    fi
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$o_state" = "$g_state" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s} state{%s}\n  gnu  (rc=%s): err{%s} state{%s}' \
    "$o_rc" "$(printf '%s' "$o_err" | tr '\n' '|')" "$(printf '%s' "$o_state" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_err" | tr '\n' '|')" "$(printf '%s' "$g_state" | tr '\n' '|')")
}

run_case() {
  local label="mknod $*"
  compare "$@"
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
  local label="mknod $*"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- FIFOs --------------------------------------------------------------------------
run_case q p
run_case q pipe
run_case q pX
run_case -m 600 q p
run_case -m 666 q p
run_case -m 777 q p
run_case -m u+x q p
run_case -m a= q p
run_case -m =rw,+x q p
run_case --mode=640 q p
run_case -m 600 -- -q p
run_case f p
run_case nodir/q p
run_case '' p
run_case dir/ p

# --- -m, checked before the operands -------------------------------------------------
run_case -m zzz q p
run_case -m zzz
run_case -m 4755 q p
run_case -m u+s q p
run_case -m +t q p
run_case -m 1777

# --- devices: everything before the refusal ------------------------------------------
run_case q b 8 1
run_case q c 1 3
run_case q u 1 3
run_case q block 8 1
run_case q character 1 3
run_case q b 0x10 010
run_case q b 0X1F 0
run_case q b x 1
run_case q b 1 x
run_case q b 1k 1
run_case q b -- -1 1
run_case q b 4294967295 1
run_case q b 4294967296 1
run_case q b 1 4294967296
run_case q b 4294967295 4294967295
run_case q b ' 1' 1
run_case q b '1 ' 1
run_case f b 8 1

# --- the operand count ------------------------------------------------------------------
run_case
run_case q
run_case q b
run_case q c 1
run_case q p 1
run_case q p 1 2
run_case q p 1 2 3
run_case q b 1 2 3
run_case q x
run_case q x 1 2
run_case q '' 1 2

# --- the command line ---------------------------------------------------------------------
run_case -x q p
run_case --nope q p
run_case --mode
run_case -m
run_case --mo=600 q p
xfail_case 'our --help omits the GNU ancillary block' q p --help
run_case --help=1
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'we refuse -Z by name; GNU ignores it without SELinux' -Z q p
xfail_case 'we refuse --context by name; GNU warns and goes on' --context=x q p

rm -rf "$DIFF_TMP/case-ours" "$DIFF_TMP/case-gnu"

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
