#!/usr/bin/env bash
# Differential test: our `lscpu` against util-linux 2.39.3's.
#
# lscpu reads /sys/devices/system/cpu, /sys/devices/system/node and /proc,
# and with --sysroot reads them under another directory. That makes it
# testable on machines other than this one: util-linux ships snapshots of
# nineteen real machines for its own tests -- x86 laptops and servers, ARM
# boards (one hybrid, with four kinds of core), POWER, s390 in LPARs and
# three kinds of virtual machine, SPARC, RISC-V, LoongArch -- and each side
# is run on each of them. (The nineteenth is Ubuntu's: see
# util-linux-source.sh.) Trees built here
# cover what the snapshots do not: every quirk of the parsers
# (`/proc/cpuinfo` lines past 4096 bytes, names past 31, `type` files without
# a newline), every hypervisor mark, and the errors.
#
# What is compared: stdout, stderr and the exit status of each case --
#
#   * the summary, flat and --hierarchic, and in --json; -e, -p and -C with
#     their default columns, every column, and lists in each form;
#   * -a/-b/-c, -x, -y, -B, --output-all, and the options' refusals;
#   * the machine itself, without --sysroot;
#   * a terminal of many widths, where the summary is hierarchic by default
#     and its long lines wrap;
#   * closed and full descriptors.
#
# The "Architecture" line is uname's on both sides even for a snapshot of
# another architecture's machine, so both print this machine's.
#
# Upstream's snapshots come from its source tarball, which
# util-linux-source.sh fetches and checks.
set -u

DIFF_PROG='lscpu'
DIFF_PKG='lscpu'
DIFF_NEED='timeout script stty tar'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
DIFF_REF=/usr/bin/lscpu
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; hung=0

snapdir=$UL_SRC/tests/ts/lscpu/dumps
if ! [ -d "$snapdir" ]; then
  echo "lscpu-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi
dumps=$DIFF_TMP/dumps
mkdir -p "$dumps"
for f in "$snapdir"/*.tar.gz; do
  tar -C "$dumps" -xzf "$f"
done
snapshots=$(cd "$dumps" && ls)

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" PATH="$bindir/$side:$PATH" timeout -k 2 30 lscpu "$@"
}
# $1 = side, $2 = columns, rest = argv: on a pty that wide.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60 && lscpu"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  diff_run env LC_ALL="${CASE_LOCALE:-C.UTF-8}" PATH="$bindir/$side:$PATH" timeout -k 1 20 \
    script -qec "$cmd" /dev/null </dev/null
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
    "$o_rc" "$(cat "$DIFF_TMP/o.err")" "$g_rc" "$(cat "$DIFF_TMP/g.err")" \
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

# both ARGV...: both sides, with these arguments.
both() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[${CASE_LOCALE:-C.UTF-8}] lscpu $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] lscpu $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs lscpu, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 30 \
    bash -c "exec lscpu \"\$@\" $how" sh "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 30 \
    bash -c "exec lscpu \"\$@\" $how" sh "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "lscpu $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")$how"
}

# --- options and their refusals -------------------------------------------------
x86=$dumps/x86_64-dell_e4310
for args in '-h' '--help' '-V' '--version' '--vers' '--bogus' '-z' '--he' \
            '-e -p' '-p -e' '-C -e' '-e -C' '-a -b' '-b -c' '-c -a' '--all --online' \
            '-a' '-b' '-c' '--all' '-a -J' \
            '--hierarchic=bad' '--hierarchic=' '--hierarchic=auto' '--hierarchic=never' \
            '--hierarchic=always' '--hierarchic always' \
            '-s' '--sysroot' 'operand' '-- operand' '-e operand' \
            '-h -z' '-z -h' '-V -h' '-e -p -h' '-h -e -p'; do
  # Each entry is an argument list, quoting and all.
  eval "set -- $args"
  both -s "$x86" "$@"
done
both -e= -s "$x86"
both --extended= -s "$x86"
both -e -s "$x86" --output-all -e+CPU
both -p= -s "$x86"
both -C= -s "$x86"
both --sysroot=/nonexistent
both -s /nonexistent -e
both -s ''

# --- each machine ------------------------------------------------------------------
for d in $snapshots; do
  root=$dumps/$d
  for args in '' '-p' '-p -y' '-e' '-e -a' '-e -b' '-e -c' '-p -a' '-p -c' '-p -b' \
              '-C' '-C -B' '-J' '-e -J' '-C -J' '-x' '-B' '-y' '-e -y' '-p -x' \
              '--hierarchic' '--hierarchic -J' '--hierarchic -B' '--hierarchic=never' \
              '-e --output-all' '-p --output-all' '-C --output-all' '-e -J --output-all' \
              '-C -J --output-all' '-e -a -y --output-all' '-p -a -y --output-all'; do
    # shellcheck disable=SC2086 # each entry is words
    both -s "$root" $args
  done
done

# --- column lists --------------------------------------------------------------------
for d in x86_64-epyc_7451 s390-lpar-drawer ppc64-POWER7 armv7 arm-A510-A710-A715-X3; do
  root=$dumps/$d
  for list in CPU cpu,core CACHE CPU,CACHE,ONLINE BOGOMIPS,MHZ,SCALMHZ%,MAXMHZ,MINMHZ \
              MODELNAME POLARIZATION,ADDRESS,CONFIGURED BOOK,DRAWER,CLUSTER NODE,SOCKET,CORE \
              +MODELNAME +CACHE,CACHE nosuch CPU,nosuch,CORE 'CPU,' ',CPU' 'CPU,,CORE' ',' '+' \
              CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU \
              CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU,CPU; do
    both -s "$root" -e"$list"
    both -s "$root" --parse="$list"
    both -s "$root" -e="$list" -J
  done
  for list in NAME name,level ALL-SIZE,ONE-SIZE ALLOC-POLICY,WRITE-POLICY PHY-LINE,SETS,COHERENCY-SIZE \
              +TYPE +WAYS,WAYS nosuch 'NAME,' ''; do
    both -s "$root" -C"$list"
    both -s "$root" --caches="$list" -J
    both -s "$root" -C"$list" -B
  done
done

# --- trees of our own ----------------------------------------------------------------
# A four-CPU machine -- two cores of two threads, L1d, L1i and L2 per core and
# one L3, one NUMA node, CPU 3 offline -- which each case below then bends.
trees=$DIFF_TMP/trees
# put TREE REL CONTENT: a file of TREE, its directories made; CONTENT is
# printf's %b, so \n, \t and \0 mean what they say.
put() { mkdir -p "$(dirname "$trees/$1/$2")" && printf '%b' "$3" > "$trees/$1/$2"; }
cpuinfo_block() {
  printf 'processor\\t: %s\\nvendor_id\\t: GenuineIntel\\ncpu family\\t: 6\\nmodel\\t\\t: 142\\n' "$1"
  printf 'model name\\t: Test CPU @ 3.00GHz\\nstepping\\t: 10\\ncpu MHz\\t\\t: 1500.000\\n'
  printf 'flags\\t\\t: fpu vme lm vmx\\nbogomips\\t: 5999.99\\naddress sizes\\t: 39 bits physical, 48 bits virtual\\n\\n'
}
base() {
  local t=$1 c=sys/devices/system/cpu n core sib i spec info=''
  rm -rf "${trees:?}/$t"
  put "$t" $c/kernel_max '63\n'
  put "$t" $c/possible '0-3\n'
  put "$t" $c/present '0-3\n'
  put "$t" $c/online '0-2\n'
  for n in 0 1 2 3; do
    core=$((n / 2))
    if [ "$core" = 0 ]; then sib=3; else sib=c; fi
    put "$t" $c/cpu$n/topology/thread_siblings "$sib\n"
    put "$t" $c/cpu$n/topology/core_siblings 'f\n'
    put "$t" $c/cpu$n/topology/core_id "$core\n"
    put "$t" $c/cpu$n/topology/physical_package_id '0\n'
    put "$t" $c/cpu$n/cpufreq/cpuinfo_max_freq '3000000\n'
    put "$t" $c/cpu$n/cpufreq/cpuinfo_min_freq '800000\n'
    put "$t" $c/cpu$n/cpufreq/scaling_cur_freq '1500000\n'
    i=0
    for spec in "Data 1 32K $sib" "Instruction 1 32K $sib" "Unified 2 256K $sib" "Unified 3 6144K f"; do
      # shellcheck disable=SC2086 # four words
      set -- $spec
      put "$t" $c/cpu$n/cache/index$i/type "$1\n"
      put "$t" $c/cpu$n/cache/index$i/level "$2\n"
      put "$t" $c/cpu$n/cache/index$i/size "$3\n"
      put "$t" $c/cpu$n/cache/index$i/shared_cpu_map "$4\n"
      put "$t" $c/cpu$n/cache/index$i/ways_of_associativity '8\n'
      put "$t" $c/cpu$n/cache/index$i/coherency_line_size '64\n'
      i=$((i + 1))
    done
    info="$info$(cpuinfo_block $n)"
  done
  put "$t" $c/vulnerabilities/meltdown 'Mitigation: PTI\n'
  put "$t" $c/vulnerabilities/spectre_v1 'Mitigation: usercopy/swapgs barriers and __user pointer sanitization\n'
  put "$t" sys/devices/system/node/node0/cpumap 'f\n'
  put "$t" proc/cpuinfo "$info"
}
# tcase TREE ARGLIST...: each argument list on TREE.
tcase() {
  local t=$1 args; shift
  for args in "$@"; do
    # shellcheck disable=SC2086 # each entry is words
    both -s "$trees/$t" $args
  done
}
std=('' '-e' '-p' '-C' '-J' '-e -J' '-p -y' '--hierarchic' '-x' '-e -a' '-B' '-C -B')
C=sys/devices/system/cpu

base base; tcase base "${std[@]}"

# The CPU lists.
base t; rm "$trees/t/$C/possible"; tcase t '' '-e'
base t; put t $C/possible ''; tcase t ''
base t; put t $C/possible '\n'; tcase t ''
base t; put t $C/possible 'abc\n'; tcase t ''
base t; put t $C/possible '0-3x\n'; tcase t ''
base t; put t $C/possible '0x,1-3\n'; tcase t '' '-e'
base t; rm "$trees/t/proc/cpuinfo"; tcase t ''
base t; rm "$trees/t/$C/kernel_max"; tcase t '' '-x' '-e'
base t; put t $C/kernel_max '2147483647\n'; tcase t '' '-x'
base t; put t $C/kernel_max '-5\n'; tcase t '' '-x'
base t; put t $C/kernel_max ' +7junk\n'; tcase t '' '-x'
base t; put t $C/kernel_max '0\n'; put t $C/online '0,2,3\n'; tcase t '' '-x' '-e'
base t; put t $C/kernel_max '0\n'; put t $C/online '9-13:2\n'; tcase t '' '-x' '-p'
base t; put t $C/kernel_max '1\n'; tcase t '' '-e' '-p' '-e -a' '-C'
base t; rm "$trees/t/$C/online"; tcase t "${std[@]}"
base t; rm "$trees/t/$C/present"; tcase t '' '-p' '-e -b'
base t; put t $C/online '0-3\n'; tcase t '' '-e' '-p -c'
base t; put t $C/online '1,3\n'; put t $C/present '1-3\n'; tcase t '' '-e' '-p' '-p -a' '-x'

# /proc/cpuinfo.
long=$(printf 'f%.0s' $(seq 1 5000))
base t; put t proc/cpuinfo "processor\t: 0\nvendor_id\t: GenuineIntel\nflags\t\t: fpu $long lm\nmodel name\t: Long\n\n"; tcase t '' '-J'
base t; put t proc/cpuinfo "processor\t: 0\nmodel name                      : spaced\nthis key is far longer than thirty-one bytes: x\n\n"; tcase t ''
base t; put t proc/cpuinfo "processor\t: abc\nmodel name\t: A\n\nprocessor\t: 4294967296\nmodel name\t: B\n\nprocessor 2: x\nmodel name\t: C\n\n"; tcase t '' '-e'
for mhz in nan -nan inf 1e999 1e-999 -5 abc 0x1p10 '' 3.5e3; do
  base t; put t proc/cpuinfo "processor\t: 0\nmodel name\t: M\ncpu MHz\t\t: $mhz\nbogomips\t: $mhz\n\n"
  rm -rf "$trees/t/$C/cpu0/cpufreq"; tcase t '' '-e' '-p=CPU,MHZ,SCALMHZ%,BOGOMIPS'
done
# Two kinds of core, and cores differing only in their flags.
info=''
for n in 0 1 2 3; do
  if [ $((n % 2)) = 0 ]; then m='Big core'; f='fpu big'; else m='Little core'; f='fpu little'; fi
  info="${info}processor\t: $n\nvendor_id\t: GenuineIntel\nmodel name\t: $m\nstepping\t: $n\nflags\t\t: $f\nbogomips\t: 100.$n\n\n"
done
base t; put t proc/cpuinfo "$info"; tcase t '' '-e' '-J' '--hierarchic' '-e=CPU,MODELNAME,BOGOMIPS'
info=''
for n in 0 1 2 3; do
  info="${info}processor\t: $n\nvendor_id\t: GenuineIntel\nmodel name\t: Same\nflags\t\t: fpu f$n\n\n"
done
base t; put t proc/cpuinfo "$info"; tcase t '' '-e'
# A trailer after the last CPU, as PowerPC's has.
base t; put t proc/cpuinfo "processor\t: 0\ncpu\t\t: POWER7\nrevision\t: 2.1\n\nprocessor\t: 1\ncpu\t\t: POWER7\nrevision\t: 2.1\n\ntimebase\t: 512000000\nplatform\t: pSeries\nmodel\t\t: IBM,8233-E8B\nmachine\t\t: CHRP IBM,8233-E8B\n"; tcase t '' '-e'
# s390: type lines before the CPUs, cache lines, and per-CPU speeds.
base t; put t proc/cpuinfo "vendor_id       : IBM/S390\n# processors    : 4\nbogomips per cpu: 3033.00\nmax thread id   : 1\nfeatures\t: esan3 zarch stfle msa ldisp eimm dfp edat etf3eh highgprs te\ncache0          : level=1 type=Data scope=Private size=128K line_size=256 associativity=8\ncache1          : level=1 type=Instruction scope=Private size=96K line_size=256 associativity=6\ncache4          : level=3 type=Unified scope=Shared size=65536K line_size=256 associativity=16\ncache5          : level=4 type=Unified scope=Shared size=491520K line_size=256 associativity=30\ncache6          : level=2 type=Mystery scope=Shared size=7K line_size=0 associativity=0\nprocessor 0: version = FF,  identification = 0123C8,  machine = 2964\nprocessor 1: version = FF,  identification = 0123C8,  machine = 2964\n\ncpu number      : 0\ncpu MHz dynamic : 5000\ncpu MHz static  : 5000\n\ncpu number      : 1\ncpu MHz dynamic : 4000\ncpu MHz static  : 5000\n"; tcase t '' '-C' '-C -B' '-B' '-J' '-e' '--hierarchic'
# ARM, and implementers that are not.
for imp in 0x41 0x51 0x4e 0x61 41 0x99 0x; do
  base t; put t proc/cpuinfo "processor\t: 0\nBogoMIPS\t: 50.00\nFeatures\t: fp asimd\nCPU implementer\t: $imp\nCPU architecture: 8\nCPU variant\t: 0x1\nCPU part\t: 0xd08\nCPU revision\t: 3\n\nprocessor\t: 1\nBogoMIPS\t: 50.00\nFeatures\t: fp asimd\nCPU implementer\t: $imp\nCPU architecture: 8\nCPU variant\t: 0x0\nCPU part\t: 0x010\nCPU revision\t: 12345678\n\n"
  tcase t '' '-e=CPU,MODELNAME'
done
base t; put t proc/cpuinfo "processor\t: 0\nCPU Family\t: Loongson-64bit\nModel Name\t: Loongson-3A5000\nCPU Revision\t: 0x10\nISA\t\t: loongarch32 loongarch64\nAddress Sizes\t: 48 bits physical, 48 bits virtual\n\n"; tcase t ''
base t; put t proc/cpuinfo "processor\t: 0\nmodel name\t: T\xc3\xabst\xff \x01CPU \\\\ \"q\"\nflags\t\t: a\tb\n\n"
for loc in C C.UTF-8 POSIX; do CASE_LOCALE=$loc tcase t '' '-J' '-e=CPU,MODELNAME' '-e=MODELNAME -J'; done

# Caches.
base t; for n in 0 1 2 3; do put t $C/cpu$n/cache/index0/type 'Data'; done; tcase t '' '-C' '-e'
for size in 32K 1.5M 0x10K 010 garbage '' 0 1G 16E; do
  base t; for n in 0 1 2 3; do put t $C/cpu$n/cache/index2/size "$size\n"; done; tcase t '' '-C' '-C -B'
done
base t; for n in 0 1 2 3; do put t $C/cpu$n/cache/index3/id '7\n'; put t $C/cpu$n/cache/index0/id "$n\n"; done; tcase t '' '-C' '-e' '-p'
base t; for n in 0 1; do rm "$trees/t/$C/cpu$n/cache/index1/level"; rm "$trees/t/$C/cpu$n/cache/index2/type"; done; tcase t '' '-C' '-e'
base t; for n in 0 1 2 3; do put t $C/cpu$n/cache/index3/allocation_policy 'ReadWriteAllocate\n'; put t $C/cpu$n/cache/index3/write_policy 'WriteBack\n'; put t $C/cpu$n/cache/index3/physical_line_partition '1\n'; put t $C/cpu$n/cache/index3/number_of_sets '8192\n'; done; tcase t '-C --output-all' '-C --output-all -J'
base t; for n in 0 1 2 3; do rm -rf "$trees/t/$C/cpu$n/cache"; put t $C/cpu$n/l1_icache_size '32768\n'; put t $C/cpu$n/l1_icache_line_size '64\n'; put t $C/cpu$n/l1_dcache_size '16384\n'; put t $C/cpu$n/l1_dcache_line_size '32\n'; put t $C/cpu$n/l2_cache_size '1048576\n'; done; tcase t '' '-C' '-e' '-p'

# s390's per-CPU facts, frequencies, and the default type's extras.
base t
for n in 0 1 2 3; do
  put t $C/cpu$n/polarization "$(printf 'vertical:low\nvertical:medium\nvertical:high\nhorizontal' | sed -n "$((n + 1))p")\n"
  put t $C/cpu$n/address "$((n * 2))\n"
  put t $C/cpu$n/configure "$((n % 2))\n"
  put t $C/cpu$n/topology/book_siblings 'f\n'
  put t $C/cpu$n/topology/drawer_siblings 'f\n'
  put t $C/cpu$n/topology/book_id '3\n'
done
put t $C/dispatching '1\n'
tcase t '' '-e' '-p' '-p -y' '-J' '-e -J' '-e --output-all'
base t; put t $C/cpu1/polarization 'bogus\n'; put t $C/cpu2/polarization ''; mkdir -p "$trees/t/$C/cpu3/polarization"; tcase t '-e' '-p'
base t; put t $C/dispatching '0\n'; put t $C/cpufreq/boost '1\n'; tcase t ''
base t; put t $C/cpufreq/boost '0\n'; tcase t ''
base t; rm "$trees/t/$C/cpu1/cpufreq/cpuinfo_max_freq"; put t $C/cpu2/cpufreq/scaling_cur_freq '4500000\n'; tcase t '' '-e'
base t; for n in 0 1 2 3; do rm "$trees/t/$C/cpu$n/cpufreq/cpuinfo_min_freq" "$trees/t/$C/cpu$n/cpufreq/cpuinfo_max_freq"; done; tcase t '' '-e'
base t; for n in 0 1 2 3; do rm -rf "$trees/t/$C/cpu$n/topology"; done; tcase t '' '-e' '-p' '-p -y'

# Vulnerabilities.
base t
put t $C/vulnerabilities/spec_store_bypass 'Mitigation: Speculative Store Bypass disabled via prctl: and seccomp\n'
put t $C/vulnerabilities/mds 'Vulnerable: Clear CPU buffers attempted, no microcode\n'
put t $C/vulnerabilities/itlb_multihit 'Not affected\n'
put t $C/vulnerabilities/zeta 'Mitigationfoo\n'
put t $C/vulnerabilities/empty ''
put t $C/vulnerabilities/newline '\n'
put t $C/vulnerabilities/Upper 'X\n'
mkdir -p "$trees/t/$C/vulnerabilities/subdir"
tcase t '' '--hierarchic' '-J'
base t; rm -rf "$trees/t/$C/vulnerabilities"; mkdir -p "$trees/t/$C/vulnerabilities/sub"; tcase t '' '--hierarchic'

# NUMA nodes.
base t
put t sys/devices/system/node/node0/cpumap '3\n'
put t sys/devices/system/node/node10/cpumap '4\n'
put t sys/devices/system/node/node1/cpumap '8\n'
mkdir -p "$trees/t/sys/devices/system/node/nodeX"
put t sys/devices/system/node/node3 'not a directory\n'
tcase t '' '-e' '-p' '-x' '-J'
base t; mkdir -p "$trees/t/sys/devices/system/node/node99999999999999999999"; tcase t ''
base t; rm -rf "$trees/t/sys/devices/system/node"; tcase t '' '-e'

# Byte order.
for order in 'big\n' 'little\n' 'big' 'little' 'weird\n' ''; do
  base t; put t sys/kernel/cpu_byteorder "$order"; tcase t ''
done

# Whose virtual machine.
base t; put t proc/sysinfo 'Manufacturer:         IBM\nType:                 2964\nVM00 Control Program: z/VM    6.4.0\nCPU Topology SW:      0 0 1 1 2 2\n'; tcase t '' '-J' '--hierarchic'
base t; put t proc/sysinfo 'Type: 3906\nControl Program: KVM/Linux\n'; tcase t ''
base t; put t proc/sysinfo 'Type:3906'; tcase t ''
base t; put t proc/sysinfo 'CPU Topology SW:      0 0 1 1\n'; tcase t ''
base t; mkdir -p "$trees/t/proc/xen"; put t proc/xen/capabilities 'control_d\n'; tcase t ''
base t; mkdir -p "$trees/t/proc/xen"; tcase t ''
for card in '0078\t15ad0710\t0' '0010\t80eebeef\t0' '0010\t58530001\t0' '0010\t80861237\t0'; do
  base t; put t proc/bus/pci/devices "0000\t80861237\t0\n$card\n"; tcase t ''
done
for compat in 'qemu,pseries\0' 'ibm,powernv\0' 'foo\0qemu,pseries\0'; do
  base t; put t proc/device-tree/compatible "$compat"; tcase t ''
done
base t; put t proc/iSeries ''; tcase t ''
base t; put t proc/device-tree/ibm,partition-name 'lpar1\n'; put t 'proc/device-tree/hmc-managed?' ''; tcase t ''
base t; put t proc/device-tree/ibm,partition-name 'full\n'; put t 'proc/device-tree/hmc-managed?' ''; tcase t ''
base t; mkdir -p "$trees/t/proc/vz"; tcase t ''
base t; mkdir -p "$trees/t/proc/vz" "$trees/t/proc/bc"; tcase t ''
base t; put t proc/cpuinfo "processor\t: 0\nmodel name\t: UML\n\n"; tcase t ''
for vx in 'VxID:\t0\n' 'VxID:\t12a\n' 'VxID: 5' 'VxID:\n'; do
  base t; put t proc/self/status "Name:\tx\n$vx"; tcase t ''
done
base t; put t proc/sys/kernel/osrelease '4.4.0-19041-Microsoft\n'; tcase t '' '-J'
base t; put t proc/cpuinfo "processor\t: 0\nflags\t\t: fpu svm\n\n"; tcase t ''

# --- this machine -------------------------------------------------------------------
for args in '' '-e' '-p' '-C' '-J' '-e -J' '-C -J' '-x' '-B' '-y' '-e -y' \
            '--hierarchic' '-e --output-all' '-p --output-all' '-C --output-all'; do
  # shellcheck disable=SC2086
  both $args
done

# --- terminals -------------------------------------------------------------------
for cols in 1 20 40 60 80 100 132 200; do
  pty_case "$cols" -s "$x86"
  pty_case "$cols" -s "$dumps/x86_64-epyc_7451"
  pty_case "$cols" -s "$dumps/s390-lpar" -e
  pty_case "$cols" -s "$dumps/x86_64-epyc_7451" --hierarchic=never
  pty_case "$cols" -s "$dumps/armv7" -C
done
pty_case 80
pty_case 80 -e

# --- closed and full descriptors -------------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" -s "$x86"
  redir_case "$how" -s "$x86" -e
  redir_case "$how" -s "$dumps/x86_64-epyc_7451" -p
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
  redir_case "$how" -s "$x86" -e nosuch
  redir_case "$how" -s /nonexistent
done

printf '%d passed, %d differed, %d broken, %d where only upstream hung\n' \
  "$pass" "$fail" "$broken" "$hung"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
