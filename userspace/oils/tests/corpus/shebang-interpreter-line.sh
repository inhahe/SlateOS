# A `#!` line names the program that runs a script, and the shell has to honour
# it wherever the OS will not. Measured against bash 5.2.
#
#   * the interpreter runs with the script's own name in front of the original
#     arguments, so `#!I` on `./s.sh a b` is `I ./s.sh a b`;
#   * whatever follows the interpreter is ONE argument however many blanks it
#     holds — the rule of every kernel that reads a `#!` line (Linux's
#     `fs/binfmt_script.c`), not a word split;
#   * the interpreter ends at the first blank, so a path with a space in it
#     cannot be one — and blanks around the whole line are trimmed away;
#   * a line naming no interpreter at all is not a refusal: `#!` alone is text
#     like any other and the shell reads it itself;
#   * an interpreter that is not there is `cannot execute: required file not
#     found` and 127, worded against the *script* — from where the caller stands
#     the script is what would not run;
#   * one that is there and will not run names both, `SCRIPT: INTERP: bad
#     interpreter: REASON`, and is 126. That line comes from bash's `sys_error`,
#     which prints no `line N:` token, so it is the one command diagnostic
#     without one;
#   * this holds wherever an external command is started: on its own, as a
#     pipeline stage, and as a `&` job. `exec` adds a second line to the second
#     of those diagnostics — and bash's says nothing went wrong, because by then
#     it has cleared the `errno` it is printing;
#
# The interpreter is a *copy* of the real `printf`, under a name of this case's
# own: the two shells spell the one on `$PATH` differently, and it lives under a
# path with a space in it, which a `#!` line cannot hold at all (see above).
# `printf` is the one chosen because its output is its arguments and nothing
# else — no `argv[0]` of its own leaks in, which is just as well, since the
# `argv[0]` an interposed interpreter is given is the host's business.
#
# Almost nothing needs folding: every diagnostic here names `$0`, which is
# `case.sh` for both shells, and the interpreter as the `#!` line spelled it.
# Only `exec` names the script by the path it resolved to, and the scratch
# directory differs per run and per shell, so that much is folded away.
here=$PWD
sq() { sed -e "s|$here|DIR|g"; }

cp "$(type -P printf)" ./pf.exe
say() { printf '#!./pf.exe %s\nBODY\n' "$1" > "$2"; chmod +x "$2"; }

echo "=== the script goes in front of the arguments the caller wrote"
say '[%s]\n' show.sh
./show.sh; echo "rc=$?"
./show.sh a "b c"; echo "rc=$?"

echo "=== the rest of the line is one argument, not several"
# Two conversions in one format: if the tail were split, the format would be
# `%s` alone and `%s|%s;\n` would be printed as an operand instead.
say '%s|%s;\n' one.sh
./one.sh arg
printf '#!./pf.exe \t [%%s]\\n \t\nBODY\n' > trim.sh; chmod +x trim.sh
./trim.sh

echo "=== a line that names no interpreter is text, and the shell reads it"
printf '#!\necho bare-line-ran\n' > bare.sh
printf '#!  \t \necho blank-line-ran\n' > blank.sh
chmod +x bare.sh blank.sh
./bare.sh; echo "rc=$?"
./blank.sh; echo "rc=$?"

echo "=== the interpreter is a pathname, resolved like one — never searched for"
# `pf.exe` is in this directory and this directory is not on `$PATH`, so a bare
# name that runs is a name resolved against the working directory. A `$PATH`
# search would find nothing.
printf '#!pf.exe (%%s)\\n\nBODY\n' > bare-name.sh; chmod +x bare-name.sh
./bare-name.sh w; echo "rc=$?"

echo "=== the interpreter ends at the first blank"
mkdir -p 'has space'
cp ./pf.exe 'has space/pf.exe'
printf '#!./has space/pf.exe [%%s]\\n\nBODY\n' > spaced.sh; chmod +x spaced.sh
./spaced.sh; echo "rc=$?"

echo "=== an interpreter that is not there is the script's failure"
printf '#!./no-such-interpreter\necho hi\n' > missing.sh; chmod +x missing.sh
./missing.sh; echo "rc=$?"
printf '#!./nodir/pf.exe\necho hi\n' > nodir.sh; chmod +x nodir.sh
./nodir.sh; echo "rc=$?"

echo "=== one that is there and will not run names itself, without a line number"
mkdir -p adir
printf '#!./adir\necho hi\n' > isdir.sh; chmod +x isdir.sh
./isdir.sh; echo "rc=$?"

echo "=== every way of starting an external command agrees"
./show.sh p | cat
echo x | ./show.sh q
./show.sh r &
wait
{ ./missing.sh | cat; echo "rc=${PIPESTATUS[0]}"; } 2>&1
./isdir.sh &
wait

echo "=== exec names the file it resolved, and says twice what went wrong"
{ ( exec ./missing.sh; echo not-reached ); echo "rc=$?"; } 2>&1 | sq
{ ( exec ./isdir.sh; echo not-reached ); echo "rc=$?"; } 2>&1 | sq

echo "=== and a script can start another one"
printf '#!./pf.exe <%%s>\\n\nBODY\n' > inner.sh; chmod +x inner.sh
printf './inner.sh from-outer\n' > outer.sh; chmod +x outer.sh
./outer.sh
