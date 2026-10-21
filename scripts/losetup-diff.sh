#!/usr/bin/env bash
# Differential test: our `losetup` against Ubuntu's, util-linux 2.39.3's.
#
# losetup learns about loop devices from sysfs -- /sys/block's loopN entries
# with a loop/backing_file, and their offset, sizelimit, flags and sector
# size -- and from the devices' nodes in /dev; only a backing file's device
# and inode need the device opened (LOOP_GET_STATUS64), which a user cannot.
# Setting a device up or detaching one needs root, which no harness here has.
#
# So each case runs in a user and mount namespace of its own (`unshare -Urm`,
# no root), as an ordinary user is: WSL's own loop devices as they are, or a
# tmpfs over /sys holding loop devices of the harness's own -- offsets, size
# limits, every flag, backing files with odd names, devices with an
# attribute missing, devices not set up -- beside the real /dev/loop0..7
# nodes, which `stat` sees as block devices with the loop major. A tmpfs
# lists a directory newest first, so the order /sys/block gives -- the order
# of the listing -- is the reverse of the order the scenario makes them in.
#
# What is compared: stdout, stderr and the exit status --
#
#   * --all's lines and the table in each form (-l, -J, --raw, -n, -O in
#     each form, --output-all), all devices, one device, -j's matches (by
#     name, by canonical name, at an offset);
#   * -f, which finds a free device by scanning sysfs when
#     /dev/loop-control cannot be written;
#   * every refusal a user meets: detach, set capacity, sector size, direct
#     I/O, setting up -- each "Permission denied" from the device's open --
#     and the options' own;
#   * on a terminal of several widths, where BACK-FILE is cut.
set -u

DIFF_PROG='losetup'
DIFF_PKG='losetup'
DIFF_REF='/usr/sbin/losetup'
DIFF_NEED='timeout script stty unshare mount'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

# Files the fake devices are "backed" by, so -j can find them by name, and
# their canonical names can differ from the names given.
mkdir -p "$DIFF_TMP/files/dir with spaces" "$DIFF_TMP/files/real"
: > "$DIFF_TMP/files/a.img"
: > "$DIFF_TMP/files/dir with spaces/b.img"
: > "$DIFF_TMP/files/real/c.img"
ln -sfn real "$DIFF_TMP/files/link"
F=$DIFF_TMP/files

# inner.sh SCENARIO OUT ERR HOW ARGV...: one case, inside the namespace.
cat > "$DIFF_TMP/inner.sh" <<'EOF'
set -u
scen=$1 out=$2 err=$3 how=$4; shift 4
# The namespace came up: a side without this file compared nothing.
: > "${out%.out}.ran"
F=$FILES
# loopdev N BACKING OFFSET SIZELIMIT AUTOCLEAR PARTSCAN DIO RO LOGSEC: a
# device's sysfs directory; BACKING `-` for one not set up, any other `-`
# for an attribute that is not there.
loopdev() {
  local n=$1 back=$2 off=$3 sz=$4 ac=$5 ps=$6 dio=$7 ro=$8 ls=$9
  local d=/sys/devices/virtual/block/loop$n
  mkdir -p "$d/queue"
  ln -s "../devices/virtual/block/loop$n" "/sys/block/loop$n"
  ln -s "../../devices/virtual/block/loop$n" "/sys/dev/block/7:$n"
  echo "7:$n" > "$d/dev"
  [ "$ro" = - ] || echo "$ro" > "$d/ro"
  [ "$ls" = - ] || echo "$ls" > "$d/queue/logical_block_size"
  [ "$back" = - ] && return 0
  mkdir "$d/loop"
  printf '%s\n' "$back" > "$d/loop/backing_file"
  [ "$off" = - ] || echo "$off" > "$d/loop/offset"
  [ "$sz" = - ] || echo "$sz" > "$d/loop/sizelimit"
  [ "$ac" = - ] || echo "$ac" > "$d/loop/autoclear"
  [ "$ps" = - ] || echo "$ps" > "$d/loop/partscan"
  [ "$dio" = - ] || echo "$dio" > "$d/loop/dio"
}
fake() {
  mount -t tmpfs tmpfs /sys || exit 90
  mkdir -p /sys/block /sys/dev/block /sys/devices/virtual/block || exit 91
}
case $scen in
  real) ;;
  nosys) mount -t tmpfs tmpfs /sys || exit 90 ;;
  empty) fake ;;
  many)
    fake
    loopdev 0 "$F/a.img" 0 0 0 1 0 0 512
    loopdev 1 - - - - - - 0 512
    loopdev 2 "$F/dir with spaces/b.img" 1048576 4096 1 0 1 1 4096
    loopdev 3 "$F/real/$(printf 'n%.0s' $(seq 1 120)).img" 0 0 0 0 0 0 512
    loopdev 4 "$F/a.img" 512 0 1 1 0 0 512
    loopdev 5 "$F/real/c.img" - - - - - - -
    loopdev 6 - - - - - - - -
    loopdev 7 "$F/link/c.img" 18446744073709551615 9223372036854775808 0 0 0 0 1024
    ;;
  allused)
    fake
    for n in 0 1 2 3 4 5 6 7; do loopdev "$n" "$F/a.img" $((n * 512)) 0 0 0 0 0 512; done
    ;;
  *) echo "inner.sh: no scenario $scen" >&2; exit 92 ;;
esac
run() { PATH="$BIN:/usr/bin:/bin" timeout --foreground -s KILL 10 "$@"; }
case $how in
  plain) run losetup "$@" > "$out" 2> "$err" < /dev/null ;;
  pty) run losetup "$@" 2> "$err" ;;
esac
EOF

# side SIDE SCENARIO HOW ARGV...: one side, its files under $DIFF_TMP/SIDE.*.
side() {
  local s=$1 scen=$2 how=$3; shift 3
  local o=$DIFF_TMP/$s
  case $how in
    pty)
      diff_run env BIN="$bindir/$s" FILES="$F" LC_ALL=C.UTF-8 timeout -k 1 20 \
        script -qec "stty cols $COLS rows 60 && unshare -Urm bash $(printf '%q ' "$DIFF_TMP/inner.sh" "$scen" "$o.out" "$o.err" pty "$@")" /dev/null \
        </dev/null >"$o.out"
      ;;
    *)
      diff_run env BIN="$bindir/$s" FILES="$F" LC_ALL=C.UTF-8 timeout -k 1 20 \
        unshare -Urm bash "$DIFF_TMP/inner.sh" "$scen" "$o.out" "$o.err" "$how" "$@"
      ;;
  esac
}

# case_ SCENARIO HOW ARGV...: both sides, judged.
case_() {
  local scen=$1 how=$2 o_rc g_rc label; shift 2
  rm -f "$DIFF_TMP"/ours.* "$DIFF_TMP"/gnu.*
  side ours "$scen" "$how" "$@"; o_rc=$?
  side gnu "$scen" "$how" "$@"; g_rc=$?
  for s in ours gnu; do for f in out err; do [ -f "$DIFF_TMP/$s.$f" ] || : > "$DIFF_TMP/$s.$f"; done; done
  label="[$scen $how${COLS:+ $COLS cols}] losetup $(printf '%q ' "$@")"
  if [ ! -f "$DIFF_TMP/ours.ran" ] || [ ! -f "$DIFF_TMP/gnu.ran" ] \
     || { [ "$o_rc" -ge 90 ] && [ "$o_rc" -le 92 ]; } || { [ "$g_rc" -ge 90 ] && [ "$g_rc" -le 92 ]; } \
     || [ "$o_rc" = 126 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 126 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 137 ] || [ "$g_rc" = 137 ]; then
    broken=$((broken + 1)); printf 'BROKEN %s (rc %s/%s)\n' "$label" "$o_rc" "$g_rc"
  elif cmp -s "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out" && cmp -s "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err" \
     && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s (rc %s/%s)\n' "$label" "$o_rc" "$g_rc"
    for f in out err; do
      cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" ||
        diff -u --label "ours.$f" --label "gnu.$f" "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" | head -30
    done
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}

LISTS=('' '-a' '-l' '-l -n' '-J' '--raw' '-n --raw' '--output-all' '--output-all -J' '--output-all --raw'
  '-O NAME' '-O name,back-file' '-O +NAME' '-O NAME,MAJ:MIN,BACK-MAJ:MIN,BACK-INO'
  '-O OFFSET,SIZELIMIT,AUTOCLEAR,PARTSCAN,RO,DIO,LOG-SEC' '-O NAME -J' '-O NAME,BOGUS' '-O ""'
  '--output-all -O NAME' '-l -O NAME' '-a -l' '-J -l' '--list --json --noheadings')
CHANGES=('-d /dev/loop0' '-d /dev/loop2 /dev/loop9 /dev/loop5' '-D' '-c /dev/loop2' '-c /dev/loop9'
              '--direct-io /dev/loop2' '--direct-io=off /dev/loop2' '--direct-io=maybe /dev/loop2'
              '-b 4096 /dev/loop2' '-b 4k /dev/loop2' '-b x /dev/loop2'
              '-f "$F/a.img"' '-f --show "$F/a.img"' '-f nofile' '/dev/loop1 "$F/a.img"' '/dev/loop9 "$F/a.img"'
              '-r -o 512 --sizelimit 1024 /dev/loop1 "$F/a.img"' '-L -f "$F/a.img"' '-L /dev/loop1 "$F/a.img"'
              '-P /dev/loop1' '--show -a' '-o 10 -a' '-o 1x -a' '--sizelimit 1 /dev/loop2' '-f a b'
              '-J --raw' '--raw -J' '-a -d /dev/loop0' '-D -a' '-O NAME -D' '-l -c /dev/loop0' 'loop2 x y'
              '/dev/loop1' '-V' '--version' '-h' '--bogus' '-Z' '-o' '--offset' '--dir' '--direct-io=on')
for scen in real many empty allused nosys; do
  for args in "${LISTS[@]}"; do
    eval "set -- $args"
    case_ "$scen" plain "$@"
  done
  for dev in /dev/loop0 /dev/loop1 /dev/loop2 /dev/loop5 /dev/loop7 /dev/loop9 loop2 /nonexistent; do
    case_ "$scen" plain "$dev"
    case_ "$scen" plain -l "$dev"
    case_ "$scen" plain -J "$dev"
  done
  case_ "$scen" plain -f
  case_ "$scen" plain --find --show
  for f in "$F/a.img" "$F/dir with spaces/b.img" "$F/real/c.img" "$F/link/c.img" "$F/../files/a.img" \
           files/a.img nofile; do
    case_ "$scen" plain -j "$f"
    case_ "$scen" plain -l -j "$f"
    case_ "$scen" plain -j "$f" -o 512
    case_ "$scen" plain -j "$f" -o 0 -l
  done
  for args in "${CHANGES[@]}"; do
    eval "set -- $args"
    case_ "$scen" plain "$@"
  done
done
for COLS in 30 60 80 120 250; do
  for args in '' '-a' '--output-all' '-O BACK-FILE,NAME' '-n'; do
    eval "set -- $args"
    case_ many pty "$@"
  done
done
unset COLS

printf '\nlosetup-diff: %d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" = 0 ] && [ "$broken" = 0 ]
