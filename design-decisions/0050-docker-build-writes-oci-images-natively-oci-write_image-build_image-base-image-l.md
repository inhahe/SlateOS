## 50. `docker build` writes OCI images natively (`oci::write_image`/`build_image`); base-image layers are carried forward as verbatim blobs, not re-tarred

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — within the operator-approved Docker/
container-runtime port (Q15). Ungated: the image writer + every non-`RUN`
Dockerfile instruction need no exec, so no operator fork was required; only
`RUN` remains gated on Q17.

`docker build` is the last big Docker-port capstone. It needed two things
`oci.rs` lacked: an OCI image **writer** (previously it could only *read*
images) and a **Dockerfile interpreter**.

**Writer (`write_image`).** Authors a standard OCI layout under a dest dir:
per layer, build an uncompressed tar → `diff_id` = sha256(tar) → gzip → blob
(digest = sha256(gzip)); then config, manifest, `index.json`, `oci-layout`.
Round-trips byte-identically through the existing `load_image`. Factored a
shared `finish_image` tail (config+manifest+index assembly from already-written
layer descriptors) so both the plain writer and the builder feed it.

**Builder (`build_image`).** Interprets a Dockerfile into an `ImageSpec` +
layers, then calls the writer. Supports every instruction **except `RUN`**:
FROM (`scratch` or a local OCI image dir), COPY/ADD (file + recursive directory
sources with Docker dest semantics), ENV (both forms + quoted values),
CMD/ENTRYPOINT (JSON exec + shell form → `/bin/sh -c`), WORKDIR (absolute +
relative-append), USER, EXPOSE (default `tcp`), LABEL, ARG, plus
`${VAR}`/`$VAR`/`${VAR:-default}` expansion and `\`-continuation/comment
handling.

**Key tradeoff — base-image layer carry-forward.** `FROM <local-oci-dir>`
inherits the base image's config *and* its layers. Two ways to carry the
layers: (a) **copy the base layer blob files verbatim** into the new image and
reuse the base's descriptors + `diff_id`s, or (b) extract each base layer and
re-tar/re-gzip it into a fresh blob. Chose **(a)**: it is byte-exact
(identical digests, so content-addressed dedup still works), avoids a
decompress→recompress round-trip that could perturb bytes, and is far cheaper.
The cost is that `build_image` must special-case "carried" layers (their
`diff_id`s come from the base config, not recomputed) — handled by seeding
`layer_descs`/`diff_ids` with the base's before appending freshly-built COPY
layers. `finish_image` then treats the concatenation uniformly.

**Other calls.** (1) `RUN` is rejected with a precise
`BuildError::RunUnsupported { line }` (it needs the Q17-gated in-container
exec), not silently dropped — an honest failure beats a wrong image. (2)
Unsupported instructions (VOLUME/HEALTHCHECK/STOPSIGNAL/SHELL/ONBUILD) are
likewise rejected with a clear message rather than ignored, for the same
reason; MAINTAINER maps to the conventional `maintainer` label. (3) The
Dockerfile is parsed as UTF-8 (a Dockerfile is text, and our VFS already models
directory-entry names as `String`), so COPY paths ride the same `&str` path
surface as the rest of `oci.rs`. (4) `BuildError` is a distinct type from
`KernelError` so the shell prints a Docker-style diagnostic.

**Where it bites.** `kernel/src/oci.rs` (`ImageSpec`/`BuildLayer`/`LayerFile`,
`write_image`/`finish_image`/`create_layout_skeleton`, `build_image` +
`BuildError` + Dockerfile helpers, self-tests 11–12); `kernel/src/kshell.rs`
(`oci build` arm + `docker build` shim delegate + help/usage). Follow-up: `RUN`
support arrives with Q17's `container exec`.
