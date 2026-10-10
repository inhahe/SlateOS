#!/usr/bin/env bash
# Differential test: `syslogd daemon` against systemd-journald, the program
# that owns `/dev/log` on the Linux every port here is measured against.
#
# ## The oracle
#
# The same datagrams go to both, from one process, so the sender each sees is
# the same process: its PID, its user, its group, its command name. journald's
# records are read back with `journalctl -o json`; ours are the lines of our
# journal file. Each pair is compared field for field, through the mapping
# design-decisions §1063 sets out:
#
# | journald | ours |
# |---|---|
# | `PRIORITY` | `level` |
# | `SYSLOG_FACILITY` | `SYSLOG_FACILITY`, both present or both absent |
# | `SYSLOG_IDENTIFIER` | `service`; absent (or empty), `service` is `_COMM` |
# | `SYSLOG_PID`, a number | `pid` |
# | `SYSLOG_PID`, anything else | `SYSLOG_PID`, and `pid` is the sender's |
# | `SYSLOG_TIMESTAMP`, `SYSLOG_RAW` | the same, both present or both absent |
# | `MESSAGE` | `msg` |
# | `_PID`, `_UID`, `_GID`, `_COMM` | the same |
#
# Values are compared as bytes: a JSON string as its UTF-8, an array as the
# bytes it lists -- which is how both spell a field that is not text.
#
# ## A private journald
#
# Inside WSL this re-executes itself under `unshare -r -m -n`, mounts tmpfs
# over journald's directories and starts a journald of its own there, so the
# frames reach neither the host's journal nor its journald. (Measured on
# 2026-10-07: systemd 255's journald runs unprivileged in a user namespace,
# complaining only that it cannot read `/dev/kmsg` or join the audit group.)
set -u

# --- into private namespaces ------------------------------------------------------
# Before `diff-wsl.sh` is sourced, as `syslog-client-check.sh` does and for its
# reason: the preamble makes a scratch directory with an `EXIT` trap, which
# `exec` would skip.
if command -v wslpath >/dev/null 2>&1 && [ -z "${SYSLOGD_DIFF_NS:-}" ]; then
  if command -v unshare >/dev/null 2>&1 && unshare -r -m -n true 2>/dev/null; then
    export SYSLOGD_DIFF_NS=1
    exec unshare -r -m -n bash "$0" "$@"
  fi
  echo "syslogd-diff: no user, mount and network namespaces here; SKIPPED"
  exit 0
fi

DIFF_PROG='syslogd'
DIFF_PKG='syslogd'
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED='timeout python3 mount journalctl'
DIFF_FORWARD='SYSLOGD_DIFF_NS'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ -z "${SYSLOGD_DIFF_NS:-}" ]; then
  echo "syslogd-diff: not inside the private namespaces; refusing to start a journald"
  exit 1
fi
journald=/lib/systemd/systemd-journald
if [ ! -x "$journald" ]; then
  echo "syslogd-diff: no $journald here to measure against; SKIPPED"
  exit 0
fi

# --- the two daemons ----------------------------------------------------------------
for dir in /run/systemd/journal /run/log/journal /var/log/journal; do
  [ -d "$dir" ] || continue
  mount -t tmpfs tmpfs "$dir" || { echo "syslogd-diff: cannot mount over $dir"; exit 1; }
done
"$journald" >"$DIFF_TMP/journald.out" 2>&1 & jpid=$!
"$OURS" daemon --socket "$DIFF_TMP/log" --journal "$DIFF_TMP/journal.jsonl" \
  2>"$DIFF_TMP/ours.err" & opid=$!
# Both stopped on every way out, as part of the preamble's cleanup -- not a
# second `EXIT` trap, which would replace it.
diff_cleanup() {
  kill "$jpid" "$opid" 2>/dev/null
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
}
for _ in $(seq 1 100); do
  [ -S /run/systemd/journal/dev-log ] && [ -S "$DIFF_TMP/log" ] && break
  sleep 0.05
done
[ -S /run/systemd/journal/dev-log ] || { echo "syslogd-diff: journald never listened"; cat "$DIFF_TMP/journald.out"; exit 1; }
[ -S "$DIFF_TMP/log" ] || { echo "syslogd-diff: ours never listened"; cat "$DIFF_TMP/ours.err"; exit 1; }

# --- the frames, and the comparison ---------------------------------------------------
cat > "$DIFF_TMP/compare.py" <<'PY'
import json, os, socket, subprocess, sys, time

ours_sock, ours_journal = sys.argv[1], sys.argv[2]
JOURNALD = "/run/systemd/journal/dev-log"

# Each a datagram, sent as it is. The local form, the forms journald does not
# take apart, and the edges measured against it.
FRAMES = [
    b"<13>Oct  7 16:30:00 tag[123]: hello local",
    b"<13>Oct  7 16:30:00 tag: no pid",
    b"<13>Oct  7 16:30:00 myhost tag[123]: with a hostname",
    b"<34>1 2026-10-07T16:30:00.123456-04:00 myhost app 456 ID47 [ex@32473 a=\"b\"] rfc5424",
    b"<13>tag: no timestamp",
    b"<13>Oct  7 16:30:00 tag[12]:msg without space",
    b"<13>Oct  7 16:30:00 tag[abc]: odd pid",
    b"<13>Oct  7 16:30:00 tag[]: empty pid",
    b"<13>Oct  7 16:30:00 [99]: no identifier",
    b"<13>Oct  7 16:30:00 tag: multi\nline",
    b"<13>Oct  7 16:30:00 tag: bad \xff byte",
    b"<13>Oct  7 16:30:00 tag: \x01control",
    b"<13>Oct  7 16:30:00 tag:   three spaces",
    b"<13>Oct  7 16:30:00 tag:\ttab separator",
    b"<13>Oct  7 16:30:00 tag:",
    b"<13>Oct  7 16:30:00 tag: trailing newline\n",
    b"<13>Oct  7 16:30:00 tag: end spaces   ",
    b"  <13>Oct  7 16:30:00 tag: leading spaces",
    b"<13>Oct  7 16:30:00 tag: nul\x00after",
    b"<0>Oct  7 16:30:00 tag: kern emerg",
    b"<5>Oct  7 16:30:00 tag: kern notice",
    b"<191>Oct  7 16:30:00 tag: local7 debug",
    b"<999>Oct  7 16:30:00 tag: big priority",
    b"<1234>tag: four digits",
    b"<abc>tag: letters in the priority",
    b"<13tag: unclosed priority",
    b"<13>Oct 07 16:30:00 tag: zero-padded day",
    b"<13>Oct  7 16:30:00tag: no space after the timestamp",
    b"<14>Oct  7 16:30:00 tag[ 77 ]: spaced pid",
    b"plain words, no header at all",
    b"<30>Oct  7 16:30:00 ntpd[4242]: step time server 127.0.0.1 offset 10.0 sec",
    "<13>Oct  7 16:30:00 tag: café ☃".encode(),
]

s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
for frame in FRAMES:
    s.sendto(frame, JOURNALD)
    s.sendto(frame, ours_sock)
    time.sleep(0.02)


def journald_records():
    for d in ("/run/log/journal", "/var/log/journal"):
        out = subprocess.run(
            ["journalctl", "-o", "json", "--no-pager", "-D", d],
            capture_output=True,
        ).stdout
        recs = [json.loads(l) for l in out.splitlines() if l.strip()]
        recs = [r for r in recs if r.get("_TRANSPORT") == "syslog"]
        if recs:
            return recs
    return []


def ours_records():
    try:
        with open(ours_journal, "rb") as f:
            return [json.loads(l) for l in f.read().splitlines() if l.strip()]
    except FileNotFoundError:
        return []


deadline = time.time() + 20
while time.time() < deadline:
    j, o = journald_records(), ours_records()
    if len(j) >= len(FRAMES) and len(o) >= len(FRAMES):
        break
    time.sleep(0.2)


def raw(v):
    """A JSON value as bytes: a string's UTF-8, an array's bytes."""
    if v is None:
        return None
    if isinstance(v, list):
        return bytes(v)
    return str(v).encode()


LEVELS = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"]
passed = failed = 0
if len(j) < len(FRAMES) or len(o) < len(FRAMES):
    print(f"FAIL: journald filed {len(j)}, ours {len(o)}, of {len(FRAMES)} frames")
    failed += 1
for frame, jr, orec in zip(FRAMES, j, o):
    want = {}
    want["level"] = LEVELS[int(jr["PRIORITY"])].encode()
    want["SYSLOG_FACILITY"] = raw(jr.get("SYSLOG_FACILITY"))
    ident = raw(jr.get("SYSLOG_IDENTIFIER"))
    want["service"] = ident if ident else raw(jr.get("_COMM"))
    spid = raw(jr.get("SYSLOG_PID"))
    if spid is not None and spid.isdigit():
        want["pid"] = spid
        want["SYSLOG_PID"] = None
    else:
        want["pid"] = raw(jr.get("_PID"))
        want["SYSLOG_PID"] = spid
    for key in ("SYSLOG_TIMESTAMP", "SYSLOG_RAW", "_PID", "_UID", "_GID", "_COMM"):
        want[key] = raw(jr.get(key))
    want["msg"] = raw(jr.get("MESSAGE"))
    got = {k: raw(orec.get(k)) for k in want}
    diffs = [k for k in want if want[k] != got[k]]
    if diffs:
        failed += 1
        print(f"DIFF {frame!r}")
        for k in diffs:
            print(f"  {k}: journald {want[k]!r}, ours {got[k]!r}")
    else:
        passed += 1
        if os.environ.get("VERBOSE"):
            print(f"ok   {frame!r}")
print(f"\n{passed} passed, {failed} differed")
sys.exit(1 if failed else 0)
PY

python3 "$DIFF_TMP/compare.py" "$DIFF_TMP/log" "$DIFF_TMP/journal.jsonl"
