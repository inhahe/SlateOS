## 1055. The regex engine has no C escapes; awk and sed resolve their own, in front of it, as gawk and GNU sed do

**Date:** 2026-10-01
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** In a regular expression, does `\t` mean a tab? It depends on the
program. In `grep`, `find`, `ed`, `expr` and the shell's `[[ =~ ]]` it means
the letter `t`, because their regex library (glibc's) has no such escape. In
`awk` it means a tab, because POSIX gives awk's language C's escapes, and in
GNU `sed` it means a tab because sed converts it before compiling. Our shared
regex engine read `\t` as a tab for everyone, so `grep 'a\tb'` matched a tab
and `grep '[\.]'` missed a backslash. Now the engine reads backslashes exactly
as glibc does, and awk and sed each convert their own escapes first -- the same
split GNU's programs have. This also reverses §333's choice for awk: `\1` in
an awk regex is now the byte 0x01, as POSIX and gawk say, not a backreference.

**The layering.** glibc's `regcomp` has no C escapes and, in every POSIX
syntax but awk's, no escapes inside a bracket at all. gawk puts `make_regexp`
in front of it (C escapes and octal, anywhere in the pattern) and compiles with
`RE_SYNTAX_POSIX_AWK`, whose `RE_BACKSLASH_ESCAPE_IN_LISTS` makes `[\.]` a
dot. GNU sed puts `normalize_text` in front (C escapes, `\dNNN`, `\oNNN`,
`\xHH`, `\cX`, inside brackets too) and compiles with a syntax that has no
escapes in lists, so `[\.]` stays a backslash or a dot. The engine now matches
glibc, `Syntax` grows the two bits awk needs (`backslash_escape_in_lists`,
`no_gnu_ops`) and a `POSIX_AWK` constant, and gawk's layer is `ere::awk`.

| Option | For | Against |
|---|---|---|
| **A. Engine = glibc; each language resolves its own escapes first** (chosen) | Every caller gets its original's answer: measured against grep 3.11, sed 4.9, gawk 5.2.1 `--posix`, findutils 4.9, bash 5.2. The engine has one meaning for a backslash, the one `regcomp` has, so a pattern means the same in `grep`, `find`, `ed`, `expr` and `[[ =~ ]]`. | Programs whose language has escapes must call their layer; the kernel shell's sed and awk (lane A) and two lane-E apps changed behaviour and were told. |
| B. Keep C escapes in the engine, add a "no escapes in brackets" flag (the plan in `known-issues.md`) | Smaller change | Leaves `grep 'a\tb'` matching a tab -- the engine would still be wrong outside brackets for every glibc caller, which the measurement showed the plan had missed. |
| C. A dialect flag per caller for C escapes | No layer in awk | The engine would carry awk's lexical rules (octal, `\/`, the warnings) as syntax bits glibc does not have; awk's escapes would then differ between a literal and a dynamic regex unless both went through the flag anyway. |

**Revisiting §333.** §333 kept `\1` a backreference in awk on the premise that
POSIX leaves `\1` in an ERE undefined, so the choice was between two
extensions. That premise holds for ERE but not for *awk*: POSIX's awk table
defines `\ddd` octal in awk EREs, "recognized both inside and outside bracket
expressions", so `\1` is byte 0x01 -- gawk's reading. With awk's layer in front
of the engine, honouring it costs the other programs nothing: the engine still
reads `\1` as a backreference for `grep -E`, `sed -E` and the rest. The xfail
it was pinned by in `awk-diff.sh` is an ordinary case now.

**Revisit when** a caller needs C escapes without being awk or sed -- it should
then get a layer of its own, not a flag in the engine.
