## TD-B-EXISTS-COLLAPSES-UNREADABLE-INTO-ABSENT (lane B, 2026-09-10) — swept, one fixed, rest verified benign

**In short:** `Path::exists()` answers `false` for a path it could not *stat*,
not only for one that is absent — a directory component you cannot search reads
exactly like a missing file. Lane B uses it **130 times** and `try_exists()`,
the API that keeps the two apart, **zero times**. Swept for the dangerous
shape; one diagnostic was wrong and no data-loss case exists.

**Why it was swept.** Lane A found the same collapse in the kernel:
`Vfs::exists` is `stat().is_ok()`, and `stat` passes the VFS permission gate,
so `PermissionDenied` reached callers as "the path does not exist".
`fs::overlay::which_layer` used it to decide **which layer serves a file**, so
an unstattable upper file became `Layer::Lower` and the caller got the base
image's older content with no error. Their fix is `Vfs::exists_or_err`, which
is `optionalfile::read_or_empty` one subsystem over.

**The filter, which is lane A's and better than the one I used before.** Do not
grep for the idiom — 130 sites, almost all harmless. Ask **which way `false`
points**: `is_mounted() == false` means *go ahead*, `is_allowed() == false`
means *deny* and is safe. Mechanically, that is `if !x.exists()` with a write,
create, remove, copy or rename in the following few lines.

Six matched. Each was read, and the verdicts are the point:

| Site | `false` leads to | Verdict |
|---|---|---|
| `backup:833` | `create_dir_all` on a dir that exists | no-op; `create_dir_all` succeeds on an existing directory |
| `backup:1178` | refuse to delete, exit 1 | **fail-closed** — an unreadable manifest declines the `remove_dir_all` |
| `bootctl:423` | refuse to update, exit 1 | fail-closed |
| `capsh:1288` | refuse, exit 1 | fail-closed, but said **"No such file"** — fixed |
| `cpio:1125` | create with `-d`, else refuse | fail-closed without `-d`, explicit with it |
| `pkg:710` | rewrite a content-addressed blob | byte-identical by construction; the path is its SHA-256 |

**Fixed:** `setcap` now uses `try_exists()` — `Ok(false)` is absent, `Err` is
"could not tell", and each says so. Reporting "No such file" for a permission
error sends the user hunting a typo that is not there.

**Not fixed, deliberately:** the other 129 sites. A rewrite would be churn
against a real risk of introducing errors, and the sweep found the dangerous
direction is not represented. What matters is the *rule*, which is recorded
here: an `exists()` whose `false` licenses a write, a delete or a privilege is
the shape to fix; one whose `false` causes a refusal is already safe.
