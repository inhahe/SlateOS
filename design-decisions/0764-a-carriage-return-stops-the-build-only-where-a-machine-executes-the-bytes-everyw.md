## 764. A carriage return stops the build only where a machine executes the bytes; everywhere else it is reported and the build goes on

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** a gate checks that files git promises to store with Unix line
endings really have them on disk. It used to refuse to build if *any* such file
had a Windows line ending. But that set includes every `.txt` and `.md` in the
tree — including the operator's own notes file — and when the operator types
into one in a Windows editor, the file picks up Windows endings and all three
agents' builds stop. That happened twice in twenty minutes on 2026-09-04, for a
notes file in which nothing was wrong. The gate now reports every such file as
before, but only *stops* the build when the file is one something actually runs:
a shell script, or any file starting with `#!`.

### What forced it

`.gitattributes` says `*.txt text eol=lf`, so `todo3.txt` — the operator's
personal scratch notes — is in scope. On 2026-09-04 it was repaired to LF, and
four minutes later it was CRLF again, because a human was typing into it and
saving. Two boot tests, 187 s and 259 s, were thrown away for it. The same rule
covers `design.txt`, `todo.txt`, `scheduler.txt`, `ipc.txt` and every `.md`, so
this is not one file's quirk: it is every lane's build blocked whenever the
operator edits prose on a Windows box.

The gate's own docstring had argued for the wide scope, and its argument was
good — the 2026-09-03 event corrupted thirteen files at once and a `.sh`-only
gate would have reported one and hidden twelve. What the argument assumed,
without saying so, is that *the writer is a tool*. Here it was a person, editing
their own file, correctly. There is no tooling bug to find and no repair that
stays repaired.

### The options

| | *What changes:* |
|---|---|
| **A. Leave it** | Builds keep stopping whenever the operator saves a text file. |
| **B. Drop `*.txt`/`*.md` from `.gitattributes`** | Git stops normalising prose on commit too, so CRLF starts reaching history — a much bigger change than the problem, and it touches a file all three lanes share. |
| **C. Exempt `todo3.txt` by name** | This one file stops blocking; `design.txt` and the next notes file still do. Treats the instance, not the cause. |
| **D. Report everything, refuse only for files a machine executes** *(chosen)* | Prose with CRLF prints a note and the build continues; a `.sh` or a `#!` file with CRLF still stops it dead. |

### Why D

Because it keeps every property the 2026-09-03 event actually asks for. Replay
that event against D: `scripts/boot-test.sh` was among the thirteen corrupted
files, so the build still stops, and the report still names all thirteen, so the
size of the cause is still visible — which was the whole objection to a
`.sh`-scoped gate. The only case whose outcome changes is the one where the
answer is "a human saved a text file", and that is not a defect.

The line is drawn at *execution*, not at *looking like code*, and that is
deliberate: `.py` is not on the fatal side. CPython decodes CRLF source
correctly, so an imported module is unharmed; a `.py` that is actually run is
caught by its shebang, which is the property that matters. Keying on the suffix
would refuse builds over library files where nothing is wrong — reintroducing
the false positive one file type to the left.

Against D: a tool that corrupts only prose files is now a note rather than a
refusal, so it could persist. That is accepted, and it is bounded — the note
prints on every run, and the docstring's own point stands that such a tool
reaches a `.sh` eventually, which is the run that stops the build. A gate that
halts three agents over a harmless byte gets bypassed, and a bypassed gate is
not protecting the `.sh` files either.

**The generalisable rule.** Severity is not scope. A gate can be right to *look*
at everything and wrong to *refuse* on everything, and collapsing the two
questions into one is what turns a correct detector into one people route
around. Ask separately: what do I report, and what do I stop the world for.
