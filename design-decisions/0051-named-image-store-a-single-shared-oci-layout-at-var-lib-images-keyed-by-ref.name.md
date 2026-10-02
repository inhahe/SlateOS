## 51. Named image store — a single shared OCI layout at `/var/lib/images` keyed by `ref.name` annotations, with blob GC on `rmi`

**Date:** 2026-07-01
**Decided by:** Claude (operator-approved scope) — within the operator-approved
Docker/container-runtime port (Q15). No operator fork: this is the
obviously-correct Docker-parity default, and the on-disk internals are
reversible.

Until now SlateOS had no image *store* keyed by name: `oci run`/`FROM`/`docker
images` all operated on an on-disk OCI layout **directory path**. That works but
diverges from Docker, where images are referenced by `name:tag`. The store adds
that name→image mapping.

**Design.** A single OCI image layout lives at `/var/lib/images`. Its
`index.json` holds one manifest descriptor **per tag**, each carrying an
`org.opencontainers.image.ref.name` annotation — the real OCI multi-image
pattern (the same layout a registry pull populates). All tags share one
content-addressed `blobs/sha256/` pool, so identical layers across images are
stored once.

**Operations (`oci.rs`).** `store_tag_from_dir(dir, ref)` imports a built image
directory into the store (copies its blobs, adds/replaces the tag);
`store_add_tag(src, dst)` re-tags an existing ref with no blob recopy (`docker
tag`); `store_resolve(ref)` → manifest digest; `store_list()` → rows for `docker
images`; `store_remove(ref)` drops a tag and **garbage-collects** every blob no
longer reachable from a surviving manifest (walk each remaining manifest → keep
its manifest+config+layer hexes → delete the rest). `normalize_ref` defaults a
bare name to `:latest` and leaves `@digest` refs untouched.

**Key tradeoff — shared layout + GC vs. per-image directories.** Alternative
(b): keep every image in its own directory and make the "store" just a
name→directory map. Chose the **shared single-layout** approach: it is what
Docker/registries actually do, gives free cross-image layer dedup, and keeps a
single `oci-layout`/`index.json` to reason about. The cost is that deletion is
no longer "rm -rf a directory" — it must reference-count blobs across all
remaining tags (the GC pass). That GC is the one piece of real complexity, and
it is covered by self-test 20 (two tags sharing blobs: removing the first GCs
nothing; removing the last GCs everything).

**Where it bites.** `kernel/src/oci.rs` (`STORE_DIR`, `StoredImage`/`StoreEntry`,
`normalize_ref`, `store_read_index`/`store_write_index`, `copy_all_blobs`,
`store_tag_from_dir`/`store_add_tag`/`store_resolve`/`store_list`/`store_remove`,
`collect_manifest_blob_hexes`, self-test 20); `kernel/src/kshell.rs` (`oci
tag`/`images`/`rmi` arms + `docker` shim routes for `images`/`tag`/`rmi`).

**Follow-up (done, same day).** Store references are now resolvable everywhere
an image is named, via `resolve_image_source(arg)` — which treats `arg` as an
on-disk OCI layout directory if it has an `oci-layout` marker, else looks it up
in the store (`store_resolve` → `load_manifest_by_digest(STORE_DIR, digest)`,
returning `STORE_DIR` as the blob-source since all store images share its blob
pool). Wired into `FROM name:tag` (base inheritance), `oci`/`docker run`,
`oci inspect|layers|history`, and `oci build -t name:tag` (auto-import the built
image into the store). A dedicated `load_manifest_by_digest` was needed because
the store is a *multi-manifest* layout — `load_image`'s host-platform manifest
selection would be ambiguous across tags. Covered by self-test 21.

**Follow-up 2 — store-aware `save`/`load` (done, same day).** `oci save
name:tag` exports *one* image (not the whole shared store) into a standalone
single-manifest layout via `store_export_ref` (copies only that manifest's
config + layer blobs and writes a one-entry `index.json` preserving the
`ref.name` annotation), then tars it; `oci load` extracts a tar and calls
`store_import_dir`, which copies the blobs into the shared pool and re-adds each
`ref.name`-annotated manifest as a store tag — matching Docker, where `load`
repopulates the local image store. `load`'s dest-dir is now optional (temp dir +
store import when omitted). The index (de)serialisers were generalised to a
`dir` parameter (`serialize_index`/`write_index_at`/`read_index_at`) so the same
code writes the store index and per-export indices. Covered by self-test 22
(build → tag → export → wipe store → import → resolve + extract original bytes).

**Follow-up 3 — `commit`: author an image from a container's changes (done,
same day).** `docker commit <container> [repo:tag]` produces a *new image* from
a running container's filesystem changes. This is distinct from the existing
native `container commit`, which *clones a container* (snapshots one container's
rootfs into a second independent container). Both semantics are legitimate and
useful, so rather than repurpose the shipped `container commit`, the image-
production path got its own verb and the two are kept separate:

- **`oci commit <container-id> <dest-dir> [name:tag]`** and **`docker commit
  <container-id> <name:tag>`** → image production (`oci::commit_image` →
  `container::commit_image`). Captures the container's overlay **upper** layer
  (added/changed files, walked iteratively via VFS `readdir`/`metadata`/
  `read_file`) plus its **whiteouts** (deletions, emitted as OCI `.wh.<base>`
  empty-file markers) as **one new layer** stacked on top of the base image the
  container was created from. The base image's config (Env/Cmd/Entrypoint/
  WORKDIR/USER/… and `onbuild`) and existing layers are carried forward verbatim
  (blobs copied by digest, descriptors + diff_ids reused), and a
  `#(nop) COMMIT` `history[]` entry is appended. Written as a standalone OCI
  layout at `dest_dir`; `docker commit` additionally stages that layout in a
  temp dir and imports it into the store under the given `name:tag`, then
  discards the temp dir (Docker's `commit` leaves no dir artifact).
- **`container commit <src-id> <new-name> <rootfs-dir>`** → unchanged
  (container clone).

To recover the base image at commit time, the container now records the image
it was created from: `ContainerConfig::image_source` (an OCI-layout dir path or
a `name:tag` store reference) is stamped at `oci run` time and stored on the
`Container`; `container::commit_image` reads it back and resolves it via
`oci::resolve_image_source` (dir-or-reference). A container created from a bind
rootfs (no image) or with no overlay is rejected with `InvalidArgument` — there
is no base to extend / no writable layer to capture. Covered by self-test 23
(build base with Cmd/Env → synthesise an overlay upper + a whiteout →
`commit_image` → assert base-layer carried + exactly one commit layer +
Cmd/Env preserved + COMMIT history entry + the commit layer's tar holds the
added files and the `.wh.` marker).

**Decided by:** Claude (operator-approved scope — the Docker/container-runtime
port was green-lit by Q15). The `docker commit`→image-production vs. native
`container commit`→clone split is a Docker-parity choice within that scope, not
a genuine fork; both behaviours are retained under distinct verbs so nothing is
lost. `RUN`/`HEALTHCHECK` (in-container rootfs exec) remain gated on Q17.
