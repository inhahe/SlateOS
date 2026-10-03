## 3. /proc/<pid> magic symlinks — cwd, root, and exe

**Date:** 2026-06-10 (cwd/root landed) / exe approved same day

**Decided by:** Claude (operator-approved scope) — the implementation design
(real procfs symlinks via the VFS `readlink` path, the `exe_path` PCB field,
fork/exec inheritance rules) was Claude's; the operator approved doing the
`exe` increment specifically.

**Context:**
Linux exposes magic symlinks in `/proc/<pid>/`: `cwd` (current working
directory), `root` (filesystem root), and `exe` (the executable image).
Tools (and some libc paths) read these. The VFS already supports symlink
resolution end-to-end (`lstat` → `EntryType::Symlink` → `readlink`), so procfs
just needs to participate.

**Decision:**
- Implement all three as real procfs symlinks via `FileSystem::readlink`.
  - **`cwd`** → the process's stored cwd (`Process::cwd`, already maintained).
  - **`root`** → always `"/"` — we have no per-process `chroot`/mount
    namespaces yet, so every process shares the global VFS root.
  - **`exe`** → the resolved absolute path of the loaded executable, captured
    at `exec` time (requires a new `exe_path` field on the PCB; see below).
- `readdir` lists them with `EntryType::Symlink`; `stat` reports `Symlink`;
  `read_file` on a link returns `InvalidArgument` (mirrors Linux `read()` →
  `EINVAL` on a symlink opened without `O_PATH`).
- **Bytes→String at the readlink boundary:** the VFS `readlink` API returns
  `String`, but paths are stored as bytes. A non-UTF-8 target surfaces as
  `InvalidArgument` rather than being lossily mangled — silent path corruption
  is never acceptable. (Canonical paths are ASCII/UTF-8 in practice, so this is
  a theoretical edge.)

**`exe` capture (the part that touches the exec path):**
- Add `pub exe_path: Vec<u8>` to `Process` (bytes, not String).
- **Inherited on `fork`** (clone), **overwritten on `exec`** (not inherited
  across exec — exec replaces the image).
- The exec/ELF-load path stores the canonicalised path of the binary into
  `exe_path` before entering userspace.

**Rationale:**
- `cwd`/`root` data already lives in the PCB (or is trivially `/`), so they
  were landed immediately as a low-risk, additive change.
- `exe` is the genuinely Linux-authentic completion; capturing the path at the
  one place that already resolves the binary (the loader) is the correct,
  non-hacky home for it.

**Alternatives considered:**
- *Resolve `self` as a real symlink too* — currently `/proc/self` is a
  transparent directory alias (resolved inline in `classify_path`), not a
  symlink. Left as-is; making it a symlink is cosmetic and out of scope.
- *Skip `exe` indefinitely* — rejected (operator approved the full increment);
  many tools rely on `/proc/self/exe`.

**Where it lives:**
- `kernel/src/fs/procfs.rs`: `PID_LINKS`, `ProcPath::PidLink`, `classify_path`,
  `readdir`/`read_file`/`stat`, `readlink`.
- `kernel/src/proc/pcb.rs`: `Process::cwd` (existing) + `exe_path` (new),
  fork inheritance, exec-time population.
- ELF loader / exec path: `exe_path` capture site.

**How to reverse:**
- Drop a link by removing it from `PID_LINKS` and its `readlink` arm.
- Drop `exe` capture by removing the `exe_path` field and its loader write;
  the link arm then returns `NotFound`.
