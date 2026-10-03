## 58. `container exec` semantics (Q17) — keep the netns-debug facade AND add real rootfs-binary exec under a distinct verb (option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended B).

**Context.** Our shipped `container exec <id> <builtin>` switches into the
container's **network namespace** and runs a **kshell builtin** there — a
network-debugging facade, not Docker's `docker exec` (which spawns a **new
program from the container's own rootfs** inside the running container's
namespaces + cgroup). The netns-debug facility is genuinely useful and would be
*lost* if `exec` were simply replaced. `docker build`'s `RUN`/`HEALTHCHECK`
instructions need the *real* rootfs exec.

**Decision.** Build **both, under distinct verbs.** `container exec` keeps its
netns-debug meaning; add a new verb (`container run-in <id> <path> [args…]`, and
accept `container exec --rootfs` as an alias) that spawns the rootfs binary in
the container's namespaces + cgroup and joins on its exit code (reusing the
proven `set_wait_task`→`block_current` join used by `container::wait`). The
`docker exec` delegate maps to the real rootfs path. `docker build`'s
`RUN`/`HEALTHCHECK` consume the real exec.

**Alternatives.** (A) Replace the facade with real exec — rejected: deletes the
netns-debug facility. (C) Keep facade only — rejected: leaves a real Docker gap
and blocks `RUN`/`HEALTHCHECK`.

**Where it lives.** `kernel/src/kshell.rs` (`container exec` arm + new `run-in`
arm + `docker` delegate map), `kernel/src/container.rs` (new
`exec(id, argv) -> KernelResult<i32>`), `kernel/src/oci.rs` (`build_image`
`RUN`/`HEALTHCHECK`). Supersedes known-issues D-CONTAINER-EXEC-WAIT.
