#!/bin/bash
# pmap-diff.sh -- run our `pmap` and procps-ng's side by side and report every
# case where they disagree.
#
# ## The reference
#
# Ubuntu's /usr/bin/pmap, procps 4.0.4-4ubuntu3.2. Debian's and Ubuntu's
# patches touch pmap's tests only (the package changelog), so the installed
# binary is upstream's program.
#
# ## The processes it reads
#
# Targets this harness builds and starts: one small static C program, run
# several ways. Static, because what `-X` prints includes each mapping's
# proportional share (`Pss`), which a shared library's pages would change
# with every process that maps the same library -- the two `pmap`s
# themselves among them. A static target shares its pages only with the
# other targets, which live for the whole run. Each is paused, so its map
# does not move between the two sides' readings.
#
#   plain    nothing extra
#   shm      a System V segment attached: `[ shmid=0x… ]`
#   hexlong  a file mapped whose path is so long the map line is cut at
#            fgets's 1023 bytes inside a run of hex digits, which the second
#            piece's `sscanf` then reads as an address
#   long     the same, cut inside a run that is not hex
#   odd      a file whose path holds a newline, an escape, a tab and \xff
#   deleted  a file mapped and then removed
#   thread   a second thread, asked about by its thread id
#
# Then a zombie, whose map reads empty, and init, whose map is not ours to
# read. Every process is killed by its pid when the harness exits.
#
# ## The knobs, reset before each group
#
#   ARGV0  argv[0] for the run (`exec -a`).
#   ERRTO  `merge` puts standard error into the standard output file, to
#          compare the two in order; `full` and `closed` as they say.
#   OUTTO  `full`, `closed`, `epipe` (the reader gone, SIGPIPE ignored).
#   RC     the text of the rc file each side finds as `~/.pmaprc` -- and in
#          its working directory as `rc` -- or unset for none. Each side runs
#          with a fresh HOME and working directory of its own, and what the
#          run leaves in them is compared too, so `-n` and `-N` are judged
#          by the file they write.
#   NOHOME HOME unset.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS; `-A`'s argument is quoted as `quoteaf` quotes
# it, which differs from upstream's bare apostrophes only for an argument
# holding one or something unprintable.
#
# Run `OURS=/usr/bin/pmap ./scripts/pmap-diff.sh` to check that the harness
# still discriminates: every case that differs on purpose should then be
# reported as no longer differing, and nothing else.
set -u

DIFF_PROG='pmap'
DIFF_NEED='timeout gcc python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- the target ---------------------------------------------------------------
cat >"$DIFF_TMP/target.c" <<'EOF'
#define _GNU_SOURCE
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/shm.h>
#include <unistd.h>

static void *idle(void *arg)
{
	(void)arg;
	for (;;)
		pause();
	return NULL;
}

int main(int argc, char **argv)
{
	for (int i = 1; i < argc; i++) {
		if (!strcmp(argv[i], "shm")) {
			int id = shmget(IPC_PRIVATE, 65536, IPC_CREAT | 0600);
			if (id < 0 || shmat(id, NULL, 0) == (void *)-1)
				return 2;
			/* Gone with the process; attached until then. */
			shmctl(id, IPC_RMID, NULL);
		} else if (!strcmp(argv[i], "file") && i + 1 < argc) {
			int fd = open(argv[++i], O_RDONLY);
			if (fd < 0 || mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, fd, 0) == MAP_FAILED)
				return 3;
			close(fd);
		} else if (!strcmp(argv[i], "thread")) {
			pthread_t t;
			if (pthread_create(&t, NULL, idle, NULL))
				return 4;
		}
	}
	printf("ready\n");
	fflush(stdout);
	for (;;)
		pause();
}
EOF
gcc -static -pthread -O0 -o "$DIFF_TMP/pmap-target" "$DIFF_TMP/target.c" || exit 1

pids=()
diff_cleanup() {
  [ "${#pids[@]}" -gt 0 ] && kill "${pids[@]}" 2>/dev/null
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
  return 0
}

# target NAME ARGS...: start one, wait for its "ready"; its pid in $tp.
target() {
  local name=$1; shift
  rm -f "$DIFF_TMP/$name.ready"
  "$DIFF_TMP/pmap-target" "$@" >"$DIFF_TMP/$name.ready" &
  tp=$!
  pids+=("$tp")
  local n=0
  until grep -q ready "$DIFF_TMP/$name.ready" 2>/dev/null; do
    n=$((n+1))
    if [ "$n" -gt 300 ] || ! kill -0 "$tp" 2>/dev/null; then
      echo "pmap-diff: target $name did not start" >&2
      exit 1
    fi
    sleep 0.01
  done
}

target plain; t_plain=$tp
target shm shm; t_shm=$tp
target thread thread; t_thread=$tp

# The long paths: four directories of 240 bytes put byte 950 of the path --
# where a map line's 1023 bytes run out, the path starting at column 73 --
# inside the fourth, whatever the scratch directory's own length.
seg() { local s; s=$(printf '%240s' ''); printf '%s' "${s// /$1}"; }
hexdir=$DIFF_TMP/$(seg a)/$(seg b)/$(seg c)/$(seg f)
txtdir=$DIFF_TMP/$(seg a)/$(seg b)/$(seg c)/$(seg g)
mkdir -p "$hexdir" "$txtdir"
head -c 4096 /dev/zero >"$hexdir/mapped"
head -c 4096 /dev/zero >"$txtdir/mapped"
target hexlong file "$hexdir/mapped"; t_hexlong=$tp
target long file "$txtdir/mapped"; t_long=$tp

odddir=$DIFF_TMP/$'odd\nname\e[1m\t\xff'
mkdir -p "$odddir"
head -c 4096 /dev/zero >"$odddir/mapped"
target odd file "$odddir/mapped"; t_odd=$tp

head -c 4096 /dev/zero >"$DIFF_TMP/doomed"
target deleted file "$DIFF_TMP/doomed"; t_deleted=$tp
rm -f "$DIFF_TMP/doomed"

t_tid=
for t in /proc/"$t_thread"/task/*; do
  t=${t##*/}
  [ "$t" != "$t_thread" ] && t_tid=$t
done
[ -n "$t_tid" ] || { echo "pmap-diff: no second thread" >&2; exit 1; }

python3 -c '
import os, time
pid = os.fork()
if pid == 0:
    os._exit(0)
print(pid, flush=True)
time.sleep(600)
' >"$DIFF_TMP/zombie" &
pids+=("$!")
n=0
until [ -s "$DIFF_TMP/zombie" ]; do
  n=$((n+1)); [ "$n" -gt 300 ] && { echo "pmap-diff: no zombie" >&2; exit 1; }
  sleep 0.01
done
t_zombie=$(cat "$DIFF_TMP/zombie")
n=0
until grep -q '^State:.*Z' "/proc/$t_zombie/status" 2>/dev/null; do
  n=$((n+1)); [ "$n" -gt 300 ] && { echo "pmap-diff: no zombie" >&2; exit 1; }
  sleep 0.01
done

# A number no process has: above any pid_max Linux allows (2^22).
p_none=2147483646

# --- one run ------------------------------------------------------------------
cat >"$DIFF_TMP/run1" <<'EOF'
#!/bin/bash
# run1 OUTTO ERRTO OUT ERR DIR ARGV0 BIN ARGS...
outto=$1 errto=$2 out=$3 err=$4 dir=$5 a0=$6 bin=$7
shift 7
cd "$dir" || exit 125
case $outto in
  file) exec >"$out" ;;
  full) exec >/dev/full ;;
  closed) exec >&- ;;
esac
case $errto in
  file) exec 2>"$err" ;;
  merge) exec 2>&1 ;;
  full) exec 2>/dev/full ;;
  closed) exec 2>&- ;;
esac
case $outto in
  epipe)
    trap '' PIPE
    { sleep 0.3; exec -a "$a0" "$bin" "$@"; } | true
    exit "${PIPESTATUS[0]}"
    ;;
esac
exec -a "$a0" "$bin" "$@"
EOF
chmod +x "$DIFF_TMP/run1"

ARGV0='pmap'; OUTTO='file'; ERRTO='file'; RC=; NOHOME=
reset_knobs() { ARGV0='pmap'; OUTTO='file'; ERRTO='file'; RC=; NOHOME=; unset RC_SET; }
unset RC_SET

run_side() {
  local side=$1; shift
  local home=$DIFF_TMP/$side.home dir=$DIFF_TMP/$side.dir
  rm -rf "$home" "$dir"
  mkdir -p "$home" "$dir"
  if [ -n "${RC_SET:-}" ]; then
    printf '%s' "$RC" >"$home/.pmaprc"
    printf '%s' "$RC" >"$dir/rc"
  fi
  : >"$DIFF_TMP/$side.out"; : >"$DIFF_TMP/$side.err"
  if [ -n "$NOHOME" ]; then
    diff_run timeout -k 2 60 env -u HOME LC_ALL=C.UTF-8 "$DIFF_TMP/run1" \
      "$OUTTO" "$ERRTO" "$DIFF_TMP/$side.out" "$DIFF_TMP/$side.err" "$dir" \
      "$ARGV0" "$bindir/$side/pmap" "$@"
  else
    diff_run timeout -k 2 60 env HOME="$home" LC_ALL=C.UTF-8 "$DIFF_TMP/run1" \
      "$OUTTO" "$ERRTO" "$DIFF_TMP/$side.out" "$DIFF_TMP/$side.err" "$dir" \
      "$ARGV0" "$bindir/$side/pmap" "$@"
  fi
  printf '%s\n' "$?" >"$DIFF_TMP/$side.rc"
  # What the run left behind, by name and content.
  (cd "$DIFF_TMP/$side.home" && find . -type f -exec sh -c 'printf "%s\n" "$1"; od -An -c "$1"' _ {} \;) \
    >"$DIFF_TMP/$side.left" 2>&1
  (cd "$DIFF_TMP/$side.dir" && find . -type f -exec sh -c 'printf "%s\n" "$1"; od -An -c "$1"' _ {} \;) \
    >>"$DIFF_TMP/$side.left" 2>&1
}

same() {
  local f
  for f in out err rc left; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local side
  for side in ours gnu; do
    printf -- '--- %s: status %s\n' "$side" "$(cat "$DIFF_TMP/$side.rc")"
    printf 'out:\n'; head -c 3000 "$DIFF_TMP/$side.out" | cat -A | head -30
    printf 'err:\n'; cat -A "$DIFF_TMP/$side.err" | head -10
    printf 'left:\n'; head -20 "$DIFF_TMP/$side.left"
  done
}

check() {
  local label=$1; shift
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
  fi
  return 0
}

xcheck() {
  local why=$1 label=$2; shift 2
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- every format, over every target ------------------------------------------
all=("$t_plain" "$t_shm" "$t_hexlong" "$t_long" "$t_odd" "$t_deleted" "$t_thread")
for t in "${all[@]}"; do
  for f in '' -x -d -X -XX -q -xq -dq -Xq -XXq -p -xp -dp -Xp; do
    check "target $t ${f:-plain}" ${f:+"$f"} "$t"
  done
done
for f in '' -x -d -X -XX; do
  check "all targets at once ${f:-plain}" ${f:+"$f"} "${all[@]}"
  check "a target twice ${f:-plain}" ${f:+"$f"} "$t_plain" "$t_plain"
  check "a zombie ${f:-plain}" ${f:+"$f"} "$t_zombie"
  check "init ${f:-plain}" ${f:+"$f"} 1
  check "a thread's id ${f:-plain}" ${f:+"$f"} "$t_tid"
  check "a missing process ${f:-plain}" ${f:+"$f"} "$p_none"
  check "found, missing, found ${f:-plain}" ${f:+"$f"} "$t_plain" "$p_none" "$t_shm"
done
check "-X then -XX across processes with other fields" -X "$t_plain" "$t_shm" "$t_odd"

# --- naming a process -------------------------------------------------------------
check "/proc/PID" "/proc/$t_plain"
check "/proc/ then not a number, alone" /proc/self
check "/proc/ then not a number, with one that is" /proc/self "$t_plain"
check "hexadecimal" "0x$(printf %x "$t_plain")"
check "octal" "0$(printf %o "$t_plain")"
for a in 0 -1 x 12x '' ' 1' 2147483648 0x 99999999999999999999999 /proc/0; do
  check "refuses '$a'" "$a"
done
check "a refusal after a good one" "$t_plain" bad
many=()
for _ in $(seq 255); do many+=("$t_plain"); done
check "255 processes" -q "${many[@]}"
check "256 processes" -q "${many[@]}" "$t_plain"

# --- ranges ---------------------------------------------------------------------
lo=$(awk -F- 'NR==2 {print $1}' "/proc/$t_plain/maps")
hi=$(awk -F'[- ]' 'NR==4 {print $2}' "/proc/$t_plain/maps")
for r in "$lo" "$lo,$hi" ",$hi" "$lo," 0,1 ffffffffffffffff 0x10,0x20 ' 10' zz 10,zz '10,' ','; do
  for f in '' -x -d -X; do
    check "-A '$r' ${f:-plain}" ${f:+"$f"} -A "$r" "$t_plain"
  done
done
check "-A twice" -A "$lo" -A ",$hi" "$t_plain"
check "--range=" "--range=$lo,$hi" "$t_plain"
xcheck "quoted as quoteaf quotes" "-A with an apostrophe" -A "1'x" "$t_plain"

# --- options ---------------------------------------------------------------------
check "no argument at all"
check "-x alone" -x
for o in -V --version; do
  xcheck "names SlateOS" "$o" "$o"
done
for o in -h --help --he -hV -x\ -h; do
  # shellcheck disable=SC2086
  check "$o" $o
done
check "-r" -r "$t_plain"
check "-r twice and -x" -r -x -r "$t_plain"
for o in -z --bogus -A --range --help=x -C -N --=x -XXX; do
  check "$o alone" "$o"
done
check "-XXX on a process" -XXX "$t_plain"
for pair in "-x -d" "-x -X" "-d -X" "-c -x" "-C f -d" "-n -x" "-N f -C g" "-n -N f"; do
  # shellcheck disable=SC2086
  check "mutually exclusive: $pair" $pair "$t_plain"
done
check "-n with -q" -n -q
check "-N with -p" -N f -p
check "-n with a process" -n "$t_plain"
check "-N with a process" -N f "$t_plain"

# --- rc files ----------------------------------------------------------------------
check "-n writes ~/.pmaprc" -n
check "-N writes the file named" -N made
RC_SET=1; RC='x'
check "-n when ~/.pmaprc is there" -n
check "-N when the file is there" -N rc
reset_knobs
NOHOME=1
check "-n with no HOME" -n
check "-c with no HOME" -c "$t_plain"
reset_knobs
check "-c with no ~/.pmaprc" -c "$t_plain"
check "-C a file that is not there" -C nothere "$t_plain"
check "-c and no process" -c
rc_cases=(
  $'[Fields Display]\nRss\nPss\nPerm\nMapping\n'
  $'[Fields Display]\nSize\n[Mapping]\nShowPath\n'
  $'[Fields Display]\nSize\nMapping\n[Mapping]\nShowPath\nShowPath\n'
  $'# nothing enabled\n[Fields Display]\n'
  $'Rss\n[Other]\nx\n[Broken\n[Fields Display] Size\nA B\nInode\nOffset\nDevice\nVmFlags\nMapping\n'
  $'  [Fields Display]  # c\n\tKernelPageSize\t#t\nRss extra\n'
  "[Fields Display]
$(printf 'x%.0s' $(seq 1100))
Rss
Mapping
"
  $'[Fields Display]\nRss'
)
i=0
for text in "${rc_cases[@]}"; do
  i=$((i+1))
  RC_SET=1; RC=$text
  check "rc $i through -c" -c "$t_plain"
  check "rc $i through -C" -C rc "$t_plain"
  check "rc $i through -c, quiet" -c -q "$t_shm"
  check "rc $i through -c, -p" -c -p "$t_odd"
  check "rc $i through -c, two processes" -c "$t_plain" "$t_deleted"
done
reset_knobs

# --- argv[0] -------------------------------------------------------------------
for a0 in /usr/local/bin/pmap ./pmap '' bin/ x; do
  ARGV0=$a0
  check "argv[0] '$a0': an unknown option" -z
  check "argv[0] '$a0': no argument"
  check "argv[0] '$a0': -n" -n
  check "argv[0] '$a0': -c with no rc" -c "$t_plain"
  check "argv[0] '$a0': a process" "$t_plain"
done
reset_knobs

# --- the order of the two streams, and where the output goes -------------------------
ERRTO='merge'
check "-r's warning comes first" -r "$t_plain"
check "a missing process between two" "$t_plain" "$p_none" "$t_shm"
RC_SET=1; RC=$'Rss
[Fields Display]
Rss
'
check "an rc warning before the map" -c "$t_plain"
unset RC_SET
reset_knobs
for o in full closed epipe; do
  OUTTO=$o
  check "stdout $o: a process" "$t_plain"
  check "stdout $o: -X" -X "$t_plain"
  check "stdout $o: --help" --help
  check "stdout $o: a missing process" "$p_none"
  check "stdout $o: init" 1
done
reset_knobs
ERRTO='full'
check "stderr full: -r" -r "$t_plain"
check "stderr full: a bad -A" -A zz "$t_plain"
ERRTO='closed'
check "stderr closed: no argument"
reset_knobs

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
