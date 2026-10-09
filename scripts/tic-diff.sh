#!/usr/bin/env bash
# Differential test: ncurses 6.4's tic, captoinfo, infotocap, infocmp and toe, ours against Ubuntu's.
#
# The terminfo compiler -- `tic`, with its aliases `captoinfo` and
# `infotocap` -- and the two tools that read what it writes. The reference
# is Ubuntu 24.04's 6.4+20240113, from `ncurses-bin`.
#
# ## What is compiled
#
# Upstream's own database source, `misc/terminfo.src` of the same release --
# taken from Ubuntu's orig tarball and checked against its SHA-256 -- whole:
# some 1800 entries, every kind of capability, long `use=` chains, and the
# warnings the release itself provokes under `-v`. Then the same as termcap,
# as the reference writes it, so that the termcap reader meets what it really
# meets. And small crafted sources for what the database never does: errors
# of every kind, duplicate names, unknown and extended capabilities, numbers
# past a `short`, and an entry for each of `tic -v`'s checks.
#
# ## What is compared
#
# Each side runs in the same directory, made fresh for it -- so that paths in
# messages agree -- with an environment of only `PATH`, a `HOME` inside that
# directory, and what the case gives. Compared: standard output, standard
# error, the exit status, and everything the run left in the directory: a
# compiled database file by file, its aliases as the symbolic links they are.
#
# ## Cases that differ on purpose
#
# `-V`, which names this build rather than the ncurses version (all three).
#
# And four where the reference dies -- crashes, or loops until it is
# killed -- and ours goes on. For these what is checked is that the
# reference did die, and that all it wrote before it did is what ours wrote
# as far as it went (`xfail_dies`):
#
# - A `use=` cycle: the reference loops for ever; ours says which `use=`
#   could not be resolved, and fails.
# - With `-v`, an `sgr` that `tparm` refuses (one with no parameters): the
#   reference `strdup`s the null pointer and dies of it; ours goes on as the
#   code means to, "sgr(0) did not return a value".
# - With `-C -r -v`, the termcap check expands each capability as `tparm`
#   would be asked to, and where `tparm` refuses -- a capability upstream's
#   tables say takes parameters but that has none (`u7=\E[6n`), or a string
#   parameter in its termcap spelling -- `strdup`s the null pointer too.
#   That is nearly half of upstream's own database: 834 of its 1828 entries,
#   each compiled alone, kill it. So `-C -r -v` is compared in full on the
#   rest, which the reference survives (`crv_survivors`).
# - With `-C -r -v`, a delay with no closing `>`: the reference loops for
#   ever.
#
# `SWEEP=0` leaves out `infocmp` over every entry of the compiled database.
set -u

DIFF_PROG='tic'
DIFF_BINS='tic infocmp toe'
DIFF_NEED='timeout curl sha256sum tar diff cmp stat'
# The database compiled some sixty times a side, and infocmp over all of it.
DIFF_TIMEOUT=${DIFF_TIMEOUT:-14400}
DIFF_FORWARD='SWEEP'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
fix=$DIFF_TMP/fix
run=$DIFF_TMP/run
mkdir -p "$work" "$fix"
case_no=0

# `captoinfo` and `infotocap` are `tic` by other names.
for side in ours gnu; do
  for a in captoinfo infotocap; do
    ln -s "$(readlink -f "$bindir/$side/tic")" "$bindir/$side/$a"
  done
done

# --- the database source ------------------------------------------------------------
src_cache=$HOME/.cache/slateos-ncurses-src
tarball=ncurses_6.4+20240113.orig.tar.gz
tarball_sha256=37a12a0f8ae2605012c9a164dd286b0cfa02b51b5055836d09eb3d597fc351b1
mkdir -p "$src_cache"
if [ ! -f "$src_cache/$tarball" ]; then
  if ! curl -fsSL -o "$src_cache/$tarball.part" \
      "http://archive.ubuntu.com/ubuntu/pool/main/n/ncurses/$tarball"; then
    echo "tic-diff: cannot fetch the reference's source; skipped"
    exit 0
  fi
  mv "$src_cache/$tarball.part" "$src_cache/$tarball"
fi
if ! printf '%s  %s\n' "$tarball_sha256" "$src_cache/$tarball" | sha256sum -c --quiet - >/dev/null 2>&1; then
  echo "tic-diff: $src_cache/$tarball is not the reference's source" >&2
  exit 1
fi
tar -xzf "$src_cache/$tarball" -C "$DIFF_TMP" ncurses-6.4-20240113/misc/terminfo.src
mv "$DIFF_TMP/ncurses-6.4-20240113/misc/terminfo.src" "$fix/terminfo.src"

# --- the case runner ----------------------------------------------------------------
ENVS=()             # the environment, beyond PATH and HOME
STDIN=/dev/null     # what standard input is
TMO=900             # seconds a side may take
reset_knobs() { ENVS=(); STDIN=/dev/null; TMO=900; }

# $1 = side, $2 = where its results go; the rest is the program and its argv.
# Both sides run in `$run`, made afresh, with the fixtures at `src`.
run_side() {
  local side=$1 p=$2; shift 2
  rm -rf "$run"
  mkdir -p "$run/home"
  ln -s "$fix" "$run/src"
  (cd "$run" && diff_run timeout -k 5 "$TMO" env -i "PATH=$bindir/$side" "HOME=$run/home" \
      "${ENVS[@]}" "$@" <"$STDIN" >"$run/.stdout" 2>"$run/.stderr")
  echo $? >"$run/.rc"
  rm -f "$run/src"
  rm -rf "$p.$side"
  mv "$run" "$p.$side"
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="$*"
  [ ${#ENVS[@]} -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ "$STDIN" != /dev/null ] && LABEL="$LABEL [stdin ${STDIN##*/}]"
  reset_knobs
  local o_rc g_rc
  o_rc=$(cat "$p.ours/.rc"); g_rc=$(cat "$p.gnu/.rc")
  case "$o_rc" in
    124|127) AGREED=broken
      REPORT="  ours rc=$o_rc  reference rc=$g_rc"
      return 0 ;;
  esac
  if diff -r --no-dereference "$p.ours" "$p.gnu" >"$p.diff" 2>&1; then
    AGREED=yes
    rm -rf "$p.ours" "$p.gnu" "$p.diff"
  else
    AGREED=no
    REPORT=$(printf '  ours rc=%s, reference rc=%s\n%s' "$o_rc" "$g_rc" \
      "$(sed -e "s|$work/||g" "$p.diff" | head -"${DIFFLINES:-40}")")
  fi
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- ours never finished\n%s\n' "$LABEL" "$REPORT"
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

xfail_case() {
  local why=$1; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s\n%s\n' "$LABEL" "$REPORT"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# Whether file $1 is where file $2 begins.
is_prefix() {
  cmp -s -n "$(stat -c %s "$1")" "$1" "$2"
}

# A case on which the reference dies -- crashes, or loops until `timeout`
# kills it -- and ours does not. It must have died, and what it wrote before
# it did must be what ours wrote, as far as it went: its standard error
# whole (unbuffered, less the line `timeout` adds to it when it reaps a
# crash), its standard output as much as its buffer had flushed.
xfail_dies() {
  local why=$1; shift
  compare "$@"
  local p=$work/c$case_no
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
    return 0
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s\n%s\n' "$LABEL" "$REPORT"
    return 0
  fi
  local g_rc
  g_rc=$(cat "$p.gnu/.rc")
  grep -v '^timeout: ' "$p.gnu/.stderr" >"$p.gnu.stderr"
  if [ "$g_rc" -ge 124 ] && is_prefix "$p.gnu/.stdout" "$p.ours/.stdout" \
     && is_prefix "$p.gnu.stderr" "$p.ours/.stderr"; then
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
    rm -rf "$p.ours" "$p.gnu" "$p.diff" "$p.gnu.stderr"
  else
    fail=$((fail + 1))
    printf 'DIFF %s  (the reference should die here, %s, and agree until it does)\n%s\n' \
      "$LABEL" "$why" "$REPORT"
  fi
  return 0
}

# The entries of the source $1 (in `$fix`) that the reference's `tic -C -r -v`
# survives with and without `-x`, each compiled alone with its `use=` from
# the database in `$fix/db`, written to $2 in their order.
crv_survivors() {
  local parts=$DIFF_TMP/parts i n opts ok
  rm -rf "$parts"
  mkdir -p "$parts"
  awk -v dir="$parts" '/^[^ \t#]/ { n++ } n > 0 { print > (dir "/" n ".src") }' "$fix/$1"
  n=$(find "$parts" -name '*.src' | wc -l)
  : >"$fix/$2"
  for i in $(seq 1 "$n"); do
    ok=yes
    for opts in -Crv -Crvx -CKrv; do
      { (cd "$fix" && timeout 20 env -i "PATH=$bindir/gnu" "HOME=$fix" TERMINFO=db \
          tic "$opts" "$parts/$i.src" >/dev/null 2>&1); } 2>/dev/null
      [ $? -ge 124 ] && ok=no
    done
    [ "$ok" = yes ] && cat "$parts/$i.src" >>"$fix/$2"
  done
  rm -rf "$parts"
}

# --- the crafted sources --------------------------------------------------------------
# Every check `tic -v` makes, an entry each (or near enough).
cat >"$fix/checks.src" <<'TI'
# comment at column 0
zz-keys|conflicting keys,
	kbs=\E[3~, kdch1=\E[3~, kend=\EOF, kll=\EOF, kf1=\EOP, kf13=\EOP, kf25=\EOP,
	kDC5=\E[3;5~, kDN2=\E[1;2B, kUP=\E[1;2A, kLFT9x=\E[1;9D, kRIT17=\E[1;17C,
	kNXT=\E[6;2~, kHOM1=\E[1;1H,
zz-params|parameter counts,
	cup=\E[%p1%d;%p3%dH, cub=\E[%p2%dD, hpa=\E[%dG, vpa=\E[%p1%d%p2%dd,
	setaf=\E[%?%p1%{8}%<%t3%p1%d%e38;5%;m, setab=\E[%p0%dm, ech=\E[%p1%d%, il=\E[%pxL,
	u6=\E[%i%d;%dR, u7=\E[6n, u8=\E[?%[;0123456789]c, u9=\E[c, smgrp=\E[%p1%dX,
	colors#8, pairs#64,
zz-delays|delays,
	bel=^G$<5>, clear=\E[H\E[J$<50*>, cuf1=\E[C$<2*>, el=\E[K$<3/>, kf2=\EOQ$<1>,
	flash=\E[?5h\E[?5l, smso=\E[7m$<5x>, rmso=\E[m$<5*/>, ind=\n$<2.5*>,
	rmcup=\E[?1049l$<abc>, smcup=$<10>\E[?1049h,
zz-delays2|flash with an embedded delay,
	flash=\E[?5h$<100/>\E[?5l, xon,
zz-acs|alternate characters,
	enacs=\E(B\E)0, smacs=^N, rmacs=^O, acsc=``aaffggjjkkllmmnnooqqssttuuvvwwxxyyzz{{||}}~~I,
zz-acs2|alternate characters again,
	smacs=\E(0, rmacs=^O, acsc=llmmkkjjI,
zz-acs3|odd acsc,
	acsc=lmk,
zz-colors|colors,
	colors#8, pairs#0, setf=\E[3%p1%dm, setaf=\E[3%p1%dm, setb=\E[4%p1%dm,
	setab=\E[4%?%p1%{1}%=%t4%e%p1%{3}%=%t6%e%p1%{4}%=%t1%e%p1%{6}%=%t3%e%p1%d%;m,
	ccc, scp=\E[%p1%dP, RGB=8/8/x,
zz-colors2|more colors,
	colors#16, pairs#8, initp=\E]%p1%d;%p2%d;%p3%d;%p4%d;%p5%d;%p6%d;%p7%d\E\\,
	op=\E[39;49m, setaf=\E[38;5;%p1%dm, setab=\E[48;5;%p1%dm, ncv#3,
zz-cursor|cursor movement,
	cud1=^J, cuu1=\E[A, cub1=^H, cuf1=\E[C, cud=\E[%p1%dB, cuu=\E[%p1%dA,
	cub=\E[%p1%dD, cuf=\E[%p1%dX, il=\E[%p1%dL, dl1=\E[M, home=\E[H,
zz-cursor2|cursor movement again,
	cud1=\E[B, cuu1=\E[B, cub1=\E[1D, cuf1=\EC, cup=\E[%i%p1%d;%p2%dH,
zz-cursor3|only some movement,
	cuu1=\E[A, cud=\E[%p1%dB,
zz-hc|hard copy,
	hc, cup=\E=%p1%c%p2%c, home=\E[H,
zz-gn|generic,
	gn, cuu1=\E[A,
zz-keypad|keypad,
	ka1=\EOw, ka3=\EOu, kb2=\EOy, kc1=\EOq, kc3=\EOs, kich1=\E[2~,
zz-keypad2|keypad in the legacy order,
	ka1=\EOq, ka3=\EOs, kb2=\EOr, kc1=\EOp, kc3=\EOn,
zz-keypad3|half a keypad,
	ka1=\EOw, kb2=\EOu,
zz-sgr|sgr against the attributes,
	sgr=\E[0%?%p1%p6%|%t;1%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p5%t;2%;m%?%p9%t\016%e\017%;,
	sgr0=\E[0m\017, smso=\E[7m, smul=\E[4m, rev=\E[7m, blink=\E[6m, dim=\E[2m,
	bold=\E[1m, invis=\E[8m, smacs=^N, rmacs=^O, ritm=\E[0m\017, rmso=\E[0m,
zz-nosgr|attributes without sgr,
	sgr0=\E[m, smso=\E[7m, rmso=\E[27m, smul=\E[4m, rmul=\E[24m, sitm=\E[3m,
zz-sgrpad|sgr with delays,
	sgr=\E[0%?%p1%t;7%;m$<2>, sgr0=\E[0m$<2>, smso=\E[7m$<2>,
zz-pairs|unpaired capabilities,
	smir=\E[4h, ich1=\E[@, smcup=\E[?1049h, rmdc=\E[l, csr=\E[%i%p1%d;%p2%dr,
	tbc=\E[3g, cvvis=\E[?25h, cnorm=\E[?25h, civis=\E[?25l, smam=\E[?7h, mgc=\E[?69l,
	prot=\E[8m, rev=\E[7m, dsl=\E]2;\007, smxon=\E[?66h, rin=\E[%p1%dT, ich=\E[%p1%d@,
	smm=\E[?1034h, mc5=\E[5i, sc=\E7, pfkey=\E%p1%d;%p2%s, pln=\E%p1%s,
zz-screen|screen.xterm-like,
	XT, XM, kmous=\E[M, colors#8, op=\E[39;49m, bce,
screen.zz|screen.zz-x,
	XT, kmous=\E[<, colors#8, tsl=\E]0;, op=\E[49;39m,
screen|screen itself,
	XT, kmous=\E[M,
zz-mouse|a mouse without XT,
	kmous=\E[M,
zz-u|user capabilities,
	u6=\E[%i%d;%dR, u7=\E[6n, u8=\E[?1;2c, u9=\E[c, NQ,
zz-ansi|ansi without u6,
	cup=\E[%i%p1%d;%p2%dH, cuu1=\E[A, cud1=^J, cuf1=\E[C, cub1=^H, ed=\E[J,
	el1=\E[1K, el=\E[K, csr=\E[%i%p1%d;%p2%dr, ind=\ED, ri=\EM,
zz-ext|extended capabilities of every type,
	XT, AX, Tc, U8#1, Ss=\E[%p1%d q, Se=\E[2 q, Cs=\E]12;%p1%s\007,
	Ms=\E]52;%p1%s;%p2%s\007, Smulx=\E[4:%p1%dm, kxIN=\E[I, E3=\E[3J, xm=\E[<%p1%d;%p2%d;%p3%dM,
	TS=\E]2;, Cr=\E]112\007, bogus#3, bogus2=\E[%p1%p2%p3%dz, kUP3=\E[1;3A, Sync=\EP=%p1%ds\E\\,
	am#1, cup, xenl=\E[x, smul@, rmul@,
TI

# Errors, one source each.
printf 'x|y z,\n\tcols#8x,\n' >"$fix/col.src"
printf 'aa|first entry,\n\tam, use=bb,\nbb|second entry,\n\tbw, use=aa,\n' >"$fix/cycle.src"
printf 'aa|xx|first,\n\tam,\nbb|xx|second,\n\tbw,\ncc|third,\n\tuse=xx,\n' >"$fix/dup.src"
printf 'aa|first,\n\tam,\nbb|second,\n\tbw,\ncc|aa|third,\n\tuse=bb,\n' >"$fix/dup2.src"
printf 'aa|first entry,\n\tam, use=aa,\n' >"$fix/self.src"
printf 'x|xx|test terminal,\n\tXT, U8#1,\n' >"$fix/unk.src"
printf 'vt|vt52 test:\\\n\t:cm=Y%%+ %%+ :up=5*A:\n' >"$fix/cap1.src"
printf 'x1|test sgr without params,\n\tsgr=\\E[0m, sgr0=\\E[m, cup=\\E[%%i%%p1%%d;%%p2%%dH,\nx2|without sgr0,\n\tsgr=\\E[0m, cup=\\E[%%i%%p1%%d;%%p2%%dH,\n' >"$fix/nosgrparm.src"
printf 'x3|unterminated delay,\n\tbel=a$<5x>, cup=\\E[%%i%%p1%%d;%%p2%%dH,\n' >"$fix/baddelay.src"
printf 'x|y,\n\tam,\001bw,\n' >"$fix/ctrl.src"
printf 'x|y,\n\tam,\000bw,\n' >"$fix/nul.src"
printf 'x|y,\r\n\tam, cols#80,\r\n\tbel=^G,\r\n' >"$fix/crlf.src"
: >"$fix/empty.src"
printf '# nothing but comments\n\n# here\n' >"$fix/comments.src"
printf 'x|y,\n\tam,\n  # indented comment\n\tbw,\n' >"$fix/indent.src"
printf -- '-x|starts with a dash,\n\tam,\n' >"$fix/dash.src"
printf 'x|y,\n\tam\n\tbw,\n' >"$fix/nocomma.src"
printf 'x|y,\n\tcols#99999, lines#40000, it#0x7fff, colors#0x8000, pairs#-1, xmc#017,\n' >"$fix/numbers.src"
printf 'x|y,\n\tuse=no-such-terminal-anywhere,\n' >"$fix/nouse.src"
printf 'x|y,\n\tam, use=vt100, use=xterm, use=vt100,\n' >"$fix/sysuse.src"
printf 'x|y,\n\tbel=\\0\\00\\000\\200\\s\\l\\e\\^\\,\\:\\a\\b\\f\\n\\r\\t\\101\\0101^@^?^a,\n' >"$fix/escapes.src"
printf 'x|y,\n\tam, am, cols#80, cols#81, bel=^G, bel=^H, am@, \n' >"$fix/repeat.src"
printf 'x|y,\n\tnonesuch, alsonot#3, northis=foo,\n' >"$fix/unknown.src"
printf '%s|y,\n\tam,\n' "$(head -c 600 /dev/zero | tr '\0' n)" >"$fix/longname.src"
{
  for i in $(seq 0 40); do printf 'big%s|dupname|big entry %s,\n\tam, cols#%s,\n' "$i" "$i" "$i"; done
} >"$fix/bigdup.src"
# termcap-only forms.
cat >"$fix/termcap.tc" <<'TC'
# termcap names and their quirks
vt52x|vt52 again:\
	:am:bs:co#80:li#24:cl=\EH\EJ:cm=\EY%+ %+ :nd=\EC:up=\EA:\
	:ho=\EH:ku=\EA:kd=\EB:kr=\EC:kl=\ED:kb=^H:ko=bt,nd,up:ml=\El:ma=^Kk^Jj:\
	:ML=\E[s:MT:MR=\E[r:dc=\EP:dm=\EA:ed=\EB:ei=\EC:pc=\200:\
	:cs=\E[%i%d;%dr:rp=%.\E[%p2%db:DO=\E[%dB:%1=\EOP:
adm3|adm3 test:\
	:cm=\E=%+ %+ :sf=^J:up=^K:nd=^L:bc=^H:ho=^^:cl=^Z:co#80:li#24:\
	:al=1*\E[L:dl=3.5*\E[M:ce=5\E[K:\
	:cm=%r%2%3%.%>xy%B%D%n%m%a+c %ap1%a-c\001%s:
bad|termcap with errors:\
	:co#80:li:cm=%Q:xx=\E[?:tc=adm3:
TC

# ---------------------------------------------------------------------------
# The cases
# ---------------------------------------------------------------------------
SRC=src/terminfo.src
# The database the reference compiles from it, for what reads one: `use=`
# resolved from disk, `infocmp -A`, `toe`.
(cd "$fix" && "$bindir/gnu/tic" -x -o db terminfo.src >/dev/null 2>&1)
crv_survivors terminfo.src terminfo-crv.src
crv_survivors checks.src checks-crv.src
echo "tic -C -r -v survives $(grep -c '^[^[:space:]#]' "$fix/terminfo-crv.src") of $(grep -c '^[^[:space:]#]' "$fix/terminfo.src") entries, and $(grep -c '^[^[:space:]#]' "$fix/checks-crv.src") of $(grep -c '^[^[:space:]#]' "$fix/checks.src") crafted ones"

# --- compiling the database ---------------------------------------------------------
run_case tic -o db "$SRC"
for opts in -x -s -sx -U -a -T -K -N '-x -T' -1 -f; do
  # shellcheck disable=SC2086  # the words are the case
  run_case tic $opts -o db "$SRC"
done
run_case tic -o db -e xterm,vt100,linux,screen "$SRC"
run_case tic -s -o db -e 'xterm, vt100 ,,linux' "$SRC"
run_case tic -o db -e xterm-kitty -x "$SRC"
printf 'xterm-256color\n  vt220  \n\nansi\n' >"$fix/names"
run_case tic -o db -e src/names "$SRC"
run_case tic -s -o db -e ./src/names "$SRC"
run_case tic -o db -e no-such-name "$SRC"
ENVS=(TERMINFO=tdb); run_case tic "$SRC"
ENVS=(TERMINFO=tdb); run_case tic -s -e vt100 "$SRC"
run_case tic -s -e vt100 "$SRC"
run_case tic -o /proc/no/such/place "$SRC"
mkdir -p "$fix/ro"; chmod 555 "$fix/ro"
run_case tic -o src/ro/db -e vt100 "$SRC"

# --- checking, and tic -v's own checks ----------------------------------------------
for opts in -c -cv -cv2 -cv3 -cv9 -cx -cxv -cxv2 -cs -cU -cvU -cvx -v '-v -x'; do
  # shellcheck disable=SC2086
  run_case tic $opts -o db "$SRC"
done
for opts in -c -cv -cv2 -cvx -cv2x -Irv -Irvx; do
  # shellcheck disable=SC2086
  run_case tic $opts src/checks.src
done
run_case tic -v -o db src/checks.src
run_case tic -vx -o db src/checks.src
# The termcap check: in full where the reference survives it.
CRV_DIES="the reference dies of a null tparm answer in its termcap check"
xfail_dies "$CRV_DIES" tic -Crv src/checks.src
xfail_dies "$CRV_DIES" tic -Crvx src/checks.src
for opts in -Crv -Crvx -Crv2 -CKrv; do
  # shellcheck disable=SC2086
  ENVS=(TERMINFO=src/db); run_case tic $opts src/checks-crv.src
done

# --- translating ----------------------------------------------------------------------
for opts in -I -C -L -K -CK -Ir -Cr -Lr -Ix -Cx -Lx -I1 -C1 '-I -w 120' '-C -w 30' \
            '-I -W -w 40' '-C -W -w 50' -If -Cf -IG -Ig -IGx -I0 -C0 -It -Ct -Ia -Ca -IN -CN \
            -IU -CU -CT -CrT -Iq -Cq '-I -R SVr1' '-I -R HP' '-I -R AIX' '-I -R BSD' \
            '-C -R BSD' '-I -R Ultrix' '-I -R nonesuch' -Irx -Crx -CKr -I1x -L1 -Lf -Lq \
            '-I -e xterm,vt100' '-C -e xterm' '-Ir -e xterm-256color' -Iv -Cv -Irv; do
  # shellcheck disable=SC2086
  run_case tic $opts "$SRC"
done
xfail_dies "$CRV_DIES" tic -Crv "$SRC"
for opts in -Crv -Crvx -Crv2 -CKrv '-Crv -T' '-Crv -e xterm,vt100'; do
  # shellcheck disable=SC2086
  ENVS=(TERMINFO=src/db); run_case tic $opts src/terminfo-crv.src
done
for q in -Q -Q1 -Q2 -Q3 '-Q -x' '-Q2 -e xterm' '-Q -I'; do
  # shellcheck disable=SC2086
  run_case tic $q -e xterm,vt100,dumb "$SRC"
done

# --- the termcap reader -------------------------------------------------------------
(cd "$fix" && "$bindir/gnu/tic" -C terminfo.src >termcap.src 2>/dev/null)
(cd "$fix" && "$bindir/gnu/tic" -Cr terminfo.src >termcap-r.src 2>/dev/null)
for f in termcap.src termcap-r.src termcap.tc cap1.src; do
  for opts in -c -cv -I -Ir -C -Cr -L -IN -CK -Ix; do
    # shellcheck disable=SC2086
    run_case tic $opts "src/$f"
  done
  run_case tic -o db "src/$f"
  run_case tic -x -o db "src/$f"
done

# --- captoinfo and infotocap ----------------------------------------------------------
run_case captoinfo src/termcap.tc
run_case captoinfo -1 src/termcap.tc
run_case captoinfo -v src/termcap.tc
run_case captoinfo -w 40 src/cap1.src
run_case infotocap src/checks.src
run_case infotocap -r -e vt100 "$SRC"
ENVS=(TERM=vt52 'TERMCAP=vt52|vt52 by hand:am:co#80:li#24:cl=\EH\EJ:'); run_case captoinfo
ENVS=(TERM=vt52x "TERMCAP=$fix/termcap.tc"); run_case captoinfo
ENVS=(TERM=vt52x); run_case captoinfo
run_case captoinfo
ENVS=(TERM=x 'TERMCAP=x:am:'); run_case captoinfo -e x

# --- the crafted sources, and errors ----------------------------------------------------
for f in col dup dup2 self unk ctrl nul crlf empty comments indent dash nocomma numbers \
         nouse sysuse escapes repeat unknown longname bigdup; do
  run_case tic -o db "src/$f.src"
  run_case tic -x -o db "src/$f.src"
  run_case tic -I "src/$f.src"
  run_case tic -C "src/$f.src"
  run_case tic -cv "src/$f.src"
done
TMO=20; xfail_dies "the reference loops for ever on a use= cycle" tic -o db src/cycle.src
TMO=20; xfail_dies "the reference loops for ever on a use= cycle" tic -c src/cycle.src
run_case tic -I src/cycle.src
xfail_dies "the reference dies of a null sgr(0)" tic -cv src/nosgrparm.src
run_case tic -c src/nosgrparm.src
TMO=20; xfail_dies "the reference loops for ever on a delay with no '>'" tic -Crv src/baddelay.src
run_case tic -cv src/baddelay.src
run_case tic /usr/share/terminfo/x/xterm
run_case tic -I /usr/share/terminfo/v/vt100
run_case tic src/no-such-file
run_case tic src
run_case tic
run_case tic -I
run_case tic a b
run_case tic -z "$SRC"
run_case tic -w "$SRC"
run_case tic -w x "$SRC"
run_case tic -v x "$SRC"
run_case tic -R "$SRC"
run_case tic -D
run_case tic -D -o /tmp/x
ENVS=(TERMINFO=/nonexistent/dir 'TERMINFO_DIRS=/usr/share/terminfo:/x'); run_case tic -D
STDIN=$fix/checks.src; run_case tic -I -
STDIN=$fix/checks.src; run_case tic -cv -
STDIN=$fix/termcap.tc; run_case tic -C -
xfail_case "our version string, not ncurses'" tic -V

# --- infocmp ------------------------------------------------------------------------
for t in xterm xterm-256color vt100 vt52 dumb linux screen tmux-256color ansi rxvt putty sun \
         cons25 vt220 xterm-kitty; do
  for opts in "" "-1" "-x" "-C" "-L" "-I" "-r -C" "-K" "-G" "-g" "-q" "-sd" "-sc" "-1 -f" \
              "-w 100" "-0" "-W -w 40" "-i" "-e" "-E" "-t -C" "-R SVr1" "-R HP" "-R AIX" \
              "-Q 1" "-Q 2" "-1 -x -f" "-C -x" "-T -C" "-D"; do
    # shellcheck disable=SC2086
    run_case infocmp $opts "$t"
  done
  run_case infocmp -A src/db -x "$t"
done
run_case infocmp xterm vt100
run_case infocmp -x xterm xterm-256color
run_case infocmp -c xterm vt100
run_case infocmp -n xterm vt100
run_case infocmp -u xterm-256color xterm
run_case infocmp -x -u xterm-256color xterm
run_case infocmp -C -u xterm-256color xterm
run_case infocmp -q -d xterm vt100
run_case infocmp -p -d xterm vt100
run_case infocmp -c xterm vt100 vt220
run_case infocmp -n xterm vt100 vt220
run_case infocmp no-such-terminal
run_case infocmp -z xterm
run_case infocmp a b c
run_case infocmp -s z xterm
run_case infocmp -w x xterm
run_case infocmp -A /usr/share/terminfo xterm
run_case infocmp -A /nonexistent xterm
run_case infocmp -B /usr/share/terminfo xterm vt100
run_case infocmp -A src/db -B /usr/share/terminfo -d xterm xterm
run_case infocmp -A src/db -B /usr/share/terminfo -dx xterm-kitty xterm-kitty
run_case infocmp -F src/terminfo.src src/terminfo.src
run_case infocmp -F src/checks.src src/terminfo.src
ENVS=(TERM=xterm-256color); run_case infocmp
ENVS=(TERM=xterm-256color); run_case infocmp -x
ENVS=(TERMINFO=src/db); run_case infocmp -x xterm-kitty
ENVS=(TERMINFO=src/db TERM=screen); run_case infocmp
xfail_case "our version string, not ncurses'" infocmp -V

# --- toe ------------------------------------------------------------------------------
# A database with what a real one should not have: an alias (listed under its
# entry), a file that is no entry, a subdirectory that is a file, an empty
# one, an entry in the wrong subdirectory, and one that cannot be read.
odd=$fix/odd
mkdir -p "$odd/x" "$odd/z" "$odd/w" "$odd/q"
cp /usr/share/terminfo/x/xterm "$odd/x/xterm"
ln -s xterm "$odd/x/xalias"
printf 'not an entry\n' >"$odd/x/xbad"
cp /usr/share/terminfo/v/vt100 "$odd/w/vt100"
cp /usr/share/terminfo/x/xterm-256color "$odd/w/xterm"
printf 'y\n' >"$odd/y"
cp /usr/share/terminfo/d/dumb "$odd/q/dumb"
chmod 300 "$odd/q"
for opts in '' -a -h -ah -s -as -ash -v -v3 -123 -hv2; do
  # shellcheck disable=SC2086
  run_case toe $opts
done
run_case toe /usr/share/terminfo
run_case toe -h /usr/share/terminfo /etc/terminfo
run_case toe -s /usr/share/terminfo /etc/terminfo /lib/terminfo
run_case toe -s "$fix/db" /usr/share/terminfo
run_case toe -sh "$fix/db" "$fix/db"
run_case toe -s "$odd" "$fix/db" "$odd"
run_case toe "$odd"
run_case toe -s "$odd"
run_case toe src/db
run_case toe -h src/db /usr/share/terminfo
run_case toe -s src/db src/odd
run_case toe /nonexistent "$odd"
run_case toe "$SRC" /usr/share/terminfo
for f in terminfo.src checks.src cycle.src dup.src self.src termcap.tc; do
  run_case toe -u "src/$f"
  run_case toe -U "src/$f"
done
run_case toe -u src/checks.src -U src/terminfo.src
run_case toe -U src/checks.src -u src/terminfo.src
run_case toe -u /nonexistent
run_case toe -u src/db
run_case toe -u src/nul.src
run_case toe -u src/col.src
run_case toe -u
run_case toe -z
run_case toe -- -h
ENVS=(TERMINFO=src/db); run_case toe
ENVS=(TERMINFO=src/db); run_case toe -a
ENVS=("TERMINFO_DIRS=$fix/db:/usr/share/terminfo:$odd"); run_case toe -as
ENVS=("TERMINFO_DIRS=hex:00"); run_case toe -a
ENVS=(TERMINFO=hex:00); run_case toe
xfail_case "our version string, not ncurses'" toe -V
chmod 700 "$odd/q"
if [ "${SWEEP:-1}" != 0 ]; then
  find "$fix/db" -type f -printf '%f\n' | sort -u >"$fix/all-names"
  while read -r t; do
    for opts in "-1x" "-Cr" "-Ix -A src/db"; do
      # shellcheck disable=SC2086
      DIFFLINES=12 run_case infocmp $opts "$t"
    done
  done <"$fix/all-names"
fi

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
