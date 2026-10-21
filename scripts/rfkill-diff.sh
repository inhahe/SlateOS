#!/usr/bin/env bash
# Differential test: our `rfkill` against util-linux 2.39.3's.
#
# rfkill talks to the kernel through /dev/rfkill -- a stream of 8-byte events
# to read, and requests to write -- and reads each device's name and type from
# /sys/class/rfkill/rfkillN/. WSL has neither, and both sides must see the
# same devices. So each case runs in a user and mount namespace of its own
# (`unshare -Urm`, no root): a tmpfs over /dev holding a FIFO as /dev/rfkill,
# its events written first through a descriptor the case keeps open (so a
# read of what is left says EAGAIN, as the kernel's device does), and a tmpfs
# over /sys/class holding the devices' files. What is left in the FIFO
# afterwards -- the requests block and unblock wrote -- is compared too, and
# so is what each side logged: the tmpfs holds a /dev/log the harness
# listens on, and each request's line (its timestamp masked) is kept. The
# log is what shows toggle's requests, which it reads back out of the FIFO.
#
# Mounts made in a user namespace are nodev, so nothing in the case may open
# a device by name: /dev/null, /dev/full and the terminal are reached through
# descriptors opened outside.
#
# What is compared: stdout, stderr, the exit status, what is left in the
# FIFO, and the syslog lines --
#
#   * the table (the default, -o in each form, --output-all, -J, -r, -n, the
#     exclusive pair) and the old `list` format, for every identifier kind --
#     a type, an alias, `all`, an index that exists, one that does not, one
#     that is not a number or is out of range, an upper-case name;
#   * the sysfs reader's quirks: a missing or empty file shows what was read
#     last, at most 127 bytes are read, a NUL ends the value;
#   * a device type with no description, which fails upstream's assertion;
#   * a short event; non-ADD events, which listing skips;
#   * block, unblock and toggle, by index, type and `all`, and the requests
#     they leave in the FIFO;
#   * event, its timestamps masked, and its end when stdout's reader has gone;
#   * the options' refusals, --help, --version, an unknown command;
#   * on a terminal of several widths, and under closed and full stdout.
#
# The reference is unpacked from Ubuntu's rfkill package by
# util-linux-extra.sh, since the WSL image does not install it.
set -u

DIFF_PROG='rfkill'
DIFF_PKG='rfkill'
DIFF_NEED='timeout script stty unshare mount mkfifo python3'
# The reference is unpacked, and put on PATH, after the preamble's re-execs.
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"
# shellcheck source=util-linux-extra.sh
. "$(dirname "$0")/util-linux-extra.sh"
ul_extra_bindir usr/sbin/rfkill

pass=0; fail=0; xfail=0; broken=0

# --- helpers run inside a case's namespace ------------------------------------
# events.py IDX,TYPE,OP,SOFT,HARD ... | raw:HEX ...: written to descriptor 3.
cat > "$DIFF_TMP/events.py" <<'EOF'
import os, struct, sys
data = b""
for a in sys.argv[1:]:
    if a.startswith("raw:"):
        data += bytes.fromhex(a[4:])
    else:
        idx, ty, op, soft, hard = (int(x) for x in a.split(","))
        data += struct.pack("=IBBBB", idx, ty, op, soft, hard)
os.write(3, data)
EOF
# drain.py: what is left on descriptor 3, as hex.
cat > "$DIFF_TMP/drain.py" <<'EOF'
import os
os.set_blocking(3, False)
data = b""
while True:
    try:
        chunk = os.read(3, 4096)
    except BlockingIOError:
        break
    if not chunk:
        break
    data += chunk
print(data.hex())
EOF
diff_devlog
# noreader.py ARGV...: ARGV with stdout a pipe whose reader has gone.
cat > "$DIFF_TMP/noreader.py" <<'EOF'
import os, subprocess, sys
r, w = os.pipe()
os.close(r)
p = subprocess.run(sys.argv[1:], stdout=w, pass_fds=())
sys.exit(p.returncode if p.returncode >= 0 else 128 - p.returncode)
EOF

# inner.sh SCENARIO OUT ERR LEFT HOW ARGV...: one case, inside the namespace.
cat > "$DIFF_TMP/inner.sh" <<'EOF'
set -u
here=$(dirname "$0")
. "$here/devlog.sh"
scen=$1 out=$2 err=$3 left=$4 how=$5; shift 5
# The namespace came up: a side without this file compared nothing.
: > "${out%.out}.ran"
log=${out%.out}.log
: > "$log"
mount -t tmpfs tmpfs /dev || exit 90
mount -t tmpfs tmpfs /sys/class || exit 91
mkdir /sys/class/rfkill
# Rust's runtime puts /dev/null where a standard descriptor is closed, and
# aborts if it cannot open it; the device cannot be had here, so a file.
: > /dev/null && chmod 666 /dev/null
# dev IDX NAME TYPE: a device's files; `-` for a file that is not there.
dev() {
  mkdir -p "/sys/class/rfkill/rfkill$1"
  [ "$2" = - ] || printf '%s\n' "$2" > "/sys/class/rfkill/rfkill$1/name"
  [ "$3" = - ] || printf '%s\n' "$3" > "/sys/class/rfkill/rfkill$1/type"
}
fifo() { mkfifo /dev/rfkill && exec 3<>/dev/rfkill; }
events() { python3 "$here/events.py" "$@"; }
std() {
  dev 0 phy0 wlan; dev 1 hci0 bluetooth; dev 2 wwan0 wwan; dev 3 nfc0 nfc
  events 0,1,0,0,0 1,2,0,1,0 2,5,0,0,1 0,1,2,1,0 3,8,0,1,1 9,1,1,0,0
}
case $scen in
  nodev) ;;
  empty) fifo ;;
  std) fifo; std ;;
  short) fifo; std; events raw:010203 ;;
  odd)
    fifo
    dev 0 phy0 wlan
    dev 1 - bluetooth
    mkdir -p /sys/class/rfkill/rfkill2; : > /sys/class/rfkill/rfkill2/name; echo wwan > /sys/class/rfkill/rfkill2/type
    dev 3 "$(printf 'L%.0s' $(seq 1 200))" gps
    mkdir -p /sys/class/rfkill/rfkill4; printf 'fm\0radio\n' > /sys/class/rfkill/rfkill4/name; echo fm > /sys/class/rfkill/rfkill4/type
    mkdir -p /sys/class/rfkill/rfkill5; printf 'uwb0' > /sys/class/rfkill/rfkill5/name; printf 'uwb' > /sys/class/rfkill/rfkill5/type
    dev 4294967295 max wimax
    dev 6 'name with  spaces' -
    events 0,1,0,0,0 1,2,0,0,0 2,5,0,1,0 3,6,0,0,0 4,7,0,0,1 5,3,0,1,1 4294967295,4,0,0,0 6,8,0,0,0
    ;;
  alltype) fifo; dev 0 phy0 wlan; events 0,1,0,0,0 1,0,0,0,0 ;;
  unknown) fifo; dev 0 phy0 wlan; dev 1 x foo; events 0,1,0,0,0 1,12,0,0,0 ;;
  many)
    fifo
    for i in $(seq 0 11); do dev "$i" "$(printf 'dev%.0s' $(seq 0 "$i"))$i" wlan; events "$i,$(( i % 8 + 1 )),0,$(( i % 2 )),$(( i % 3 == 0 ))"; done
    ;;
  *) echo "inner.sh: no scenario $scen" >&2; exit 92 ;;
esac
devlog_start "$log"
# --foreground: timeout kills only the program, not itself as well, so the
# shell has no `Killed` notice -- which names a PID -- to print. The python
# helper is the one case with a child of its own, and needs the group killed.
run() { PATH="$BIN:/usr/bin:/bin" timeout --foreground -s KILL "${KILL_AFTER:-5}" "$@"; }
case $how in
  plain) run rfkill "$@" > "$out" 2> "$err" < "$0" 3>&- ;;
  pty) run rfkill "$@" 2> "$err" 3>&- ;;
  full) run rfkill "$@" >&5 2> "$err" < "$0" 3>&- ;;
  closed) run rfkill "$@" >&- 2> "$err" < "$0" 3>&- ;;
  noreader)
    PATH="$BIN:/usr/bin:/bin" timeout -s KILL "${KILL_AFTER:-5}" \
      python3 "$here/noreader.py" rfkill "$@" 2> "$err" < "$0" 3>&- ;;
esac
rc=$?
if [ "$scen" = nodev ]; then : > "$left"; else python3 "$here/drain.py" > "$left"; fi
devlog_stop
exit "$rc"
EOF

# --- running a case ---------------------------------------------------------------
# side SIDE SCENARIO HOW ARGV...: one side, its files under $DIFF_TMP/SIDE.*.
side() {
  local s=$1 scen=$2 how=$3; shift 3
  local o=$DIFF_TMP/$s
  case $how in
    pty)
      diff_run env BIN="$bindir/$s" LC_ALL=C.UTF-8 KILL_AFTER="${KILL_AFTER:-5}" timeout -k 1 15 \
        script -qec "stty cols $COLS rows 60 && unshare -Urm bash $(printf '%q ' "$DIFF_TMP/inner.sh" "$scen" "$o.out" "$o.err" "$o.left" pty "$@")" /dev/null \
        </dev/null >"$o.out"
      ;;
    full)
      diff_run env BIN="$bindir/$s" LC_ALL=C.UTF-8 KILL_AFTER="${KILL_AFTER:-5}" timeout -k 1 15 \
        unshare -Urm bash "$DIFF_TMP/inner.sh" "$scen" "$o.out" "$o.err" "$o.left" full "$@" 5>/dev/full
      ;;
    *)
      diff_run env BIN="$bindir/$s" LC_ALL=C.UTF-8 KILL_AFTER="${KILL_AFTER:-5}" timeout -k 1 15 \
        unshare -Urm bash "$DIFF_TMP/inner.sh" "$scen" "$o.out" "$o.err" "$o.left" "$how" "$@"
      ;;
  esac
}

# The timestamps `event` prints differ from run to run.
mask_times() {
  sed -E -i 's/^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2},[0-9]{6}[+-][0-9]{2}:[0-9]{2}: /TS: /' "$@"
}

judge() {
  local o_rc=$1 g_rc=$2
  # No `.ran`: the namespace never came up (`unshare` refused). 90-93: it
  # could not be set up; 126 and 127: the program was never run -- either
  # way nothing was compared.
  if [ ! -f "$DIFF_TMP/ours.ran" ] || [ ! -f "$DIFF_TMP/gnu.ran" ] \
     || { [ "$o_rc" -ge 90 ] && [ "$o_rc" -le 93 ]; } || { [ "$g_rc" -ge 90 ] && [ "$g_rc" -le 93 ]; } \
     || [ "$o_rc" = 126 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 126 ] || [ "$g_rc" = 127 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out" \
     && cmp -s "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err" \
     && cmp -s "$DIFF_TMP/ours.left" "$DIFF_TMP/gnu.left" \
     && cmp -s "$DIFF_TMP/ours.log" "$DIFF_TMP/gnu.log" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q left %q log %q\n  gnu  (rc=%s): out %q err %q left %q log %q' \
    "$o_rc" "$(head -c 1500 "$DIFF_TMP/ours.out")" "$(head -c 600 "$DIFF_TMP/ours.err")" "$(cat "$DIFF_TMP/ours.left")" "$(cat "$DIFF_TMP/ours.log")" \
    "$g_rc" "$(head -c 1500 "$DIFF_TMP/gnu.out")" "$(head -c 600 "$DIFF_TMP/gnu.err")" "$(cat "$DIFF_TMP/gnu.left")" "$(cat "$DIFF_TMP/gnu.log")")
}

# case_ SCENARIO HOW ARGV...: both sides, judged.
case_() {
  local scen=$1 how=$2 o_rc g_rc label; shift 2
  rm -f "$DIFF_TMP"/ours.* "$DIFF_TMP"/gnu.*
  side ours "$scen" "$how" "$@"; o_rc=$?
  side gnu "$scen" "$how" "$@"; g_rc=$?
  for f in out err left log; do
    [ -f "$DIFF_TMP/ours.$f" ] || : > "$DIFF_TMP/ours.$f"
    [ -f "$DIFF_TMP/gnu.$f" ] || : > "$DIFF_TMP/gnu.$f"
  done
  [ -n "${MASK:-}" ] && mask_times "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out"
  diff_mask_logged_time "$DIFF_TMP/ours.log" "$DIFF_TMP/gnu.log"
  # The shell's notice of a program killed by a signal names its PID.
  sed -E -i 's/^(.*inner\.sh: line [0-9]+: +)[0-9]+ /\1PID /' "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err"
  judge "$o_rc" "$g_rc"
  label="[$scen ${how}${COLS:+ $COLS cols}] rfkill $(printf '%q ' "$@")"
  # A case that never ran is broken whether or not it was expected to
  # differ: nothing was compared.
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$label" "$REPORT"
  elif [ -n "${WHY:-}" ]; then
    if [ "$AGREED" = yes ]; then
      fail=$((fail + 1)); printf 'XPASS %s (expected to differ: %s)\n' "$label" "$WHY"
    else
      xfail=$((xfail + 1)); [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n%s\n' "$label" "$WHY" "$REPORT"
    fi
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$label" "$REPORT"
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}
c() { case_ std plain "$@"; }

# --- the table and the old format ----------------------------------------------------
c
c list
for id in wlan wifi bluetooth uwb ultrawideband wimax wwan gps fm nfc all 0 1 2 3 9 4294967295 \
          WLAN bogus '' 1x 99999999999 -1 +1 ' 1' 0x1 01; do
  c list "$id"
  c -o ID,TYPE list "$id"
done
c list 0 1
c list wlan bluetooth
c list 0 bogus 1
c list 0 1x 1
c -o ID list 0 1x
for o in ID TYPE DEVICE TYPE-DESC SOFT HARD id type-desc ID,TYPE +TYPE-DESC +ID,ID ',ID' 'ID,' '' + \
         BOGUS ID,BOGUS,TYPE 'ID,,TYPE' ID,TYPE,DEVICE,TYPE-DESC,SOFT,HARD,ID,TYPE,DEVICE,TYPE-DESC,SOFT,HARD \
         +ID,TYPE,DEVICE,TYPE-DESC,SOFT,HARD,ID; do
  c -o "$o"
done
c -o ID -o TYPE
c -oID
c --output=ID
c --outp=ID
c --output-all
c --output-al
c --output-all -o +ID
c --output-all list
c -J
c -J --output-all
c -r
c -r --output-all
c -n
c -nr
c -Jn
c -J -r
c -r -J
c --json --raw
c -J list
c -J list wlan
c -o ID list
c list -o ID
c list -J -o ID,DEVICE
c -r list wlan
c -n -o +TYPE-DESC

# --- options and commands --------------------------------------------------------------
c help
c help list
c --help
c -h
c -h list
c -V
c --version
c --vers
c -Vh
c bogus
c LIST
c -x
c --bogus
c -o
c --output
c -- list
c list --
c -- -J
c block
c unblock
c toggle

# --- other device sets -------------------------------------------------------------------
for s in nodev empty short odd unknown alltype many; do
  case_ "$s" plain
  case_ "$s" plain list
  case_ "$s" plain --output-all
  case_ "$s" plain -J
  case_ "$s" plain -o +TYPE-DESC list
  case_ "$s" plain list 1
  case_ "$s" plain list wlan
  case_ "$s" plain block 0
  # Not after a short event: toggle reads back from the FIFO the requests it
  # wrote -- the kernel's device never echoes them -- and once the stray
  # bytes have put the reads out of step, what it reads back can match, so
  # both sides write and read for ever.
  [ "$s" = short ] || case_ "$s" plain toggle all
  case_ "$s" plain help
done
case_ odd plain -o DEVICE,TYPE
case_ odd plain -o TYPE,DEVICE
case_ odd plain -o DEVICE
case_ odd plain list 4294967295
case_ odd plain list 6
case_ odd plain -o DEVICE list 1 2 0
case_ odd plain -r
case_ odd plain -J
case_ many plain -r --output-all
case_ many plain -n
case_ many plain list all
case_ short plain list 0
case_ short plain -o ID list wlan nfc

# --- requests --------------------------------------------------------------------------------
for a in block unblock; do
  for id in 0 1 3 wlan wifi bluetooth nfc all 9 bogus 1x 4294967295 99999999999; do
    c "$a" "$id"
  done
  c "$a" 0 1
  c "$a" 0 bogus 1
  c "$a" 0 1x 1
  c "$a" wlan all
done
for id in 0 1 2 3 wlan bluetooth wwan nfc all 9 bogus 1x; do
  c toggle "$id"
done
c toggle 0 1
c toggle wlan bluetooth all
case_ odd plain toggle all
case_ odd plain block 4294967295
case_ odd plain unblock 6

# --- event --------------------------------------------------------------------------------------
KILL_AFTER=1 MASK=1 c event
KILL_AFTER=1 MASK=1 c event extra args
KILL_AFTER=1 MASK=1 c -J event
KILL_AFTER=1 MASK=1 case_ short plain event
KILL_AFTER=1 MASK=1 case_ odd plain event
KILL_AFTER=1 MASK=1 case_ empty plain event
case_ nodev plain event
KILL_AFTER=1 MASK=1 case_ std noreader event
KILL_AFTER=1 MASK=1 case_ std closed event

# --- standard output -----------------------------------------------------------------------------
for how in full closed noreader; do
  case_ std "$how"
  case_ std "$how" list
  case_ std "$how" -J
  case_ std "$how" --help
  case_ std "$how" -V
  case_ std "$how" block 0
  case_ many "$how" --output-all
  case_ nodev "$how"
done

# --- a terminal of several widths ----------------------------------------------------------------
for COLS in 10 20 30 40 50 60 80 120; do
  case_ std pty
  case_ std pty --output-all
  case_ many pty --output-all
  case_ odd pty -o DEVICE,TYPE-DESC,ID
  case_ std pty list
  case_ std pty -r --output-all
done
unset COLS

# --- what is not upstream's ---------------------------------------------------------------------------
WHY='an identifier in a diagnostic is escaped (design-decisions 370)' c list "$(printf 'wl\001an')"
WHY='an identifier in a diagnostic is escaped (design-decisions 370)' c block "$(printf 'x\tz')"

printf '\nrfkill-diff: %d passed, %d differed, %d differ on purpose, %d broken\n' "$pass" "$fail" "$xfail" "$broken"
[ "$fail" = 0 ] && [ "$broken" = 0 ]
