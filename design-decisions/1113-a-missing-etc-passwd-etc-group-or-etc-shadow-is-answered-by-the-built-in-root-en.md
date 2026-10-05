## 1113. A missing `/etc/passwd`, `/etc/group` or `/etc/shadow` is answered by the built-in root entry, not by "no such user"

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** the C library now reads the real account files. When one of
them is not there at all -- as on today's system image, which ships none of
them -- it answers as if the file held one line for `root` (uid 0, home `/`,
shell `/bin/sh`; in `/etc/shadow`, a locked password), which is all it ever
answered before. When a file is there, only what it says counts: no
built-in root is added to it. glibc would answer "no such user" for
everything instead.

| Choice | For | Against |
|---|---|---|
| **Built-in root when the file is missing (chosen)** | `whoami`, shell prompts, Python's `getpass.getuser()` and every program that looks up uid 0 keep working on an image without the files, as they do today | differs from glibc; could hide an image that has lost its files |
| glibc's answer: no such user | exactly glibc | on today's image every user lookup fails, root's included |
| Built-in root always, beside the file | root found even where the file omits it | a file that deliberately changes or omits root is overridden: two sources of truth |

The built-in shadow entry is locked (`!`, which no `crypt` output matches),
so the built-in root can be looked up but not logged into with a password.
A file that exists and cannot be read is an error, as in glibc. Revisit once
the image ships the files -- the rootfs recipe together with lane B's
`/etc/users.yaml` -- at which point glibc's behaviour costs nothing.
