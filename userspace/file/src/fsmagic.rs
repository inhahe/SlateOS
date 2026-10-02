//! libmagic's `fsmagic.c`: what the file system alone says about a name --
//! a directory, a device, a FIFO, a socket, a symbolic link, an empty file --
//! before anything is read from it. Set-user-ID, set-group-ID and sticky bits
//! are said first, and lead whatever follows.

use crate::apprentice::os_path;
use crate::buffer::{
    S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK, S_ISGID, S_ISUID,
    S_ISVTX, Stat,
};
use crate::funcs::Ms;
use crate::magic::*;
use crate::printf::Arg;

/// `BUFSIZ`: `readlink` reads `BUFSIZ - 1` bytes of a link at most.
const BUFSIZ: usize = 8192;

/// glibc's `major()` for a Linux `dev_t`.
#[must_use]
pub fn major(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}

/// glibc's `minor()`.
#[must_use]
pub fn minor(dev: u64) -> u64 {
    (dev & 0xff) | ((dev >> 12) & !0xff)
}

/// `bad_link`: a symbolic link to nothing.
fn bad_link(ms: &mut Ms, err: &std::io::Error, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if mime & MAGIC_MIME_TYPE != 0 {
        if ms.print_str(b"inode/symlink") == -1 {
            return -1;
        }
    } else if mime == 0 {
        if ms.flags & MAGIC_ERROR != 0 {
            let mut msg = b"broken symbolic link to ".to_vec();
            msg.extend_from_slice(buf);
            ms.error(Some(err), &msg);
            return -1;
        }
        if ms.printf(b"broken symbolic link to %s", &[Arg::Str(buf)]) == -1 {
            return -1;
        }
    }
    1
}

/// `handle_mime`: `inode/what`, and the charset when asked.
fn handle_mime(ms: &mut Ms, mime: u32, what: &[u8]) -> i32 {
    if mime & MAGIC_MIME_TYPE != 0 {
        if ms.printf(b"inode/%s", &[Arg::Str(what)]) == -1 {
            return -1;
        }
        if mime & MAGIC_MIME_ENCODING != 0 && ms.print_str(b"; charset=") == -1 {
            return -1;
        }
    }
    if mime & MAGIC_MIME_ENCODING != 0 && ms.print_str(b"binary") == -1 {
        return -1;
    }
    0
}

/// `file_fsmagic`: 1 when the name's type was said, 0 when the file has to be
/// read, -1 on an error. `sb` is filled with the `stat` (or `lstat`).
#[allow(clippy::too_many_lines)]
pub fn file_fsmagic(ms: &mut Ms, fname: Option<&[u8]>, sb: &mut Stat) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    let silent = ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0;
    let Some(fname) = fname else {
        return 0;
    };
    let mut did = 0usize;
    let comma = |did: &mut usize| -> &'static [u8] {
        let r: &[u8] = if *did > 0 { b", " } else { b"" };
        *did += 1;
        r
    };
    let path = os_path(fname);
    // `lstat`, so a link is seen as one -- unless `-L` follows them.
    let got = if ms.flags & MAGIC_SYMLINK == 0 {
        std::fs::symlink_metadata(&path)
    } else {
        std::fs::metadata(&path)
    };
    let md = match got {
        Ok(md) => md,
        Err(e) => {
            ms.errno = Some(crate::funcs::Errno::of(&e));
            if ms.flags & MAGIC_ERROR != 0 {
                let mut msg = b"cannot stat `".to_vec();
                msg.extend_from_slice(fname);
                msg.push(b'\'');
                ms.error(Some(&e), &msg);
                return -1;
            }
            let why = errmsg::strerror(&e);
            if ms.printf(b"cannot open `%s' (%s)", &[Arg::Str(fname), Arg::Str(why.as_bytes())]) == -1 {
                return -1;
            }
            return 0;
        }
    };
    *sb = Stat::from_metadata(&md);

    let mut ret = 1;
    if mime == 0 && !silent {
        for (bit, word) in [(S_ISUID, &b"setuid"[..]), (S_ISGID, b"setgid"), (S_ISVTX, b"sticky")] {
            if sb.mode & bit != 0 {
                let c = comma(&mut did);
                if ms.printf(b"%s%s", &[Arg::Str(c), Arg::Str(word)]) == -1 {
                    return -1;
                }
            }
        }
    }

    match sb.mode & S_IFMT {
        S_IFDIR => {
            if mime != 0 {
                if handle_mime(ms, mime, b"directory") == -1 {
                    return -1;
                }
            } else if !silent {
                let c = comma(&mut did);
                if ms.printf(b"%sdirectory", &[Arg::Str(c)]) == -1 {
                    return -1;
                }
            }
        }
        t @ (S_IFCHR | S_IFBLK) => {
            // With `-s`, read a device like a file.
            if ms.flags & MAGIC_DEVICES != 0 {
                ret = 0;
            } else if mime != 0 {
                let what: &[u8] = if t == S_IFCHR { b"chardevice" } else { b"blockdevice" };
                if handle_mime(ms, mime, what) == -1 {
                    return -1;
                }
            } else if !silent {
                let c = comma(&mut did);
                let fmt: &[u8] = if t == S_IFCHR {
                    b"%scharacter special (%ld/%ld)"
                } else {
                    b"%sblock special (%ld/%ld)"
                };
                if ms.printf(fmt, &[Arg::Str(c), Arg::I64(major(sb.rdev)), Arg::I64(minor(sb.rdev))]) == -1 {
                    return -1;
                }
            }
        }
        S_IFIFO => {
            if ms.flags & MAGIC_DEVICES == 0 {
                if mime != 0 {
                    if handle_mime(ms, mime, b"fifo") == -1 {
                        return -1;
                    }
                } else if !silent {
                    let c = comma(&mut did);
                    if ms.printf(b"%sfifo (named pipe)", &[Arg::Str(c)]) == -1 {
                        return -1;
                    }
                }
            }
        }
        S_IFLNK => {
            let target = match std::fs::read_link(&path) {
                Ok(t) => {
                    let mut b = crate::apprentice::os_bytes(t.as_os_str());
                    b.truncate(BUFSIZ - 1);
                    Ok(b)
                }
                Err(e) => Err(e),
            };
            let buf = match target {
                Ok(b) if !b.is_empty() => b,
                other => {
                    // `readlink`'s own error; an empty link (which Linux
                    // does not make) leaves C's `errno` at whatever it was.
                    let e = match other {
                        Err(e) => e,
                        Ok(_) => std::io::Error::from(std::io::ErrorKind::InvalidInput),
                    };
                    if ms.flags & MAGIC_ERROR != 0 {
                        let mut msg = b"unreadable symlink `".to_vec();
                        msg.extend_from_slice(fname);
                        msg.push(b'\'');
                        ms.error(Some(&e), &msg);
                        return -1;
                    }
                    if mime != 0 {
                        if handle_mime(ms, mime, b"symlink") == -1 {
                            return -1;
                        }
                    } else if !silent {
                        let c = comma(&mut did);
                        let why = errmsg::strerror(&e);
                        if ms.printf(
                            b"%sunreadable symlink `%s' (%s)",
                            &[Arg::Str(c), Arg::Str(fname), Arg::Str(why.as_bytes())],
                        ) == -1
                        {
                            return -1;
                        }
                    }
                    return finish(ms, silent, mime, did, ret);
                }
            };
            // A broken link says so and stops. On Linux the link's own name is
            // `stat`ed, since procfs links (`pipe:[3515864880]`) name nothing
            // that can be.
            if let Err(e) = std::fs::metadata(&path) {
                return bad_link(ms, &e, &buf);
            }
            if ms.flags & MAGIC_SYMLINK != 0 {
                // Upstream follows the link through `magic_file` here, with
                // every flag but `-L` cleared (`&=`, where `&= ~` was meant)
                // -- but `-L` means `stat` above, which never reports a link,
                // so this is not reached.
                let saved = ms.flags;
                ms.flags &= MAGIC_SYMLINK;
                let p = crate::magicapi::magic_file(ms, Some(&buf));
                ms.flags |= MAGIC_SYMLINK;
                let _ = saved;
                if p.is_none() {
                    return -1;
                }
            } else if mime != 0 {
                if handle_mime(ms, mime, b"symlink") == -1 {
                    return -1;
                }
            } else if !silent {
                let c = comma(&mut did);
                if ms.printf(b"%ssymbolic link to %s", &[Arg::Str(c), Arg::Str(&buf)]) == -1 {
                    return -1;
                }
            }
        }
        S_IFSOCK => {
            if mime != 0 {
                if handle_mime(ms, mime, b"socket") == -1 {
                    return -1;
                }
            } else if !silent {
                let c = comma(&mut did);
                if ms.printf(b"%ssocket", &[Arg::Str(c)]) == -1 {
                    return -1;
                }
            }
        }
        S_IFREG => {
            // A regular file that `stat` says is empty is said to be so here,
            // without reading it -- unless `-s`, since some systems report
            // raw disk partitions as empty.
            if ms.flags & MAGIC_DEVICES == 0 && sb.size == 0 {
                if mime != 0 {
                    if handle_mime(ms, mime, b"x-empty") == -1 {
                        return -1;
                    }
                } else if !silent {
                    let c = comma(&mut did);
                    if ms.printf(b"%sempty", &[Arg::Str(c)]) == -1 {
                        return -1;
                    }
                }
            } else {
                ret = 0;
            }
        }
        _ => {
            ms.error(None, format!("invalid mode 0{:o}", sb.mode).as_bytes());
            return -1;
        }
    }
    finish(ms, silent, mime, did, ret)
}

/// The end of `file_fsmagic`: a space before what the contents will add, and
/// no match when only an extension or Apple type was wanted.
fn finish(ms: &mut Ms, silent: bool, mime: u32, did: usize, ret: i32) -> i32 {
    if !silent && mime == 0 && did != 0 && ret == 0 && ms.print_str(b" ") == -1 {
        return -1;
    }
    if ret == 1 && silent {
        return 0;
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_numbers_split_as_glibc_splits_them() {
        // makedev(8, 1) and makedev(259, 70000).
        assert_eq!((major(0x801), minor(0x801)), (8, 1));
        let dev = (259u64 & 0xfff) << 8 | (70000u64 & 0xff) | ((70000u64 & !0xff) << 12);
        assert_eq!((major(dev), minor(dev)), (259, 70000));
    }
}
