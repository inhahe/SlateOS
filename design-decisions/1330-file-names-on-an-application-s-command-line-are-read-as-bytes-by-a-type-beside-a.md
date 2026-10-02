## 1330. File names on an application's command line are read as bytes, by a type beside `Args`

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Claude (autonomous) -- answering lane E's request
`e-f-a-file-named-on-the-command-line-may-be-any-bytes.md`, which proposed
changing `Args::rest` to `Vec<OsString>`.

**In short:** a program opened on a file whose name is not valid text used to
crash before its window appeared. Programs now read file names as raw bytes
through a new `ArgsOs`, and the old `Args` refuses such a name with a message
instead of crashing. The old type was kept, rather than changed as asked, so
that no program stopped building while the others were being moved over.

**Alternatives.**

| | For | Against |
|---|---|---|
| `ArgsOs` beside `Args` (chosen) | nothing breaks; each application moves when its lane does | two types for a while |
| `Args::rest` becomes `Vec<OsString>` (asked) | one type | `main` fails to build for every lane until lane E's callers follow |

**How to reverse.** Once every file-taking application reads `ArgsOs`,
`Args` can be narrowed to the programs whose arguments are text, or removed.
