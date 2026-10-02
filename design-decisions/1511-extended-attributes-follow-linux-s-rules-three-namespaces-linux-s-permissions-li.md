## 1511. Extended attributes follow Linux's rules: three namespaces, Linux's permissions, Linux's order

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** an extended attribute is a named piece of data a file carries
beside its contents -- `user.mime_type`, say. The kernel stored any name
for anyone. An ordinary program could write the `trusted.` attributes Linux
keeps for the administrator, put attributes on links and devices where
Linux refuses them, and store names Linux would not accept at all. The
kernel now applies Linux 6.6's rules, for native and Linux programs alike:
which names exist, who may read and write each kind, and which refusal
comes first. Lane D asked for it
(`requests/d-a-xattr-answers-only-the-filesystem-can-give.md`).

**What changed:**
- **`fs::xattr_policy`** holds the rules:
  - three namespaces, `user.`, `trusted.` and `security.`. Any other name,
    `system.` among them, is `NotSupported` (`EOPNOTSUPP`); a bare prefix
    is `InvalidArgument`;
  - `trusted.` is privileged, and absent to anyone else;
  - `user.` goes on regular files and directories only, and a sticky
    directory's is its owner's;
  - writing `security.` wants privilege;
  - an immutable or append-only file takes no change.
- **Every VFS xattr call** goes through one helper, `Vfs::xattr_on`. It
  decides the rules under the same hold of the filesystem's lock as the
  change, as Linux holds the inode's lock.
- **`KernelError::NotPermitted`** (-402, `EPERM`) is new: a refusal no
  ordinary access would get past, as against `PermissionDenied`'s
  `EACCES`.
- **The native handlers** copy a value only when all of it fits, and no
  longer check the buffer before the lookup.
- **ext4** names the two POSIX ACL indexes.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. The VFS applies the rules, for both ABIs (chosen)** | a file's attributes obey one set of rules whoever writes them | one place; the native and Linux views of a file agree | native programs lose names in no namespace (none in the tree used one) |
| B. The Linux layer alone applies them | the native ABI is unchanged | no native change | `trusted.` protected only from Linux programs is not protected |
| C. Each filesystem applies them, as Linux's handlers do | memfs and ext4 each refuse | Linux's structure | two copies now, and one more for every filesystem that keeps attributes |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| `system.` names are refused | store them as plain attributes; or translate `system.posix_acl_*` to and from `fs::acl` | stored, an ACL attribute would claim access control it does not give. Translating is the right end state, and its own piece of work (known-issues `A-XATTR-ACLS-NOT-REACHABLE-AS-ATTRIBUTES`) |
| "Privileged" is user id 0, or a kernel task | a capability | it is how the Linux layer models `CAP_SYS_ADMIN` elsewhere, and the permission gate's root bypass; no capability for administrative authority over files exists yet |
| A listing is checked against the capability tags only | the ACL's read permission, as before | Linux's `listxattr` checks nothing: a name is not its value, and the value's own rules still stand |
| When an ACL exists, the rules are decided twice, before the ACL and again under the lock | once, under the lock, with the ACL first | Linux checks the file's permission between the rules and the name. The ACL cannot be read under the filesystem's lock (it finds the file through the VFS) |
| A filesystem says whether it keeps attributes at all (`FileSystem::xattrs_supported`) | let each refuse when asked | Linux refuses on such a filesystem before it looks at the name, so a bare prefix there is `EOPNOTSUPP`, not `EINVAL` |
| `security.capability` is any `security.` name | Linux's `CAP_SETFCAP` and its header check | user id 0 has both here, and the kernel has no file capabilities to check a header for |
| A call is two steps, `Vfs::xattr_target` then `xattr_get` and the rest | one call taking the path and the name | Linux reads the name after the path for get, list and remove, and before it for set; the Linux layer reads it between the steps, so each refusal comes in Linux's order |
| A descriptor's calls act on the file it holds, by identity, and its ACL is asked by identity (`check_object_access`) | act by the name the descriptor was opened under | Linux acts on the open file; a name can be another file's by now. The ACL check stays, as Linux's `fsetxattr` still asks the file's permission |
| A pipe, socket, event descriptor or device has the rules applied, then `EOPNOTSUPP` | `EOPNOTSUPP` at once | Linux's `xattr_permission` comes before its `IOP_XATTR` refusal, so `user.` on a pipe is `ENODATA` or `EPERM` |

**Revisit** when ACLs become reachable as attributes, or when the kernel
gains a capability for administrative authority over files.
