#!/usr/bin/env bash
# Differential test: our `lsblk` against util-linux 2.39.3's.
#
# Three kinds of case, each run in C.UTF-8 and in C (whose tree and group
# charts are drawn in different symbols), comparing stdout, stderr and the
# exit status:
#
#   * util-linux's own snapshots of two machines (tests/ts/lsblk/dumps: an
#     LVM system and an NVMe one), read with --sysroot: each snapshot's
#     .cols files as upstream's test suite runs them, and then the same
#     machine through every output format, sort, dedup, tree column,
#     width and filter -- the snapshot's udev properties included, which
#     it keeps in the files standing in for the device nodes;
#   * this machine, as an ordinary user sees it: udev's database, the
#     mount table and swaps, the same options, and devices named on the
#     command line (a partition, a disk, something that is not a block
#     device, something that does not exist);
#   * options and their refusals.
#
# Some columns are read from the running system even with --sysroot (the
# device nodes' owners, FSAVAIL and friends, a loop device's backing file);
# both sides read the same system, so they agree or differ together -- unless
# it changes between the two runs, for which a case that differs is run again.
set -u

DIFF_PROG='lsblk'
DIFF_PKG='lsblk'
DIFF_NEED='timeout xz tar'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

dumpsrc=$UL_SRC/tests/ts/lsblk/dumps
if ! [ -d "$dumpsrc" ]; then
  echo "lsblk-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi

pass=0; fail=0
dumps=$DIFF_TMP/dumps
mkdir -p "$dumps"
for t in "$dumpsrc"/*.tar.xz; do
  tar -C "$dumps" --xz -xf "$t"
done

# run_both LOC ARGV...: each side once, its out, err and status kept.
run_both() {
  local loc=$1; shift
  diff_run timeout -k 2 30 env LC_ALL="$loc" PATH="$bindir/ours" lsblk "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  diff_run timeout -k 2 30 env LC_ALL="$loc" PATH="$bindir/gnu" lsblk "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]
}

compare() {
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc loc
  for loc in C.UTF-8 C; do
    # The filesystems' use is read live, and the disk may fill between the
    # two runs (FSUSED 280.7G against 280.8G): a case that differs is run
    # once more, and said to have been.
    if run_both "$loc" "$@" \
       || { run_both "$loc" "$@" && printf 'AGAIN [%s] lsblk %s (agreed when run again)\n' "$loc" "$*"; }; then
      pass=$((pass + 1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   [%s] lsblk %s\n' "$loc" "$*"
    else
      fail=$((fail + 1))
      printf 'DIFF [%s] lsblk %s\n  ours rc=%s, gnu rc=%s\n' "$loc" "$*" "$o_rc" "$g_rc"
      diff -u "$g_out" "$o_out" | sed -n '3,16p' | sed 's/^/  out /'
      diff -u "$g_err" "$o_err" | sed -n '3,8p' | sed 's/^/  err /'
    fi
  done
  return 0
}

# --- options and refusals -------------------------------------------------------------
compare -h
compare --help
compare -V
compare --version
compare --bogus
compare -Q
compare -x
compare -x BOGUS
compare -E BOGUS
compare -o BOGUS
compare -o NAME,BOGUS,SIZE
compare -o +
compare -o ,
compare -o NAME,
compare -w abc
compare -w 99999999999
compare -w -1
compare -e x
compare -e 8,,9
compare -e 99999999999999999999
compare -I 8x
compare -J -P
compare -J -r
compare -O -o NAME
compare -O -f
compare -P -T
compare -l -r
compare -D -O
compare -I 8 -e 9
compare --sysroot
compare -T BOGUS -d
compare /nonexistent
compare /dev/null
compare -d /dev/null /nonexistent

# --- util-linux's snapshots -------------------------------------------------------------
for dump in "$dumps"/*/; do
  dump=${dump%/}
  for cols in "$dump"/*.cols; do
    compare --sysroot "$dump" --output "$(cat "$cols")"
  done
  for args in '' '-a' '-l' '-r' '-P' '-J' '-i' '-n' '-b' '-p' '-d' '-s' '-M' '-A' \
              '-l -a' '-r -a' '-P -a' '-J -a' '-J -l' '-J -b' '-J -O' '-J -f' '-J -T' \
              '-f' '-m' '-t' '-D' '-z' '-S' '-N' '-v' '-O' '-O -l' '-O -J' \
              '-x NAME' '-x SIZE' '-x MAJ:MIN' '-x TYPE' '-x NAME -T' '-l -x SIZE' '-r -x NAME' \
              '-x SIZE -o NAME' '-E NAME' '-E TYPE' '-E UUID -o NAME,UUID' '-T PKNAME' '-T=SIZE' \
              '-T -l' '-s -l' '-s -r' '-M -i' '-M -J' '-y -P' '-y -J' '-e 259' '-e 8,259' '-I 259' \
              '-I 8,253' '-w 20' '-w 40' '-w 60' '-w 80' '-w 200' '-w 40 -i' '-w 40 -O' \
              '-o NAME,FSTYPE,LABEL,UUID,PARTTYPENAME,PARTTYPE,PTTYPE' \
              '-o NAME,MODEL,SERIAL,REV,VENDOR,WWN,HCTL,TRAN,SUBSYSTEMS,ID-LINK,ID' \
              '-o NAME,KNAME,PKNAME,PATH,MAJ:MIN,DISK-SEQ,PARTN,PARTFLAGS,PARTLABEL,PARTUUID' \
              '-o NAME,RO,RM,HOTPLUG,ROTA,RAND,SCHED,RQ-SIZE,MQ,STATE,DAX,ZONED' \
              '-o NAME,ALIGNMENT,MIN-IO,OPT-IO,PHY-SEC,LOG-SEC,DISC-ALN,DISC-GRAN,DISC-MAX,DISC-ZERO,WSAME' \
              '-o NAME,START,SIZE,FSSIZE,FSAVAIL,FSUSED,FSUSE%,FSROOTS,MOUNTPOINT,MOUNTPOINTS' \
              '-o +UUID' '-b -o NAME,SIZE,DISC-MAX,ZONE-SZ,WSAME'; do
    # shellcheck disable=SC2086  # word-splitting is the point
    compare --sysroot "$dump" $args
  done
  # Named devices are looked up in the running system's sysfs even with a
  # snapshot, as upstream looks them up.
  compare --sysroot "$dump" /dev/null
done

# --- this machine ----------------------------------------------------------------------
# Here SUBSYSTEMS cannot be compared byte for byte. util-linux 2.39.3's
# sysfs_blkdev_get_devchain (lib/sysfs.c:409) copies the chain into its
# caller's buffer without the terminating NUL, so the path it then looks up
# runs on into whatever that stack buffer last held. Where that is not a
# NUL, the device's own `subsystem` link is missed and the column loses its
# leading `block` -- for some devices and not others, as what ran before
# decides: loop0 and the ram disks here, and not loop1. (The snapshots read
# with --sysroot have always found a NUL there, and keep the column.) So the
# cases below leave SUBSYSTEMS out -- `-O` becomes every other column -- and
# `compare_subsystems` checks it device by device.
all_but_subsystems=$("$bindir/gnu/lsblk" --help \
  | awk '/Available output columns/{p=1; next} p && /^ +[A-Z]/{print $1}' | grep -vx SUBSYSTEMS | paste -sd, -)
case $all_but_subsystems in
  *,MODEL,*) ;;
  *) echo "lsblk-diff: no column list in lsblk --help: '$all_but_subsystems'"; exit 1 ;;
esac

# compare_subsystems: SUBSYSTEMS of every device, ours against upstream's,
# where upstream's may lack the leading `block` its own link gives.
compare_subsystems() {
  local o g on ov gn gv bad=0 lost=0
  o=$(env LC_ALL=C PATH="$bindir/ours" lsblk -r -n -a -o NAME,SUBSYSTEMS 2>&1)
  g=$(env LC_ALL=C PATH="$bindir/gnu" lsblk -r -n -a -o NAME,SUBSYSTEMS 2>&1)
  if [ "$(printf '%s\n' "$o" | wc -l)" != "$(printf '%s\n' "$g" | wc -l)" ]; then
    bad=1
  else
    while read -r on ov && read -r gn gv <&3; do
      if [ "$on" != "$gn" ]; then
        bad=1
      elif [ "$ov" = "$gv" ]; then
        :
      elif [ "$ov" = "block${gv:+:$gv}" ]; then
        lost=$((lost + 1))
      else
        bad=1
      fi
    done < <(printf '%s\n' "$o") 3< <(printf '%s\n' "$g")
  fi
  if [ "$bad" = 0 ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   lsblk -o NAME,SUBSYSTEMS (upstream lost its leading block for %d)\n' "$lost"
  else
    fail=$((fail + 1))
    printf 'DIFF lsblk -r -n -a -o NAME,SUBSYSTEMS\n'
    diff -u <(printf '%s\n' "$g") <(printf '%s\n' "$o") | sed -n '3,30p' | sed 's/^/  out /'
  fi
}
compare_subsystems

for args in '' '-a' '-l' '-r' '-P' '-J' '-i' '-n' '-b' '-p' '-d' '-s' '-M' '-A' \
            '-f' '-m' '-t' '-D' '-z' '-S' '-N' '-v' \
            "-o $all_but_subsystems" "-J -o $all_but_subsystems" "-l -o $all_but_subsystems" \
            '-x NAME' '-x SIZE' '-l -x SIZE' '-E TYPE' '-T PKNAME' '-w 40' '-w 80' '-w 200' \
            '-o NAME,FSTYPE,FSVER,LABEL,UUID,FSAVAIL,FSUSE%,MOUNTPOINT,MOUNTPOINTS,FSROOTS' \
            '-o NAME,MODEL,SERIAL,WWN,HCTL,TRAN,ID-LINK,ID,OWNER,GROUP,MODE' \
            '-e 7' '-I 8' '-a -I 7'; do
  # shellcheck disable=SC2086
  compare $args
done
for d in /dev/sda /dev/sdb /dev/sdc /dev/sdd /dev/loop0; do
  [ -b "$d" ] || continue
  compare "$d"
  compare -s "$d"
  compare -J -o "$all_but_subsystems" "$d"
done
compare /dev/sda /dev/null
compare /dev/sda /nonexistent

echo "lsblk-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
