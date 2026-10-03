## §373 — `cmp` quotes the file names in its own output, because GNU prints them raw and one newline forges a whole result line

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)

**In short:** When two files differ, `cmp` prints one line naming both of them:
`a b differ: byte 5, line 2`. GNU pastes the names into that line exactly as
they arrived, so a file whose *name* contains a newline splits the line in two
and a file whose name contains no printable text scribbles on the terminal. We
print the names quoted and escaped instead, the same way every other utility
here already prints a name. The cost is that our line is not byte-identical to
GNU's for such names; the benefit is that a name cannot fabricate output.

### What GNU does, measured

GNU diffutils 3.10, in an empty directory holding `sp ace` and a file whose
name is the four characters `nl`, a newline, and `name`:

```
$ cmp 'sp ace' $'nl\nname'
sp ace nl
name differ: byte 1, line 1
$ cmp 'sp ace' $'nl\nname' | od -An -c
   s   p       a   c   e       n   l  \n   n   a   m   e       d
   i   f   f   e   r   :       b   y   t   e       1   ,       l
   i   n   e       1  \n
```

Two lines where there was one difference. A script reading `cmp`'s output line
by line — and that is the only reason to read it rather than the exit status —
now sees `sp ace nl` as a complete record and `name differ: byte 1, line 1` as
another. Neither is true. The same applies to a name that is not text at all:

```
$ cmp $'\xff\xfe-bad' $'\xff\xfe-bad2' | od -An -c
 377 376   -   b   a   d     377 376   -   b   a   d   2       d
   i   f   f   e   r   :       b   y   t   e       1   ,       l
   i   n   e       1  \n
```

The `\377\376` go to the terminal as-is. On this OS that is not a curiosity:
`design.txt` permits every byte but `/` and NUL in a name, so a name that is
not valid UTF-8 is ordinary, not hostile.

### What we do

The same `quotef` every other utility here uses, which is gnulib's
`shell-escape` style — bare when the name is safe, single-quoted when it is
not, and `$'…'` around anything that has to be escaped:

```
$ cmp 'sp ace' $'nl\nname'
'sp ace' 'nl'$'\n''name' differ: byte 1, line 1
$ cmp $'\xff\xfe-bad' $'\xff\xfe-bad2'
''$'\377\376''-bad' ''$'\377\376''-bad2' differ: byte 1, line 1
```

One line, always. The `EOF on …` note on stderr is quoted the same way, and so
is the `cmp: NAME: No such file or directory` diagnostic — which GNU *already*
quotes, through `error (0, errno, "%s", file[i])` reaching gnulib's quoting.
That inconsistency inside GNU is part of the argument: upstream quotes the name
when it prints it as a diagnostic and does not when it prints it as a result,
and there is no reason for the two to differ.

### The alternative, and why it lost

**Reproduce GNU byte for byte, raw names and all.** This project's default is
to match upstream exactly, and every departure has to earn itself; a harness
full of "differs on purpose" entries is a harness nobody reads. The case for
raw names is that something, somewhere, might diff `cmp`'s output against a
recorded copy produced by GNU.

It loses on two counts. First, nothing can *usefully* depend on the raw form:
the moment a name contains a space — `sp ace`, above — GNU's line is already
ambiguous about where one name ends and the next begins, so a consumer parsing
it is broken before newlines are even considered. Quoting makes the line
parseable for the first time. Second, this is the same call already made three
times in this tree for the same reason: §369 (a diagnostic naming an
undecodable byte prints `\377`, never raw and never U+FFFD), §371 (`stat`'s
`%N` is quoted in the built-in block so a name cannot forge a line of `stat`'s
own output), and the whole existence of `userspace/quoting` (§370). A utility
that printed names raw would be the only one here that does.

**What it costs.** Four cases in `scripts/cmp-diff.sh` are recorded as `xfail`
rather than pass. Anyone comparing our result line against GNU's for a name
containing a space, a newline, a quote or a non-UTF-8 byte will see a
difference — and will see it *reported*, which is the point of the xfail.

**If this is revisited,** the switch is small and lives in one place: `cmp.rs`
calls `quotef` at exactly three sites (the result line, the EOF note, the open
diagnostic). Dropping the quoting is a three-line change plus four xfail
entries becoming plain cases. The reverse — adding it later, after something
has come to depend on the raw form — is the expensive direction, which is why
it is being done now.
