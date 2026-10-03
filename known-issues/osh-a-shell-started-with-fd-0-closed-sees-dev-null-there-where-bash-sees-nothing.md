## osh: a shell started with fd 0 closed sees `/dev/null` there, where bash sees nothing

**Status:** open, and arguably **won't fix** — recorded so it is not
rediscovered as a defect. Found 2026-08-26.

```text
$ bash --norc -c 'read l < /dev/stdin <<< x; echo "[$l]"' <&-
bash: /dev/stdin: No such file or directory
[]
$ osh  --norc -c 'read l < /dev/stdin <<< x; echo "[$l]"' <&-
[x]
```

The cause is not in osh's code at all. Rust's standard library sanitises the
standard descriptors before `main` runs: if fd 0, 1 or 2 is closed at `exec`,
it opens `/dev/null` onto it, so that a program which later opens a file cannot
have it silently become "stdout". Inspecting the fd table shows it plainly —
with fd 0 closed at exec, bash's fd 0 is absent while osh's is `/dev/null`.

Every *shell-modelled* closed descriptor is handled correctly and matches bash
exactly, which is the case that actually occurs in scripts:

```text
{ read l < /dev/stdin; echo "[$l]"; } <&-        both: /dev/stdin: No such file or directory
exec 0<&-; { read l < /dev/stdin; ... }          both: /dev/stdin: No such file or directory
{ cat < /dev/stdin; } <&-; echo "st=$?"          both: … + st=1
exec 3<&-; cat < /dev/fd/3; echo "st=$?"         both: /dev/fd/3: … + st=1
```

Only the *invocation* form — the shell process itself started with fd 0
already closed — differs. Matching bash would mean defeating std's guard, and
by the time `main` runs the distinction is gone: `/dev/null` on fd 0 looks the
same whether std put it there or the caller did. Detecting it would require a
pre-`main` constructor in `.init_array`, which trades a real safety property
for a corner case no script can reach.
