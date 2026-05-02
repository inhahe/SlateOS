//! ext4 filesystem implementation (read-write).
//!
//! This is a native Rust implementation that understands the ext4 on-disk
//! format.  It maintains full compatibility with Linux ext4 — disk images
//! created here are mountable on Linux and vice versa.
//!
//! ## Design
//!
//! The design spec mandates: "Port ext4 first. Don't write a custom
//! filesystem."  While this is a Rust implementation rather than a C port,
//! it faithfully implements the ext4 on-disk format as documented in the
//! Linux kernel source (`fs/ext4/`) and the ext4 wiki.
//!
//! ## Block Size
//!
//! Standard ext4 uses 4 KiB blocks (the default `mkfs.ext4` setting).
//! Our OS uses 16 KiB pages.  The block device layer handles the mismatch:
//! each page contains 4 ext4 blocks.  We read/write at 4 KiB granularity
//! through the buffer cache.
//!
//! ## Phased Implementation
//!
//! 1. **Read-only** — superblock, block groups, inode lookup, directory
//!    traversal, extent-based file reading.
//! 2. **Read-write** — file creation, deletion, inode allocation, block
//!    allocation, directory insertion.
//! 3. **Journal** — write-ahead logging for crash recovery.
//!
//! Currently: Phase 1 (on-disk structures and superblock parsing).

pub mod balloc;
pub mod driver;
pub mod io;
pub mod journal;
pub mod ondisk;
pub mod superblock;
pub mod vfs_impl;

use alloc::boxed::Box;
use alloc::format;
use crate::error::KernelResult;
use crate::serial_println;

/// Try to mount an ext4 filesystem from the given device at the specified path.
///
/// Reads the superblock, validates the ext4 magic number and feature flags,
/// then registers the filesystem with the VFS.
///
/// # Errors
///
/// Returns an error if the device doesn't contain a valid ext4 filesystem
/// or if the mount point is already in use.
pub fn mount(device: &str, mount_path: &str) -> KernelResult<()> {
    let fs = vfs_impl::Ext4Fs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!("[ext4] Mounted {} at {}", device, mount_path);
    Ok(())
}

/// Probe a block device for an ext4 superblock.
///
/// Returns `true` if the device contains a valid ext4 filesystem.
/// Does not mount or modify the device.
pub fn probe(device: &str) -> bool {
    // Try to open — this reads and validates the superblock.
    driver::Ext4Driver::open(device).is_ok()
}

/// Self-test: verify ext4 structures parse correctly.
///
/// This test only runs if an ext4 device is available.  If no ext4
/// filesystem is mounted, the test is skipped silently.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ext4] Running self-test...");

    // Check if an ext4 filesystem is mounted anywhere.
    let mounts = crate::fs::Vfs::mounts();
    let ext4_mount = mounts.iter().find(|(_, fs_type)| fs_type == "ext4");

    let mount_path = match ext4_mount {
        Some((path, _)) => path.clone(),
        None => {
            serial_println!("[ext4]   No ext4 filesystem mounted — skipping self-test.");
            return Ok(());
        }
    };

    serial_println!("[ext4]   ext4 mounted at '{}' — testing...", mount_path);

    // List the root directory of the ext4 mount.
    let root = if mount_path == "/" { "/".into() } else { mount_path.clone() };
    let entries = crate::fs::Vfs::readdir(&root)?;
    serial_println!("[ext4]   Root directory ({} entries):", entries.len());
    for entry in &entries {
        let type_str = match entry.entry_type {
            crate::fs::EntryType::File => "FILE",
            crate::fs::EntryType::Directory => "DIR ",
            crate::fs::EntryType::Symlink => "LINK",
            crate::fs::EntryType::VolumeLabel => "VOL ",
        };
        serial_println!(
            "[ext4]     {} {:20} {} bytes",
            type_str, entry.name, entry.size
        );
    }

    // Try stat on the root.
    let root_stat = crate::fs::Vfs::stat(&root)?;
    serial_println!(
        "[ext4]   Root stat: type={:?}, size={}",
        root_stat.entry_type, root_stat.size
    );

    // Try to read the first regular file we find (if any).
    if let Some(first_file) = entries.iter().find(|e| e.entry_type == crate::fs::EntryType::File) {
        let file_path = if root == "/" {
            format!("/{}", first_file.name)
        } else {
            format!("{}/{}", root, first_file.name)
        };
        match crate::fs::Vfs::read_file(&file_path) {
            Ok(data) => {
                serial_println!(
                    "[ext4]   Read '{}': {} bytes",
                    first_file.name, data.len()
                );
                // Show first 64 bytes as text if valid UTF-8.
                let preview_len = data.len().min(64);
                if let Ok(text) = core::str::from_utf8(data.get(..preview_len).unwrap_or(&[])) {
                    serial_println!("[ext4]     Preview: {}", text.trim_end());
                }
            }
            Err(e) => {
                serial_println!(
                    "[ext4]   WARNING: Could not read '{}': {:?}",
                    first_file.name, e
                );
            }
        }
    }

    // --- Extended attribute (xattr) tests ---
    serial_println!("[ext4]   Testing extended attributes...");
    {
        // Create a temporary test file for xattr tests.
        let xattr_path = if root == "/" {
            alloc::string::String::from("/_ext4_xattr_test")
        } else {
            format!("{}/_ext4_xattr_test", root)
        };
        crate::fs::Vfs::write_file(&xattr_path, b"xattr test data")?;

        // Initially, no xattrs should be set.
        let keys = crate::fs::Vfs::list_xattrs(&xattr_path)?;
        if !keys.is_empty() {
            serial_println!("[ext4]   FAIL: new file should have no xattrs, got {}", keys.len());
            let _ = crate::fs::Vfs::remove(&xattr_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     list_xattrs on new file: empty OK");

        // Set an xattr.
        crate::fs::Vfs::set_xattr(&xattr_path, "user.test_key", b"test_value")?;
        serial_println!("[ext4]     set_xattr user.test_key OK");

        // Read it back.
        let val = crate::fs::Vfs::get_xattr(&xattr_path, "user.test_key")?;
        if val != b"test_value" {
            serial_println!("[ext4]   FAIL: get_xattr returned {:?}, expected 'test_value'", val);
            let _ = crate::fs::Vfs::remove(&xattr_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     get_xattr user.test_key: {:?} OK", core::str::from_utf8(&val).unwrap_or("?"));

        // Set a second xattr.
        crate::fs::Vfs::set_xattr(&xattr_path, "user.another", b"second value")?;
        let keys = crate::fs::Vfs::list_xattrs(&xattr_path)?;
        if keys.len() != 2 {
            serial_println!("[ext4]   FAIL: expected 2 xattrs, got {}", keys.len());
            let _ = crate::fs::Vfs::remove(&xattr_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     list_xattrs after 2 sets: {} keys OK", keys.len());

        // Overwrite the first xattr with a new value.
        crate::fs::Vfs::set_xattr(&xattr_path, "user.test_key", b"updated")?;
        let val = crate::fs::Vfs::get_xattr(&xattr_path, "user.test_key")?;
        if val != b"updated" {
            serial_println!("[ext4]   FAIL: overwritten xattr = {:?}, expected 'updated'", val);
            let _ = crate::fs::Vfs::remove(&xattr_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     overwrite xattr value OK");

        // Remove one xattr.
        crate::fs::Vfs::remove_xattr(&xattr_path, "user.test_key")?;
        let keys = crate::fs::Vfs::list_xattrs(&xattr_path)?;
        if keys.len() != 1 || keys.first().map(|s| s.as_str()) != Some("user.another") {
            serial_println!("[ext4]   FAIL: after remove, expected ['user.another'], got {:?}", keys);
            let _ = crate::fs::Vfs::remove(&xattr_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     remove_xattr + verify remaining OK");

        // Getting a removed xattr should fail.
        match crate::fs::Vfs::get_xattr(&xattr_path, "user.test_key") {
            Err(crate::error::KernelError::NotFound) => {
                serial_println!("[ext4]     get removed xattr: NotFound OK");
            }
            other => {
                serial_println!("[ext4]   FAIL: get removed xattr should be NotFound, got {:?}", other);
                let _ = crate::fs::Vfs::remove(&xattr_path);
                return Err(crate::error::KernelError::InternalError);
            }
        }

        // Delete the test file — xattr block should be freed.
        crate::fs::Vfs::remove(&xattr_path)?;
        serial_println!("[ext4]     xattr test file cleaned up OK");
    }

    // --- Symlink tests ---
    serial_println!("[ext4]   Testing symlinks...");
    {
        let target_path = if root == "/" {
            alloc::string::String::from("/_ext4_symlink_target")
        } else {
            format!("{}/_ext4_symlink_target", root)
        };
        let link_path = if root == "/" {
            alloc::string::String::from("/_ext4_symlink_link")
        } else {
            format!("{}/_ext4_symlink_link", root)
        };

        // Create a target file and a symlink to it.
        crate::fs::Vfs::write_file(&target_path, b"symlink target content")?;
        crate::fs::Vfs::symlink(&link_path, &target_path)?;

        // readlink should return the target path.
        let target_read = crate::fs::Vfs::readlink(&link_path)?;
        if target_read != target_path {
            serial_println!("[ext4]   FAIL: readlink = '{}', expected '{}'", target_read, target_path);
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&target_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     readlink OK: {}", target_read);

        // lstat on the symlink should return Symlink type.
        let link_stat = crate::fs::Vfs::lstat(&link_path)?;
        if link_stat.entry_type != crate::fs::EntryType::Symlink {
            serial_println!("[ext4]   FAIL: lstat on symlink should be Symlink, got {:?}", link_stat.entry_type);
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&target_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     lstat type=Symlink OK");

        // stat (follow) on the symlink should return File type.
        let target_stat = crate::fs::Vfs::stat(&link_path)?;
        if target_stat.entry_type != crate::fs::EntryType::File {
            serial_println!("[ext4]   FAIL: stat on symlink should follow to File, got {:?}", target_stat.entry_type);
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&target_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     stat follows symlink to File OK");

        // Read through the symlink should return the target's content.
        let content = crate::fs::Vfs::read_file(&link_path)?;
        if content != b"symlink target content" {
            serial_println!("[ext4]   FAIL: read through symlink returned wrong data");
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&target_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     read through symlink OK ({} bytes)", content.len());

        // Clean up.
        crate::fs::Vfs::remove(&link_path)?;
        crate::fs::Vfs::remove(&target_path)?;
        serial_println!("[ext4]     symlink test files cleaned up OK");
    }

    // --- Timestamp (set_times) test ---
    serial_println!("[ext4]   Testing set_times...");
    {
        let ts_path = if root == "/" {
            alloc::string::String::from("/_ext4_timestamp_test")
        } else {
            format!("{}/_ext4_timestamp_test", root)
        };
        crate::fs::Vfs::write_file(&ts_path, b"timestamp test")?;

        // Set specific timestamps (1_700_000_000 seconds = 2023-11-14).
        let ts_ns: u64 = 1_700_000_000_000_000_000;
        crate::fs::Vfs::set_times(&ts_path, ts_ns, ts_ns)?;

        // Read back via metadata and verify (ext4 stores seconds, so
        // we lose sub-second precision).
        let meta = crate::fs::Vfs::metadata(&ts_path)?;
        let expected_sec = 1_700_000_000_u64;
        let actual_atime_sec = meta.accessed_ns / 1_000_000_000;
        let actual_mtime_sec = meta.modified_ns / 1_000_000_000;
        if actual_atime_sec != expected_sec || actual_mtime_sec != expected_sec {
            serial_println!(
                "[ext4]   FAIL: set_times atime_sec={}, mtime_sec={}, expected {}",
                actual_atime_sec, actual_mtime_sec, expected_sec
            );
            let _ = crate::fs::Vfs::remove(&ts_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     set_times: atime/mtime match expected epoch OK");

        crate::fs::Vfs::remove(&ts_path)?;
        serial_println!("[ext4]     timestamp test file cleaned up OK");
    }

    // --- Hard link tests ---
    serial_println!("[ext4]   Testing hard links...");
    {
        let file_path = if root == "/" {
            alloc::string::String::from("/_ext4_hardlink_src")
        } else {
            format!("{}/_ext4_hardlink_src", root)
        };
        let link_path = if root == "/" {
            alloc::string::String::from("/_ext4_hardlink_dst")
        } else {
            format!("{}/_ext4_hardlink_dst", root)
        };

        // Create a source file.
        crate::fs::Vfs::write_file(&file_path, b"hard link test data")?;

        // Verify initial nlinks = 1.
        let meta = crate::fs::Vfs::metadata(&file_path)?;
        if meta.nlinks != 1 {
            serial_println!("[ext4]   FAIL: initial nlinks = {}, expected 1", meta.nlinks);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     initial nlinks=1 OK");

        // Create a hard link.
        crate::fs::Vfs::link(&file_path, &link_path)?;
        serial_println!("[ext4]     link() OK");

        // Both paths should now show nlinks = 2.
        let meta_src = crate::fs::Vfs::metadata(&file_path)?;
        let meta_dst = crate::fs::Vfs::metadata(&link_path)?;
        if meta_src.nlinks != 2 || meta_dst.nlinks != 2 {
            serial_println!(
                "[ext4]   FAIL: after link, src.nlinks={}, dst.nlinks={}, expected 2",
                meta_src.nlinks, meta_dst.nlinks
            );
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     both paths show nlinks=2 OK");

        // Reading through the link should return the same content.
        let content = crate::fs::Vfs::read_file(&link_path)?;
        if content != b"hard link test data" {
            serial_println!("[ext4]   FAIL: read through hard link returned wrong data");
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     read through hard link OK ({} bytes)", content.len());

        // Both should have the same file size.
        if meta_src.size != meta_dst.size {
            serial_println!(
                "[ext4]   FAIL: size mismatch: src={}, dst={}",
                meta_src.size, meta_dst.size
            );
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     sizes match ({} bytes) OK", meta_src.size);

        // Linking to a directory should fail.
        let dir_path = if root == "/" {
            alloc::string::String::from("/_ext4_hardlink_dir_test")
        } else {
            format!("{}/_ext4_hardlink_dir_test", root)
        };
        crate::fs::Vfs::mkdir(&dir_path)?;
        let link_dir_result = crate::fs::Vfs::link(&dir_path, &format!("{}_link", dir_path));
        if !matches!(link_dir_result, Err(crate::error::KernelError::IsADirectory)) {
            serial_println!("[ext4]   FAIL: linking directory should return IsADirectory, got {:?}", link_dir_result);
            let _ = crate::fs::Vfs::rmdir(&dir_path);
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        crate::fs::Vfs::rmdir(&dir_path)?;
        serial_println!("[ext4]     link-to-directory rejected OK");

        // Linking to an existing name should fail.
        let dup_result = crate::fs::Vfs::link(&file_path, &link_path);
        if !matches!(dup_result, Err(crate::error::KernelError::AlreadyExists)) {
            serial_println!("[ext4]   FAIL: duplicate link should return AlreadyExists, got {:?}", dup_result);
            let _ = crate::fs::Vfs::remove(&link_path);
            let _ = crate::fs::Vfs::remove(&file_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     duplicate link rejected OK");

        // Remove one link — the other should still work with nlinks = 1.
        crate::fs::Vfs::remove(&file_path)?;
        let meta_after = crate::fs::Vfs::metadata(&link_path)?;
        if meta_after.nlinks != 1 {
            serial_println!("[ext4]   FAIL: after removing one link, nlinks={}, expected 1", meta_after.nlinks);
            let _ = crate::fs::Vfs::remove(&link_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     after remove: nlinks=1 OK");

        // Data should still be accessible through the remaining link.
        let remaining = crate::fs::Vfs::read_file(&link_path)?;
        if remaining != b"hard link test data" {
            serial_println!("[ext4]   FAIL: data through remaining link is wrong");
            let _ = crate::fs::Vfs::remove(&link_path);
            return Err(crate::error::KernelError::InternalError);
        }
        serial_println!("[ext4]     data through remaining link OK");

        // Clean up.
        crate::fs::Vfs::remove(&link_path)?;
        serial_println!("[ext4]     hard link test files cleaned up OK");
    }

    serial_println!("[ext4] Self-test passed.");
    Ok(())
}
