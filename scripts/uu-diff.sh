#!/usr/bin/env bash
# Differential test: our `uuencode` and `uudecode` against GNU sharutils 4.15.2's.
#
# The reference is Ubuntu 24.04's build, unpacked by sharutils-ref.sh since
# the WSL image does not install it. Each case runs twice -- once per side --
# in a world rebuilt from nothing each time at the same path: a home
# directory (for `~/.sharrc`, `--save-opts` and `~/` names) and a working
# directory (for the files uudecode creates). Both sides are reached through
# one directory on PATH whose entries are swapped between the runs, so
# argv[0], libopts' `pathfind` of it and every message naming either are the
# same bytes on both sides.
#
# What is compared, per case: stdout, stderr, the exit status, and the world
# afterwards -- every file's type, mode and contents. Covered:
#
#   * encoding: every short-line length, a line of exactly 45 and the ones
#     around it, all 256 byte values, 200 KB; traditional and base64; the
#     mode from the file (set-id bits and all), from the umask for stdin;
#     `-e` names, empty and odd names;
#   * decoding: round trips both ways; `begin` lines of every shape
#     (`-base64`, `-encoded`, both, repeated, missing parts, odd spacing,
#     CRLF, NUL, an over-long line); traditional bodies that are short, lie
#     about their length, lack the `end` line or end it with CR; base64 bodies
#     with blank lines, CRLF, bad bytes, padding mid-line, and no `====`;
#     names that are `~/`, `~user/`, bad, trailing blanks, `-`, `/dev/stdout`,
#     encoded; `-o` in every position, `-c`, `POSIXLY_CORRECT`, several files,
#     missing files;
#   * libopts: flags, bundles, abbreviations, case and separators, optional
#     arguments swallowing the next word, every usage error, `--help`,
#     `--more-help` through PAGER, `--version` in each mode, `AUTOOPTS_USAGE`'s
#     computed layouts, `~/.sharrc` in each of its syntaxes and with its
#     documented oddities, `--load-opts`, `--no-load-opts`, `--save-opts`;
#   * descriptors: stdout full or closed, stdin closed, stderr full.
#
# Two things are normalised, and only where named: the date `--save-opts`
# writes, and in the three `--save-opts` warnings where upstream passes one
# argument to a two-`%s` format, whatever that second `%s` printed (a
# register's leftovers; the port prints nothing there).
set -u

DIFF_PROG='uu'
DIFF_PKG='uuencode uudecode'
DIFF_BINS='uuencode uudecode'
DIFF_NEED='python3 timeout cksum'
# The references are unpacked, and the PATH directory made, after the
# preamble's re-execs.
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"
# shellcheck source=sharutils-ref.sh
. "$(dirname "$0")/sharutils-ref.sh"

gnu_uuencode=$(sharutils_ref uuencode) || exit 0
gnu_uudecode=$(sharutils_ref uudecode) || exit 0
if [ -n "${OURS:-}" ]; then
  # A family harness takes a directory: `OURS=$HOME/.cache/slateos-sharutils/root/usr/bin`
  # runs the reference against itself, which must pass every case.
  our_uuencode=$OURS/uuencode
  our_uudecode=$OURS/uudecode
else
  our_uuencode=$(diff_ours uuencode)
  our_uudecode=$(diff_ours uudecode)
fi

pass=0; fail=0
W=$DIFF_TMP/w
RUN=$DIFF_TMP/run
IN=$DIFF_TMP/in
OUT=$DIFF_TMP/out
mkdir -p "$RUN" "$IN" "$OUT"

# --- inputs -----------------------------------------------------------------------
python3 - "$IN" <<'PY'
import os, random, sys
d = sys.argv[1]
r = random.Random(4242)
def w(name, data, mode=0o644):
    p = os.path.join(d, name)
    with open(p, 'wb') as f:
        f.write(data)
    os.chmod(p, mode)
w('empty', b'')
for n in (1, 2, 3, 4, 5, 44, 45, 46, 89, 90, 91, 135):
    w('n%d' % n, bytes(r.randrange(256) for _ in range(n)))
w('all256', bytes(range(256)))
w('zeros', bytes(100))
w('big', bytes(r.randrange(256) for _ in range(200000)))
w('text', b'hello world\n' * 50)
for mode in (0o600, 0o755, 0o4755, 0o2755, 0o1644, 0o777, 0o444):
    w('mode%o' % mode, b'mode test\n', mode)
w('unreadable', b'secret\n', 0)
PY

# Encoded fixtures, made by the reference encoder: what uudecode is fed.
for f in empty n1 n2 n3 n45 n46 n135 all256 zeros big text; do
  "$gnu_uuencode" "$IN/$f" /dev/stdout > "$IN/$f.uu"
  "$gnu_uuencode" -m "$IN/$f" /dev/stdout > "$IN/$f.b64"
  "$gnu_uuencode" "$IN/$f" "out-$f" > "$IN/$f.file.uu"
  "$gnu_uuencode" -m "$IN/$f" "out-$f" > "$IN/$f.file.b64"
done

# Hand-made inputs, where the shape is the point.
python3 - "$IN" <<'PY'
import os, sys
d = sys.argv[1]
def w(name, data):
    with open(os.path.join(d, name), 'wb') as f:
        f.write(data)
body = b'#0V%T\n`\nend\n'
b64 = b'Zm9v\n====\n'
w('h-plain', b'begin 644 /dev/stdout\n' + body)
w('h-junk-before', b'From: x\n\nsome text\n  begin 644 x\nbegin 644 /dev/stdout\n' + body)
w('h-tab', b'begin\t644 /dev/stdout\n' + body)
w('h-spaces', b'begin   0644    /dev/stdout\n' + body)
w('h-crlf', b'begin 644 /dev/stdout\r\n' + body)
w('h-no-name', b'begin 644\n' + body)
w('h-blank-name', b'begin 644 \n' + body)
w('h-no-mode', b'begin  /dev/stdout\n' + body)
w('h-bad-mode', b'begin 9 x\n' + body)
w('h-neg-mode', b'begin -644 neg\n' + body)
w('h-big-mode', b'begin 7777777777777777777777 big\n' + body)
w('h-mode-digits', b'begin 678 x\n' + body)
w('h-beginx', b'beginning of the story\n' + body)
w('h-begin-only', b'begin\n' + body)
w('h-b64', b'begin-base64 644 /dev/stdout\n' + b64)
w('h-b64-b64', b'begin-base64-base64 644 /dev/stdout\n' + b64)
w('h-enc', b'begin-encoded 644 ' + b'L2Rldi9zdGRvdXQ=' + b'\n' + body)
w('h-b64-enc', b'begin-base64-encoded 644 L2Rldi9zdGRvdXQ=\n' + b64)
w('h-enc-b64', b'begin-encoded-base64 644 L2Rldi9zdGRvdXQ=\n' + b64)
w('h-enc-enc', b'begin-encoded-encoded 644 eA==\n' + body)
w('h-enc-bad', b'begin-encoded 644 !!!!\n' + body)
w('h-enc-short', b'begin-encoded 644 eA\n' + body)
w('h-enc-nul', b'begin-encoded 644 YQBi\n' + body)
w('h-dash-x', b'begin-x 644 x\n' + body)
w('h-nul', b'junk\x00line\nbegin 644 /dev/stdout\n' + body)
w('h-nul-begin', b'begin 644 a\x00b\n' + body)
w('h-long', b'x' * 20000 + b'\nbegin 644 /dev/stdout\n' + body)
w('h-no-nl', b'begin 644 /dev/stdout')
w('h-none', b'no begin line here\n')
# Traditional bodies.
w('u-no-end', b'begin 644 /dev/stdout\n#0V%T\n`\n')
w('u-end-cr', b'begin 644 /dev/stdout\n#0V%T\n`\nend\r\n')
w('u-end-late', b'begin 644 /dev/stdout\n#0V%T\n`\n\nend\n')
w('u-end-nonl', b'begin 644 /dev/stdout\n#0V%T\n`\nend')
w('u-end-junk', b'begin 644 /dev/stdout\n#0V%T\n`\nendless\n')
w('u-space-zero', b'begin 644 /dev/stdout\n#0V%T\n \nend\n')
w('u-short', b'begin 644 /dev/stdout\n' + b'M' + b'86)C' * 15 + b'\n#0V\nend\n')
w('u-lie', b'begin 644 /dev/stdout\nM86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C86)C\n&0V%T\n`\nend\n')
w('u-crlf', b'begin 644 /dev/stdout\r\n#0V%T\r\n`\r\nend\r\n')
w('u-trunc', b'begin 644 /dev/stdout\n#0V%T\n')
w('u-long', b'begin 644 /dev/stdout\n' + b'M' + b'86)C' * 5000 + b'\n`\nend\n')
w('u-highbit', b'begin 644 /dev/stdout\n#\xe0\xf1\xa2\xb3\n`\nend\n')
# Base64 bodies.
w('b-blank', b'begin-base64 644 /dev/stdout\nZm9v\n\n====\n')
w('b-crlf', b'begin-base64 644 /dev/stdout\r\nZm9v\r\nYmFy\r\n====\r\n')
w('b-bad', b'begin-base64 644 /dev/stdout\nZm9v!\nYmFy\n====\n')
w('b-bad-last', b'begin-base64 644 /dev/stdout\nZm9v!\n====\n')
w('b-midpad', b'begin-base64 644 /dev/stdout\nZg==Zg==\n====\n')
w('b-split', b'begin-base64 644 /dev/stdout\nZm\n9vYmFy\n====\n')
w('b-trunc', b'begin-base64 644 /dev/stdout\nZm9v\n')
w('b-trunc-file', b'begin-base64 644 made\nZm9v\n')
w('b-end-junk', b'begin-base64 644 /dev/stdout\nZm9v\n====junk\n')
w('b-short-eq', b'begin-base64 644 /dev/stdout\nZm9v\n===\n====\n')
w('b-partial', b'begin-base64 644 /dev/stdout\nZm9vY\n====\n')
# Names.
for name, hdr in (
    ('n-tilde', b'~/tfile'), ('n-tilde-user', b'~nosuchuser_xyz/f'),
    ('n-tilde-alone', b'~'), ('n-tilde-nouser', b'~nosuchuser_xyz'),
    ('n-tilde-root', b'~root/nope'), ('n-trailing', b'trail   \t'),
    ('n-dash', b'-'), ('n-dir', b'.'), ('n-nodir', b'no/such/dir/f'),
    ('n-space', b'with space'), ('n-bytes', b'\xff\xfe'),
    ('n-mode0', None), ('n-mode4755', None), ('n-link', b'link'),
):
    if hdr is None:
        continue
    w(name, b'begin 644 ' + hdr + b'\n' + body)
w('n-mode0', b'begin 0 m0\n' + body)
w('n-mode4755', b'begin 4755 m4755\n' + body)
w('n-mode777', b'begin 777 m777\n' + body)
# `~user` with no slash: upstream's scan for the slash runs on through the
# header buffer, where an earlier, longer line left one.
w('n-scan-nouser', b'x' * 40 + b'/tail-of-junk\n' + b'begin 644 ~nosuchuser_xyz\n' + body)
w('n-scan-root', b'x' * 40 + b'/tail-of-junk\n' + b'begin 644 ~root\n' + body)
# Two encodings in one file: only the first is decoded.
w('two', b'begin 644 /dev/stdout\n' + body + b'begin 644 second\n' + body)
PY
chmod 000 "$IN/unreadable"

# --- running a case ---------------------------------------------------------------
# A case is `check LABEL COMMAND ARGS...`, shaped by variables reset after it:
C_SETUP=      # a function run in the fresh world before the command
C_ENV=()      # extra environment
C_IN=         # stdin: a file, 'closed', or empty for /dev/null
C_OUT=        # stdout: empty for a file, 'full' or 'closed'
C_ERR=        # stderr: empty for a file, or 'full'
C_UMASK=022
C_NOHOME=     # set: no HOME at all
C_NORM=       # 'savewarn': see the header

reset_case() {
  C_SETUP=; C_ENV=(); C_IN=; C_OUT=; C_ERR=; C_UMASK=022; C_NOHOME=; C_NORM=
}

# Point the names on PATH at one side.
point() {
  if [ "$1" = ours ]; then
    ln -sfn "$our_uuencode" "$RUN/uuencode"
    ln -sfn "$our_uudecode" "$RUN/uudecode"
    ln -sfn "$our_uuencode" "$RUN/uue"
  else
    ln -sfn "$gnu_uuencode" "$RUN/uuencode"
    ln -sfn "$gnu_uudecode" "$RUN/uudecode"
    ln -sfn "$gnu_uuencode" "$RUN/uue"
  fi
}

# One side of a case, in a world rebuilt from nothing.
run_side() {
  local side=$1; shift
  local o=$OUT/$side
  point "$side"
  chmod -R u+rwx "$W" 2>/dev/null
  rm -rf "$W" "$o"
  mkdir -p "$W/home" "$W/wd" "$W/tmp" "$o"
  if [ -n "$C_SETUP" ]; then
    ( cd "$W" && "$C_SETUP" ) || { echo "uu-diff: setup $C_SETUP failed" >&2; return 1; }
  fi
  local envs=(PATH="$RUN:/usr/bin:/bin" LC_ALL=C.UTF-8 TZ=UTC TMPDIR="$W/tmp")
  [ -z "$C_NOHOME" ] && envs+=(HOME="$W/home")
  envs+=("${C_ENV[@]}")
  (
    cd "$W/wd" || exit 125
    case $C_IN in
      closed) exec 0<&- ;;
      '') exec 0</dev/null ;;
      *) exec 0<"$C_IN" ;;
    esac
    case $C_OUT in
      full) exec 1>/dev/full ;;
      closed) exec 1>&- ;;
      *) exec 1>"$o/stdout" ;;
    esac
    case $C_ERR in
      full) exec 2>/dev/full ;;
      *) exec 2>"$o/stderr" ;;
    esac
    # After the redirections, so the harness can read what it created.
    umask "$C_UMASK"
    exec timeout 30 env -i "${envs[@]}" "$@"
  )
  echo $? > "$o/rc"
  [ -f "$o/stdout" ] || : > "$o/stdout"
  [ -f "$o/stderr" ] || : > "$o/stderr"
  chmod -R u+r "$W" 2>/dev/null
  ( cd "$W" && find . -mindepth 1 -printf '%y %m %p -> %l\n' | LC_ALL=C sort ) > "$o/world"
  # Contents, with the date --save-opts writes on its third line blanked.
  ( cd "$W" && find . -type f | LC_ALL=C sort | while IFS= read -r f; do
      printf '%s ' "$f"
      sed '2{/^#  preset\/initialization file$/{n;s/^#  .*$/#  DATE/}}' "$f" | cksum
    done ) > "$o/contents" 2>&1
}

norm_err() {
  case $C_NORM in
    savewarn) sed 's/cannot save options - .* not regular file$/cannot save options - ? not regular file/' "$1" ;;
    *) cat "$1" ;;
  esac
}

check() {
  local label=$1; shift
  run_side ours "$@" || { fail=$((fail + 1)); reset_case; return; }
  run_side gnu "$@" || { fail=$((fail + 1)); reset_case; return; }
  local what=
  cmp -s "$OUT/ours/stdout" "$OUT/gnu/stdout" || what="$what stdout"
  norm_err "$OUT/ours/stderr" > "$OUT/ours/stderr.n"
  norm_err "$OUT/gnu/stderr" > "$OUT/gnu/stderr.n"
  cmp -s "$OUT/ours/stderr.n" "$OUT/gnu/stderr.n" || what="$what stderr"
  cmp -s "$OUT/ours/rc" "$OUT/gnu/rc" || what="$what status"
  cmp -s "$OUT/ours/world" "$OUT/gnu/world" || what="$what files"
  cmp -s "$OUT/ours/contents" "$OUT/gnu/contents" || what="$what contents"
  if [ -z "$what" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL $label:$what"
    for part in stderr.n rc world contents; do
      if ! cmp -s "$OUT/ours/$part" "$OUT/gnu/$part"; then
        diff -u --label "gnu/$part" --label "ours/$part" "$OUT/gnu/$part" "$OUT/ours/$part" | head -20 | sed 's/^/    /'
      fi
    done
    if ! cmp -s "$OUT/ours/stdout" "$OUT/gnu/stdout"; then
      echo "    stdout: gnu $(wc -c < "$OUT/gnu/stdout") bytes, ours $(wc -c < "$OUT/ours/stdout") bytes"
      diff <(head -c 600 "$OUT/gnu/stdout" | od -c | head -12) <(head -c 600 "$OUT/ours/stdout" | od -c | head -12) | head -12 | sed 's/^/    /'
    fi
  fi
  reset_case
}

# --- uuencode ---------------------------------------------------------------------
for f in empty n1 n2 n3 n4 n5 n44 n45 n46 n89 n90 n91 n135 all256 zeros big text; do
  C_IN=$IN/$f; check "uuencode <$f" uuencode name
  C_IN=$IN/$f; check "uuencode -m <$f" uuencode -m name
  check "uuencode $f" uuencode "$IN/$f" name
  check "uuencode -m $f" uuencode --base64 "$IN/$f" name
done
for m in mode600 mode755 mode4755 mode2755 mode1644 mode777 mode444; do
  check "mode of $m" uuencode "$IN/$m" x
done
check "unreadable input" uuencode "$IN/unreadable" x
check "missing input" uuencode "$IN/nosuch" x
check "directory input" uuencode "$IN" x
check "input named -" uuencode - x
for u in 000 022 077 027 0777 0666; do
  C_UMASK=$u C_IN=$IN/n3; check "stdin mode, umask $u" uuencode x
done
C_IN=$IN/n3; check "-e" uuencode -e 'a name'
C_IN=$IN/n3; check "-me" uuencode -me 'a name'
C_IN=$IN/n3; check "-e empty name" uuencode -e ''
C_IN=$IN/n3; check "empty name" uuencode ''
C_IN=$IN/n3; check "odd bytes in the name" uuencode "$(printf 'x\377\ny')"
C_IN=$IN/n3; check "-e odd bytes" uuencode -e "$(printf 'x\377\ny')"
C_IN=$IN/text; check "/dev/stdout name" uuencode /dev/stdout
check "no operands" uuencode
check "three operands" uuencode a b c
C_IN=closed; check "stdin closed" uuencode x
C_IN=$IN/n3 C_OUT=full; check "stdout full, small" uuencode x
C_IN=$IN/big C_OUT=full; check "stdout full, large" uuencode x
C_IN=$IN/big C_OUT=full; check "stdout full, large, -m" uuencode -m x
C_IN=$IN/n3 C_OUT=closed; check "stdout closed" uuencode x
C_IN=$IN/big C_OUT=closed; check "stdout closed, large" uuencode x
C_ERR=full; check "stderr full, usage error" uuencode
C_ERR=full; check "stderr full, bad option" uuencode -z x

# --- options, both programs -------------------------------------------------------
for p in uuencode uudecode; do
  check "$p -z" "$p" -z
  check "$p --bogus" "$p" --bogus
  check "$p --b" "$p" --b
  check "$p --=x" "$p" --=x
  check "$p long name" "$p" "--$(printf 'x%.0s' $(seq 130))"
  check "$p --help" "$p" --help
  check "$p -h" "$p" -h
  check "$p --HE" "$p" --HE
  check "$p --help=x" "$p" --help=x
  check "$p -h -z" "$p" -h -z
  check "$p -z -h" "$p" -z -h
  check "$p --version" "$p" --version
  check "$p -v" "$p" -v
  for v in v c n V C N x ''; do
    check "$p --version=$v" "$p" "--version=$v"
  done
  check "$p -vc" "$p" -vc
  check "$p -v c" "$p" -v c
  check "$p -v file" "$p" -v file
  check "$p -v -h" "$p" -v -h
  C_ENV=(PAGER=cat); check "$p --more-help" "$p" --more-help
  C_ENV=(PAGER=cat); check "$p -!" "$p" '-!'
  C_ENV=(PAGER='cat -n'); check "$p --more-help, cat -n" "$p" --more-help
  C_ENV=(PAGER=cat TMPDIR=/nonexistent); check "$p --more-help, no TMPDIR" "$p" --more-help
  check "$p -r" "$p" -r
  check "$p --load-opts" "$p" --load-opts
  check "$p --load-opts missing" "$p" --load-opts=/nonexistent/x
  check "$p --load-opts directory" "$p" --load-opts=/
  check "$p --no-load-opts=x" "$p" --no-load-opts=x
  check "$p --no-l" "$p" --no-l -h
  check "$p -R -R" "$p" -R -R
  for au in compute gnu,compute autoopts,compute 'compute, misuse_usage' 'compute,no-misuse-usage' 'gnu,autoopts,compute' bogus autoopts 'COMPUTE'; do
    C_ENV=(AUTOOPTS_USAGE="$au"); check "$p --help, AUTOOPTS_USAGE=$au" "$p" --help
    C_ENV=(AUTOOPTS_USAGE="$au"); check "$p -z, AUTOOPTS_USAGE=$au" "$p" -z
    C_ENV=(AUTOOPTS_USAGE="$au"); check "$p --version, AUTOOPTS_USAGE=$au" "$p" --version
  done
  C_ENV=(AUTOOPTS_USAGE=compute); check "$p --help under another name" uue --help
done
C_IN=$IN/n3; check "--ba abbreviation" uuencode --ba x
C_IN=$IN/n3; check "--BASE64" uuencode --BASE64 x
C_IN=$IN/n3; check "--encode_file_name" uuencode --encode_file_name x
C_IN=$IN/n3; check "--encode^file^name" uuencode '--encode^file^name' x
C_IN=$IN/n3; check "-m -m" uuencode -m -m x
C_IN=$IN/n3; check "--base64=x" uuencode --base64=x x
C_IN=$IN/n3; check "-- then -m" uuencode -- -m
C_IN=$IN/n3; check "operand then -m" uuencode x -m
C_IN=$IN/n3; check "-me bundle" uuencode -em x

# --- ~/.sharrc --------------------------------------------------------------------
rc_case() {  # label rc-text command...
  local label=$1 text=$2; shift 2
  printf '%s' "$text" > "$DIFF_TMP/rc-text"
  C_SETUP=setup_rc
  check "rc: $label" "$@"
}
setup_rc() { cp "$DIFF_TMP/rc-text" home/.sharrc; }
for text in 'base64
' 'base64' 'm
' 'base64 extra
' 'BASE64
' 'base_64
' '# comment
base64
' '[UUENCODE]
base64
' '[UUDECODE]
base64
' '[UUDECODE]
encode-file-name
[UUENCODE]
base64
' '<?program uuencode>
base64
' '<?program uudecode>
encode-file-name
<?program uuencode>
base64
' '<base64/>
' '<base64 cooked/>
' '<base64>x</base64>
' '<!-- c -->
base64
' 'help
base64
' 'version
' 'save-opts x
' 'encode-file-name: yes
' 'encode-file-name=
base64
' 'base64 \
encode-file-name
' '<?auto-options compute>
bogus
'; do
  C_IN=$IN/n3; rc_case "$(printf '%s' "$text" | head -c 40 | tr '\n' '|')" uuencode x
done
C_IN=$IN/n3; rc_case "no-load-opts in the file" 'no-load-opts
base64
' uuencode x
C_IN=$IN/n3; rc_case "rc ignored with --no-load-opts" 'base64
' uuencode --no-load-opts x
setup_nested() {
  printf 'base64\n' > inner
  printf 'load-opts %s/inner\n' "$PWD" > home/.sharrc
}
C_IN=$IN/n3 C_SETUP=setup_nested; check "rc: nested load-opts" uuencode x
C_IN=$IN/n3 C_SETUP=setup_nested; check "rc: nested load-opts, then -m" uuencode -m x
setup_home_file() { printf 'base64\n' > home/file; }
C_IN=$IN/n3 C_SETUP=setup_home_file C_ENV=(HOME=/nonexistent); check "HOME missing" uuencode x
C_IN=$IN/n3 C_NOHOME=1; check "HOME unset" uuencode x
setup_rc_is_home() { printf 'base64\n' > rcfile; }
C_IN=$IN/n3 C_SETUP=setup_rc_is_home C_NOHOME=1 C_ENV=(HOME="$DIFF_TMP/w/rcfile"); check "HOME names a file" uuencode x

# rc for uudecode: -o's procedure runs from the file.
setup_rc_output() { printf 'output-file from-rc\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_output; check "rc: output-file" uudecode
setup_rc_output_empty() { printf 'output-file\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_output_empty; check "rc: empty output-file" uudecode
setup_rc_xml() { printf '<output-file>Xfoo</output-file>\n<ignore-chmod/>\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_xml; check "rc: xml value" uudecode
setup_rc_xml_nl() { printf '<output-file>\nbar\n</output-file>\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_xml_nl; check "rc: xml value on its own line" uudecode
setup_rc_quoted() { printf 'output-file "q\\tz"\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_quoted; check "rc: quoted value" uudecode
setup_rc_cont() { printf 'output-file = a\\\nignore-chmod\n' > home/.sharrc; }
C_IN=$IN/n3.uu C_SETUP=setup_rc_cont; check "rc: backslash at the end of a line" uudecode

# --- --save-opts ------------------------------------------------------------------
check "save to a file" uuencode -m -R saved x
check "save, both options" uuencode -me --save-opts=saved x
check "save to the home directory" uuencode -m -R
check "save to a directory" uuencode -m --save-opts=.
check "save, -R swallows the next word" uuencode -R saved x
setup_old_save() { printf 'old\n' > wd/saved; ln -s saved wd/link; }
C_SETUP=setup_old_save; check "save over an old file" uuencode -m -R saved
C_SETUP=setup_old_save; check "save through a symlink" uuencode -m -R link
check "save uudecode" uudecode -c -o "$(printf 'a\nb')" -R saved
check "save to /dev/null" uuencode -R /dev/null
C_NORM=savewarn; check "save to a missing directory" uuencode -m -R /nonexistent/x
setup_ro() { mkdir wd/ro; chmod 555 wd/ro; }
C_NORM=savewarn C_SETUP=setup_ro; check "save into a read-only directory" uuencode -m -R ro/f
C_NORM=savewarn C_NOHOME=1; check "save with HOME unset" uuencode -m -R
setup_rc_m() { printf 'base64\n' > home/.sharrc; }
C_SETUP=setup_rc_m; check "save what the rc set" uuencode -e -R saved

# --- uudecode ---------------------------------------------------------------------
for f in empty n1 n2 n3 n45 n46 n135 all256 zeros big text; do
  C_IN=$IN/$f.uu; check "uudecode <$f.uu" uudecode
  C_IN=$IN/$f.b64; check "uudecode <$f.b64" uudecode
  check "uudecode $f.file.uu" uudecode "$IN/$f.file.uu"
  check "uudecode $f.file.b64" uudecode "$IN/$f.file.b64"
done
for f in "$IN"/h-* "$IN"/u-* "$IN"/b-* "$IN"/n-* "$IN"/two; do
  C_IN=$f; check "uudecode <$(basename "$f")" uudecode
done
setup_home_dir() { :; }
C_IN=$IN/n-tilde C_SETUP=setup_home_dir; check "name under HOME" uudecode
C_IN=$IN/n-tilde C_NOHOME=1; check "name under HOME, HOME unset" uudecode
setup_link() { printf 'old\n' > wd/target; ln -s target wd/link; }
C_IN=$IN/n-link C_SETUP=setup_link; check "name is a symlink" uudecode
setup_dangling() { ln -s nowhere/x wd/link; }
C_IN=$IN/n-link C_SETUP=setup_dangling; check "name is a dangling symlink" uudecode
setup_existing() { printf 'old contents\n' > wd/out-n3; chmod 600 wd/out-n3; }
C_SETUP=setup_existing; check "output exists" uudecode "$IN/n3.file.uu"
check "-o file" uudecode -o made "$IN/n3.file.uu"
check "-o - " uudecode -o - "$IN/n3.file.uu"
check "-o /dev/stdout" uudecode -o /dev/stdout "$IN/n3.file.b64"
check "-o empty" uudecode -o '' "$IN/n3.file.uu"
check "-o, two files" uudecode -o made "$IN/n3.file.uu" "$IN/n1.file.uu"
check "-o twice" uudecode -o a -o b "$IN/n3.file.uu"
check "-o /dev/null" uudecode -o /dev/null "$IN/n3.file.uu"
check "-o /dev/null -c" uudecode -c -o /dev/null "$IN/n3.file.uu"
C_ENV=(POSIXLY_CORRECT=1); check "-o /dev/null, POSIXLY_CORRECT" uudecode -o /dev/null "$IN/n3.file.uu"
check "-o missing directory" uudecode -o no/such/f "$IN/n3.file.uu"
check "two files" uudecode "$IN/n3.file.uu" "$IN/n1.file.b64"
check "two files to stdout" uudecode "$IN/n3.uu" "$IN/n1.b64"
check "a file, then stdout" uudecode "$IN/n3.file.uu" "$IN/n1.uu"
check "missing file among others" uudecode "$IN/n3.file.uu" "$IN/nosuch" "$IN/n1.file.uu"
check "missing file, then a bad one" uudecode "$IN/nosuch" "$IN/h-none"
check "an unreadable file" uudecode "$IN/unreadable"
check "a directory" uudecode "$IN"
check "chmod fails, then a missing file" uudecode -c "$IN/n-mode0" "$IN/nosuch"
C_IN=closed; check "uudecode, stdin closed" uudecode
C_IN=$IN/big.uu C_OUT=full; check "uudecode, stdout full, large" uudecode
C_IN=$IN/big.b64 C_OUT=full; check "uudecode -m, stdout full, large" uudecode
C_IN=$IN/n3.uu C_OUT=full; check "uudecode, stdout full, small" uudecode
C_IN=$IN/n3.b64 C_OUT=full; check "uudecode -m, stdout full, small" uudecode
C_IN=$IN/n3.uu C_OUT=closed; check "uudecode, stdout closed" uudecode
C_IN=$IN/n3.file.uu C_OUT=closed; check "uudecode to a file, stdout closed" uudecode
C_ERR=full; check "uudecode, stderr full" uudecode "$IN/nosuch"

echo "uu-diff: $pass passed, $fail differed"
[ "$fail" -eq 0 ]
