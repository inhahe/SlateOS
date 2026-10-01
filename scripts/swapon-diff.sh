#!/usr/bin/env bash
# Differential test: our `swapon` and `swapoff` against util-linux 2.39.3's.
#
# An ordinary user cannot enable or disable swap, so the system calls fail
# on both sides alike (EPERM); what is compared is everything around them:
#
#   * `--show` and `-s` on swap tables of the harness's own (LIBMOUNT_SWAPS)
#     naming swap files it made with mkswap -- so UUID and LABEL are probed
#     -- and this machine's own /proc/swaps;
#   * the checks swapon makes before the call, on files of its own: the
#     permission and owner warnings, holes, a missing signature, the page
#     size a header was made for (and --fixpgsz re-making it with mkswap),
#     and old software-suspend data (which swapon overwrites) -- each run on
#     its own copy, the copies' bytes compared afterwards;
#   * -a on fstabs of its own (-T and LIBMOUNT_FSTAB): noauto, nofail,
#     pri=, discard=, specs that do not resolve, areas already active;
#   * swapoff by path, -L, -U and -a, which ends at "Not superuser.";
#   * options and their refusals.
set -u

DIFF_PROG='swapon'
DIFF_PKG='swapon'
DIFF_BINS='swapon swapoff'
DIFF_NEED='timeout mkswap sha256sum'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "swapon-diff: refusing to run as root: swap would really be enabled." >&2
  exit 2
fi

pass=0; fail=0
fx=$DIFF_TMP/fx
mkdir -p "$fx"
cd "$fx" >/dev/null || exit 1

# mkswap is run by swapon --fixpgsz, so both sides need it on their PATH.
for side in ours gnu; do
  ln -sf "$(command -v mkswap)" "$bindir/$side/mkswap"
done

# --- fixtures ----------------------------------------------------------------------------
mk() { # name size_kib [mkswap args...]
  local name=$1 kib=$2; shift 2
  head -c $((kib * 1024)) /dev/zero > "$name"
  [ $# -gt 0 ] && mkswap -q "$@" "$name" >/dev/null
  chmod 600 "$name"
}
mk plain.swap 1024 -L lab1 -U 11111111-2222-3333-4444-555555555555
mk nolabel.swap 1024 -U 66666666-7777-8888-9999-aaaaaaaaaaaa
mk pg8k.swap 1024 -p 8192 -L big -U 12345678-1234-1234-1234-123456789abc
mk pg64k.swap 2048 -p 65536
mk empty.swap 1024
cp plain.swap loose.swap; chmod 644 loose.swap
cp plain.swap susp.swap
printf 'S1SUSPEND' | dd of=susp.swap bs=1 seek=4086 conv=notrunc status=none
cp plain.swap hib.swap
printf 'LINHIB0001' | dd of=hib.swap bs=1 seek=4086 conv=notrunc status=none
cp plain.swap short.swap; truncate -s 8192 short.swap
# A header whose last page is past the file's end.
cp plain.swap huge.swap; printf '\377\377\000\000' | dd of=huge.swap bs=1 seek=1028 conv=notrunc status=none
truncate -s 1M holes.swap; mkswap -q holes.swap >/dev/null; chmod 600 holes.swap

cat > swaps.all <<EOF
Filename				Type		Size		Used		Priority
$fx/plain.swap                            file		1020		0		-2
$fx/nolabel.swap                          file		1020		4		5
$fx/pg8k.swap                             file		1016		0		-3
$fx/empty.swap                            file		1024		0		-4
/dev/sdc                                partition	8388604		12345678	-5
$fx/a-very-long-name-that-is-longer-than-forty-bytes.swap file 123 0 1
/nonexistent/area                       file		99999999	10000000	-1
EOF
: > swaps.none
printf 'Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n' > swaps.header
printf 'Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\ngarbage\n%s/plain.swap file 1020 0 -2\n' "$fx" > swaps.bad

cat > fstab.mixed <<EOF
# swap areas and others
$fx/plain.swap   none  swap  sw,pri=3                0 0
$fx/pg8k.swap    none  swap  defaults,discard=pages  0 0
$fx/nolabel.swap none  swap  noauto                  0 0
LABEL=lab1       none  swap  sw                      0 0
UUID=00000000-0000-0000-0000-000000000000 none swap nofail 0 0
/nonexistent/x   none  swap  nofail,pri=x            0 0
/nonexistent/y   none  swap  discard=once            0 0
/dev/sda         /     ext4  defaults                0 1
EOF
printf '%s none swap sw 0 0\n' "$fx/plain.swap" > fstab.active

# --- one case: each side in its own copy of the fixtures ------------------------------------
run_side() {
  local side=$1 prog=$2; shift 2
  local d=$DIFF_TMP/run-$side
  rm -rf "$d"; mkdir -p "$d"
  cp -a "$fx/." "$d/"
  # Paths in the tables name the shared fixtures; the copies are what a
  # case that writes is given as operands (so each side changes its own).
  (cd "$d" && diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 ${SWAPS:+LIBMOUNT_SWAPS=$SWAPS} \
    ${FSTAB:+LIBMOUNT_FSTAB=$FSTAB} PATH="$bindir/$side" "$prog" "$@")
}

state() {
  local d=$DIFF_TMP/run-$1 f
  for f in "$d"/*.swap; do
    [ -f "$f" ] && printf '%s %s\n' "$(basename "$f")" "$(sha256sum < "$f" | cut -c1-16)"
  done
}

compare() {
  local prog=$1; shift
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc
  run_side ours "$prog" "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  state ours >>"$o_out"
  run_side gnu "$prog" "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  state gnu >>"$g_out"
  # The copies live in different directories; their names are the same.
  sed -i "s|$DIFF_TMP/run-ours|RUN|g" "$o_out" "$o_err"
  sed -i "s|$DIFF_TMP/run-gnu|RUN|g" "$g_out" "$g_err"
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s %s\n' "$prog" "$*"
  else
    fail=$((fail + 1))
    printf 'DIFF %s %s  [SWAPS=%s FSTAB=%s]\n  ours rc=%s, gnu rc=%s\n' "$prog" "$*" "${SWAPS:-}" "${FSTAB:-}" "$o_rc" "$g_rc"
    diff -u "$g_out" "$o_out" | sed -n '3,14p' | sed 's/^/  out /'
    diff -u "$g_err" "$o_err" | sed -n '3,10p' | sed 's/^/  err /'
  fi
  return 0
}
on() { compare swapon "$@"; }
off() { compare swapoff "$@"; }

# --- options and refusals -------------------------------------------------------------------
for p in on off; do
  $p -h; $p --help; $p -V; $p --version; $p --bogus; $p -x
done
on -p; on -p x plain.swap; on -p 40000 plain.swap; on -p -1 -v plain.swap
on -d=bogus plain.swap; on --discard=bogus plain.swap
on -e plain.swap
on -a -o sw; on -s --show; on -a --raw; on --bytes -o pri=1
on --show=NAME,BOGUS; on --show=; on --show=NAME,
off; off -L; off -U

# --- --show and -s ----------------------------------------------------------------------------
for s in "$fx/swaps.all" "$fx/swaps.none" "$fx/swaps.header" "$fx/swaps.bad" /nonexistent/swaps ""; do
  for args in '' '--show' '--show=NAME,TYPE,SIZE,USED,PRIO,UUID,LABEL' '--output-all' \
              '--show --bytes' '--show --raw' '--show --noheadings' '--show --raw --noheadings --bytes' \
              '--show=LABEL,uuid,name' '-s' '--summary' '-s -v'; do
    # shellcheck disable=SC2086  # word-splitting is the point
    SWAPS=$s on $args
  done
done

# --- the checks before swapon(2) --------------------------------------------------------------
SWAPS=$fx/swaps.none
for f in plain nolabel pg8k pg64k empty loose susp hib short huge holes; do
  on "$f.swap"
  on -v "$f.swap"
done
on -f pg8k.swap
on -f -v pg64k.swap
on -p 7 -d plain.swap
on -d=once -v plain.swap
on --discard=pages plain.swap
on -o pri=3,discard plain.swap
on nonexistent.swap
on -L lab1
on -U 11111111-2222-3333-4444-555555555555
on LABEL=lab1
on plain.swap nolabel.swap
SWAPS=

# --- -a on fstabs of the harness's own ---------------------------------------------------------
for s in "$fx/swaps.none" "$fx/swaps.all"; do
  SWAPS=$s on -a -T "$fx/fstab.mixed"
  SWAPS=$s on -a -v -T "$fx/fstab.mixed"
  SWAPS=$s on -a -e -v -T "$fx/fstab.mixed"
  SWAPS=$s FSTAB=$fx/fstab.mixed on -a -v
  SWAPS=$s on -a -v -T "$fx/fstab.active"
  SWAPS=$s on -a -T /nonexistent/fstab
done
SWAPS=

# --- swapoff -----------------------------------------------------------------------------------
for s in "$fx/swaps.all" "$fx/swaps.none"; do
  SWAPS=$s off plain.swap
  SWAPS=$s off -v plain.swap
  SWAPS=$s off nonexistent.swap
  SWAPS=$s off -L lab1
  SWAPS=$s off -U 66666666-7777-8888-9999-aaaaaaaaaaaa
  SWAPS=$s off LABEL=lab1
  SWAPS=$s off LABEL=nosuch
  SWAPS=$s off -a
  SWAPS=$s off -a -v
  SWAPS=$s FSTAB=$fx/fstab.mixed off -a -v
done
SWAPS=

echo "swapon-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
