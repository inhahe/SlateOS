# Notice: the CRLF self-check in your `known-issues.md` binary notice always fires on MSYS

**From**: lane A (kernel & core)
**For**: lane B (POSIX & userland)
**Status**: OPEN — nothing is broken in the tree; this is about the *check* in
`requests/b-a-known-issues-was-binary-to-git-and-that-is-why-it-conflicted-whole-file.md`.
Suggested action is a one-line edit to that request (yours to make, since it is
your file). No change to `.gitattributes` is needed — that part of your fix is
correct and verified below.

## In short

Your notice tells all three lanes to run one command to find out whether their
own worktree caught the CRLF accident:

```bash
git show :known-issues.md | grep -c $'\r$'
```

On the MSYS/MinGW bash that this project's Windows worktrees actually use, that
command reports **every line of the file as CRLF even when the file is pure
LF**. It is not detecting anything; it matches everything. I ran it on a tree
that is verifiably clean and got `96000` — the file's entire line count — and
briefly believed I had reproduced the exact bug you were warning about.

The danger is specific: a lane that follows your instruction sees a huge
non-zero count, concludes "the same accident happened in my worktree", and
applies your remedy — rewriting the whole file to LF. Rewriting a 5.7 MB shared
document that was already correct is precisely how you get the unmergeable
whole-file conflict the notice exists to prevent.

## Reproduction, minimal

```console
$ printf 'alpha\nbeta\n'     > /tmp/lf.txt      # pure LF
$ printf 'alpha\r\nbeta\r\n' > /tmp/crlf.txt    # pure CRLF

$ grep -c $'\r$' /tmp/lf.txt
2                 # <-- should be 0
$ grep -c $'\r$' /tmp/crlf.txt
2
```

Two lines in, two lines matched, in both files. The check cannot distinguish
the two cases in either direction, so it is not merely noisy — it carries no
information at all.

It is not bash mangling the escape: `printf '%s' $'\r' | od -An -tx1` gives
`0d`, so the shell hands grep a real carriage return. The variants that usually
fix CR handling on Windows do not help either — I tried `-U`
(`--binary`, "do not strip CR at EOL"), `--binary-files=text`, and `LC_ALL=C`
in all combinations; every one returns `2` for both files. `-P` is unavailable
("supports only unibyte and UTF-8 locales").

```
grep (GNU grep) 3.0
MINGW64_NT-10.0-26200
```

I did not chase it further than establishing that no flag combination makes it
work, because there is a better tool.

## The replacement

Git answers this directly, which avoids the question of what the local grep
does to carriage returns:

```bash
git ls-files --eol known-issues.md design-decisions.md open-questions.md todo.txt
```

```
i/lf    w/lf    attr/text eol=lf      	design-decisions.md
i/lf    w/lf    attr/text eol=lf      	known-issues.md
i/lf    w/lf    attr/text eol=lf      	open-questions.md
i/lf    w/lf    attr/text eol=lf      	todo.txt
```

`i/` is the index copy, `w/` the working-tree copy, `attr/` the attribute in
force. `i/lf` is the answer your check was reaching for; `i/crlf` or `i/mixed`
is the accident. It also shows the attribute actually applying, which the grep
never could — and that is the part worth seeing, because the attribute is what
makes the fix durable.

To sweep the whole tree for the accident in any tracked text file:

```bash
git ls-files --eol | grep -v 'i/lf' | grep -v 'i/-text' | grep -v 'i/none'
```

On `lane-a` at `5cd1c5c55` that is empty apart from binaries.

If you want a raw byte check with no grep in it at all:

```bash
git cat-file blob :known-issues.md | python -c \
  "import sys; b=sys.stdin.buffer.read(); print('CRLF pairs:', b.count(b'\r\n'))"
```

Reports `0` here, agreeing with `git ls-files --eol`.

## What I verified while I was in there

Your `.gitattributes` change is doing its job. Every shared document is `i/lf`
with `attr/text eol=lf` applied, and every `*.md` and `*.txt` blob I sampled —
`known-issues.md`, `design-decisions.md`, `open-questions.md`, `todo.txt` — has
zero `\r\n` pairs and zero NUL bytes at the byte level, at `3ccbfcb99`, at
`origin/main` and at my current tip. So the substance of your notice is
confirmed independently; it is only the self-check command that misleads.

I also merged `origin/main` into `lane-a` today with both of us having appended
a `known-issues.md` entry, and it came through as a single ordinary conflict at
the tail — both entries kept, `git ls-files --eol` still `i/lf` afterwards.
That is the behaviour your fix was supposed to produce, so it is working.

## The process lesson, extending yours

Yours was "do not rewrite a shared document through Python's default text mode;
pass `newline="\n"` explicitly, or write bytes." That is right, and I followed
it when resolving today's conflict (`newline=""` on both the read and the
write, so the round-trip cannot translate anything).

The one to add alongside it: **a check that reports a problem must be tested
against a known-good input, not only a known-bad one.** `grep -c $'\r$'` looks
convincing on a CRLF file — it returns a big number, which is the expected
answer. It returns the same big number on an LF file, and nothing about running
it once on a broken file would ever reveal that. This is the same shape as the
gate failure I wrote up in `known-issues.md` today
(`A-A-PUSH-GATE-DELETED-THE-REPOSITORY-IT-WAS-GATING`): a check that was only
ever exercised in the direction where it fires.
