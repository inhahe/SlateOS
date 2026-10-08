#!/usr/bin/env bash
# Differential test: our `sysctl` against procps-ng 4.0.4's, built from the
# release (procps-ref.sh).
#
# ## A made-up /proc/sys, and made-up configuration
#
# `sysctl` reads and writes the running kernel's settings, so neither side is
# let near the real ones. Each case runs both programs in their own
# `unshare -mUr` with a fixture directory bind-mounted over `/proc`, so
# `/proc/sys` is a tree of ordinary files this script wrote -- with the modes
# real ones have, since `sysctl` decides by the mode bits whether a key may be
# read or written -- and each side gets its own copy of it, so a write by one
# cannot be seen by the other. After every case the copy's files are dumped,
# path and contents, and the two dumps are compared as well as the output:
# a write is checked by what it wrote.
#
# `--system` and a bare `-p` read configuration from fixed places, so those
# are fixtures too: `/etc/sysctl.conf`, `/etc/sysctl.d` and
# `/usr/lib/sysctl.d` bind-mounted over the host's, and `/run` and
# `/usr/local/lib` covered with an empty `tmpfs` each, in which the world's
# `sysctl.d` (if it has one) is mounted.
#
# ## Cases that differ on purpose
#
# `--version` and `-V`: this port names its own build.
set -u

DIFF_PROG='sysctl'
DIFF_NEED="python3 unshare timeout"
# shellcheck source=procps-ref.sh
. "$(dirname "$0")/procps-ref.sh"
DIFF_REF=$PROCPS_REF_SYSCTL
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# Each side's copy of a world lives on a tmpfs, not under $DIFF_TMP. procps'
# `DisplayAll` reads a directory's first two entries and throws them away
# unread, taking them to be `.` and `..` -- as procfs lists them, and tmpfs.
# ext4, where $DIFF_TMP is, lists `..` first and `.` last, so on it the
# reference skips a real entry and walks into `.` and `..` themselves, without
# end. Measured with `ls -f` on both before this was chosen.
shm=$(mktemp -d /dev/shm/sysctl-diff.XXXXXX) || {
  echo "sysctl-diff: cannot make a directory under /dev/shm" >&2
  exit 2
}
# diff-wsl.sh asks for its cleanup to be extended rather than a second EXIT
# trap set; this one does what its does, and the tmpfs copies too -- unless
# DIFF_KEEP is set (a `DIFF_*` name, so it crosses into WSL with the rest),
# which leaves every case's output, error and state files for reading and
# says where.
diff_cleanup() {
  if [ -n "${DIFF_KEEP:-}" ]; then
    echo "sysctl-diff: DIFF_KEEP set; the cases are in $work"
    chmod -R u+rwx "$shm" 2>/dev/null
    rm -rf "$shm"
    return
  fi
  chmod -R u+rwx "$shm" "$DIFF_TMP" 2>/dev/null
  rm -rf "$shm" "$DIFF_TMP"
}

# ---------------------------------------------------------------------------
# mkworld.py DEST: the fake machines.
# ---------------------------------------------------------------------------
mkworld=$DIFF_TMP/mkworld.py
cat >"$mkworld" <<'PY'
import os, sys

dest = sys.argv[1]


MODES = []


def put(path, data, mode=0o644, rel=None):
    """Write the file readable; a mode other than 0644 is recorded for the
    harness to apply to each side's copy, after the copy is made (a file
    without its read bit could not be copied)."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if isinstance(data, str):
        data = data.encode()
    with open(path, "wb") as f:
        f.write(data)
    if mode != 0o644 and rel is not None:
        MODES.append("%o %s" % (mode, rel))


SYS = {
    "kernel/hostname": "slateos\n",
    "kernel/domainname": "(none)\n",
    "kernel/ostype": ("Linux\n", 0o444),
    "kernel/osrelease": ("6.6.0-slateos\n", 0o444),
    "kernel/pid_max": "4194304\n",
    "kernel/printk": "4\t4\t1\t7\n",
    "kernel/sem": "32000\t1024000000\t500\t32000\n",
    "kernel/random/boot_id": ("6f2e0b7c-9a51-4c1e-8d2a-3b4c5d6e7f80\n", 0o444),
    "kernel/random/entropy_avail": ("256\n", 0o444),
    "kernel/empty": "",
    "kernel/secret": ("hidden\n", 0o000),
    "kernel/writeonly": ("", 0o200),
    "kernel/multiline": "first line\nsecond line\nthird, no newline",
    "kernel/nul": b"ab\x00cd\n",
    "vm/swappiness": "60\n",
    "vm/overcommit_memory": "0\n",
    "vm/dirty_ratio": "20\n",
    "vm/compact_memory": ("", 0o200),
    "vm/drop_caches": ("", 0o200),
    "net/core/somaxconn": "4096\n",
    "net/ipv4/ip_forward": "0\n",
    "net/ipv4/tcp_syncookies": "1\n",
    "net/ipv4/conf/all/forwarding": "0\n",
    "net/ipv4/conf/all/rp_filter": "1\n",
    "net/ipv4/conf/default/forwarding": "0\n",
    "net/ipv4/conf/default/rp_filter": "1\n",
    "net/ipv4/conf/eth0.100/forwarding": "0\n",
    "net/ipv4/conf/eth0.100/rp_filter": "1\n",
    "net/ipv4/neigh/default/base_reachable_time": "30\n",
    "net/ipv4/neigh/default/base_reachable_time_ms": "30000\n",
    "net/ipv4/neigh/default/retrans_time": "100\n",
    "net/ipv4/neigh/default/gc_thresh1": "128\n",
    "dev/cdrom/info": "CD-ROM information, Id: cdrom.c 3.20 2003/12/17\n\ndrive name:\t\nCan close tray:\t\n\n",
    "fs/file-max": "9223372036854775807\n",
    "fs/nr_open": "1048576\n",
}

CONF = {
    "etc/sysctl.conf": (
        "# the default file\n"
        "kernel.hostname = fromconf\n"
        "; a semicolon comment\n"
        "badline\n"
        "-vm.nosuchkey = 1\n"
        "vm.nosuchkey2 = 2\n"
        "net.ipv4.conf.*.rp_filter = 2\n"
        "-net.ipv4.conf.all.rp_filter\n"
        "net.ipv4.conf.default.rp_filter = 3\n"
        "   vm.swappiness   =   10   \n"
        "kernel.domainname=\n"
        "x\n"
        "kernel.osrelease = 1\n"
        "net/core/somaxconn = 1024\n"
    ),
    "etc/sysctl.d/10-first.conf": "vm.dirty_ratio = 11\nkernel.pid_max = 32768\n",
    "etc/sysctl.d/20-dup.conf": "vm.dirty_ratio = 22\n",
    "etc/sysctl.d/readme": "not a .conf file\nvm.dirty_ratio = 99\n",
    "etc/sysctl.d/.conf": "kernel.printk = 1 1 1 1\n",
    "usr/lib/sysctl.d/20-dup.conf": "vm.dirty_ratio = 77\n",
    "usr/lib/sysctl.d/50-lib.conf": "net.ipv4.ip_forward = 1\nfs.nr_open = 2048\n",
    "run/sysctl.d/05-run.conf": "net.core.somaxconn = 555\n",
    "usr/local/lib/sysctl.d/40-local.conf": "net.ipv4.tcp_syncookies = 0\n",
    "files/simple.conf": "kernel.hostname = simple\nvm.swappiness = 1\n",
    "files/other.conf": "vm.overcommit_memory = 2\n",
    "files/glob.conf": "net.ipv4.conf.*.forwarding = 1\n-net.ipv4.conf.eth0/100.forwarding\n",
    "files/bad.conf": "kernel.osrelease = 2\nkernel = 3\nnosuch.key = 4\n-ignored.key = 5\n",
}


def world(name, sys_files=SYS, conf=CONF, extra=None, drop=()):
    root = os.path.join(dest, name)
    del MODES[:]
    for rel, spec in sys_files.items():
        if rel in drop:
            continue
        data, mode = spec if isinstance(spec, tuple) else (spec, 0o644)
        put(os.path.join(root, "proc", "sys", rel), data, mode, "proc/sys/" + rel)
    for rel, data in conf.items():
        put(os.path.join(root, rel), data)
    for d in ("etc/sysctl.d", "usr/lib/sysctl.d", "run", "usr/local/lib", "files"):
        os.makedirs(os.path.join(root, d), exist_ok=True)
    os.makedirs(os.path.join(root, "proc", "sys", "wdir"), exist_ok=True)
    # A link out of /proc/sys, and one to nowhere.
    os.symlink("/etc/hostname", os.path.join(root, "proc", "sys", "kernel", "outside"))
    os.symlink("/nonexistent/target", os.path.join(root, "proc", "sys", "kernel", "dangling"))
    for rel, data in (extra or {}).items():
        put(os.path.join(root, rel), data)
    with open(os.path.join(root, "modes.txt"), "w") as f:
        f.write("".join(m + "\n" for m in MODES))


world("main")
# The conf directories empty, and no /etc/sysctl.conf at all.
world("noconf", conf={k: v for k, v in CONF.items() if k.startswith("files/")})
PY

python3 "$mkworld" "$work/worlds" || { echo "sysctl-diff: could not build the worlds" >&2; exit 2; }

# The modes /proc/sys has, applied to a world's copy once it is made: the
# files' from the world's modes.txt, and the directories' without their write
# bit -- except the one fixture (`wdir`) that keeps it, so a write to a
# directory reaches EISDIR rather than EPERM.
apply_modes() {
  local w=$1 mode rel
  while read -r mode rel; do
    chmod "$mode" "$w/$rel"
  done <"$w/modes.txt"
  find "$w/proc/sys" -mindepth 1 -type d ! -name wdir -exec chmod 0555 {} +
  chmod 0555 "$w/proc/sys"
}

# --- knobs ------------------------------------------------------------------
WORLD=main
LOCALE=C.UTF-8
STDIN=/dev/null
REDIR=
reset_knobs() { WORLD=main; LOCALE=C.UTF-8; STDIN=/dev/null; REDIR=; }

# $1 = side, $2 = output prefix; the rest is sysctl's argv.
run_side() {
  local side=$1 p=$2; shift 2
  local w=$shm/${p##*/}.$side.world
  if [ -e "$w" ]; then chmod -R u+rw "$w" && rm -rf "$w"; fi
  cp -a "$work/worlds/$WORLD" "$w"
  apply_modes "$w"
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=$LOCALE" "TZ=UTC")
  diff_run timeout -k 5 60 unshare -mUr --propagation private sh -c '
      w=$1 redir=$2; shift 2
      mount --bind "$w/proc" /proc || exit 125
      mount --bind "$w/usr/lib/sysctl.d" /usr/lib/sysctl.d || exit 125
      if [ -f "$w/etc/sysctl.conf" ]; then
        mount --bind "$w/etc/sysctl.conf" /etc/sysctl.conf || exit 125
      else
        # No /etc/sysctl.conf at all: an empty /etc, but for sysctl.d.
        mount -t tmpfs none /etc || exit 125
        mkdir /etc/sysctl.d || exit 125
      fi
      mount --bind "$w/etc/sysctl.d" /etc/sysctl.d || exit 125
      mount -t tmpfs none /run || exit 125
      if [ -d "$w/run/sysctl.d" ]; then
        mkdir /run/sysctl.d && mount --bind "$w/run/sysctl.d" /run/sysctl.d || exit 125
      fi
      mount -t tmpfs none /usr/local/lib || exit 125
      if [ -d "$w/usr/local/lib/sysctl.d" ]; then
        mkdir /usr/local/lib/sysctl.d \
          && mount --bind "$w/usr/local/lib/sysctl.d" /usr/local/lib/sysctl.d || exit 125
      fi
      cd "$w/files" 2>/dev/null || cd / || exit 125
      eval "exec \"\$@\" $redir"' _ "$w" "$REDIR" "${envs[@]}" sysctl "$@" \
    >"$p.$side.out" 2>"$p.$side.err" <"$STDIN"
  echo $? >"$p.$side.rc"
  # What the run left in /proc/sys: every file's mode, and then -- made
  # readable for the purpose -- its bytes.
  (cd "$w/proc/sys" && find . -type f -printf '%p %m\n' | LC_ALL=C sort) >"$p.$side.state" 2>&1
  chmod -R u+rw "$w"
  (cd "$w/proc/sys" && find . -type f | LC_ALL=C sort | while IFS= read -r f; do
     printf '%s: ' "$f"
     od -An -c "$f" | tr -s ' \n' ' '
     printf '\n'
   done) >>"$p.$side.state" 2>&1
  rm -rf "$w"
  return 0
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="sysctl $*"
  [ "$WORLD" != main ] && LABEL="$LABEL [world=$WORLD]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  [ "$STDIN" != /dev/null ] && LABEL="$LABEL <${STDIN##*/}"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs

  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  # 125 is a mount that failed and 124 a timeout: the case never reached
  # sysctl, which is not agreement however alike the two sides look.
  case "$o_rc $g_rc" in
    *124*|*125*|*127*)
      AGREED=broken
      REPORT="  ours rc=$o_rc  gnu rc=$g_rc
$(head -5 "$p.ours.err")
$(head -5 "$p.gnu.err")"
      return 0 ;;
  esac
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && cmp -s "$p.ours.state" "$p.gnu.state" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s\n  ~~~ state differs:\n%s' \
    "$o_rc" "$(cat -A "$p.ours.out" | head -40)" "$(cat -A "$p.ours.err" | head -20)" \
    "$g_rc" "$(cat -A "$p.gnu.out" | head -40)" "$(cat -A "$p.gnu.err" | head -20)" \
    "$(diff "$p.ours.state" "$p.gnu.state" | head -20)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached sysctl on one or both sides\n%s\n' "$LABEL" "$REPORT"
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
# A guard against vacuous agreement: the reference must have read the fixture.
# ---------------------------------------------------------------------------
run_case kernel.hostname
if ! grep -q '^kernel.hostname = slateos$' "$work/c1.gnu.out"; then
  echo "sysctl-diff: the reference did not see the fixture /proc/sys:" >&2
  cat "$work/c1.gnu.out" "$work/c1.gnu.err" >&2
  exit 1
fi

# --- reading ----------------------------------------------------------------------
run_case kernel/hostname
run_case -n kernel.hostname
run_case -b kernel.hostname
run_case -N kernel.hostname
run_case kernel.hostname vm.swappiness net.core.somaxconn
run_case kernel.printk kernel.sem
run_case kernel.multiline
run_case -n kernel.multiline
run_case -b kernel.multiline
run_case dev.cdrom.info
run_case kernel.empty
run_case -n kernel.empty
run_case kernel.nul
run_case kernel.secret
run_case kernel.writeonly
run_case vm.compact_memory
run_case kernel.outside
run_case kernel.dangling
run_case -e kernel.dangling
run_case nosuch.key
run_case -e nosuch.key
run_case nosuch.a nosuch.b
run_case ''
run_case '' ''
run_case kernel..hostname
run_case kernel//hostname
run_case .kernel.hostname
run_case net.ipv4.conf.eth0/100.forwarding
run_case net/ipv4/conf/eth0.100/forwarding
run_case net.ipv4.conf.eth0.100.forwarding
run_case kernel
run_case kernel.random
run_case -N kernel
run_case net.ipv4.neigh.default
run_case --deprecated net.ipv4.neigh.default
run_case fs.file-max kernel.osrelease

# --- everything --------------------------------------------------------------------
run_case -a
run_case -A
run_case -X
run_case --all
run_case -a --deprecated
run_case -a -N
run_case -a -n
run_case -a -b
run_case -a -e
run_case -a kernel.hostname
run_case -a -r '^net\.ipv4\.conf\.'
run_case -a -r 'random'
run_case -a -r '['
run_case -r 'host' kernel.hostname vm.swappiness
run_case --pattern=swap -a
LOCALE=C; run_case -a -r 'é'

# --- writing ------------------------------------------------------------------------
run_case kernel.hostname=newhost
run_case kernel/hostname=newhost
run_case -w kernel.hostname=newhost
run_case -w kernel.hostname
run_case -w 'kernel.hostname = spaced  '
run_case kernel.hostname=
run_case -q kernel.hostname=quiet
run_case -n kernel.hostname=values
run_case -b kernel.hostname=binary
run_case -N kernel.hostname=names
run_case --dry-run kernel.hostname=dry
run_case --dry-run -q kernel.hostname=dry
run_case kernel.osrelease=readonly
run_case kernel.random=adir
run_case wdir=adir
run_case nosuch.key=1
run_case -e nosuch.key=1
run_case -- -nosuch.key=1
run_case kernel.outside=1
run_case kernel.hostname=a vm.swappiness=5 kernel.osrelease=x nosuch.k=1
run_case kernel.hostname=a kernel.pid_max
run_case 'net.ipv4.conf.*.forwarding=1'
run_case -r host kernel.hostname=matched
run_case -r nomatch kernel.hostname=unmatched
run_case kernel.hostname=a=b
run_case vm.drop_caches=3

# --- loading ------------------------------------------------------------------------
run_case -p
run_case -p simple.conf
run_case -psimple.conf
run_case --load=simple.conf
run_case -f simple.conf
run_case -p simple.conf other.conf
run_case -p 'simple.conf' -q
run_case -p '{simple,other}.conf'
run_case -p '*.conf'
run_case -p glob.conf
run_case -p bad.conf
run_case -p nosuch.conf
run_case -p nosuch.conf simple.conf
run_case -p /etc
run_case -p -r swap simple.conf
run_case -p -N simple.conf
run_case -p --dry-run simple.conf
STDIN=$work/worlds/main/files/simple.conf; run_case -p -
STDIN=$work/worlds/main/files/simple.conf; run_case -p - -
STDIN=$work/worlds/main/files/simple.conf; run_case -p - other.conf
run_case --system
run_case --system -q
run_case -q --system
run_case --system --nosuchoption
WORLD=noconf; run_case --system
WORLD=noconf; run_case -p

# --- the command line -------------------------------------------------------------
run_case
run_case -Z
run_case --nosuch
run_case -r
run_case --pattern
run_case -h
run_case -d
run_case --help
run_case --all=x
run_case -n
run_case -N -q kernel.hostname
run_case -o -x kernel.hostname
xfail_case "our version string, not procps-ng's" -- -V
xfail_case "our version string, not procps-ng's" -- --version
run_case kernel.hostname -Z

# --- a standard descriptor that cannot be written --------------------------------
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  REDIR=$redir; run_case kernel.hostname
  REDIR=$redir; run_case -a
  REDIR=$redir; run_case nosuch.key
  REDIR=$redir; run_case --help
  REDIR=$redir; run_case kernel.hostname=x
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
