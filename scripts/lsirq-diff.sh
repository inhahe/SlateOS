#!/usr/bin/env bash
# Differential test: our `lsirq` against util-linux 2.39.3's.
#
# lsirq reads /proc/interrupts and /proc/softirqs by those names and has no
# option to read anything else, and the real ones change between the two
# sides' runs. So each side runs in a user and mount namespace of its own
# (`unshare -rm`, which needs no root) with files of the harness's own bound
# over the two: the same bytes for both.
#
# What is compared: stdout, stderr and the exit status of each case --
#
#   * the parse: counters as fixed eleven-byte fields, however wide they
#     really are; one that does not parse leaving the rest unread; a minus
#     sign wrapping; names with runs of blanks, tabs and trailing blanks;
#     lines with no colon, no counters or no name; a header naming more or
#     fewer CPUs than the lines hold; NUL, CR and bytes that are not UTF-8;
#   * the order: by total (glibc's qsort, a merge sort, keeps ties in file
#     order), by IRQ (strverscmp) and by name;
#   * -o in each form (`+`, unknown names, a full list), -J, -P, -n, -S, the
#     options' refusals, and on a terminal of many widths, where NAME is cut;
#   * an empty file, whose message reports an errno nothing set -- which
#     depends on the locale -- and one that cannot be opened;
#   * closed and full descriptors: lsirq registers no close_stdout, so a
#     failed write changes nothing.
#
# The reference is unpacked from Ubuntu's util-linux-extra package by
# util-linux-extra.sh, since the WSL image does not install it.
#
# At the narrowest terminals upstream never finishes: libsmartcols' width
# arithmetic wraps a size_t, and it pads a column in the quintillions until
# it is killed. The port stops at zero (the smartcols crate's docs, "Where it
# is not upstream's"). Such a case is counted apart, as `hung`: it is one only
# if upstream was the side that had to be killed and ours finished.
set -u

DIFF_PROG='lsirq'
DIFF_PKG='lsirq'
DIFF_NEED='timeout script stty unshare mount python3'
# shellcheck source=util-linux-extra.sh
. "$(dirname "$0")/util-linux-extra.sh"
DIFF_REF="$UL_EXTRA_ROOT/usr/bin/lsirq"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; hung=0

in=$DIFF_TMP/in
mkdir -p "$in"

# --- inputs ---------------------------------------------------------------------
# A /proc/interrupts line: the IRQ right-aligned in 3 (as the kernel does for
# numbers), then " %10u" per CPU, then the name.
irqline() {
  local irq=$1; shift
  printf '%3s:' "$irq"
  while [ $# -gt 1 ]; do printf ' %10s' "$1"; shift; done
  printf '  %s\n' "$1"
}
{
  printf '           CPU0       CPU1       CPU2       CPU3       \n'
  irqline 0 44 0 0 0 'IR-IO-APIC    2-edge      timer'
  irqline 1 0 9 0 0 'IR-IO-APIC    1-edge      i8042'
  irqline 8 0 0 1 0 'IR-IO-APIC    8-edge      rtc0'
  irqline 9 0 1234 0 0 'IR-IO-APIC    9-fasteoi   acpi'
  irqline 12 0 0 0 150 'IR-IO-APIC   12-edge      i8042'
  irqline 16 5 5 5 5 'IR-IO-APIC   16-fasteoi   ehci_hcd:usb1, i801_smbus'
  irqline 120 1 2 3 4 'DMAR-MSI    0-edge      dmar0'
  irqline 121 99999 0 0 0 'IR-PCI-MSI 327680-edge      xhci_hcd'
  irqline 122 0 0 777 0 'IR-PCI-MSI 32768-edge      i915'
  irqline NMI 3 4 5 6 'Non-maskable interrupts'
  irqline LOC 123456 234567 345678 456789 'Local timer interrupts'
  irqline SPU 0 0 0 0 'Spurious interrupts'
  irqline PMI 3 4 5 6 'Performance monitoring interrupts'
  irqline RES 1000 2000 3000 4000 'Rescheduling interrupts'
  irqline CAL 50 50 50 50 'Function call interrupts'
  irqline TLB 20 30 40 50 'TLB shootdowns'
  printf '%3s: %10s\n' ERR 0
  printf '%3s: %10s\n' MIS 0
} > "$in/x86"
{
  printf '                    CPU0       CPU1\n'
  for n in HI TIMER NET_TX NET_RX BLOCK IRQ_POLL TASKLET SCHED HRTIMER RCU OTHER; do
    printf '%12s: %10s %10s\n' "$n" "$((${#n} * 7))" "$((${#n} * 3))"
  done
} > "$in/softirqs"
{
  printf '           CPU0       CPU1\n'
  printf '  0: 5 7         8 short fields\n'
  printf '  1:          1       abc          2 unparsed field\n'
  printf '  2:         -5          1 wraps\n'
  printf '  3: 12345678901          1 too long\n'
  printf '  4:          9          9\t\tname  with   blanks  \t\n'
  printf '  5:          9          9\n'
  printf '  6:\n'
  printf 'no colon here\n'
  printf '\n'
  printf ' \t LOC :          1          2 irq with blanks\n'
  printf '  7:          3          3 name\r\n'
  printf '  8:          3          3 na\0me\n'
  printf '\0 9:          3          3 hidden\n'
  printf '  10:          1          1 caf\303\251 \351t\351\n'
  printf '  11:         +4          1 plus\n'
  printf '  12:          x          1 not a number\n'
  printf '  13:          1          2          3          4 more fields than cpus\n'
} > "$in/odd"
printf 'CPU0 CPU1 CPU2\n  0:          1\n  1:          2          3          4          5 x\n' > "$in/fewer"
printf 'no cpus in this header\n  0:          1 zero cpus\n  1:          2 two\n' > "$in/nocpu"
printf 'CPUCPUCPU\n  0:          1          2          3 three\n' > "$in/cpucpu"
printf '           CPU0\n' > "$in/header-only"
: > "$in/empty"
printf '           CPU0\n' > "$in/ties"
for i in 5 1 5 3 1 5 2; do printf '%3s: %10s  n%s\n' "t$RANDOM" "$i" "$i"; done >> "$in/ties"
{
  printf '           CPU0\n'
  for i in 10 9 100 1 NMI 2 LOC 010 9a 9b; do printf '%3s: %10s  name-%s\n' "$i" 7 "$i"; done
} > "$in/versions"
python3 -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1])' "$in/socket"

# $1 = side, $2 = interrupts file, $3 = softirqs file, rest = argv: in a
# namespace where the two files are /proc's.
run_side() {
  local side=$1 irq=$2 soft=$3; shift 3
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" PATH="$bindir/$side:$PATH" timeout -k 2 20 \
    unshare -rm sh -c 'mount --bind "$1" /proc/interrupts && mount --bind "$2" /proc/softirqs && shift 2 && exec lsirq "$@"' \
    sh "$irq" "$soft" "$@"
}
# $1 = side, $2 = columns, rest = argv: the x86 files, on a pty that wide.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="mount --bind $(printf %q "$in/x86") /proc/interrupts && stty cols $cols rows 60 && lsirq"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 1 10 \
    unshare -rm script -qec "$cmd" /dev/null </dev/null
}

judge() {
  local o_rc=$1 g_rc=$2
  if { [ "$g_rc" = 124 ] || [ "$g_rc" = 137 ]; } && [ "$o_rc" != 124 ] && [ "$o_rc" != 137 ]; then
    # Upstream never finished and ours did: a terminal so narrow that
    # libsmartcols' size_t arithmetic wraps (the smartcols crate's docs,
    # "Where it is not upstream's").
    AGREED=hung
  elif [ "$o_rc" = 127 ] && [ ! -s "$DIFF_TMP/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(head -c 1500 "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(head -c 1500 "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
}

report() {
  if [ "$AGREED" = hung ]; then
    hung=$((hung + 1))
    if [ -n "${VERBOSE:-}" ]; then
      printf 'HUNG %s (upstream never finished; ours did)\n' "$1"
    fi
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# on FILE ARGV...: FILE as /proc/interrupts (and the softirqs file as
# /proc/softirqs).
on() {
  local file=$1 o_rc g_rc; shift
  run_side ours "$in/$file" "$in/softirqs" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$in/$file" "$in/softirqs" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[${CASE_LOCALE:-C.UTF-8} $file] lsirq $(printf '%q ' "$@")"
}
# soft FILE ARGV...: FILE as /proc/softirqs.
soft() {
  local file=$1 o_rc g_rc; shift
  run_side ours "$in/x86" "$in/$file" -S "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$in/x86" "$in/$file" -S "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[softirqs $file] lsirq -S $(printf '%q ' "$@")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] lsirq $(printf '%q ' "$@")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs lsirq, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 20 \
    unshare -rm bash -c "mount --bind \"\$1\" /proc/interrupts && shift && exec lsirq \"\$@\" $how" \
    sh "$in/x86" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 20 \
    unshare -rm bash -c "mount --bind \"\$1\" /proc/interrupts && shift && exec lsirq \"\$@\" $how" \
    sh "$in/x86" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "lsirq $(printf '%q ' "$@")$how"
}

# --- options and their refusals -------------------------------------------------
for args in '-h' '--help' '-V' '--version' '--vers' '--bogus' '-z' '--s' '--so' \
            '--sor irq' '-s' '-s bogus' '-s ""' '-o' '-J -P' '-P -J' '-J -J' \
            'operand ignored' '-- -n' '-h -z' '-z -h' '-s bogus -h' '-h -s bogus'; do
  # Each entry is an argument list, quoting and all.
  eval "on x86 $args"
done

# --- the table --------------------------------------------------------------------
for f in x86 odd fewer nocpu cpucpu header-only ties versions; do
  on "$f"
  on "$f" -n
  on "$f" -J
  on "$f" -P
  on "$f" -s irq
  on "$f" -s NAME
  on "$f" -s delta
  on "$f" -s total
  on "$f" -o +DELTA
done
for list in IRQ irq,name NAME,TOTAL,IRQ +NAME +NAME,NAME,NAME,NAME,NAME \
            +NAME,NAME,NAME,NAME,NAME,NAME IRQ,IRQ,IRQ,IRQ,IRQ,IRQ,IRQ,IRQ \
            IRQ,IRQ,IRQ,IRQ,IRQ,IRQ,IRQ,IRQ,IRQ delta,DELTA nosuch IRQ,nosuch,NAME \
            'IRQ,' ',IRQ' 'IRQ,,NAME' ',' '' '+' '+,' 'I' 'irQ'; do
  on x86 -o "$list"
  on x86 -o "$list" -J
done
on x86 -o NAME -o IRQ
on x86 -o bogus -o IRQ
on x86 -n -o TOTAL,IRQ -s irq
CASE_LOCALE=C on odd
CASE_LOCALE=C on odd -J
CASE_LOCALE=C on odd -s name
CASE_LOCALE=POSIX on x86
soft softirqs
soft softirqs -s name
soft softirqs -J
soft x86
soft odd

# --- files that will not do ---------------------------------------------------------
for loc in C.UTF-8 C POSIX; do
  CASE_LOCALE=$loc on empty
  CASE_LOCALE=$loc on socket
done
soft empty
soft socket
on header-only -J

# --- terminals -------------------------------------------------------------------
for cols in 1 10 20 25 30 35 40 50 60 80 132; do
  pty_case "$cols"
  pty_case "$cols" -o NAME,IRQ
  pty_case "$cols" -n -s name
done

# --- closed and full descriptors -------------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how"
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
  redir_case "$how" -o nosuch
  redir_case "$how" -J
done

printf '%d passed, %d differed, %d broken, %d where only upstream hung\n' \
  "$pass" "$fail" "$broken" "$hung"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
