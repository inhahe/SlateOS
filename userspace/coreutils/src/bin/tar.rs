//! tar — tape archive utility.
//!
//! Usage: tar -c [-f ARCHIVE] [-v] [FILE...]   create archive
//!        tar -x [-f ARCHIVE] [-v] [-C DIR]    extract archive
//!        tar -t [-f ARCHIVE]                   list archive
//!
//! Supports basic POSIX/ustar tar format (uncompressed).
//! Files > 8GB and paths > 255 chars are not supported.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut create = false;
    let mut extract = false;
    let mut list = false;
    let mut verbose = false;
    let mut archive_file: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg[1..].chars() {
                match c {
                    'c' => create = true,
                    'x' => extract = true,
                    't' => list = true,
                    'v' => verbose = true,
                    'f' => {
                        i += 1;
                        if i < args.len() {
                            archive_file = Some(args[i].clone());
                        }
                    }
                    'C' => {
                        i += 1;
                        if i < args.len() {
                            directory = Some(args[i].clone());
                        }
                    }
                    _ => {
                        eprintln!("tar: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg.clone());
        }
        i += 1;
    }

    if create {
        do_create(&archive_file, &files, verbose);
    } else if extract {
        do_extract(&archive_file, directory.as_deref(), verbose);
    } else if list {
        do_list(&archive_file);
    } else {
        eprintln!("tar: must specify -c, -x, or -t");
        process::exit(1);
    }
}

// ============================================================================
// TAR header format (512 bytes, POSIX ustar)
// ============================================================================

const BLOCK_SIZE: usize = 512;

#[repr(C)]
struct TarHeader {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typeflag: u8,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    _pad: [u8; 12],
}

impl TarHeader {
    fn new() -> Self {
        Self {
            name: [0; 100],
            mode: [0; 8],
            uid: [0; 8],
            gid: [0; 8],
            size: [0; 12],
            mtime: [0; 12],
            checksum: [0; 8],
            typeflag: 0,
            linkname: [0; 100],
            magic: [0; 6],
            version: [0; 2],
            uname: [0; 32],
            gname: [0; 32],
            devmajor: [0; 8],
            devminor: [0; 8],
            prefix: [0; 155],
            _pad: [0; 12],
        }
    }

    fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(99);
        self.name[..len].copy_from_slice(&bytes[..len]);
    }

    fn set_octal(field: &mut [u8], value: u64) {
        let s = format!("{:0>width$o}", value, width = field.len() - 1);
        let bytes = s.as_bytes();
        let start = if bytes.len() >= field.len() {
            bytes.len() - (field.len() - 1)
        } else {
            0
        };
        let copy_len = bytes[start..].len().min(field.len() - 1);
        field[..copy_len].copy_from_slice(&bytes[start..start + copy_len]);
    }

    fn compute_checksum(&mut self) {
        // Fill checksum field with spaces for computation
        self.checksum = [b' '; 8];

        let header_bytes =
            unsafe { std::slice::from_raw_parts(self as *const _ as *const u8, BLOCK_SIZE) };
        let sum: u32 = header_bytes.iter().map(|&b| b as u32).sum();

        let s = format!("{:06o}\0 ", sum);
        self.checksum[..s.len().min(8)].copy_from_slice(&s.as_bytes()[..s.len().min(8)]);
    }

    fn as_bytes(&self) -> &[u8; BLOCK_SIZE] {
        unsafe { &*(self as *const Self as *const [u8; BLOCK_SIZE]) }
    }
}

fn do_create(archive_file: &Option<String>, files: &[String], verbose: bool) {
    let mut out: Box<dyn Write> = match archive_file {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdout()),
    };

    for path_str in files {
        let path = Path::new(path_str);
        if path.is_dir() {
            add_directory_recursive(path, &path_str, &mut out, verbose);
        } else {
            add_file(path, path_str, &mut out, verbose);
        }
    }

    // Two zero blocks mark end of archive
    let zero_block = [0u8; BLOCK_SIZE];
    let _ = out.write_all(&zero_block);
    let _ = out.write_all(&zero_block);
}

fn add_directory_recursive(dir: &Path, prefix: &str, out: &mut dyn Write, verbose: bool) {
    // Add directory entry
    let mut header = TarHeader::new();
    let name = format!("{}/", prefix);
    header.set_name(&name);
    TarHeader::set_octal(&mut header.mode, 0o755);
    TarHeader::set_octal(&mut header.uid, 0);
    TarHeader::set_octal(&mut header.gid, 0);
    TarHeader::set_octal(&mut header.size, 0);
    TarHeader::set_octal(&mut header.mtime, 0);
    header.typeflag = b'5'; // directory
    header.magic = *b"ustar\0";
    header.version = *b"00";
    header.compute_checksum();
    let _ = out.write_all(header.as_bytes());

    if verbose {
        eprintln!("{name}");
    }

    // Add contents
    if let Ok(entries) = fs::read_dir(dir) {
        for entry_result in entries {
            if let Ok(entry) = entry_result {
                let entry_path = entry.path();
                let entry_name = format!(
                    "{}/{}",
                    prefix,
                    entry.file_name().to_string_lossy()
                );
                if entry_path.is_dir() {
                    add_directory_recursive(&entry_path, &entry_name, out, verbose);
                } else {
                    add_file(&entry_path, &entry_name, out, verbose);
                }
            }
        }
    }
}

fn add_file(path: &Path, name: &str, out: &mut dyn Write, verbose: bool) {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("tar: {name}: {e}");
            return;
        }
    };

    let mut header = TarHeader::new();
    header.set_name(name);
    TarHeader::set_octal(&mut header.mode, meta.mode() as u64 & 0o7777);
    TarHeader::set_octal(&mut header.uid, meta.uid() as u64);
    TarHeader::set_octal(&mut header.gid, meta.gid() as u64);
    TarHeader::set_octal(&mut header.size, meta.len());
    TarHeader::set_octal(&mut header.mtime, meta.mtime() as u64);
    header.typeflag = b'0'; // regular file
    header.magic = *b"ustar\0";
    header.version = *b"00";
    header.compute_checksum();
    let _ = out.write_all(header.as_bytes());

    if verbose {
        eprintln!("{name}");
    }

    // Write file content
    if let Ok(mut f) = File::open(path) {
        let mut buf = [0u8; BLOCK_SIZE];
        loop {
            let n = f.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            // Pad last block with zeros
            if n < BLOCK_SIZE {
                buf[n..].fill(0);
            }
            let _ = out.write_all(&buf);
        }
    }
}

fn do_extract(archive_file: &Option<String>, directory: Option<&str>, verbose: bool) {
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    if let Some(dir) = directory {
        if let Err(e) = env::set_current_dir(dir) {
            eprintln!("tar: {dir}: {e}");
            process::exit(1);
        }
    }

    loop {
        let mut header_buf = [0u8; BLOCK_SIZE];
        if input.read_exact(&mut header_buf).is_err() {
            break;
        }

        // Check for end-of-archive (two zero blocks)
        if header_buf.iter().all(|&b| b == 0) {
            break;
        }

        let name = extract_string(&header_buf[..100]);
        let size = parse_octal(&header_buf[124..136]);
        let typeflag = header_buf[156];

        if name.is_empty() {
            break;
        }

        if verbose {
            eprintln!("{name}");
        }

        match typeflag {
            b'5' | b'\0' if name.ends_with('/') => {
                // Directory
                let _ = fs::create_dir_all(&name);
            }
            b'0' | b'\0' => {
                // Regular file
                if let Some(parent) = Path::new(&name).parent() {
                    let _ = fs::create_dir_all(parent);
                }

                let blocks = (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
                let mut file_data = Vec::with_capacity(size as usize);

                for _ in 0..blocks {
                    let mut block = [0u8; BLOCK_SIZE];
                    if input.read_exact(&mut block).is_err() {
                        break;
                    }
                    file_data.extend_from_slice(&block);
                }

                file_data.truncate(size as usize);
                if let Err(e) = fs::write(&name, &file_data) {
                    eprintln!("tar: {name}: {e}");
                }
            }
            _ => {
                // Skip unknown types
                let blocks = (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
                for _ in 0..blocks {
                    let mut block = [0u8; BLOCK_SIZE];
                    let _ = input.read_exact(&mut block);
                }
            }
        }
    }
}

fn do_list(archive_file: &Option<String>) {
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    loop {
        let mut header_buf = [0u8; BLOCK_SIZE];
        if input.read_exact(&mut header_buf).is_err() {
            break;
        }

        if header_buf.iter().all(|&b| b == 0) {
            break;
        }

        let name = extract_string(&header_buf[..100]);
        let size = parse_octal(&header_buf[124..136]);

        if name.is_empty() {
            break;
        }

        println!("{name}");

        // Skip file data blocks
        let blocks = (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
        for _ in 0..blocks {
            let mut block = [0u8; BLOCK_SIZE];
            let _ = input.read_exact(&mut block);
        }
    }
}

fn extract_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).to_string()
}

fn parse_octal(buf: &[u8]) -> u64 {
    let s = extract_string(buf);
    u64::from_str_radix(s.trim(), 8).unwrap_or(0)
}
