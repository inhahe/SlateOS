//! Making a directory and every directory above it, then giving it its owner
//! and its mode: gnulib's `mkancesdirs.c`, `mkdir-p.c` and `dirchownmod.c`,
//! which `install -d` and `install -D` are built on.
//!
//! # What upstream's walk does, and what this one does instead
//!
//! Upstream walks a name the way a person at a shell would: it makes the
//! first component, `chdir`s into it, makes the second, and so on (gnulib's
//! `savewd`), so that every `mkdir` it issues names a single component. That
//! buys two things. A name longer than `PATH_MAX` can still be made, because
//! no call is handed more than one component of it; and once it has stepped
//! into a directory it made -- which it does with `O_NOFOLLOW` -- nobody can
//! redirect the rest of the walk by putting a symbolic link where that
//! directory was.
//!
//! This walk makes each directory by its whole name. The last step, the one
//! that hands out an owner and permission bits, is still taken through a
//! descriptor opened `O_NOFOLLOW` when the directory was just made, exactly
//! as upstream's [`dirchownmod`] takes it, so the step that grants access
//! cannot be pointed at something else. What it gives up is the other two: a
//! name longer than `PATH_MAX` fails with `ENAMETOOLONG` where upstream
//! carries on, and an ancestor swapped for a symbolic link between two of the
//! `mkdir`s can make the next directory land somewhere else. Both are
//! `known-issues.md` -> `TD-B-MKDIRP-WALKS-BY-NAME`, with what the proper walk
//! needs: a descriptor that can step into a directory its holder may search
//! but not read, which `chdir` can do and an `O_RDONLY` open cannot -- the
//! reason [`crate::dirfd`]'s walk, built for removal, is not this one.
//!
//! # Who prints
//!
//! Upstream's `make_dir_parents` prints its own diagnostics. These return a
//! [`Failure`] instead, which names the sentence and carries the error, and
//! the caller prints it after its own `prog: ` -- the same split
//! [`crate::copy`] uses, so a test can see what failed without capturing a
//! stream.

use crate::fsattr::{self, Link, On, Owner};
use crate::quote::{os_bytes, os_from_bytes};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `S_ISUID`, `S_ISGID` and `S_ISVTX`, as `mode_bits` and `mode` name them.
const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_ISVTX: u32 = 0o1000;

/// What makes one directory the walk needs: handed the directory's name, it
/// makes it or says why not. Upstream's `make_ancestor` callback, which
/// `install` uses to announce each directory under `-v` as it goes.
pub type MakeDir<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;

/// What a directory is to end up as: the arguments of gnulib's
/// `make_dir_parents` that describe the result rather than the walk.
#[derive(Clone, Copy)]
pub struct Target {
    /// `mode`: the permission bits it is to have.
    pub mode: u32,
    /// `mode_bits`: which bits of [`Self::mode`] are to be enforced. A bit
    /// outside it is left as the directory has it -- which matters for one
    /// that already existed, and for the set-user-ID, set-group-ID and sticky
    /// bits, which `mkdir` does not reliably set.
    pub mode_bits: u32,
    /// The owner and group to give it; `None` for either leaves it alone.
    pub owner: Owner,
    /// `preserve_existing`: a directory that already exists is accepted as it
    /// is, with no change of owner or mode.
    pub preserve_existing: bool,
}

/// Where [`mkancesdirs`] stopped, and why.
#[derive(Debug)]
pub struct Stopped {
    /// The ancestor it could not make or step into -- and the name upstream's
    /// diagnostic then gives, because its walk truncates the caller's string
    /// there and leaves it so: `install -D f a.txt/sub/g`, with `a.txt` a
    /// file, says `cannot create directory 'a.txt'`.
    pub at: PathBuf,
    pub err: io::Error,
}

/// What went wrong, as the sentence upstream prints for it. The caller adds
/// its program name and the name -- see [`Failure::subject`] and
/// [`Failure::sentence`].
#[derive(Debug)]
pub enum Failure {
    /// `cannot create directory %s`: making the directory failed.
    Create(io::Error),
    /// `cannot create directory %s` about an ancestor, which [`Self::subject`]
    /// names rather than the directory.
    Ancestor(Stopped),
    /// `cannot stat %s`: it was there, and could not be looked at.
    Stat(io::Error),
    /// `cannot change permissions of %s`, or `cannot change owner and
    /// permissions of %s` when an owner or group was asked for too.
    Attributes {
        /// Whether an owner or group was being given as well.
        owner_too: bool,
        err: io::Error,
    },
}

impl Failure {
    /// The name the sentence is about: `dir`, or the ancestor the walk
    /// stopped at.
    #[must_use]
    pub fn subject<'a>(&'a self, dir: &'a Path) -> &'a Path {
        match self {
            Failure::Ancestor(stopped) => &stopped.at,
            _ => dir,
        }
    }

    /// The sentence, with the [`Self::subject`] already quoted the way the
    /// caller quotes it -- `make_dir_parents` upstream uses `quote`, the
    /// locale's quotation marks.
    #[must_use]
    pub fn sentence(&self, dir: &str) -> String {
        let (what, err) = match self {
            Failure::Create(e) | Failure::Ancestor(Stopped { err: e, .. }) => {
                ("cannot create directory", e)
            }
            Failure::Stat(e) => ("cannot stat", e),
            Failure::Attributes {
                owner_too: false,
                err,
            } => ("cannot change permissions of", err),
            Failure::Attributes {
                owner_too: true,
                err,
            } => ("cannot change owner and permissions of", err),
        };
        format!("{what} {dir}: {}", crate::errmsg::strerror(err))
    }
}

/// gnulib's `mkancesdirs`: make every missing ancestor of `file`, calling
/// `make_dir` with the name of each.
///
/// An ancestor is a prefix of `file` that ends at a separator with something
/// after it, so `a/b/c` has the ancestors `a` and `a/b`, and `a/b/` has only
/// `a`: a trailing separator does not make the last component an ancestor. A
/// component of `.` is skipped, and `..` is stepped into without being made,
/// both as upstream does. `make_dir` failing is not yet an error -- the
/// directory may simply exist -- so what decides is whether the walk can then
/// step into it; see [`step_into`].
///
/// # Errors
///
/// The step into an ancestor failing, as upstream's `chdir` would, with the
/// ancestor's name. When that is because the ancestor does not exist and
/// `make_dir` had failed to make it, the error is `make_dir`'s, which says
/// why.
pub fn mkancesdirs(file: &Path, make_dir: MakeDir<'_>) -> Result<(), Stopped> {
    let bytes = os_bytes(file.as_os_str());
    // Upstream's `sep`: just past the last byte of the latest component -- the
    // separator that ends it -- and `component`, where that component began.
    let mut sep: Option<usize> = None;
    let mut component = 0usize;
    // Not reset when `make_dir` fails, which is upstream's: a failure keeps
    // whatever the last success or `..` left here.
    let mut made_dir = false;
    let mut at = 0usize;
    while let Some(&c) = bytes.get(at) {
        at = at.saturating_add(1);
        let next = bytes.get(at).copied();
        if next == Some(b'/') {
            if c != b'/' {
                sep = Some(at);
            }
        } else if c == b'/'
            && next.is_some()
            && let Some(end) = sep
        {
            let name = bytes.get(component..end).unwrap_or_default();
            if name != b"." {
                let prefix = PathBuf::from(os_from_bytes(bytes.get(..end).unwrap_or_default()));
                let mut make_dir_err = None;
                if name == b".." {
                    made_dir = false;
                } else {
                    match make_dir(&prefix) {
                        Ok(()) => made_dir = true,
                        Err(e) => make_dir_err = Some(e),
                    }
                }
                if let Err(e) = step_into(&prefix, made_dir) {
                    let err = match make_dir_err {
                        Some(made) if e.kind() == io::ErrorKind::NotFound => made,
                        _ => e,
                    };
                    return Err(Stopped { at: prefix, err });
                }
            }
            component = at;
        }
    }
    Ok(())
}

/// Upstream's `savewd_chdir` into an ancestor, asked of the name: whether the
/// walk could step into `dir`.
///
/// One the walk just made is opened `O_NOFOLLOW` upstream, so a symbolic link
/// there now is refused; one it found is entered with a plain `chdir`, which
/// follows. Search permission is not asked about separately: lacking it fails
/// the next `mkdir` with the `EACCES` the `chdir` would have given.
fn step_into(dir: &Path, made: bool) -> io::Result<()> {
    let meta = if made {
        fs::symlink_metadata(dir)?
    } else {
        fs::metadata(dir)?
    };
    if meta.is_dir() {
        Ok(())
    } else if meta.file_type().is_symlink() {
        Err(eloop())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// `ELOOP`, which is what `open (…, O_NOFOLLOW)` says of a symbolic link.
fn eloop() -> io::Error {
    #[cfg(unix)]
    {
        io::Error::from_raw_os_error(40)
    }
    #[cfg(not(unix))]
    {
        io::Error::other("Too many levels of symbolic links")
    }
}

/// gnulib's `make_dir_parents`: make `dir`, its missing ancestors first when
/// `make_ancestor` is given, then give it [`Target`]'s owner and mode.
///
/// `announce` is told of `dir` the moment it is made, whatever happens after;
/// `make_ancestor` announces its own. A `dir` that already exists is not an
/// error when ancestors are being made (`mkdir -p`'s rule): it gets the owner
/// and mode instead, unless [`Target::preserve_existing`] says to leave it.
///
/// # Errors
///
/// A [`Failure`], for the caller to print with `dir`'s name.
pub fn make_dir_parents(
    dir: &Path,
    make_ancestor: Option<MakeDir<'_>>,
    target: Target,
    announce: &mut dyn FnMut(&Path),
) -> Result<(), Failure> {
    let making_ancestors = make_ancestor.is_some();
    // Upstream's `mkdir_errno`, seeded with `savewd_errno`: a walk by name has
    // no working directory to have lost, so it starts clear.
    let mut mkdir_err: Option<io::Error> = None;
    if let Some(make) = make_ancestor
        && let Err(stopped) = mkancesdirs(dir, make)
    {
        // Upstream's `prefix_len < 0`, which goes straight to `cannot create
        // directory` -- about the ancestor, its walk having cut `dir` there.
        return Err(Failure::Ancestor(stopped));
    }

    {
        // If the owner may change, or the directory would be writable by
        // others while its special bits are not yet set, it is made more
        // restrictive first, "so unauthorized users cannot nip in before the
        // directory is ready".
        let keep_owner = target.owner.is_empty();
        let keep_special_mode_bits =
            ((target.mode_bits & (S_ISUID | S_ISGID)) | (target.mode & S_ISVTX)) == 0;
        let mut mkdir_mode = target.mode;
        if !keep_owner {
            mkdir_mode &= !0o077;
        } else if !keep_special_mode_bits {
            mkdir_mode &= !0o022;
        }

        let mut preserve_existing = target.preserve_existing;
        // `mkdir_mode = -1` upstream, for "this call did not make it".
        let made = match crate::copy::create_dir_with_mode(dir, mkdir_mode) {
            Ok(()) => {
                // Whether the caller cares what the umask did to the bits.
                let umask_must_be_ok = (target.mode & target.mode_bits & 0o777) == 0;
                announce(dir);
                preserve_existing = keep_owner && keep_special_mode_bits && umask_must_be_ok;
                true
            }
            Err(e) => {
                mkdir_err = Some(e);
                false
            }
        };

        if preserve_existing {
            match &mkdir_err {
                None => return Ok(()),
                Some(e) if e.kind() != io::ErrorKind::NotFound && making_ancestors => {
                    match fs::metadata(dir) {
                        Ok(m) if m.is_dir() => return Ok(()),
                        Ok(_) => {}
                        Err(stat_err) => {
                            if e.kind() == io::ErrorKind::AlreadyExists
                                && stat_err.kind() != io::ErrorKind::NotFound
                                && stat_err.kind() != io::ErrorKind::NotADirectory
                            {
                                return Err(Failure::Stat(stat_err));
                            }
                        }
                    }
                }
                Some(_) => {}
            }
        } else {
            match dirchownmod(dir, made, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let mkdir_was_absent = mkdir_err
                        .as_ref()
                        .is_some_and(|m| m.kind() == io::ErrorKind::NotFound);
                    if mkdir_err.is_none()
                        || (!mkdir_was_absent
                            && making_ancestors
                            && e.kind() != io::ErrorKind::NotADirectory)
                    {
                        return Err(Failure::Attributes {
                            owner_too: !keep_owner,
                            err: e,
                        });
                    }
                }
            }
        }
    }

    Err(Failure::Create(
        mkdir_err.unwrap_or_else(|| io::Error::from(io::ErrorKind::Other)),
    ))
}

/// gnulib's `dirchownmod`, reached as `make_dir_parents` reaches it: the
/// directory opened first -- `O_NOFOLLOW` when this call made it -- and its
/// owner and mode set through the descriptor, falling back to the name when
/// the open fails, as upstream's `savewd_chdir` does.
///
/// The owner goes first: on some systems a `chown` clears the set-user-ID and
/// set-group-ID bits, so a mode set before it would not survive it. Any bit
/// that may have been cleared that way is counted as needing to be set again.
fn dirchownmod(dir: &Path, made: bool, target: Target) -> io::Result<()> {
    match open_dir(dir, made) {
        Ok(handle) => {
            let meta = handle.metadata()?;
            settle(On::File(&handle), On::File(&handle), &meta, target)
        }
        Err(_) => {
            // Upstream's `dirchownmod (-1, dir, …)`: by name, `stat`ed
            // following, owned with `lchown` when this call made it.
            let meta = fs::metadata(dir)?;
            let owner_on = if made {
                On::Path(dir, Link::NoFollow)
            } else {
                On::Path(dir, Link::Follow)
            };
            settle(owner_on, On::Path(dir, Link::Follow), &meta, target)
        }
    }
}

/// The body of [`dirchownmod`], once it has something to act on.
fn settle(
    owner_on: On<'_>,
    mode_on: On<'_>,
    meta: &fs::Metadata,
    target: Target,
) -> io::Result<()> {
    if !meta.is_dir() {
        return Err(io::Error::from(io::ErrorKind::NotADirectory));
    }
    let dir_mode = fsattr::permission_bits(meta);
    let now = fsattr::owner_of(meta);
    let mut indeterminate = 0;
    let changes_owner = target.owner.uid.is_some_and(|u| Some(u) != now.uid)
        || target.owner.gid.is_some_and(|g| Some(g) != now.gid);
    if changes_owner {
        fsattr::set_owner(owner_on, target.owner)?;
        if dir_mode & 0o111 != 0 {
            indeterminate = dir_mode & (S_ISUID | S_ISGID);
        }
    }
    if ((dir_mode ^ target.mode) | indeterminate) & target.mode_bits != 0 {
        let chmod_mode = target.mode | (dir_mode & 0o7777 & !target.mode_bits);
        fsattr::set_mode(mode_on, chmod_mode)?;
    }
    Ok(())
}

/// Open `dir` as a directory, refusing a symbolic link when `nofollow`.
#[cfg(unix)]
fn open_dir(dir: &Path, nofollow: bool) -> io::Result<fs::File> {
    use crate::dirfd::oflag;
    use std::os::unix::fs::OpenOptionsExt;
    let flags = oflag::DIRECTORY | oflag::NONBLOCK | if nofollow { oflag::NOFOLLOW } else { 0 };
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(dir)
}

/// A host with no directory descriptors goes by name, which [`dirchownmod`]
/// falls back to on any failure here.
#[cfg(not(unix))]
fn open_dir(_dir: &Path, _nofollow: bool) -> io::Result<fs::File> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    /// Every name `mkancesdirs` asks to have made, in order.
    fn ancestors(file: &str) -> (Vec<String>, Result<(), Stopped>) {
        let mut asked = Vec::new();
        let result = mkancesdirs(Path::new(file), &mut |p: &Path| {
            asked.push(p.to_string_lossy().into_owned());
            // Pretend each exists already; the step into it then fails, but
            // only after the name has been recorded.
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        });
        (asked, result)
    }

    #[test]
    fn the_ancestors_are_the_prefixes_before_the_last_component() {
        let dir = ScratchDir::new("mkdirp_prefixes");
        let base = dir.dir().to_string_lossy().into_owned();
        // Every ancestor of `base/a/b/c` up to `base` exists, so the walk asks
        // for each prefix and steps into the ones that are there.
        let mut asked = Vec::new();
        let file = format!("{base}/a/b/c");
        let result = mkancesdirs(Path::new(&file), &mut |p: &Path| {
            asked.push(p.to_string_lossy().into_owned());
            fs::create_dir(p)
        });
        assert!(result.is_ok(), "{result:?}");
        assert!(asked.ends_with(&[format!("{base}/a"), format!("{base}/a/b")]));
        assert!(Path::new(&format!("{base}/a/b")).is_dir());
        assert!(
            !Path::new(&file).exists(),
            "the last component is not an ancestor"
        );
    }

    #[test]
    fn dot_is_skipped_and_dotdot_is_not_made() {
        let (asked, _) = ancestors("./x/../y/z");
        assert_eq!(asked.first().map(String::as_str), Some("./x"));
        assert!(!asked.iter().any(|a| a.ends_with("..")), "{asked:?}");
    }

    #[test]
    fn a_trailing_separator_does_not_make_the_last_component_an_ancestor() {
        let (asked, result) = ancestors("only/");
        assert!(asked.is_empty(), "{asked:?}");
        assert!(result.is_ok());
        let (asked, _) = ancestors("a//b/");
        assert_eq!(asked, vec!["a".to_string()]);
    }

    #[test]
    fn a_missing_ancestor_reports_why_it_could_not_be_made() {
        let (asked, result) = ancestors("nosuch-mkdirp-ancestor/b/c");
        assert_eq!(asked, vec!["nosuch-mkdirp-ancestor".to_string()]);
        // The step in found nothing (`ENOENT`), and `make_dir` had failed, so
        // the error reported is `make_dir`'s -- upstream's `if (make_dir_errno
        // != 0 && errno == ENOENT) errno = make_dir_errno`. The fake fails
        // every `make_dir` with `EEXIST`, which is what comes back.
        let stopped = result.unwrap_err();
        assert_eq!(stopped.err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(stopped.at, PathBuf::from("nosuch-mkdirp-ancestor"));
    }

    #[test]
    fn an_ancestor_that_is_a_file_is_not_a_directory() {
        let dir = ScratchDir::new("mkdirp_file_ancestor");
        let file = dir.dir().join("f");
        fs::write(&file, b"x").unwrap();
        // `/`, not `Path::join`: the walk splits on the target's separator,
        // and on a Windows host `join` would write a backslash.
        let below = PathBuf::from(format!("{}/sub/leaf", file.display()));
        let result = mkancesdirs(&below, &mut |p: &Path| fs::create_dir(p));
        let stopped = result.unwrap_err();
        assert_eq!(stopped.err.kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            stopped.at, file,
            "the walk names the ancestor it stopped at"
        );
    }

    fn target(mode: u32) -> Target {
        Target {
            mode,
            mode_bits: 0o7777,
            owner: Owner::default(),
            preserve_existing: false,
        }
    }

    #[test]
    fn a_new_directory_is_announced_and_its_ancestors_made() {
        let dir = ScratchDir::new("mkdirp_new");
        let leaf = PathBuf::from(format!("{}/p/q", dir.dir().display()));
        let mut made = Vec::new();
        let mut announced = Vec::new();
        let result = make_dir_parents(
            &leaf,
            Some(&mut |p: &Path| {
                made.push(p.to_path_buf());
                fs::create_dir(p)
            }),
            target(0o755),
            &mut |p: &Path| announced.push(p.to_path_buf()),
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(leaf.is_dir());
        assert_eq!(announced, vec![leaf.clone()]);
        assert!(made.last().is_some_and(|p| p.ends_with("p")));
    }

    #[test]
    fn an_existing_file_cannot_be_made_a_directory() {
        let dir = ScratchDir::new("mkdirp_over_file");
        let file = dir.dir().join("f");
        fs::write(&file, b"x").unwrap();
        let result = make_dir_parents(
            &file,
            Some(&mut |p: &Path| fs::create_dir(p)),
            target(0o755),
            &mut |_: &Path| {},
        );
        match result {
            Err(Failure::Create(e)) => assert_eq!(e.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected `cannot create directory`, got {other:?}"),
        }
    }

    #[test]
    fn an_existing_directory_is_accepted_when_ancestors_are_made() {
        let dir = ScratchDir::new("mkdirp_existing");
        let mut announced = 0;
        let result = make_dir_parents(
            dir.dir(),
            Some(&mut |p: &Path| fs::create_dir(p)),
            target(0o755),
            &mut |_: &Path| announced += 1,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(announced, 0, "nothing was made, so nothing is announced");
    }

    #[test]
    fn without_ancestors_a_missing_parent_is_the_failure() {
        let dir = ScratchDir::new("mkdirp_no_parent");
        let leaf = PathBuf::from(format!("{}/absent/leaf", dir.dir().display()));
        match make_dir_parents(&leaf, None, target(0o755), &mut |_: &Path| {}) {
            Err(Failure::Create(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            other => panic!("expected `cannot create directory`, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_mode_is_settled_on_an_existing_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = ScratchDir::new("mkdirp_mode");
        let sub = dir.dir().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        let result = make_dir_parents(
            &sub,
            Some(&mut |p: &Path| fs::create_dir(p)),
            target(0o700),
            &mut |_: &Path| {},
        );
        assert!(result.is_ok(), "{result:?}");
        let mode = fs::metadata(&sub).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn the_sentences_are_upstreams() {
        let e = || io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(
            Failure::Create(e()).sentence("'d'"),
            "cannot create directory 'd': No such file or directory"
        );
        assert_eq!(
            Failure::Attributes {
                owner_too: true,
                err: e()
            }
            .sentence("'d'"),
            "cannot change owner and permissions of 'd': No such file or directory"
        );
        assert_eq!(
            Failure::Attributes {
                owner_too: false,
                err: e()
            }
            .sentence("'d'"),
            "cannot change permissions of 'd': No such file or directory"
        );
    }
}
