#!/usr/bin/env bash
# Differential test: our `column` against util-linux 2.39.3's.
#
# What is compared: stdout, stderr and the exit status of each case, run by
# each side in turn:
#
#   * the list modes -- columns filled down (default) and across (-x), at
#     many output widths, with wide characters, blank lines (-L), a last line
#     without its newline, and so narrow that it falls back to one a line;
#   * the table -- the default greedy separators and -s's exact ones, -o,
#     -N, -d, -m, -l, -e with LINES, and every column-list option (-R -T -W
#     -E -H) in each form a list takes: numbers, names, ranges, negative
#     ranges counting from the end, `0`, `-`, and the forms nothing checks
#     (`5x-3`); --table-order, including upstream's own list surgery when a
#     column is named twice in a row (`-O 1,1`);
#   * --table-column properties: abbreviations, `noextremes` (upstream sets
#     STRICTWIDTH), json types, quoted names, and `width=`, which libsmartcols
#     refuses whenever `errno` is set -- so each case is run where upstream's
#     errno is set (a pipe, a UTF-8 locale) and where it is clear (COLUMNS or
#     LINES in the environment, -c on a terminal, the C locale on one);
#   * --json and --tree, with duplicate IDs and a parent loop;
#   * input as bytes: NUL, CR, invalid UTF-8 and a literal `\x` (`\x5c`), and
#     the C locale, in which every byte above 0x7f is invalid;
#   * the exit status as upstream computes it: missing files counted, and a
#     printed table's status replacing the count;
#   * refusals, and closed or full standard descriptors, with output small
#     and larger than glibc's buffer -- which for the list modes is a wide
#     stream's, counting characters.
#
# Some cases upstream never finishes: every column wrapped (`-W 0`) on a
# terminal too narrow for the table, where libsmartcols' size_t arithmetic
# wraps a width into the quintillions and pads it for ever; and a `width=`
# hint that C's undefined double-to-size_t conversion turns into a width near
# 2^64 (`-5` with -m, `1e19`). The port stops at zero in the first, and
# refuses the table in the second (the smartcols crate's docs, "Where it is
# not upstream's"). Such a case is counted apart, as `hung`: it is one only
# if upstream was the side the timeout killed and ours finished.
set -u

DIFF_PROG='column'
DIFF_PKG='column'
DIFF_NEED='timeout script stty'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; hung=0

in=$DIFF_TMP/in
mkdir -p "$in"

# --- inputs ---------------------------------------------------------------------
printf '%s\n' alpha beta gamma delta epsilon zeta eta theta iota kappa lambda \
  mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega \
  a-rather-long-entry-that-is-wide x yy zzz > "$in/list"
printf 'NAME SIZE TYPE MOUNT\nsda 10G disk\nsda1 1G part /boot\nsda2 9G part /\nsr0 1024M rom\n' > "$in/table"
printf 'a:b:c\n::x\nlong-field-value:y:\n:\n' > "$in/colon"
printf '  lead\ntrail  \n\n\t\ttabs\n   \n\x0b\x0c\ncr\r\nlast' > "$in/ws"
printf 'ab\0cd ef\ngh ij\n\0\nkl\n' > "$in/nul"
printf 'a\351b c\nx\\xy z\nok \\x line\n\342\202\n' > "$in/invalid"
printf '\344\270\200\344\272\214 \344\270\211\ncaf\303\251 na\303\257ve\ne\314\201 x\n\357\274\241 wide\n' > "$in/utf8"
printf '1 0 root\n2 1 child-a\n3 1 child-b\n4 2 grand\n5 9 orphan\n1 3 dup-id\n6 6 self\n' > "$in/tree"
printf 'a "quoted" b\\slash \001ctl\ntab\there x\n' > "$in/json"
: > "$in/empty"
printf 'one two three\nfour five' > "$in/nonl"
printf 'COMMAND PID USER DESCRIPTION\n' > "$in/wide"
for i in 1 2 3 4 5 6; do
  printf 'some-command-%s %s user%s a long description of what process %s is doing right now\n' \
    "$i" "$((i * 1111))" "$i" "$i" >> "$in/wide"
done
for i in $(seq 1 700); do printf 'entry%s\n' "$i"; done > "$in/big"
for i in $(seq 1 600); do printf 'r%s c%s %s\n' "$i" "$i" "$((i * 7))"; done > "$in/bigtable"
for i in $(seq 1 3000); do printf '\303\251\n'; done > "$in/bigwide"
for i in $(seq 1 20); do printf 'row%s val%s\n' "$i" "$i"; done > "$in/rows"

# $1 = side, rest = argv. stdin is /dev/null unless the caller redirects.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 column "$@"
}
# $1 = side, $2 = a bash script that runs `column` from that side's PATH.
run_side_sh() {
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$1:$PATH" timeout -k 2 20 bash -c "$2"
}
# $1 = side, $2 = columns, rest = argv: on a pty of that width, stdin empty.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60; column"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  diff_run env LC_ALL="${PTY_LOCALE:-C.UTF-8}" PATH="$bindir/$side:$PATH" \
    timeout -k 1 10 script -qec "$cmd" /dev/null </dev/null
}

judge() {
  local o_rc=$1 g_rc=$2
  if { [ "$g_rc" = 124 ] || [ "$g_rc" = 137 ]; } && [ "$o_rc" != 124 ] && [ "$o_rc" != 137 ]; then
    # Upstream never finished and ours did: the cases smartcols' docs list
    # under "Where it is not upstream's".
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
    "$o_rc" "$(head -c 1500 "$DIFF_TMP/o.out")" "$(head -c 600 "$DIFF_TMP/o.err")" \
    "$g_rc" "$(head -c 1500 "$DIFF_TMP/g.out")" "$(head -c 600 "$DIFF_TMP/g.err")")
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

# case_ ARGV...: stdin empty.
case_() {
  local o_rc g_rc
  run_side ours "$@" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "column $(printf '%q ' "$@")"
}
# in_case FILE ARGV...: FILE on stdin.
in_case() {
  local file=$1 o_rc g_rc; shift
  run_side ours "$@" <"$file" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" <"$file" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "column $(printf '%q ' "$@")< ${file##*/}"
}
# sh_case SCRIPT: a bash script calling `column`, run for each side.
sh_case() {
  local o_rc g_rc
  run_side_sh ours "$1" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_sh gnu "$1" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "bash -c $(printf '%q' "$1")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols ${PTY_LOCALE:-C.UTF-8}] column $(printf '%q ' "$@")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs column, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 20 \
    bash -c "exec column \"\$@\" $how" column "$@" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 20 \
    bash -c "exec column \"\$@\" $how" column "$@" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "column $(printf '%q ' "$@")$how"
}

# --- options and their refusals -------------------------------------------------
case_ -h
case_ --help
case_ -V
case_ --version
case_ --vers
case_ --bogus
case_ -z
case_ --t
case_ --table-c
case_ --table-co "$in/table"
case_ --table-column name=a -t "$in/table"
case_ --table-columns a -t "$in/table"
case_ --table-columns-l 2 -t "$in/table"
case_ --table-n x -t "$in/table"
case_ --table-no -t "$in/table"
case_ --tree-
case_ --o x
case_ --output
case_ --col 20 "$in/list"
case_ --columns 20 "$in/list"
case_ --table-empty-lines -t "$in/ws"
case_ --keep "$in/ws"
case_ -c
case_ -c x "$in/list"
case_ -c -1 "$in/list"
case_ -c -0 "$in/list"
case_ -c '' "$in/list"
case_ -c 99999999999 "$in/list"
case_ -c 4294967296 "$in/list"
case_ -c 4294967295 "$in/list"
case_ -c ' 20' "$in/list"
case_ -c 20x "$in/list"
case_ -l 0 -t "$in/table"
case_ -l x -t "$in/table"
case_ -l -1 -t "$in/table"
case_ -l 99999999999 -t "$in/table"
case_ -s "$(printf '\377')" -t "$in/table"
case_ -N a -C b -t "$in/table"
case_ -C b -N a -t "$in/table"
case_ -J -x "$in/table"
case_ -x -J "$in/table"
case_ -t -x "$in/table"
case_ -x -t "$in/table"
case_ -r NAME "$in/table"
case_ -r NAME -i ID "$in/table"
case_ -r NAME -p PARENT "$in/table"
case_ -O 1 "$in/table"
case_ -n x "$in/table"
case_ -W 1 "$in/table"
case_ -E 1 "$in/table"
case_ -R 1 "$in/table"
case_ -H 1 "$in/table"
case_ -T 1 "$in/table"
case_ -N a,b "$in/table"
case_ -C name=a "$in/table"
case_ -J "$in/table"
case_ -J -d "$in/table"
case_ -i ID -p P "$in/list"
case_ -d -m -e -L "$in/list"
case_ -l 2 "$in/list"
case_ -o '|' "$in/list"
case_ -s : "$in/colon"

# --- the list modes ---------------------------------------------------------------
for c in 1 8 9 16 20 30 40 41 80 132 unlimited 0; do
  case_ -c "$c" "$in/list"
  case_ -x -c "$c" "$in/list"
done
for f in utf8 ws nul invalid nonl empty big bigwide; do
  case_ -c 60 "$in/$f"
  case_ -x -c 60 "$in/$f"
  case_ -L -c 60 "$in/$f"
done
in_case "$in/list" -c 50
in_case "$in/list"
case_ "$in/list" "$in/utf8"
case_ -c 50 - "$in/utf8"
sh_case 'cd '"$(printf %q "$in")"' && COLUMNS=33 column list && COLUMNS=x column -x list && COLUMNS=0 column list'
sh_case 'cd '"$(printf %q "$in")"' && COLUMNS=99999999999 column list'

# --- the table ------------------------------------------------------------------
for f in table colon ws nul invalid utf8 nonl wide rows json; do
  case_ -t "$in/$f"
  case_ -t -s : "$in/$f"
  case_ -t -s ' :' -o '|' "$in/$f"
done
case_ -t -N A,B,C,D "$in/table"
case_ -t -N ,A,,B, "$in/table"
case_ -t -N '' "$in/table"
case_ -t -N , "$in/table"
case_ -t -N A,B -N X "$in/table"
case_ -t -d -N A,B,C,D "$in/table"
case_ -t -d "$in/table"
case_ -t -m -N A,B,C,D "$in/table"
case_ -t -m -c 60 -N A,B,C,D "$in/table"
case_ -t -m -c unlimited "$in/table"
case_ -t -l 1 "$in/table"
case_ -t -l 2 "$in/table"
case_ -t -l 2 -s : "$in/colon"
case_ -t -l 3 "$in/wide"
case_ -t -o '' "$in/table"
case_ -t -o ' | ' "$in/table"
case_ -t -o "$(printf '\303\251')" "$in/table"
case_ -t -s '' "$in/colon"
case_ -t -s "$(printf '\303\251')" "$in/utf8"
case_ -t -L "$in/ws"
case_ -t -L -N A,B "$in/ws"
case_ -t -c 80 "$in/wide"
case_ -t -c 60 "$in/wide"
case_ -t -c 40 "$in/wide"
case_ -t -c 20 "$in/wide"
case_ -t -c 1 "$in/wide"
case_ -t -c unlimited "$in/wide"
case_ -t -c 0 "$in/wide"
for w in 20 30 40 50 60 80; do
  case_ -t -c "$w" -N CMD,PID,USER,DESC -T DESC "$in/wide"
  case_ -t -c "$w" -N CMD,PID,USER,DESC -W DESC "$in/wide"
  case_ -t -c "$w" -N CMD,PID,USER,DESC -E DESC "$in/wide"
  case_ -t -c "$w" -N CMD,PID,USER,DESC -T 1,4 -R PID "$in/wide"
  case_ -t -c "$w" -N CMD,PID,USER,DESC -W 0 "$in/wide"
done
sh_case 'LINES=5 column -t -e -N A,B '"$(printf %q "$in/rows")"
sh_case 'LINES=5 column -t -e '"$(printf %q "$in/rows")"
sh_case 'LINES=5 column -t -e -c unlimited -N A,B '"$(printf %q "$in/rows")"
sh_case 'LINES=x column -t -e -N A,B '"$(printf %q "$in/rows")"

# Column lists, in each of their forms.
for list in 1 2 4 5 0 -1 -2 -3 -9 '-2--1' '-3--2' '-1--3' 1-2 2-3 3-1 1-9 0-2 \
            '5x-3' '1-2x' 2- 'a-b' 1,1 1,,2 ',' '' NAME SIZE,TYPE nosuch '-' '1,-' \
            '-,' ':2' '2:' '1:2' '99999999999' '4294967296' '4294967297-4294967298' \
            '-2147483648--2147483647' '1-99999999' '-99999--1'; do
  case_ -t -N NAME,SIZE,TYPE,MOUNT -R "$list" "$in/table"
  case_ -t -N NAME,SIZE,TYPE,MOUNT -H "$list" "$in/table"
done
case_ -t -H - "$in/table"
case_ -t -H 1,- -N NAME "$in/table"
case_ -t -H -2--1 -R -1 "$in/table"
case_ -t -H 4 -E -1 -c 20 -N A,B,C,D "$in/table"
case_ -t -O 4,3,2,1 "$in/table"
case_ -t -O 2 "$in/table"
case_ -t -O MOUNT,NAME -N NAME,SIZE,TYPE,MOUNT "$in/table"
case_ -t -O 1,1 -N A,B,C,D "$in/table"
case_ -t -O 2,1,1 -N A,B,C,D "$in/table"
case_ -t -O 3,3,3 -N A,B,C,D "$in/table"
case_ -t -O 1,1,2 -N A,B,C,D "$in/table"
case_ -t -O 1,1,3 -N A,B,C,D "$in/table"
case_ -t -O 2,1,2 -N A,B,C,D "$in/table"
case_ -t -O 4,4,1 -N A,B,C,D "$in/table"
case_ -t -O 0 "$in/table"
case_ -t -O X -N A,B,C,D "$in/table"
case_ -t -O -1 -N A,B,C,D "$in/table"
case_ -t -O -1,1 -H 4 -N A,B,C,D "$in/table"

# --- --table-column ---------------------------------------------------------------
for props in 'name=A,right' 'right,name=A' 'name=A,trunc' 'name=A,t' 'name=A,r' \
             'name=A,tre' 'name=A,tree' 'n=A,right' 'name=A,w=5' 'name=A,hidden' \
             'name=A,h' 'name=A,noextremes' 'name=A,strictwidth' 'name=A,wrap' \
             'name=A,truncate' 'name="A,B",right' 'name="open,right' ',,,name=A,,' \
             'name=A,color=red' 'name=A,color=nosuch,right' 'name=' '' 'right' \
             'name=A,json=number' 'name=A,width=5,right' 'name=A,right,width=5' \
             'name=A,width=0.5,right' 'name=A,width=abc,right' 'name=A,width=,right' \
             'name=A,width=' 'name=A,width=1e999,right' 'name=A,width=inf,right' \
             'name=A,width=nan,right' 'name=A,width=-5,right' 'name=A,width=0x10,right' \
             'name=A,width=10000000000000000000,right' 'name=A,wi=5,right'; do
  case_ -t -C "$props" -C name=B,right "$in/table"
  sh_case "COLUMNS=80 column -t -C $(printf %q "$props") -C name=B $(printf %q "$in/table")"
  sh_case "LINES=40 column -t -m -C $(printf %q "$props") -C name=B $(printf %q "$in/table")"
done
sh_case 'LINES=99999999999 column -t -C name=A,width=12,right -C name=B '"$(printf %q "$in/table")"
sh_case 'LINES=x column -t -C name=A,width=12,right -C name=B '"$(printf %q "$in/table")"
sh_case 'COLUMNS=80 column -t -C name=A,width=12,right -C name=B nosuch '"$(printf %q "$in/table")"
sh_case 'COLUMNS=80 column -t -C name=A,width=12,right -C name=B '"$(printf %q "$in/invalid")"
sh_case 'COLUMNS=80 column -t -L -C name=A,width=12,right -C name=B '"$(printf %q "$in/ws")"
sh_case 'LC_ALL=C COLUMNS=80 column -t -C name=A,width=12,right -C name=B '"$(printf %q "$in/table")"

# --- JSON -----------------------------------------------------------------------------
case_ -J -N A,B,C,D "$in/table"
case_ -J -N A,B,C,D -n disks "$in/table"
case_ -J -N A,B,C,D -n '' "$in/table"
case_ -J -N A,B "$in/table"
case_ -J -N A "$in/table"
case_ -J -N , "$in/table"
case_ -J -H - -N A,B "$in/table"
case_ -J -H 2 -N A,B,C,D "$in/table"
case_ -J -N A,B,C "$in/json"
case_ -J -s : -N A,B,C "$in/colon"
case_ -J -L -N A,B "$in/ws"
case_ -J -C name=A,json=number -C name=B,json=boolean -C name=C,json=array-string -C name=D "$in/table"
case_ -J -C name=A,json=num -C name=B,json=a -C name=C,json=bool -C name=D,json= "$in/table"
case_ -J -C name=A -C json=number "$in/table"
case_ -J -N A,B,C,D -O 4,1 "$in/table"
case_ -J -N ID,PARENT,NAME -r NAME -i ID -p PARENT "$in/tree"

# --- trees ---------------------------------------------------------------------------
for args in '-N ID,PARENT,NAME -r NAME -i ID -p PARENT' \
            '-N ID,PARENT,NAME -r 3 -i 1 -p 2' \
            '-N ID,PARENT,NAME -r ID -i ID -p PARENT' \
            '-N ID,PARENT,NAME -r NAME -i PARENT -p ID' \
            '-N ID,PARENT,NAME -r NAME -i ID -p PARENT -O NAME,ID' \
            '-N ID,PARENT,NAME -r NAME -i ID -p PARENT -H PARENT' \
            '-N ID,PARENT,NAME -r NAME -i ID -p PARENT -R -1' \
            '-N ID,PARENT,NAME -r X -i ID -p PARENT' \
            '-N ID,PARENT,NAME -r NAME -i X -p PARENT' \
            '-N ID,PARENT,NAME -r NAME -i ID -p X' \
            '-r 3 -i 1 -p 2' \
            '-C name=ID -C name=PARENT -C name=NAME,tree -i ID -p PARENT -r NAME' \
            '-C name=ID -C name=PARENT -C name=NAME,tree'; do
  # Word-split on purpose: each entry is an argument list.
  # shellcheck disable=SC2086
  case_ -t $args "$in/tree"
  # shellcheck disable=SC2086
  case_ -t -c 20 $args "$in/tree"
done
case_ -r 3 -i 1 -p 2 -s : "$in/colon"
case_ -r 1 -i 1 -p 2 -s : "$in/colon"

# --- the locale ---------------------------------------------------------------------
for loc in C POSIX; do
  sh_case "LC_ALL=$loc column -c 40 $(printf %q "$in/utf8")"
  sh_case "LC_ALL=$loc column -t $(printf %q "$in/utf8")"
  sh_case "LC_ALL=$loc column -t $(printf %q "$in/invalid")"
  sh_case "LC_ALL=$loc column -t -s \$(printf '\\303\\251') $(printf %q "$in/utf8")"
  sh_case "LC_ALL=$loc column -J -N A,B $(printf %q "$in/utf8")"
  sh_case "LC_ALL=$loc column -t -r 3 -i 1 -p 2 $(printf %q "$in/tree")"
done
sh_case "unset LC_ALL; LANG=C.UTF-8 column -t $(printf %q "$in/utf8")"

# --- files and the exit status ------------------------------------------------------
case_ nosuch
case_ nosuch nosuch2
case_ nosuch "$in/list"
case_ -t nosuch "$in/table"
case_ -t nosuch nosuch2
case_ -t nosuch "$in/empty"
case_ "$in/empty"
case_ "$in/empty" nosuch
case_ -t "$in/empty"
case_ ''
case_ "$in"
case_ -t "$in"
case_ "$in/list" "$in"
sh_case "column $(printf %q "$in/list") <&-"
sh_case "column <&-"
sh_case "column -t <&-"
sh_case "column -t -J -N A,B $(printf %q "$in/table") <&-"
in_case "$in/table" -t
in_case "$in/table" -t -
sh_case 'n=; for i in $(seq 1 256); do n="$n nosuch$i"; done; column $n 2>/dev/null; echo "rc=$?"'
sh_case 'n=; for i in $(seq 1 257); do n="$n nosuch$i"; done; column $n 2>/dev/null; echo "rc=$?"'

# --- terminals -------------------------------------------------------------------
for cols in 1 10 20 40 60 80 132; do
  pty_case "$cols" "$in/list"
  pty_case "$cols" -x "$in/list"
  pty_case "$cols" -t "$in/wide"
  pty_case "$cols" -t -N CMD,PID,USER,DESC -W DESC "$in/wide"
  pty_case "$cols" -t -c unlimited "$in/wide"
  pty_case "$cols" -t -e -N A,B "$in/rows"
done
for loc in C.UTF-8 C POSIX en_US.UTF-8; do
  PTY_LOCALE=$loc pty_case 80 -t -C name=A,width=12,right -C name=B "$in/table"
  PTY_LOCALE=$loc pty_case 80 -t -c 70 -C name=A,width=12,right -C name=B "$in/table"
  PTY_LOCALE=$loc pty_case 80 -t -l 9 -C name=A,width=12,right -C name=B "$in/table"
  PTY_LOCALE=$loc pty_case 80 -t -C name=A,width=0.3,right,trunc -C name=B -m "$in/wide"
done

# --- what the buffer loses, and closed or full descriptors -----------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
  redir_case "$how" nosuch
  redir_case "$how" "$in/list"
  redir_case "$how" "$in/big"
  redir_case "$how" -c 40 "$in/bigwide"
  redir_case "$how" -t "$in/table"
  redir_case "$how" -t "$in/bigtable"
  redir_case "$how" -J -N A,B,C "$in/bigtable"
  redir_case "$how" -t -O X -N A "$in/table"
done

printf '%d passed, %d differed, %d broken, %d where only upstream hung\n' \
  "$pass" "$fail" "$broken" "$hung"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
