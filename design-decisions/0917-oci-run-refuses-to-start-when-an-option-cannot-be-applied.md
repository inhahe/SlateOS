## §917 — `oci run` refuses to start when an option cannot be applied

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

**In short:** when a user asks `oci run` to apply an option (e.g. a
resource limit, a namespace flag) that the runtime cannot honour, should it
start the container anyway or refuse? The operator chose **option A**:
refuse to start. A container running with fewer restrictions than requested
is a security gap; a clear error at launch is preferable.

**Where it lives.** `kernel/src/container/` — the OCI runtime's option
validation path.
