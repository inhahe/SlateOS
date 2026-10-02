## E-Q3 — [E] System Restore now keeps the programs' settings and data. Should it also cover the system's own files, and how? — Status: OPEN (raised 2026-09-27)

**In short:** System Restore can now take a restore point of every program's
settings and data and put them back. It cannot do the same for the system
itself -- its programs, how it starts, its services, its installed packages --
and shows those as "needs the system's permission". Undoing a bad system
update is the other half of what people expect from System Restore. The
question is which way to build that half, because the three ways differ in
what they need from the rest of the system and in how much can go wrong.

| Option | What changes for the user | What it needs | Risk |
|---|---|---|---|
| **A. A privileged restore service** that copies system folders (`/etc`, `/boot`, `/usr`...) the way the settings folder is copied | "System files" becomes a component that can be ticked | a service running as root that System Restore asks, and a way to replace files the running system is using (a restart into a restore mode) | high: replacing a live system's files can leave it unbootable if interrupted |
| **B. Filesystem snapshots** (a copy-on-write filesystem, as `design.txt` prefers) | whole-disk restore points, taken instantly | a copy-on-write filesystem -- SlateOS is ext4 today (`design.txt`: "ext4 first") | low once the filesystem exists; blocked until then |
| **C. Package generations** (Nix-style, as `design.txt` also suggests: "roll back filesystem, reinstall package generation") | "Undo the last update" in the package manager rather than here | the package manager to keep each install as a generation | low; covers updates, not hand-edited system files |

*What changes:* A -- a restore point can include the system. B -- the same,
without copying. C -- a bad update is undone from the package manager, and
System Restore stays about the user's own files.

**Recommendation:** C for updates when the package manager can keep
generations, and B for whole-system snapshots if a copy-on-write filesystem is
added; not A. A copy-based restore of a running system's files is the one
design here that can leave a machine that does not start.

**If never answered:** nothing gets worse. System Restore keeps the user's
settings and data and says plainly that it does not cover the system.

**Where:** `apps/systemrestore/src/points.rs` (`SnapshotComponent::source`,
`NEEDS_THE_SYSTEM`); `design-decisions.md` §1217.
