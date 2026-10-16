#!/usr/bin/env bash
# Differential test: our `lastlog` against shadow-utils 4.13's.
#
# The reference is Ubuntu's build, in `login`.
#
# `lastlog` has no option naming its file: it reads /var/log/lastlog,
# /etc/passwd and /etc/login.defs and nothing else. So most cases run both
# sides in a user namespace (`unshare -Ur`), where `-R ROOT` can chroot, into
# a root built here -- a fresh copy for each side of each case, since `-S` and
# `-C` write the file.
#
# What is compared: stdout, stderr and the exit status of each case, and
# after `-S` and `-C` the lastlog file each side leaves, record by record --
# a time within a minute of now reading as `NOW`, since the two sides run a
# moment apart.
#
#   * records before the file's end and past it; lines longer than the
#     column, hosts of every width; times in several zones;
#   * every account, -u by name, by uid, by every shape of range, and ones
#     that name nobody; -t and -b in each base getulong reads;
#   * LASTLOG_UID_MAX set, unset, unparsable, above and below the -u given;
#     items login.defs may and may not hold; a line longer than fgets reads;
#   * -S and -C on one account and on ranges, and their refusals;
#   * -R and --root in each spelling process_root_flag scans, and the ones
#     it does not; the root missing, relative, a file;
#   * the command line's refusals, and the real system's files, unchrooted.
#
# Cases that differ on purpose: none.
set -u

DIFF_PROG='lastlog'
DIFF_NEED='timeout python3 unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if ! unshare -Ur true 2>/dev/null; then
  echo "lastlog-diff: SKIPPED -- unprivileged user namespaces are refused here"
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0; broken=0
tmpl=$DIFF_TMP/template
mkdir -p "$tmpl"

python3 - "$tmpl" <<'PY'
import os, struct, sys, time
d = sys.argv[1]
os.makedirs(os.path.join(d, "etc"))
os.makedirs(os.path.join(d, "var/log"))

def put(name, data):
    with open(os.path.join(d, name), "wb") as f:
        f.write(data)

put("etc/passwd", b"".join([
    b"root:x:0:0:root:/root:/bin/bash\n",
    b"daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n",
    b"alice:x:1000:1000::/home/alice:/bin/sh\n",
    b"bob:x:1001:1001::/home/bob:/bin/sh\n",
    b"carol:x:1002:1002::/home/carol:/bin/sh\n",
    b"averyveryverylongname:x:1003:1003::/home/x:/bin/sh\n",
    b"dupe:x:1000:1000::/home/dupe:/bin/sh\n",
    b"nobody:x:65534:65534::/nonexistent:/usr/sbin/nologin\n",
    b"far:x:4294967294:100::/:/bin/sh\n",
]))
put("etc/group", b"root:x:0:\n")
put("etc/nsswitch.conf", b"passwd: files\ngroup: files\n")

def ll(t, line=b"", host=b""):
    return struct.pack("<i32s256s", t, line, host)

now = int(time.time())
recs = {
    0: ll(1700000000, b"pts/0", b"192.0.2.1"),
    1: ll(0),
    1000: ll(now - 2 * 86400, b"tty123456789", b"fe80::1234:5678:9abc:def0%enp0s31f6"),
    1001: ll(now - 40 * 86400, b"ssh", b"h" * 60),
    1002: ll(-100, b"ttyS0", b""),
}
size = 1003 * 292
data = bytearray(size)
for uid, r in recs.items():
    data[uid * 292:(uid + 1) * 292] = r
put("var/log/lastlog", bytes(data))
put("etc/login.defs", b"# nothing set\n")
PY

# --- knobs ------------------------------------------------------------------
# DEFS: login.defs for this case (a file's contents; DEFS_DIR=1 makes it a
# directory, DEFS_NONE=1 removes it). NOLASTLOG=1: no lastlog file. ENVS:
# extra environment. NOROOT=1: run unchrooted and without a user namespace.
# ROOTARG: what -R names (default the side's root).
DEFS=; DEFS_DIR=; DEFS_NONE=; NOLASTLOG=; ENVS=(); NOROOT=; ROOTARG=; REDIR=
reset_knobs() { DEFS=; DEFS_DIR=; DEFS_NONE=; NOLASTLOG=; ENVS=(); NOROOT=; ROOTARG=; REDIR=; }

# The lastlog file a side left, as text: a time within a minute of now reads
# as NOW, since the two sides run a moment apart.
dump_lastlog() {
  python3 - "$1" <<'PY'
import struct, sys, time
try:
    data = open(sys.argv[1], "rb").read()
except OSError as e:
    print("(no file: %s)" % e.strerror)
    sys.exit(0)
now = time.time()
print("size %d" % len(data))
for i in range(0, len(data) - len(data) % 292, 292):
    t, line, host = struct.unpack("<i32s256s", data[i:i + 292])
    if t == 0 and line.strip(b"\0") == b"" and host.strip(b"\0") == b"":
        continue
    shown = "NOW" if abs(t - now) < 60 else str(t)
    print("uid %d: %s %r %r" % (i // 292, shown, line.rstrip(b"\0"), host.rstrip(b"\0")))
PY
}

run_side() {
  local side=$1; shift
  local root=$DIFF_TMP/root-$side
  rm -rf "$root"
  cp -a "$tmpl" "$root"
  [ -n "$DEFS" ] && printf '%s' "$DEFS" >"$root/etc/login.defs"
  [ -n "$DEFS_NONE" ] && rm -f "$root/etc/login.defs"
  [ -n "$DEFS_DIR" ] && { rm -f "$root/etc/login.defs"; mkdir "$root/etc/login.defs"; }
  [ -n "$NOLASTLOG" ] && rm -f "$root/var/log/lastlog"
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=C.UTF-8" "TZ=UTC" "${ENVS[@]}")
  if [ -n "$NOROOT" ]; then
    local -a cmd=(timeout -k 2 20 "${envs[@]}" sh -c \
      'redir=$1; shift; eval "exec lastlog \"\$@\" $redir"' _ "$REDIR" "$@")
    diff_run "${cmd[@]}" </dev/null
    return
  fi
  local rootarg=${ROOTARG:-$root}
  local -a cmd=(timeout -k 2 20 unshare -Ur "${envs[@]}" sh -c \
    'redir=$1; shift; eval "exec lastlog \"\$@\" $redir"' _ "$REDIR" -R "$rootarg" "$@")
  ( cd "$DIFF_TMP" && diff_run "${cmd[@]}" </dev/null )
  local rc=$?
  { echo "=== lastlog afterwards"; dump_lastlog "$root/var/log/lastlog"; } >>"$DIFF_TMP/$side.after"
  return $rc
}

compare() {
  : >"$DIFF_TMP/ours.after"; : >"$DIFF_TMP/gnu.after"
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  cat "$DIFF_TMP/ours.after" >>"$DIFF_TMP/o.out"
  cat "$DIFF_TMP/gnu.after" >>"$DIFF_TMP/g.out"
  LABEL="lastlog $*"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ -n "$DEFS" ] && LABEL="$LABEL [login.defs: $(printf '%s' "$DEFS" | head -c 60 | tr '\n' '|')]"
  [ -n "$DEFS_NONE" ] && LABEL="$LABEL [no login.defs]"
  [ -n "$DEFS_DIR" ] && LABEL="$LABEL [login.defs a directory]"
  [ -n "$NOLASTLOG" ] && LABEL="$LABEL [no lastlog]"
  [ -n "$NOROOT" ] && LABEL="$LABEL [unchrooted]"
  [ -n "$ROOTARG" ] && LABEL="$LABEL [-R $ROOTARG]"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs
  if [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$DIFF_TMP/o.out" | head -40)" "$(cat -A "$DIFF_TMP/o.err" | head -10)" \
    "$g_rc" "$(cat -A "$DIFF_TMP/g.out" | head -40)" "$(cat -A "$DIFF_TMP/g.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached lastlog on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

# A guard against vacuous agreement: the reference must have read the root.
run_case
if ! grep -q '^root .*192\.0\.2\.1' "$DIFF_TMP/g.out"; then
  echo "lastlog-diff: the reference did not read the fixture root:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- who -----------------------------------------------------------------------
for u in root daemon alice bob carol averyveryverylongname dupe nobody far \
         nosuch 0 1 1000 1003 65534 4294967294 4294967295 99999 \
         1000- -1001 1000-1002 1002-1000 0-0 1- -0 '' - -- 1-x x-1 '1 ' ' 1' \
         +5 0x10 4294968296 18446744073709551615 18446744073709551616; do
  run_case -u "$u"
done
run_case -u alice -u bob
run_case --user=carol
run_case --user carol

# --- how recent ------------------------------------------------------------------
for t in 0 1 3 30 39 41 1000 0x10 010 '' x 1x -1 99999999999999999999; do
  run_case -t "$t"
  run_case -b "$t"
done
run_case -t 3 -b 1
run_case -t 50 -b 30 -u 1000-
run_case --time=3
run_case --before 1

# --- zones -------------------------------------------------------------------------
for z in UTC 'ABC-5:30' 'XYZ+3' 'EST5EDT,M3.2.0,M11.1.0' 'ABC0:30'; do
  ENVS=("TZ=$z"); run_case
done

# --- login.defs ------------------------------------------------------------------------
DEFS_NONE=1; run_case
DEFS_DIR=1; run_case
DEFS=$'LASTLOG_UID_MAX 1000\n'; run_case
DEFS=$'LASTLOG_UID_MAX 1000\n'; run_case -u 1001
DEFS=$'LASTLOG_UID_MAX 1000\n'; run_case -u 900-2000
DEFS=$'LASTLOG_UID_MAX 1000\n'; run_case -u -2000
DEFS=$'LASTLOG_UID_MAX 1000\n'; run_case -S -u 1001
DEFS=$'LASTLOG_UID_MAX 0x3e8\n'; run_case
DEFS=$'LASTLOG_UID_MAX abc\n'; run_case
DEFS=$'LASTLOG_UID_MAX\n'; run_case
DEFS=$'LASTLOG_UID_MAX ""\n'; run_case
DEFS=$'LASTLOG_UID_MAX "1001" ignored\n'; run_case
DEFS=$'  LASTLOG_UID_MAX\t\t  1002  \n'; run_case
DEFS=$'LASTLOG_UID_MAX 4294967296\n'; run_case
DEFS=$'LASTLOG_UID_MAX 4294967296\n'; run_case -u 0-
DEFS=$'LASTLOG_UID_MAX -1\n'; run_case
DEFS=$'LASTLOG_UID_MAX 1000\nLASTLOG_UID_MAX 1\n'; run_case
DEFS=$'FOO bar\nBCRYPT_MAX_ROUNDS 5\nTCB_SYMLINKS yes\nCHFN_AUTH yes\nMOTD_FIRSTONLY no\nUID_MIN 1000\n# FOO\n'; run_case
DEFS=$'FOO\n'; run_case
long=$(printf '%01100d' 0 | tr 0 X)
DEFS="$long 5"$'\nLASTLOG_UID_MAX 1\n'; run_case
DEFS=$'LASTLOG_UID_MAX 1000'; run_case

# --- -S and -C ---------------------------------------------------------------------------
for u in alice 1000 bob 1003 nobody 65534 0-1 1000-1002 1001- -1 nosuch 99999; do
  run_case -S -u "$u"
  run_case -C -u "$u"
done
run_case -S -u alice -t 1
run_case --set --user alice
run_case --clear --user=bob
run_case -S
run_case -C
run_case -S -C -u alice
run_case -C -S
NOLASTLOG=1; run_case -S -u alice
NOLASTLOG=1; run_case
NOLASTLOG=1; run_case -u alice

# --- -R and --root ------------------------------------------------------------------------
# In the namespace, a root named again is a second one.
ROOTARG=relative; run_case
ROOTARG=/nonexistent-lastlog-root; run_case
ROOTARG=/etc/passwd; run_case
run_case -R /tmp
run_case --root=/tmp
run_case -R
# Outside it, chroot itself is refused -- after every other check.
for args in '-R /tmp' '--root /tmp' '--root=/tmp' '--root=' '-R' '-u root -R' '-- -R' \
            '-R/tmp' '-R relative' '-R /nonexistent-lastlog-root' '-R /etc/passwd' \
            '-R /tmp -R /tmp' '-R -R' '--root=/tmp extra'; do
  # shellcheck disable=SC2086  # the arguments are words, on purpose
  NOROOT=1; run_case $args
done

# --- the command line ------------------------------------------------------------------------
for args in '-h' '--help' '-h -Z' '-Z' '--nosuch' '-u' '-t' '-b' 'extra' 'extra more' \
            '-u root extra' '--hel' '--he' '-C -u' '--clear=1'; do
  # shellcheck disable=SC2086  # the arguments are words, on purpose
  run_case $args
done

# --- the system's own files, unchrooted ----------------------------------------------------
NOROOT=1; run_case
NOROOT=1; run_case -u root
NOROOT=1; run_case -u 0-100
NOROOT=1; run_case -t 1
NOROOT=1; run_case -h
NOROOT=1; REDIR='>&-'; run_case
NOROOT=1; REDIR='>/dev/full'; run_case
NOROOT=1; REDIR='2>&-'; run_case -u nosuch
NOROOT=1; REDIR='>/dev/full'; run_case -h

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
