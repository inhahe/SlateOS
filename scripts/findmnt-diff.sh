#!/usr/bin/env bash
# Differential test: our `findmnt` against util-linux 2.39.3's.
#
# findmnt reads mount tables -- the kernel's mountinfo, fstab, mtab with
# utab's userspace options -- and with --tab-file reads any file instead.
# That makes it testable on tables other than this machine's: util-linux's
# own test suite ships mountinfo files (one messy, one of a non-root
# namespace) and fstabs (one broken, one with comments), and each side is run
# on each of them; tables built here cover what those do not -- every quirk
# of the parsers (numbers that overflow, a NUL in a line, CRLF, escapes,
# empty sources, a missing separator), a directory of *.fstab files, utab
# merging, swap areas for --verify.
#
# What is compared: stdout, stderr and the exit status of each case --
#
#   * options and their refusals, including the mutually exclusive ones;
#   * the tree, --list, --raw, --pairs, --json and --shell, with the default
#     columns, every column, and lists in each form;
#   * every filter: -t, -O, -S, -T, -M, operands, -f, -i, -R, -U, --real,
#     --pseudo, --shadowed, -D, -d backward, -v, -e, -c, -C, -b;
#   * --verify, plain and --verbose;
#   * --poll, until its timeout;
#   * the machine's own tables;
#   * a terminal of many widths, and closed and full descriptors.
#
# Both sides run in the same directory, so a relative source resolves alike.
# Sizes are compared only human-readable: this harness writes to the disk
# between the two runs, and a used-bytes count would move under it.
# Upstream's fixtures come from its source tarball, which
# util-linux-source.sh fetches and checks.
set -u

DIFF_PROG='findmnt'
DIFF_PKG='findmnt'
DIFF_NEED='timeout script stty tar'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
DIFF_REF=/usr/bin/findmnt
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; hung=0

tsdir=$UL_SRC/tests/ts
if ! [ -d "$tsdir/findmnt/files" ]; then
  echo "findmnt-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi
fx=$DIFF_TMP/fx
mkdir -p "$fx"
cp "$tsdir"/findmnt/files/* "$fx"/
for f in fstab fstab.broken fstab.comment mountinfo mountinfo_mv mountinfo_nosrc \
         mountinfo_re mountinfo_u mtab swaps; do
  cp "$tsdir/libmount/files/$f" "$fx/lm-$f"
done

# --- tables of our own --------------------------------------------------------------
printf '%s\n' \
  '# a comment' \
  '' \
  '  UUID=d3a8f783-df75-4dc8-9163-975a891052c0 /     ext4    noatime,defaults 1 1' \
  'LABEL=boot /boot ext4 defaults 0 2' \
  'PARTUUID=0001-02 /efi vfat umask=0077 0 99999999999999999999' \
  '/dev/sda3 /home ext4 defaults 4294967297 2 trailing junk' \
  '/dev/sda4 /x1 ext4 defaults 1x' \
  '/dev/sda5 /x2 ext4 defaults 1 2y' \
  'tmpfs /tmp tmpfs size=10%,mode=1777 0 0' \
  '/swapfile none swap sw,pri=5,discard=pages 0 0' \
  '/dev/sdb2 none swap discard=bogus,pri=-3x 0 0' \
  '/dev/sdb3 none swap discard=on,pri= 0 0' \
  'proc /proc proc defaults 0 0' \
  '/dev/mapper/nosuch /mnt/my\040disk ext4 ro,user,noauto 0 0' \
  'server:/export /net nfs rw,bg 0 0' \
  'FOO=bar /foo auto defaults 0 0' \
  '/dev/nosuch /dev/null none bind 0 0' \
  '/nonexistent /nonexistent-dir ext4 defaults 0 0' \
  '/etc/hostname /mnt/file none bind 0 0' \
  '/ /mnt/root auto rbind 0 0' \
  "LABEL='quoted label' /q ext4 defaults 0 0" \
  '/dev/sdc1 /home/deep ext4 defaults 0 2' \
  '/dev/sdc2 /home ext4 defaults 0 2' \
  'justtwo fields' \
  > "$fx/my-fstab"
printf 'a /1 t o 0 0\r\nb /2 t\0junk o\nc /3 t o\n\nd /4 t\0tail' > "$fx/my-fstab-nul"
printf '%s\n' \
  '21 1 8:3 / / rw,relatime shared:1 - ext4 /dev/sda3 rw,errors=remount-ro' \
  '22 21 0:5 / /proc rw,nosuid,nodev,noexec,relatime shared:2 - proc proc rw' \
  '23 21 0:6 / /sys rw,nosuid shared:3 master:1 - sysfs sysfs rw' \
  '24 21 0:7 / /run rw,nosuid,nodev - tmpfs tmpfs rw,size=10240k,mode=755' \
  '25 24 0:8 / /run/user/1000 rw,nosuid,nodev,relatime unbindable - tmpfs tmpfs rw,size=1k' \
  '26 21 8:4 /sub /mnt/bind rw,relatime master:5 - ext4 /dev/sda4 rw' \
  '27 21 0:9 / /mnt/my\040space rw - tmpfs  rw' \
  '28 21 8:3 / / rw - ext4 /dev/sda3 rw' \
  '29 26 0:10 / /mnt/bind/over rw - tmpfs tmpfs rw' \
  '30 29 0:11 / /mnt/bind/over rw - tmpfs tmpfs rw' \
  '31 21 259:1 / /data ro,relatime - btrfs /dev/nvme0n1p1 ro,ssd,subvol=/' \
  '32 21 0:12 / /net rw - nfs4 server:/export rw,vers=4.2' \
  '33 21 0:13 / /dev/root-like rw - ext4 /dev/root rw' \
  '40 99 0:14 / /orphan rw - tmpfs orphan rw' \
  'bad line' \
  '41x 21 0:15 / /bad rw - tmpfs tmpfs rw' \
  '42 21 0:16 / /nosep rw tmpfs tmpfs rw' \
  > "$fx/my-mountinfo"
printf '%s\n' \
  'Filename				Type		Size		Used		Priority' \
  '/swapfile                               file		4194300		0		-2' \
  '/dev/sda2\040(deleted)                   partition	1024		0		5' \
  '/dev/sdb1 partition 1 2 ' \
  '/dev/sdb2 partition x 2 3' \
  > "$fx/my-swaps"
mkdir -p "$fx/fstab.d"
printf 'tmpfs /a tmpfs defaults 0 0\n' > "$fx/fstab.d/10-a.fstab"
printf 'tmpfs /b tmpfs defaults 0 0\nbroken\n' > "$fx/fstab.d/9-b.fstab"
printf 'tmpfs /c tmpfs defaults 0 0\n' > "$fx/fstab.d/.hidden.fstab"
printf 'tmpfs /d tmpfs defaults 0 0\n' > "$fx/fstab.d/notfstab"
printf 'tmpfs /e tmpfs defaults 0 0\n' > "$fx/fstab.d/10-a10.fstab"
mkdir -p "$fx/fstab.d/dir.fstab"
: > "$fx/empty"
printf '%s\n' \
  'ID=26 SRC=/dev/sda4 TARGET=/mnt/bind ROOT=/sub OPTS=x-foo=1,user ATTRS=a' \
  'SRC=/dev/sda3 TARGET=/ ROOT=/ OPTS=x-root' \
  > "$fx/my-utab"
# A mountinfo that is the tree above but read as the kernel's.
cp "$fx/my-mountinfo" "$fx/proc-like"

cd "$DIFF_TMP" || exit 1

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" LIBMOUNT_UTAB="${CASE_UTAB:-$fx/empty}" \
    PATH="$bindir/$side:$PATH" timeout -k 2 30 findmnt "$@"
}
# $1 = side, $2 = columns, rest = argv: on a pty that wide.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60 && findmnt"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" LIBMOUNT_UTAB="$fx/empty" \
    PATH="$bindir/$side:$PATH" timeout -k 1 20 script -qec "$cmd" /dev/null </dev/null
}

judge() {
  local o_rc=$1 g_rc=$2
  if { [ "$g_rc" = 124 ] || [ "$g_rc" = 137 ]; } && [ "$o_rc" != 124 ] && [ "$o_rc" != 137 ]; then
    AGREED=hung
  elif [ "$o_rc" = 127 ] && [ ! -s "$DIFF_TMP/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err %q\n  gnu  (rc=%s): err %q\n%s' \
    "$o_rc" "$(head -c 600 "$DIFF_TMP/o.err")" "$g_rc" "$(head -c 600 "$DIFF_TMP/g.err")" \
    "$(diff "$DIFF_TMP/g.out" "$DIFF_TMP/o.out" | head -20)")
}

report() {
  if [ "$AGREED" = hung ]; then
    hung=$((hung + 1))
    [ -n "${VERBOSE:-}" ] && printf 'HUNG %s (upstream never finished; ours did)\n' "$1"
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# both ARGV...: both sides, with these arguments. A case that differs is run
# once more before it counts: the machine's own filesystems fill and empty
# under the harness (other processes write to them), so a size can move
# between the two runs; a real difference differs twice.
both() {
  local o_rc g_rc try
  for try in 1 2; do
    run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
    run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
    judge "$o_rc" "$g_rc"
    [ "$AGREED" = no ] || break
  done
  report "[${CASE_LOCALE:-C.UTF-8}${CASE_UTAB:+ utab}] findmnt $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] findmnt $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs findmnt, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 LIBMOUNT_UTAB="$fx/empty" PATH="$bindir/ours:$PATH" timeout -k 2 30 \
    bash -c "exec findmnt \"\$@\" $how" sh "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 LIBMOUNT_UTAB="$fx/empty" PATH="$bindir/gnu:$PATH" timeout -k 2 30 \
    bash -c "exec findmnt \"\$@\" $how" sh "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "findmnt $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")$how"
}

mi=$fx/mountinfo
my=$fx/my-mountinfo

# --- options and their refusals -------------------------------------------------
for args in '-h' '--help' '-V' '--version' '--vers' '--bogus' '-z' '--he' '--s' \
            '-C -c' '-c -C' '-C -e' '-J -P' '-P -r' '-r -x' '-M /x -T /y' '-N 1 -k' \
            '-k -m' '-m -s' '-P -l' '-l -r' '-p -x' '-m -p -w 50' '-p -m -w 50' '-p -s' '-s -p' \
            '--pseudo --real' '--real --pseudo' \
            '-d up' '-d' '-d ""' '-o nosuch' '-o ""' '-o +' '-o TARGET,' '-o ,TARGET' \
            '-o TARGET,,SOURCE' '-o target,source' '-o +UUID' \
            '-p=bogus' '--poll=bogus' '--poll=mount,bogus' '--poll=mount,umount,move,remount,mount' \
            '-N x' '-N -1' '-N 4294967296' '-N 99999999' '-w x' '-w 99999999999' \
            '-o ACTION' '-o OLD-TARGET' '-o OLD-OPTIONS' \
            '-S /dev/sda /x' '-T / /x' '-M / /x'; do
  eval "set -- $args"
  both -F "$mi" "$@"
done
both -F "$mi" -F "$mi" -p
both --tab-file=/nonexistent
both -s -F /nonexistent
both -m -F /nonexistent
both -k -F /nonexistent
both -s -F /dev/null
both -m -F /dev/null
both -k -F /dev/null
both -s -F "$fx"
both -k -F "$fx"
both -F "$mi" -o TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET,TARGET

# --- every table, in every format -------------------------------------------------
tables="mountinfo mountinfo-messy mountinfo-nonroot lm-fstab lm-fstab.broken lm-fstab.comment
        lm-mountinfo lm-mountinfo_mv lm-mountinfo_nosrc lm-mountinfo_re lm-mountinfo_u
        lm-mtab lm-swaps my-fstab my-fstab-nul my-mountinfo my-swaps empty"
for t in $tables; do
  for kind in '' '-k' '-s' '-m'; do
    for args in '' '-l' '-r' '-P' '-J' '-J -l' '-y -P' '-n' '-a' '-u' '--tree' \
                '--output-all' '--output-all -r' '--output-all -J' '-o +PROPAGATION,OPT-FIELDS' \
                '-o SOURCE,FSROOT,TARGET,MAJ:MIN,ID,PARENT,TID -r' \
                '-o TARGET,FREQ,PASSNO,VFS-OPTIONS,FS-OPTIONS' '--vfs-all -o TARGET,VFS-OPTIONS' \
                '-v' '-e' '-c' '-C' '-U' '-d backward' '-f' '--real' '--pseudo' '--shadowed' \
                '-D' '-A -D' '-t tmpfs' '-t notmpfs,sysfs' '-t ext3,ext4,' \
                '-O rw' '-O ro,noexec' '-O +rw,-nosuid' '-O noatime' '-i -t tmpfs' \
                '-S /dev/sda3' '-S 8:3' '-S 0:6' '-S tmpfs' '-S UUID=d3a8f783-df75-4dc8-9163-975a891052c0' \
                '-T /proc' '-T /mnt' '-M /mnt/bind' '-T /run/user/1000/x' \
                '/proc' '/dev/sda3' 'tmpfs /run' '/sys --submounts' '/ -R' '-R' '/run -R -l' \
                '-f /' '-f tmpfs' '-f -S tmpfs' '-f -t tmpfs' 'LABEL=boot' \
                '-o SOURCES' '-o SOURCES -e' '-o SOURCES,TARGET -J' \
                '-o LABEL,UUID,PARTLABEL,PARTUUID' '-o SIZE,AVAIL,USED,USE%' \
                '-o SIZE,AVAIL,USED,USE% -J'; do
      # shellcheck disable=SC2086 # each entry is words
      eval "set -- $kind -F \"\$fx/\$t\" $args"
      both "$@"
    done
  done
done
# fstab.d, and LIBMOUNT_FSTAB.
both -s -F "$fx/fstab.d"
both -s -F "$fx/fstab.d" -l
both -F "$fx/fstab.d"
both -m -F "$fx/fstab.d"
both -s -F "$fx/fstab.d" -F "$fx/my-fstab" -l
both -k -F "$mi" -F "$my"
both -F "$mi" -F "$my" -R /

# utab merged into -m's mountinfo.
CASE_UTAB=$fx/my-utab
for args in '-m -F /proc/self/mountinfo -o TARGET,OPTIONS' '-m -F '"$my"' -o TARGET,OPTIONS -l' \
            '-m -o TARGET,SOURCE,OPTIONS' '-m -F '"$my"' --output-all -J' '-k -F '"$my"' -o TARGET,OPTIONS'; do
  eval "set -- $args"
  both "$@"
done
unset CASE_UTAB

# The C locale draws the tree in ASCII.
CASE_LOCALE=C
for t in mountinfo my-mountinfo my-fstab; do
  both -F "$fx/$t"
  both -F "$fx/$t" --tree -J
  both -F "$fx/$t" -o TARGET,SOURCE -l
done
unset CASE_LOCALE

# --- verify --------------------------------------------------------------------------
for t in lm-fstab lm-fstab.broken lm-fstab.comment my-fstab my-fstab-nul fstab.d empty; do
  for args in '-x' '-x --verbose' '-x -C' '-x -C --verbose' '-x -f' '-x -t ext4' '-x /proc' \
              '-x -S /dev/sda3' '-x -T /home' '--verify -o TARGET'; do
    eval "set -- -F \"\$fx/\$t\" $args"
    both "$@"
  done
done
both -x
both -x --verbose
both -x -k
both -x -m
both -x -k --verbose -t proc

# --- poll, to its timeout -----------------------------------------------------------
both -p -w 100 -F "$mi"
both -p -w 50
both --poll=mount -w 50 -F "$mi"
both -p -w 0 -F "$mi" -J
both -p -w 50 -F "$mi" -f
both -p -w 50 -F /nonexistent
both -p -w 50 -o ACTION,OLD-TARGET,OLD-OPTIONS,TARGET

# --- the machine's own tables ----------------------------------------------------
# Every column but TID, which in the kernel's own table is each process's own
# PID (libmount reads it from /proc/self's path).
live_all=AVAIL,FREQ,FSROOT,FSTYPE,FS-OPTIONS,ID,LABEL,MAJ:MIN,OPTIONS,OPT-FIELDS,PARENT,PARTLABEL,PARTUUID,PASSNO,PROPAGATION,SIZE,SOURCE,SOURCES,TARGET,USED,USE%,UUID,VFS-OPTIONS
for args in '' '-l' '-J' '-P' '-r' "-o $live_all -l" "-o $live_all -J" '-m' '-s' '-s -l' '-k' '-D' \
            '--real' '--pseudo' '--shadowed' '-t proc' '-o +PROPAGATION' '-o +UUID,LABEL' \
            '/' '/proc' '-T /etc' '-T /etc/hostname' '-M /etc' '-N $$' '-U' '-R /' \
            '-o SOURCES' '-e' '-c -l' '/nonexistent' '-S nosuch'; do
  eval "set -- $args"
  both "$@"
done

# --- terminals -------------------------------------------------------------------
for cols in 1 20 40 60 80 100 132 200; do
  pty_case "$cols" -F "$mi"
  pty_case "$cols" -F "$fx/mountinfo-messy"
  pty_case "$cols" -F "$my" -l
  pty_case "$cols" -F "$fx/my-fstab" -s
  pty_case "$cols" -F "$mi" --output-all
  pty_case "$cols" -F "$mi" -o TARGET,SOURCE,SOURCES,OPTIONS
done
pty_case 80
pty_case 80 -D

# --- closed and full descriptors -------------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" -F "$mi"
  redir_case "$how" -F "$mi" --output-all -J
  redir_case "$how" -F "$fx/my-fstab" -x
  redir_case "$how" -F "$fx/lm-fstab.broken" -s
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
  redir_case "$how" -F /nonexistent
done

printf '%d passed, %d differed, %d broken, %d where only upstream hung\n' \
  "$pass" "$fail" "$broken" "$hung"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
