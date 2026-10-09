## 1526. A sealed file is sealed on every route: the VFS asks the seal rules on each change of contents, size or mode, and whoever may write the file may seal it

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: §978, "re-key them by file identity, then connect them to the VFS permission and write paths") · **Lane:** A

**In short:** a program can seal a file -- promise that it will not be
written, or not grow, or not shrink, or that its execute bits stay put -- and
seals cannot be undone, only ended by deleting the file. Until now a seal was
recorded and nothing checked it. Now the VFS asks the seal rules on every
write, append, whole-file write, truncate, allocation and `chmod`, by the
file's identity, so the seal holds whichever name or open descriptor the
change comes through; a refusal is `EPERM`. A program seals through a
descriptor it has open for writing (`SYS_FS_ADD_SEALS`, 1124), as Linux
requires for `F_ADD_SEALS`.

**The rules** (`fs::sealing`, Linux's memfd seals where they say the same):

| Seal | Refuses |
|---|---|
| `SHRINK` | a truncate or whole-file write to fewer bytes |
| `GROW` | any write past the end, an append, a truncate larger, an allocation past the end |
| `WRITE` | any write, an append, a whole-file write, any truncate -- the contents frozen outright |
| `EXEC` | a `chmod` that changes an execute bit |
| `SEAL` | adding another seal |

`WRITE` refuses truncation too, as this module has always defined it; Linux's
memfd allows truncating a write-sealed file (its size is `SHRINK`'s and
`GROW`'s business). These are not memfd's seals -- they apply to any file --
so the stricter reading, "the contents are fixed", stands.

| Who may seal | What changes | For | Against |
|---|---|---|---|
| **Whoever has the file open for writing (chosen -- Linux's `memfd_add_seals`)** | a program seals a file it is writing, through that descriptor | the one rule Linux has; a reader cannot restrict the writers; no new privilege idea | the owner cannot seal a file they could open but have not |
| The file's owner, or root | by name, like `chattr` | -- | a different rule from Linux's for the same operation; and seals are irrevocable, so who may set one matters more than who may set `+i` |

| Where the rules are asked | For | Against |
|---|---|---|
| **The VFS, under the filesystem's lock, by identity (chosen)** | every filesystem, every route -- path, second name, held descriptor; atomic with the change | a metadata lookup per write while any file is sealed -- none while none is: one relaxed load decides |
| Each filesystem | no lookup | the seal table is the VFS's, not the inode's; every filesystem would have to reach into it |

**The door** (native, by handle, in Linux's `F_SEAL_*` bits so one encoding
serves both ABIs): `SYS_FS_ADD_SEALS(handle, seals)` (1124) and
`SYS_FS_GET_SEALS(handle)` (1125). The Linux `fcntl(F_ADD_SEALS)` stays a
memfd's alone (`ipc::memfd`), as on Linux, where a regular file answers
`EINVAL`.

**Not covered:** a writable shared mapping, which this kernel does not make
(`mmap` answers `ENOSYS`); when it does, `WRITE` must refuse one, as Linux's
`F_SEAL_WRITE` does, and adding `WRITE` to a file so mapped must be `EBUSY`.
