## TD-A-SPARSE-FSTRIM-WRONG-DEVICE — hole punching queued discards for a nonexistent device at a file offset

**Lane A. Found 2026-08-23, during the §261 byte-clean conversion.**

`fs::sparse::punch_hole` called
`fs::fstrim::notify_free(path, offset, actual_len)` — passing the punched
**file's** path and its **file** offset to a function whose parameter is a
**block device** and whose offset is a device LBA. Every other caller in
the tree passes `/dev/sda`, `/dev/nvme0` and the like. The call was wrong
on both axes at once.

**Why nothing broke yet:** `fstrim::issue_trim` is a stub — its first line
is `let _ = device;` — so the device name is discarded and only the byte
count is added to the statistics. The observable damage today is inflated
TRIM statistics and a discard queue holding entries keyed by a filename
that matches no registered device, which `flush(device_filter)` can never
select and `coalesce_ranges` groups into a phantom device of its own.

**Why it matters:** the moment a real `block_device::discard(device,
offset, length)` is wired in behind that stub — which is exactly what the
comment at `fstrim.rs:423` says is coming — this becomes a discard issued
at a *file* offset. On a device where the name happened to resolve, that
is data loss at an unrelated location on the volume. The bug is latent
precisely because the subsystem it depends on is unfinished, so it will
surface at the worst moment: when someone implements discard and assumes
the existing callers were correct.

**Action taken:** the call is removed and replaced with a comment
explaining why, at `fs/sparse.rs` in `punch_hole`. Removing it is the
correct interim state — a queued entry that cannot map to a device has no
value today and is a landmine tomorrow.

**The proper fix** is to notify fstrim with the *device extents* the hole
actually freed. That needs a file-offset-to-device-extent mapping — a
`fiemap`/`bmap` equivalent on the `FileSystem` trait — which does not
exist anywhere in the tree today (`grep -rn "fiemap\|fn bmap" kernel/src/fs`
returns nothing). Two steps:

1. Add `fn extents(&mut self, path: &Path, offset: u64, len: u64) ->
   KernelResult<Vec<(u64, u64)>>` to `FileSystem`, returning device-relative
   `(offset, length)` pairs, with a default implementation returning
   `Err(Unsupported)` so only filesystems that can answer do.
2. Have `punch_hole` (and `Vfs::remove`/`truncate`, which have the same
   gap and today notify fstrim not at all) call it and pass each extent
   plus the mount's backing device name to `notify_free`.

**Trigger to promote this to active work:** anyone implementing real block
device discard behind `fstrim::issue_trim`. Do not implement that stub
without doing step 1 and 2 first.
