#!/usr/bin/env bash
# Differential test: our `vmstat` against procps-ng 4.0.4's, built from the
# release (procps-ref.sh).
#
# ## A made-up machine that moves when told to
#
# `vmstat` reports the machine, twice over: the first line since boot, and
# every later one the difference between two readings a delay apart. Neither
# side reads the real `/proc`. Each case runs both programs in their own
# `unshare -mUr` with a fixture directory bind-mounted over `/proc` and
# another over `/sys` (for `/sys/block`, which decides what `-d` calls a
# disk), and everything `vmstat` reads -- `stat`, `meminfo`, `vmstat`,
# `uptime`, `diskstats`, `slabinfo`, `cpuinfo` -- is a file this script
# wrote, and the same bytes for both sides.
#
# A static fixture pins the first line and makes every interval line a
# difference of nothing, which tests half of the arithmetic. So a world may
# carry STATES: after half a second, and then once a second, a ticker
# rewrites the fixture's files in place with the next state's. The program
# holds its files open and re-reads them from the start, as procps' library
# does, so it sees each state at the reading after it lands; the half second
# keeps every rewrite well clear of every read. Each side gets its own copy of
# the world, so the ticker of one cannot touch what the other reads.
#
# The states are where upstream's arithmetic shows: ticks that run backwards
# (clamped to none by the library), an idle count that jumps past 2^31 and
# turns into a C `int` debt carried to the next line, interrupt and context
# switch counts whose difference is cut to an `int`, devices that vanish and
# come back (and are listed at the end of the table when they do).
#
# ## The clock
#
# `-t` prints the time of day, which the two sides read seconds apart. The
# timestamps are compared as times, within a tolerance, and then masked --
# see `tsmask.py` below -- so a timestamp in the wrong zone or format still
# fails.
#
# ## Cases that differ on purpose
#
# `--version` and `-V`: this port names its own build. A `/proc/uptime` that
# holds no numbers, and an empty delay or count: upstream appends whatever
# `errno` an earlier call left behind. An argument with an apostrophe or a
# control character in it, echoed in a diagnostic: quoted or escaped here, as
# `free` does. The port's module docs say more of each.
set -u

DIFF_PROG='vmstat'
DIFF_NEED="python3 unshare timeout"
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
DIFF_REF=$PROCPS_REF_VMSTAT
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# ---------------------------------------------------------------------------
# ptyrun.py: COMMAND with standard output on a pty COLS columns wide and ROWS
# rows; what it wrote there comes out on our standard output.
# ---------------------------------------------------------------------------
ptyrun=$DIFF_TMP/ptyrun.py
cat >"$ptyrun" <<'PY'
import fcntl, os, struct, subprocess, sys, termios

cols, rows = int(sys.argv[1]), int(sys.argv[2])
master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
p = subprocess.Popen(sys.argv[3:], stdin=subprocess.DEVNULL, stdout=slave)
os.close(slave)
out = b""
while True:
    try:
        chunk = os.read(master, 65536)
    except OSError:
        break
    if not chunk:
        break
    out += chunk
sys.stdout.buffer.write(out.replace(b"\r\n", b"\n"))
sys.exit(p.wait())
PY

# ---------------------------------------------------------------------------
# ticker.py WORLD: writes WORLD/states/K/proc/* over WORLD/proc/* in place --
# the same inode, which is what the program has open -- K = 1, 2, ... at
# 0.5 s, 1.5 s, 2.5 s ... after it starts.
# ---------------------------------------------------------------------------
ticker=$DIFF_TMP/ticker.py
cat >"$ticker" <<'PY'
import os, sys, time

w = sys.argv[1]
start = time.monotonic()
k = 1
while True:
    sd = os.path.join(w, "states", str(k), "proc")
    if not os.path.isdir(sd):
        break
    delay = start + 0.5 + (k - 1) - time.monotonic()
    if delay > 0:
        time.sleep(delay)
    for name in sorted(os.listdir(sd)):
        with open(os.path.join(sd, name), "rb") as f:
            data = f.read()
        with open(os.path.join(w, "proc", name), "r+b") as f:
            f.write(data)
            f.truncate()
    k += 1
PY

# ---------------------------------------------------------------------------
# tsmask.py A B: if A and B hold the same number of timestamps and each pair
# is within TOLERANCE seconds, replace them all with <TS> in both files.
# Otherwise leave both alone, so the comparison fails and shows them.
# ---------------------------------------------------------------------------
tsmask=$DIFF_TMP/tsmask.py
cat >"$tsmask" <<'PY'
import calendar, re, sys, time

TOLERANCE = 20
pat = re.compile(rb"[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}")
a, b = sys.argv[1], sys.argv[2]
with open(a, "rb") as f:
    da = f.read()
with open(b, "rb") as f:
    db = f.read()
ta, tb = pat.findall(da), pat.findall(db)


def secs(t):
    return calendar.timegm(time.strptime(t.decode(), "%Y-%m-%d %H:%M:%S"))


if len(ta) != len(tb) or any(abs(secs(x) - secs(y)) > TOLERANCE for x, y in zip(ta, tb)):
    sys.exit(0)
for path, data in ((a, da), (b, db)):
    with open(path, "wb") as f:
        f.write(pat.sub(b"<TS>", data))
PY

# ---------------------------------------------------------------------------
# mkworld.py DEST: the fake machines.
# ---------------------------------------------------------------------------
mkworld=$DIFF_TMP/mkworld.py
cat >"$mkworld" <<'PY'
import os, shutil, sys

dest = sys.argv[1]


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if isinstance(data, str):
        data = data.encode()
    with open(path, "wb") as f:
        f.write(data)


CPU = dict(user=10132153, nice=290696, system=3084719, idle=46828483,
           iowait=16683, irq=0, sirq=25195, steal=0, guest=175628, gnice=0)
SYS = dict(intr=199292, ctxt=3584789283, btime=1696000000, processes=345678,
           running=3, blocked=1)
ORDER = ["user", "nice", "system", "idle", "iowait", "irq", "sirq", "steal",
         "guest", "gnice"]


def stat(fields=10, cpu_line=None, **kw):
    c = dict(CPU)
    s = dict(SYS)
    for k, v in kw.items():
        (c if k in c else s)[k] = v
    line = cpu_line if cpu_line is not None else \
        "cpu  " + " ".join(str(c[k]) for k in ORDER[:fields])
    return "\n".join([
        line,
        "cpu0 1393280 32966 572056 13343292 6130 0 17875 0 23933 0",
        "cpu1 1335229 34436 553186 13376218 3373 0 2400 0 23960 0",
        "intr %s 9 0 0 0 0 1" % s["intr"],
        "ctxt %s" % s["ctxt"],
        "btime %s" % s["btime"],
        "processes %s" % s["processes"],
        "procs_running %s" % s["running"],
        "procs_blocked %s" % s["blocked"],
        "softirq 12345 0 1 2 3",
    ]) + "\n"


MEM = [("MemTotal", 16000000), ("MemFree", 2100000), ("MemAvailable", 8300000),
       ("Buffers", 510000), ("Cached", 4100000), ("SwapCached", 12000),
       ("Active", 6000000), ("Inactive", 3000000), ("SwapTotal", 4000000),
       ("SwapFree", 3100000), ("Dirty", 3000), ("Shmem", 130000),
       ("SReclaimable", 260000), ("Committed_AS", 5200000)]


def meminfo(**kw):
    out = []
    for k, v in MEM:
        v = kw.get(k, v)
        if v is None:
            continue
        out.append("%-15s %9s kB" % (k + ":", v))
    return "\n".join(out) + "\n"


def vmstat(pgpgin=12345678, pgpgout=23456789, pswpin=4567, pswpout=8901):
    return ("nr_free_pages 525000\npgpgin %s\npgpgout %s\npswpin %s\n"
            "pswpout %s\npgfault 99999999\n" % (pgpgin, pgpgout, pswpin, pswpout))


DISKS = [
    "   7       0 loop0 1234 0 5678 90 0 0 0 0 0 100 90 0 0 0 0 0 0",
    "   8       0 sda 456789 12345 98765432 345678 234567 34567 87654321 456789 2 567890 802467 0 0 0 0 1234 5678",
    "   8       1 sda1 450000 12000 98000000 340000 230000 34000 87000000 450000 0 560000 790000 0 0 0 0 0 0",
    "   8       2 sda2 6789 345 765432 5678 4567 567 654321 6789 0 7890 12467 0 0 0 0 0 0",
    " 259       0 nvme0n1 1000000 2000 300000000 400000 500000 6000 700000000 800000 1500 900000 1200000 0 0 0 0 0 0",
    " 259       1 nvme0n1p1 999 0 9999 99 0 0 0 0 0 99 99 0 0 0 0 0 0",
]


def diskstats(lines=None):
    return "\n".join(DISKS if lines is None else lines) + "\n"


SLABS = [
    "kmalloc-64          12345  13000     64   64    1 : tunables    0    0    0 : slabdata    203    203      0",
    "dentry             234567 240000    192   21    1 : tunables    0    0    0 : slabdata  11428  11428      0",
    "TCP                    45     48   2368   13    8 : tunables    0    0    0 : slabdata      4      4      0",
    "a_very_long_cache_name_that_runs_past_24 10 20 128 32 1 : tunables 0 0 0 : slabdata 1 1 0",
    "ext4_inode_cache    56789  57000   1176   27    8 : tunables    0    0    0 : slabdata   2111   2111      0",
    "Acpi-State            204    204     80   51    1 : tunables    0    0    0 : slabdata      4      4      0",
]
SLAB_HEAD = ("slabinfo - version: 2.1\n"
             "# name            <active_objs> <num_objs> <objsize> <objperslab> "
             "<pagesperslab> : tunables <limit> <batchcount> <sharedfactor> : "
             "slabdata <active_slabs> <num_slabs> <sharedavail>\n")


def slabinfo(lines=None, head=SLAB_HEAD):
    return head + "\n".join(SLABS if lines is None else lines) + "\n"


CPUINFO = "processor\t: 0\ncore id\t\t: 0\n\nprocessor\t: 1\ncore id\t\t: 1\n\n"
BLOCK = ["loop0", "sda", "nvme0n1"]


def world(name, states=(), block=BLOCK, omit=(), **files):
    root = os.path.join(dest, name)
    base = dict(stat=stat(), meminfo=meminfo(), vmstat=vmstat(),
                uptime="123456.78 456789.12\n", diskstats=diskstats(),
                slabinfo=slabinfo(), cpuinfo=CPUINFO)
    base.update(files)
    for k, v in base.items():
        if k not in omit:
            write(os.path.join(root, "proc", k), v)
    os.makedirs(os.path.join(root, "sys"), exist_ok=True)
    if block is not None:
        for b in block:
            os.makedirs(os.path.join(root, "sys", "block", b), exist_ok=True)
    for i, st in enumerate(states, 1):
        for k, v in st.items():
            write(os.path.join(root, "states", str(i), "proc", k), v)


world("main")

# --- a machine that moves -----------------------------------------------------
# State 1, 2 and 3 land before the second, third and fourth readings.
c = CPU
world("tick", states=[
    dict(stat=stat(user=c["user"] + 150, nice=c["nice"] + 10,
                   system=c["system"] + 40, idle=c["idle"] + 590,
                   iowait=c["iowait"] + 7, sirq=c["sirq"] + 3,
                   guest=c["guest"] + 30, intr=SYS["intr"] + 1234,
                   ctxt=SYS["ctxt"] + 5678, running=5, blocked=0),
         meminfo=meminfo(MemFree=2000000, Buffers=520000, Cached=4150000,
                         SwapFree=3000000, Active=6100000),
         vmstat=vmstat(pgpgin=12345678 + 4096, pgpgout=23456789 + 1000,
                       pswpin=4567 + 25, pswpout=8901 + 100),
         diskstats=diskstats(DISKS[1:]),
         slabinfo=slabinfo(SLABS[:3])),
    # Idle runs backwards (clamped to none), user jumps by more than 2^31
    # ticks, and the counters' differences pass 2^32 (cut to an int).
    dict(stat=stat(user=c["user"] + 150 + 3000000000, nice=c["nice"] + 10,
                   system=c["system"] + 50, idle=c["idle"] + 500,
                   iowait=c["iowait"] + 7, sirq=c["sirq"] + 3,
                   guest=c["guest"] + 30, intr=SYS["intr"] + 1234 + 4294967296 + 77,
                   ctxt=SYS["ctxt"] + 5678 - 10, running=1, blocked=2),
         vmstat=vmstat(pgpgin=12345678 + 4096 + 7, pgpgout=23456789 + 1000,
                       pswpin=4567 + 25, pswpout=8901 + 100 + 2**40),
         diskstats=diskstats(DISKS + [
             "   8      16 sdb 1 2 3 4 5 6 7 8 9 10 11"])),
    # Idle jumps by more than 2^31 (an int debt), then the next line pays it.
    dict(stat=stat(user=c["user"] + 150 + 3000000000 + 100,
                   nice=c["nice"] + 10, system=c["system"] + 60,
                   idle=c["idle"] + 500 + 3000000000, iowait=c["iowait"] + 7,
                   sirq=c["sirq"] + 3, guest=c["guest"] + 30,
                   intr=SYS["intr"] + 1234 + 4294967296 + 77 + 100,
                   ctxt=SYS["ctxt"] + 5678 - 10 + 100, running=2, blocked=0),
         diskstats=diskstats(DISKS[:2] + DISKS[3:])),
], block=BLOCK + ["sdb"])
world("tickdebt", states=[
    dict(stat=stat(idle=c["idle"] + 3000000000, user=c["user"] + 10)),
    dict(stat=stat(idle=c["idle"] + 3000000000 + 400, user=c["user"] + 20)),
])

# --- the shapes of /proc/stat ---------------------------------------------------
world("stat8", stat=stat(fields=8))
world("stat7", stat=stat(fields=7))
world("statnocpu", stat="intr 1\nctxt 2\n")
world("statjunk", stat="cpu  1 2 3 4 5 6 7 8 9 10\nfoo procs_running 7 \x00ctxt 99\n")
world("zeroticks", stat=stat(cpu_line="cpu  0 0 0 0 0 0 0 0 0 0"))
world("bigticks", stat=stat(cpu_line="cpu  9223372036854775800 5 6 7 8 0 0 0 9 0",
                            ctxt=18446744073709551615))
world("negticks", stat=stat(cpu_line="cpu  -5 2 3 4 5 6 7 8 9 10", running=0))
world("nostat", omit=("stat",))

# --- the other files ------------------------------------------------------------
world("novmstat", omit=("vmstat",))
world("emptyvmstat", vmstat="")
world("oddvmstat", vmstat="pgpgin 5\nnospace\npgpgout 6\npswpin -1\npswpout +3\n")
world("nomeminfo", omit=("meminfo",))
world("oddmem", meminfo="MemTotal: 1000 kB\njunk line\nMemFree: 400 kB\n"
      "Buffers:\n 77 kB\nCached: -1 kB\nSwapTotal: 99999999999999999999999 kB\n"
      "Active: 5\x00 kB\nInactive: 6 kB\n")
world("noswapfree", meminfo=meminfo(SwapFree=None))
world("nouptime", omit=("uptime",))
world("baduptime", uptime="up a while\n")
world("oneuptime", uptime="100.5\n")
world("zerouptime", uptime="0.00 0.00\n")
world("nodisk", omit=("diskstats",))
world("baddisk", diskstats=diskstats(DISKS[:2] + ["   8 1 sda1 1 2 3 4 5 6 7 8 9 10"]))
world("longdisk", diskstats=diskstats(
    ["8 0 %s 1 2 3 4 5 6 7 8 9 10 11" % ("d" * 34),
     "8 1 %s 1 2 3 4 5 6 7 8 9 10 11" % ("e" * 35)]))
world("emptydisk", diskstats="")
world("partfirst", diskstats=diskstats([DISKS[2], DISKS[1], DISKS[0]]))
world("nosysblock", block=None)
world("dotdisk", diskstats=diskstats(DISKS[:2] + ["8 9 . 1 2 3 4 5 6 7 8 9 10 11"]))
world("noslab", omit=("slabinfo",))
world("oldslab", slabinfo=slabinfo(head="slabinfo - version: 1.1\n"))
world("badslab", slabinfo=slabinfo(SLABS[:2] + ["broken 1 2 3 4 5"]))
world("emptyslab", slabinfo="")
world("noslabs", slabinfo=SLAB_HEAD)
world("nocpuinfo", omit=("cpuinfo",))
PY

python3 "$mkworld" "$work/worlds" || { echo "vmstat-diff: could not build the worlds" >&2; exit 2; }

# --- knobs ------------------------------------------------------------------
WORLD=main       # which fake machine
LOCALE=C.UTF-8
ZONE=UTC
PTY=             # "COLS ROWS": run with standard output on a pty that size
REDIR=           # a redirection for vmstat itself: '>&-', '2>/dev/full' ...
TICK=            # non-empty: run the world's ticker alongside
reset_knobs() {
  WORLD=main; LOCALE=C.UTF-8; ZONE=UTC; PTY=; REDIR=; TICK=
}

# $1 = side, $2 = output prefix; the rest is vmstat's argv.
run_side() {
  local side=$1 p=$2; shift 2
  local w=$p.$side.world
  rm -rf "$w"
  cp -a "$work/worlds/$WORLD" "$w"
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=$LOCALE" "TZ=$ZONE")
  local -a ns=(timeout -k 5 60 unshare -mUr --propagation private sh -c '
      w=$1 ticker=$2 redir=$3; shift 3
      mount --bind "$w/proc" /proc || exit 125
      mount --bind "$w/sys" /sys || exit 125
      cd / || exit 125
      if [ -n "$ticker" ]; then
        python3 "$ticker" "$w" </dev/null >/dev/null 2>&1 &
      fi
      eval "exec \"\$@\" $redir"' _ "$w" "${TICK:+$ticker}" "$REDIR" \
      "${envs[@]}" vmstat "$@")
  if [ -n "$PTY" ]; then
    # shellcheck disable=SC2086 # COLS ROWS are two words on purpose
    diff_run python3 "$ptyrun" $PTY "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err"
  else
    diff_run "${ns[@]}" >"$p.$side.out" 2>"$p.$side.err" </dev/null
  fi
  echo $? >"$p.$side.rc"
  return 0
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  python3 "$tsmask" "$p.ours.out" "$p.gnu.out"
  LABEL="vmstat $*"
  [ "$WORLD" != main ] && LABEL="$LABEL [world=$WORLD]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  [ "$ZONE" != UTC ] && LABEL="$LABEL [TZ=$ZONE]"
  [ -n "$PTY" ] && LABEL="$LABEL [pty $PTY]"
  [ -n "$TICK" ] && LABEL="$LABEL [ticking]"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs

  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  # 125 is a mount that failed and 124 a timeout: the case never reached
  # vmstat, which is not agreement however alike the two sides look.
  case "$o_rc $g_rc" in
    *124*|*125*|*127*)
      AGREED=broken
      REPORT="  ours rc=$o_rc  gnu rc=$g_rc
$(head -5 "$p.ours.err")
$(head -5 "$p.gnu.err")"
      return 0 ;;
  esac
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$p.ours.out" | head -40)" "$(cat -A "$p.ours.err" | head -20)" \
    "$g_rc" "$(cat -A "$p.gnu.out" | head -40)" "$(cat -A "$p.gnu.err" | head -20)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached vmstat on one or both sides\n%s\n' "$LABEL" "$REPORT"
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

# `xfail_case REASON -- ARGS...`: a difference on purpose.
xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# A guard against vacuous agreement: the reference must have read the
# fixture -- its free memory, its cached-plus-reclaimable figure.
# ---------------------------------------------------------------------------
run_case
if ! grep -q ' 2100000 *510000 *4360000 ' "$work/c1.gnu.out"; then
  echo "vmstat-diff: the reference did not see the fixture /proc:" >&2
  cat "$work/c1.gnu.out" "$work/c1.gnu.err" >&2
  exit 1
fi

# --- the default report, and its options ------------------------------------------
run_case -w
run_case -a
run_case -a -w
run_case -n
run_case --one-header --wide --active
run_case 1 1
run_case 5 0
run_case ' 1' 1
run_case -- 1 1
run_case 1 1 -a
run_case -t
run_case -t -w
run_case -t -a 1 1
ZONE=Asia/Kolkata; run_case -t
ZONE=America/New_York; run_case -t -w
ZONE='<+0530>-5:30'; run_case -t
for u in b B k K m M; do
  run_case -S "$u"
  run_case -S "$u" -s
done
run_case -S kB -s
run_case --unit=M -w
run_case -S x
run_case -S ''
run_case -S

# --- intervals --------------------------------------------------------------------
run_case 1 2
run_case -y 1 2
run_case -y
run_case -n 1 3
TICK=1; WORLD=tick; run_case 1 4
TICK=1; WORLD=tick; run_case -w -a 1 4
TICK=1; WORLD=tick; run_case -y 1 3
TICK=1; WORLD=tick; run_case -S M 1 3
TICK=1; WORLD=tick; run_case 2 2
TICK=1; WORLD=tickdebt; run_case 1 3
TICK=1; WORLD=tick; run_case -t 1 2

# --- the header comes back every (rows - 3) lines on a terminal ------------------------
PTY="80 5"; run_case 1 4
PTY="80 4"; run_case 1 3
PTY="80 3"; run_case 1 2
PTY="80 5"; run_case -n 1 3
PTY="132 6"; run_case -w -t 1 3

# --- the command line ---------------------------------------------------------------------
run_case 0
run_case -5
run_case abc
run_case 1x
run_case 0x10
run_case 99999999999999999999
run_case 4294967296 1
run_case 4294967295 1
run_case 1 abc
run_case 1 99999999999999999999
run_case 1 2 3
xfail_case "upstream appends a stale errno to an empty number" -- ''
# An empty COUNT agrees: `strtol_or_err` zeroed `errno` when it parsed the
# delay before it, so upstream has no stale reason to append there either.
run_case 1 ''
xfail_case "an apostrophe is quoted as free quotes it" -- "it's"
xfail_case "a control character is escaped" -- "$(printf 'a\033[2Jb')"
xfail_case "a control character is escaped" -- -p "$(printf 'sd\033[2Ja')"
run_case -Z
run_case --nosuch
run_case --d
run_case --disk-s
run_case --un=M
run_case -p
run_case --partition
run_case -d -s
run_case -d -D
run_case -m -p sda1
run_case -h
run_case --help
run_case -h -Z
run_case -Z -h
run_case -s -h
xfail_case "our version string, not procps-ng's" -- -V
xfail_case "our version string, not procps-ng's" -- --version
run_case -S x -V

# --- -f, -s and -D ------------------------------------------------------------------------
run_case -f
run_case --forks
run_case -f -Z
run_case -Z -f
run_case -s
run_case --stats -S m
run_case -D
run_case --disk-sum
WORLD=stat8; run_case -s
WORLD=bigticks; run_case -s
WORLD=noswapfree; run_case -s
WORLD=oddmem; run_case -s
WORLD=oddvmstat; run_case -s
WORLD=nosysblock; run_case -D
WORLD=dotdisk; run_case -D
WORLD=longdisk; run_case -D
WORLD=emptydisk; run_case -D

# --- -d and -p -----------------------------------------------------------------------------
run_case -d
run_case -d -w
run_case -d -n
run_case -d -t
run_case -d 1 2
TICK=1; WORLD=tick; run_case -d 1 4
TICK=1; WORLD=tick; run_case -d -n 1 4
PTY="80 4"; run_case -d
PTY="80 5"; run_case -d -w
WORLD=partfirst; run_case -d
WORLD=partfirst; run_case -d -n
WORLD=nosysblock; run_case -d
WORLD=dotdisk; run_case -d
run_case -p sda1
run_case -p /dev/sda1
run_case -p sda
run_case -p loop0
run_case --partition=nvme0n1p1 1 2
run_case -p nosuch
run_case -p ''
run_case -p /dev/
run_case -psda2
TICK=1; WORLD=tick; run_case -p sda1 1 3
TICK=1; WORLD=tick; run_case -p sda2 1 4

# --- -m ------------------------------------------------------------------------------------
run_case -m
run_case -m -n
run_case -m 1 2
PTY="80 5"; run_case -m
PTY="80 4"; run_case -m -n
TICK=1; WORLD=tick; run_case -m 1 3
WORLD=noslabs; run_case -m

# --- malformed and missing files -------------------------------------------------------------
for f in '' -s -f; do
  # shellcheck disable=SC2086 # the empty option is no word at all on purpose
  WORLD=stat7; run_case $f
  # shellcheck disable=SC2086
  WORLD=statnocpu; run_case $f
  # shellcheck disable=SC2086
  WORLD=nostat; run_case $f
done
WORLD=stat8; run_case
WORLD=statjunk; run_case
WORLD=statjunk; run_case -f
WORLD=zeroticks; run_case
WORLD=bigticks; run_case
WORLD=bigticks; run_case -w
WORLD=negticks; run_case
WORLD=novmstat; run_case
WORLD=novmstat; run_case -s
WORLD=emptyvmstat; run_case
WORLD=oddvmstat; run_case
WORLD=nomeminfo; run_case
WORLD=nomeminfo; run_case -s
WORLD=oddmem; run_case
WORLD=oddmem; run_case -a
WORLD=noswapfree; run_case
WORLD=nouptime; run_case
# A word where a number should be: upstream appends the `errno` the library
# left -- `No such process`, from a miss in the hash table it looks every
# `/proc/meminfo` key up in -- which depends on what else is in `/proc`.
WORLD=baduptime; xfail_case "upstream appends a stale errno" --
# One number and then the end of the file: glibc's `fscanf` zeroes `errno`
# when it meets the end of the file while skipping blanks, so upstream has no
# reason to append, and the two agree.
WORLD=oneuptime; run_case
WORLD=oneuptime; run_case -s
WORLD=zerouptime; run_case
WORLD=nodisk; run_case -d
WORLD=nodisk; run_case -D
WORLD=nodisk; run_case -p sda
WORLD=baddisk; run_case -d
WORLD=baddisk; run_case -p sda
WORLD=longdisk; run_case -d
WORLD=emptydisk; run_case -d
WORLD=noslab; run_case -m
WORLD=oldslab; run_case -m
WORLD=badslab; run_case -m
WORLD=nocpuinfo; run_case
WORLD=nocpuinfo; run_case -s

# --- a standard descriptor that cannot be written ------------------------------------------------
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  REDIR=$redir; run_case
  REDIR=$redir; run_case -s
  REDIR=$redir; run_case -Z
  REDIR=$redir; run_case --help
  REDIR=$redir; run_case -f
  REDIR=$redir; run_case 0
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
