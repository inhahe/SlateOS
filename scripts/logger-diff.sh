#!/usr/bin/env bash
# Differential test: our `logger` against util-linux 2.39.3's.
#
# ## What is compared
#
# Almost every case runs with `--no-act --stderr`: the frame logger would send
# goes to stderr, nothing is sent, and neither side writes to the host's
# journal. stdout, stderr and the exit status must match -- except the clock.
#
# ## Keeping the clock out without masking it
#
# A frame carries a clock -- `Mmm dd hh:mm:ss` (the local header and
# `--rfc3164`) or `YYYY-MM-DDThh:mm:ss.uuuuuu+hh:mm` (`--rfc5424`) -- and the
# two sides read it at different instants. It is compared as a NUMBER, within
# the time the case actually took (uptime-diff.sh's method), never masked: a
# zone error moves it by hours, which a mask would hide. The microseconds are
# dropped (they always differ); the UTC offset is kept, because the zone cases
# exist to test it; and `syncAccuracy` -- the kernel's live `maxerror`, which
# moves between two reads -- keeps its presence and its `isSynced`, not its
# digits.
#
# ## What is deliberately not compared
#
# `--journald` writes a journal: upstream's goes to systemd-journald, ours to
# the SlateOS journal file, so only its refusals are compared. And a real
# send to `/dev/log` is avoided everywhere -- `-u` to a private socket is how
# the one delivery case sees what each side actually sends.
set -u

DIFF_PROG='logger'
DIFF_PKG='logger'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; xfail=0

# $1 = side (ours|gnu), rest = argv; stdin is the caller's.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 logger "$@"
}

# Each frame with its clock taken out.
normalize() {
  sed -E \
    -e 's/^(([0-9]+ )?<[0-9]+>)[A-Z][a-z]{2} [ 0-9][0-9] [0-9]{2}:[0-9]{2}:[0-9]{2} /\1<DATE> <CLOCK> /' \
    -e 's/^(([0-9]+ )?<[0-9]+>1 )[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}([+-][0-9]{2}:[0-9]{2}) /\1<DATE>T<CLOCK>.<USEC>\3 /' \
    -e 's/syncAccuracy="[0-9]+"/syncAccuracy="<N>"/'
}

# Seconds since midnight of every clock in the frames, one per line.
clocks() {
  sed -nE \
    -e 's/^([0-9]+ )?<[0-9]+>[A-Z][a-z]{2} [ 0-9][0-9] ([0-9]{2}):([0-9]{2}):([0-9]{2}) .*/\2 \3 \4/p' \
    -e 's/^([0-9]+ )?<[0-9]+>1 [0-9]{4}-[0-9]{2}-[0-9]{2}T([0-9]{2}):([0-9]{2}):([0-9]{2})\..*/\2 \3 \4/p' \
  | while read -r h m s; do echo $(( 10#$h * 3600 + 10#$m * 60 + 10#$s )); done
}

# The largest distance between corresponding clocks, midnight-aware; 99999
# when the two sides do not carry the same number of clocks.
max_skew() {
  local a=$1 b=$2 worst=0 x y d
  if [ "$(printf '%s' "$a" | grep -c .)" != "$(printf '%s' "$b" | grep -c .)" ]; then
    echo 99999; return
  fi
  while IFS= read -r x && IFS= read -r y <&3; do
    [ -n "$x" ] || continue
    d=$(( x - y )); [ "$d" -lt 0 ] && d=$(( -d ))
    [ "$d" -gt 43200 ] && d=$(( 86400 - d ))
    [ "$d" -gt "$worst" ] && worst=$d
  done <<<"$a" 3<<<"$b"
  echo "$worst"
}

# compare INPUT ARGV... : INPUT is fed on stdin.
compare() {
  local input=$1; shift
  local o_out g_out o_err g_err o_rc g_rc t0 t1 allowed skew
  local o_file=$DIFF_TMP/o.err g_file=$DIFF_TMP/g.err
  t0=$(date +%s)
  o_out=$(printf '%s' "$input" | run_side ours "$@" 2>"$o_file"); o_rc=$?
  g_out=$(printf '%s' "$input" | run_side gnu "$@" 2>"$g_file"); g_rc=$?
  t1=$(date +%s)
  o_err=$(cat "$o_file"); g_err=$(cat "$g_file")
  # At least 2 seconds, here and in every case below that reads a clock: WSL
  # re-syncs its clock to the host's by stepping it, and a step BACK between
  # the two sides makes the later run print the earlier second -- measured,
  # ours 10:07:33 then upstream 10:07:32 -- with `t1 - t0` then 0.
  allowed=$(( t1 - t0 + 1 )); [ "$allowed" -lt 2 ] && allowed=2

  # NEITHER SIDE RUNNING IS NOT AGREEMENT: 127 is command-not-found, 124 a
  # timeout.
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken; REPORT="  ours rc=$o_rc  gnu rc=$g_rc"; return 0
  fi
  skew=$(max_skew "$(printf '%s\n' "$o_err" | clocks)" "$(printf '%s\n' "$g_err" | clocks)")
  if [ "$(printf '%s\n' "$o_err" | normalize)" = "$(printf '%s\n' "$g_err" | normalize)" ] \
     && [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$skew" -le "$allowed" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s\n  clock skew: %ss (allowed %ss)' \
    "$o_rc" "$(printf '%s' "$o_err" | tr '\n' '|')" "$g_rc" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$skew" "$allowed")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# case_ INPUT ARGV...
case_() {
  local input=$1; shift
  compare "$input" "$@"
  report "$(printf '%q ' "$@")<<<$(printf '%q' "$input")"
}

# xfail_case WANT INPUT ARGV...: a difference the port makes on purpose. Ours
# must print exactly WANT on stderr, upstream something else, with the same
# exit status. Upstream printing WANT too is an XPASS: the deviation is gone,
# and so should this case be.
xfail_case() {
  local want=$1 input=$2; shift 2
  local o_err g_err o_rc g_rc
  o_err=$(printf '%s' "$input" | run_side ours "$@" 2>&1 >/dev/null); o_rc=$?
  g_err=$(printf '%s' "$input" | run_side gnu "$@" 2>&1 >/dev/null); g_rc=$?
  REPORT=$(printf '  ours (rc=%s): %q\n  gnu  (rc=%s): %q\n  want: %q' \
    "$o_rc" "$o_err" "$g_rc" "$g_err" "$want")
  if [ "$o_err" = "$want" ] && [ "$g_err" != "$want" ] && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes; xfail=$((xfail + 1))
  else
    AGREED=no
    [ "$o_err" = "$g_err" ] && REPORT="  XPASS: both sides agree now; drop this xfail
$REPORT"
  fi
  report "xfail: $(printf '%q ' "$@")"
}

NA=(--no-act -s --id=4242 -t mytag)

# --- framing, in several zones ------------------------------------------------
for tz in UTC America/New_York Asia/Kolkata Australia/Lord_Howe 'EST5EDT,M3.2.0,M11.1.0' ':Europe/London' ''; do
  for hdr in local --rfc3164 --rfc5424 --rfc5424=notime --rfc5424=notq --rfc5424=nohost --rfc5424=notime,nohost; do
    opts=("${NA[@]}")
    [ "$hdr" = local ] || opts+=("$hdr")
    TZ=$tz case_ '' "${opts[@]}" 'hello world'
  done
done
# (TZ unset -- /etc/localtime -- is every case below that sets none.)
case_ '' "${NA[@]}" --octet-count hi
case_ '' "${NA[@]}" --rfc5424 --msgid MSG1 hi
case_ '' "${NA[@]}" --rfc5424 --sd-id zoo@123 --sd-param 'tiger="hungry"' --sd-param 'zebra="running"' hi
case_ '' "${NA[@]}" --rfc5424 --sd-id timeQuality --sd-param 'tzKnown="0"' hi
case_ '' "${NA[@]}" --rfc5424 --sd-id meta hi
case_ '' --no-act -s -t t "$(printf '\nx')"
case_ '' --no-act -s -t t 'x
'
case_ '' --no-act -s --id=4294967295 -t t x
case_ '' --no-act -s -i -t t x --id=77
case_ '' --no-act -s -t "$(printf 'x%.0s' {1..200})" --rfc3164 x

# --- priorities ------------------------------------------------------------------
for p in user.notice notice LOCAL3.ERR 8.5 5 none 16 kernel.err critical 4294967304.5 0.3 mark.info 192.1 security.err .info user. user.info.x 96.1 8 12x; do
  case_ '' "${NA[@]}" -p "$p" x
done

# --- message boundaries --------------------------------------------------------
case_ '' "${NA[@]}" -S 10 aaaa bbbb cc
case_ '' "${NA[@]}" -S 4 ab abcdefgh cd
case_ '' "${NA[@]}" -S 1.5K x
case_ $'a\nb' "${NA[@]}"
case_ $'a\n\nb\n' "${NA[@]}"
case_ $'a\n\nb\n' "${NA[@]}" -e
case_ $'abcdefg\n' "${NA[@]}" -S 3
case_ $'<11>a\nb\n<3>c\n' "${NA[@]}" --prio-prefix
case_ $'<1234>msg\n<1999>\n<13\n<13>\n' "${NA[@]}" --prio-prefix
case_ $'<13>\n' "${NA[@]}" --prio-prefix -e
case_ $'x\n' "${NA[@]}" --rfc5424 --prio-prefix
printf 'line one\n\nline three\n' > "$DIFF_TMP/f.txt"
case_ '' "${NA[@]}" -f "$DIFF_TMP/f.txt"
case_ '' "${NA[@]}" -e -f "$DIFF_TMP/f.txt"
case_ '' "${NA[@]}" -f "$DIFF_TMP/f.txt" message-wins

# --- option errors and warnings ------------------------------------------------------
case_ '' -Q x
case_ '' --zzq x
case_ '' --pri user.err x
case_ '' -t
case_ '' --id=abc x
case_ '' --id==5 x
case_ '' -S abc x
case_ '' -S 1.9 x
case_ '' --msgid 'a b' --rfc5424 x
case_ '' --sd-id bad x
case_ '' --sd-id zoo@1 --sd-id zoo@1 x
case_ '' --sd-param 'x="y"' x
case_ '' --sd-id zoo@1 --sd-param 'x=y' x
case_ '' --no-act -s --rfc5424=bogus,notq x
case_ '' --no-act -s --socket-errors=maybe x
case_ '' -f "$DIFF_TMP/no-such-file" x
case_ '' --journald="$DIFF_TMP/no-such-file"
case_ '' -u /nonexistent/sock --socket-errors=on x
case_ '' -u "$DIFF_TMP/$(printf 'x%.0s' {1..120})" x
# Names and arguments in diagnostics: everything printable is upstream's --
# quotes, spaces, `$` and the empty name included -- ...
case_ '' "${NA[@]}" -p "it's a.info" x
case_ '' "${NA[@]}" -p 'user.$x "y"' x
case_ '' --sd-id 'bad id' x
case_ '' --sd-id "a'b" --sd-id "a'b" x
case_ '' --sd-id zoo@1 --sd-param 'x = "a b"' x
case_ '' --no-act -s --socket-errors='o n' x
case_ '' --no-act -s --rfc5424="notq,it's" x
case_ '' --no-act -s --rfc5424 -t "$(printf "'%.0s" {1..49})" x
case_ '' -f "$DIFF_TMP/it's \"missing\"" x
case_ '' -u "$DIFF_TMP/it's a \$socket" --socket-errors=on x
case_ '' -n 'no such host.invalid' -P 514 x
# ... and what is not printable is escaped, where upstream writes the raw byte
# -- a newline that would start a line of its own, an escape sequence that
# would drive the terminal (main.rs, "What is not upstream's").
xfail_case 'logger: unknown facility name: a\012b' '' "${NA[@]}" -p $'a\nb.info' x
xfail_case "logger: invalid structured data ID: 'a\\033[31mb'" '' --sd-id $'a\e[31mb' x
xfail_case 'logger: invalid argument: \015on: using automatic errors' '' --no-act --socket-errors=$'\ron' x

case_ '' -V
case_ '' --help

# --- what is actually sent -------------------------------------------------------------
# A private socket, read by a listener that prints each datagram on a line;
# both sides send the same messages to it, one side at a time.
#
# $1 path, $2 how many datagrams, $3 seconds to wait for each, $4 mode:
#   plain         a datagram socket; each datagram on a line
#   creds         the same, with SO_PASSCRED on, each followed by
#                 ` <creds:PID>` -- the PID the kernel says sent it
#   stream-creds  a stream socket: one connection read to its end, printed as
#                 received, then `<creds:PID>` for its first segment
# `$1.ready` appears once it can be reached -- for a stream, after `listen()`,
# not at `bind()`, when the socket file exists but refuses connections.
cat > "$DIFF_TMP/listen.py" <<'PY'
import socket, struct, sys

path, n, wait, mode = sys.argv[1], int(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
out = sys.stdout.buffer
space = socket.CMSG_SPACE(12)


def sender(anc):
    for level, kind, body in anc:
        if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS:
            return str(struct.unpack("i", body[:4])[0]).encode()
    return b"none"


kind = socket.SOCK_STREAM if mode == "stream-creds" else socket.SOCK_DGRAM
s = socket.socket(socket.AF_UNIX, kind)
if mode != "plain":
    s.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
s.bind(path)
s.settimeout(wait)
if mode == "stream-creds":
    s.listen(1)
open(path + ".ready", "w").close()
if mode == "stream-creds":
    try:
        c, _ = s.accept()
    except socket.timeout:
        sys.exit(0)
    c.settimeout(wait)
    first = None
    while True:
        try:
            data, anc, _, _ = c.recvmsg(65536, space)
        except socket.timeout:
            break
        if not data:
            break
        if first is None:
            first = sender(anc)
        out.write(data)
    out.write(b"<creds:" + (first or b"none") + b">\n")
else:
    for _ in range(n):
        try:
            data, anc, _, _ = s.recvmsg(65536, space)
        except socket.timeout:
            break
        if mode == "creds":
            data += b" <creds:" + sender(anc) + b">"
        out.write(data + b"\n")
        out.flush()
PY
listen_once() {
  python3 "$DIFF_TMP/listen.py" "$1" "$2" 5 plain
}
# The listener binds in the background; wait for its socket, but not forever.
wait_socket() {
  for _ in {1..100}; do
    [ -S "$1" ] && return 0
    sleep 0.05
  done
  return 1
}
sent_case() {
  local label=$1 n=$2; shift 2
  local o g sock t0 t1 allowed skew
  t0=$(date +%s)
  sock=$DIFF_TMP/o.sock; rm -f "$sock"
  listen_once "$sock" "$n" > "$DIFF_TMP/o.sent" & local lp=$!
  wait_socket "$sock" || { AGREED=broken; REPORT="  listener never bound $sock"; report "sent: $label"; return 0; }
  run_side ours -u "$sock" "$@" </dev/null 2>/dev/null; wait "$lp"
  sock=$DIFF_TMP/g.sock; rm -f "$sock"
  listen_once "$sock" "$n" > "$DIFF_TMP/g.sent" & lp=$!
  wait_socket "$sock" || { AGREED=broken; REPORT="  listener never bound $sock"; report "sent: $label"; return 0; }
  run_side gnu -u "$sock" "$@" </dev/null 2>/dev/null; wait "$lp"
  t1=$(date +%s)
  o=$(cat "$DIFF_TMP/o.sent"); g=$(cat "$DIFF_TMP/g.sent")
  allowed=$(( t1 - t0 + 1 )); [ "$allowed" -lt 2 ] && allowed=2
  skew=$(max_skew "$(printf '%s\n' "$o" | clocks)" "$(printf '%s\n' "$g" | clocks)")
  if [ -n "$o" ] && [ "$(printf '%s\n' "$o" | normalize)" = "$(printf '%s\n' "$g" | normalize)" ] && [ "$skew" -le "$allowed" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours sent: %s\n  gnu  sent: %s' "$(printf '%s' "$o" | tr '\n' '|')" "$(printf '%s' "$g" | tr '\n' '|')")
  report "sent: $label"
}
sent_case 'local header' 1 --id=4242 -t mytag 'hello world'
sent_case 'rfc5424' 1 --rfc5424 --id=4242 -t mytag 'hello world'
sent_case 'octet count' 1 --octet-count --id=4242 -t mytag 'hello world'
sent_case 'chunked' 3 -S 4 --id=4242 -t mytag aaaa bbbb cccc

# --- credentials: --id as root ------------------------------------------------------
# As root, upstream attaches SCM_CREDENTIALS naming the --id PID to a local
# message, when that PID is a live process other than logger itself. Root
# here is a user namespace (`unshare -r`): enough for `geteuid() == 0` and
# `kill(pid, 0)`, which decide whether logger ATTACHES the credentials. The
# kernel then decides whether it ACCEPTS them: it refuses (EPERM) in the
# host's PID namespace, which the namespace's root does not own, and accepts
# them in a PID namespace of its own (`unshare -p`). The listener reports
# which PID the kernel delivered: `claimed` or the `sender`'s own.
#
# One side's run, in whatever namespace the caller put it: $1 the side's bin
# directory, $2 the socket, $3 the listener mode, then logger's argv, in which
# CLAIM becomes the PID of a live helper process. Prints the exit status,
# stderr and what arrived, with the helper's PID named.
cat > "$DIFF_TMP/cred-side.sh" <<'SH'
set -u
dir=$1 sock=$2 mode=$3; shift 3
rm -f "$sock" "$sock.ready"
sleep 30 & helper=$!
python3 "${sock%/*}/listen.py" "$sock" 1 2 "$mode" > "$sock.out" & lp=$!
for _ in {1..100}; do [ -e "$sock.ready" ] && break; sleep 0.05; done
args=()
for a in "$@"; do args+=("${a//CLAIM/$helper}"); done
env LC_ALL=C.UTF-8 PATH="$dir:$PATH" timeout -k 2 20 logger -u "$sock" "${args[@]}" \
  </dev/null 2>"$sock.err"
rc=$?
wait "$lp"
kill "$helper" 2>/dev/null; wait "$helper" 2>/dev/null
printf 'rc=%s\n' "$rc"
sed 's/^/stderr: /' "$sock.err"
# The PID the kernel delivered is the helper's (claimed) or logger's own
# (sender). Logger's own PID differs run to run, so where the frame carries
# it -- `-i` -- it is named, not compared.
p=$(sed -n 's/.*<creds:\([0-9-]*\)>.*/\1/p' "$sock.out" | head -n 1)
if [ -n "$p" ] && [ "$p" = "$helper" ]; then who=claimed; else who=sender; fi
exprs=(-e "s/\[$helper\]/[CLAIM]/")
[ -n "$p" ] && exprs+=(-e "s/\[$p\]/[SENDER]/")
exprs+=(-e "s/<creds:[0-9-]*>/<creds:$who>/")
sed "${exprs[@]}" "$sock.out"
SH
# cred_case LABEL WRAPPER-WORDS -- MODE ARGV...: WRAPPER-WORDS (none for no
# namespace) run each side's script.
cred_case() {
  local label=$1; shift
  local wrap=()
  while [ "$1" != -- ]; do wrap+=("$1"); shift; done
  shift
  local mode=$1; shift
  local o g t0 t1 allowed skew
  t0=$(date +%s)
  o=$("${wrap[@]}" bash "$DIFF_TMP/cred-side.sh" "$bindir/ours" "$DIFF_TMP/c.sock" "$mode" "$@" 2>&1)
  g=$("${wrap[@]}" bash "$DIFF_TMP/cred-side.sh" "$bindir/gnu" "$DIFF_TMP/c.sock" "$mode" "$@" 2>&1)
  t1=$(date +%s)
  allowed=$(( t1 - t0 + 1 )); [ "$allowed" -lt 2 ] && allowed=2
  skew=$(max_skew "$(printf '%s\n' "$o" | clocks)" "$(printf '%s\n' "$g" | clocks)")
  if [ -n "$o" ] && [ "$(printf '%s\n' "$o" | normalize)" = "$(printf '%s\n' "$g" | normalize)" ] \
     && [ "$skew" -le "$allowed" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n  gnu : %s' "$(printf '%s' "$o" | tr '\n' '|')" "$(printf '%s' "$g" | tr '\n' '|')")
  report "credentials: $label"
}
if unshare -r true 2>/dev/null && unshare -r -p -f --mount-proc true 2>/dev/null; then
  userns=(unshare -r)
  pidns=(unshare -r -p -f --mount-proc)
  cred_case 'refused in the host PID namespace' "${userns[@]}" -- creds --id=CLAIM -t t hello
  cred_case 'accepted in a PID namespace of our own' "${pidns[@]}" -- creds --id=CLAIM -t t hello
  cred_case 'accepted, on a stream' "${pidns[@]}" -- stream-creds -T --id=CLAIM -t t hello
  cred_case 'not attached for our own PID' "${pidns[@]}" -- creds -i -t t hello
  cred_case 'not attached for a PID with no process' "${pidns[@]}" -- creds --id=99999 -t t hello
  cred_case 'not attached when not root' -- creds --id=CLAIM -t t hello
else
  echo "logger-diff: no user and PID namespaces here; the credentials cases did not run"
  broken=$((broken + 1))
fi

echo "logger-diff: $pass passed ($xfail of them expected differences), $fail failed, $broken broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
