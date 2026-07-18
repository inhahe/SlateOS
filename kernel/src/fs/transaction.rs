//! Atomic filesystem transactions.
//!
//! Allows grouping multiple filesystem operations into a single atomic
//! unit.  Either all operations succeed and are committed, or none take
//! effect (rollback).  This prevents partial writes that leave the
//! filesystem in an inconsistent state on errors or interruptions.
//!
//! ## Design
//!
//! ```text
//! let tx = transaction::begin()?;
//! transaction::tx_write(tx, "/etc/config.new", data)?;
//! transaction::tx_rename(tx, "/etc/config.new", "/etc/config")?;
//! transaction::commit(tx)?;  // atomic: both or neither
//! ```
//!
//! Internally, operations are recorded in a write-ahead log.  On commit,
//! operations are executed in order.  If any operation fails, all
//! previously-executed operations are rolled back (files are restored
//! from saved copies).
//!
//! ## Limitations
//!
//! - Transactions are in-memory only (not persistent across reboots)
//! - Maximum 64 operations per transaction
//! - Only supports write_file, remove, rename, mkdir, symlink
//! - Rollback is best-effort (if the rollback itself fails, the
//!   transaction is marked "dirty" for manual recovery)
//!
//! ## Use Cases
//!
//! - Package installation: write all files, create dirs atomically
//! - Config updates: write-new + rename-to-old + rename-new-to-current
//! - Multi-file saves: document + metadata + index all-or-nothing
//!
//! ## Reference
//!
//! design.txt: "ability of programs to group any writes into an atomic write"
//! design.txt: "make it atomic - can undo the whole copy or move or delete"

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::Vfs;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique transaction identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TxId(pub u64);

/// State of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    /// Transaction is open and accepting operations.
    Active,
    /// Transaction was committed successfully.
    Committed,
    /// Transaction was rolled back.
    RolledBack,
    /// Transaction commit failed and rollback also failed.
    /// Manual intervention needed.
    Dirty,
}

/// An individual operation within a transaction.
#[derive(Debug, Clone)]
enum TxOp {
    /// Write file content.
    WriteFile { path: String, data: Vec<u8> },
    /// Remove a file.
    Remove { path: String },
    /// Create a directory.
    Mkdir { path: String },
    /// Rename/move a file or directory.
    Rename { from: String, to: String },
    /// Create a symlink.
    Symlink { path: String, target: String },
}

/// Undo information for rolling back a single operation.
#[derive(Debug, Clone)]
enum UndoOp {
    /// File was created — remove it.
    RemoveFile { path: String },
    /// File was overwritten — restore original content.
    RestoreFile { path: String, data: Vec<u8> },
    /// File was removed — restore it.
    WriteFile { path: String, data: Vec<u8> },
    /// Directory was created — remove it.
    Rmdir { path: String },
    /// Rename was done — reverse it.
    Rename { from: String, to: String },
    /// Symlink was created — remove it.
    RemoveSymlink { path: String },
}

/// A filesystem transaction.
struct Transaction {
    id: TxId,
    state: TxState,
    ops: Vec<TxOp>,
    undo_stack: Vec<UndoOp>,
    /// Optional human-readable label.
    label: String,
    /// Timestamp when transaction was created.
    created_ns: u64,
}

/// Public info about a transaction.
#[derive(Debug, Clone)]
pub struct TxInfo {
    pub id: TxId,
    pub state: TxState,
    pub ops_count: usize,
    pub label: String,
    pub created_ns: u64,
}

/// Maximum operations per transaction.
const MAX_OPS: usize = 64;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct TxInner {
    transactions: BTreeMap<TxId, Transaction>,
    next_id: u64,
}

static TRANSACTIONS: Mutex<TxInner> = Mutex::new(TxInner {
    transactions: BTreeMap::new(),
    next_id: 1,
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Begin a new transaction.
///
/// Returns a transaction ID that must be passed to subsequent operations
/// and finally to `commit()` or `rollback()`.
pub fn begin() -> KernelResult<TxId> {
    begin_with_label("")
}

/// Begin a new transaction with a descriptive label.
pub fn begin_with_label(label: &str) -> KernelResult<TxId> {
    let mut inner = TRANSACTIONS.lock();
    let id = TxId(inner.next_id);
    inner.next_id = inner.next_id.saturating_add(1);

    let tx = Transaction {
        id,
        state: TxState::Active,
        ops: Vec::new(),
        undo_stack: Vec::new(),
        label: String::from(label),
        created_ns: crate::timekeeping::clock_realtime(),
    };

    inner.transactions.insert(id, tx);
    Ok(id)
}

/// Add a write operation to the transaction.
pub fn tx_write(tx_id: TxId, path: &str, data: &[u8]) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    check_capacity(tx)?;

    tx.ops.push(TxOp::WriteFile {
        path: String::from(path),
        data: data.to_vec(),
    });

    Ok(())
}

/// Add a remove operation to the transaction.
pub fn tx_remove(tx_id: TxId, path: &str) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    check_capacity(tx)?;

    tx.ops.push(TxOp::Remove {
        path: String::from(path),
    });

    Ok(())
}

/// Add a mkdir operation to the transaction.
pub fn tx_mkdir(tx_id: TxId, path: &str) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    check_capacity(tx)?;

    tx.ops.push(TxOp::Mkdir {
        path: String::from(path),
    });

    Ok(())
}

/// Add a rename operation to the transaction.
pub fn tx_rename(tx_id: TxId, from: &str, to: &str) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    check_capacity(tx)?;

    tx.ops.push(TxOp::Rename {
        from: String::from(from),
        to: String::from(to),
    });

    Ok(())
}

/// Add a symlink creation to the transaction.
pub fn tx_symlink(tx_id: TxId, path: &str, target: &str) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    check_capacity(tx)?;

    tx.ops.push(TxOp::Symlink {
        path: String::from(path),
        target: String::from(target),
    });

    Ok(())
}

/// Commit the transaction: execute all operations atomically.
///
/// If any operation fails, all previously-executed operations are
/// rolled back.  Returns Ok(()) if all operations succeeded.
pub fn commit(tx_id: TxId) -> KernelResult<()> {
    // Extract ops from the transaction (we need to release the lock
    // before executing VFS operations to avoid deadlock).
    let ops = {
        let mut inner = TRANSACTIONS.lock();
        let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
        check_active(tx)?;
        tx.ops.clone()
    };

    let mut undo_stack: Vec<UndoOp> = Vec::new();

    // Execute operations in order.
    for (i, op) in ops.iter().enumerate() {
        match execute_op(op) {
            Ok(undo) => {
                undo_stack.push(undo);
            }
            Err(e) => {
                // Operation failed — roll back all executed operations.
                serial_println!(
                    "[tx] Operation {} failed ({:?}), rolling back {} ops",
                    i, e, undo_stack.len(),
                );

                let rollback_ok = rollback_ops(&undo_stack);

                let mut inner = TRANSACTIONS.lock();
                if let Some(tx) = inner.transactions.get_mut(&tx_id) {
                    tx.state = if rollback_ok {
                        TxState::RolledBack
                    } else {
                        TxState::Dirty
                    };
                    tx.undo_stack = undo_stack;
                }

                return Err(e);
            }
        }
    }

    // All operations succeeded — mark committed.
    let mut inner = TRANSACTIONS.lock();
    if let Some(tx) = inner.transactions.get_mut(&tx_id) {
        tx.state = TxState::Committed;
        tx.undo_stack = undo_stack;
    }

    Ok(())
}

/// Explicitly roll back an active transaction without executing anything.
///
/// Discards all queued operations.
pub fn rollback(tx_id: TxId) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get_mut(&tx_id).ok_or(KernelError::NotFound)?;
    check_active(tx)?;
    tx.state = TxState::RolledBack;
    tx.ops.clear();
    Ok(())
}

/// Get info about a transaction.
pub fn info(tx_id: TxId) -> KernelResult<TxInfo> {
    let inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get(&tx_id).ok_or(KernelError::NotFound)?;
    Ok(TxInfo {
        id: tx.id,
        state: tx.state,
        ops_count: tx.ops.len(),
        label: tx.label.clone(),
        created_ns: tx.created_ns,
    })
}

/// List all transactions.
pub fn list() -> Vec<TxInfo> {
    let inner = TRANSACTIONS.lock();
    inner
        .transactions
        .values()
        .map(|tx| TxInfo {
            id: tx.id,
            state: tx.state,
            ops_count: tx.ops.len(),
            label: tx.label.clone(),
            created_ns: tx.created_ns,
        })
        .collect()
}

/// Remove a completed transaction (Committed/RolledBack/Dirty).
///
/// Active transactions cannot be removed — commit or rollback first.
pub fn remove(tx_id: TxId) -> KernelResult<()> {
    let mut inner = TRANSACTIONS.lock();
    let tx = inner.transactions.get(&tx_id).ok_or(KernelError::NotFound)?;
    if tx.state == TxState::Active {
        return Err(KernelError::InvalidArgument);
    }
    inner.transactions.remove(&tx_id);
    Ok(())
}

/// Get the number of active transactions.
pub fn active_count() -> usize {
    let inner = TRANSACTIONS.lock();
    inner
        .transactions
        .values()
        .filter(|tx| tx.state == TxState::Active)
        .count()
}

// ---------------------------------------------------------------------------
// Internal execution
// ---------------------------------------------------------------------------

/// Execute a single operation and return the undo info.
fn execute_op(op: &TxOp) -> KernelResult<UndoOp> {
    match op {
        TxOp::WriteFile { path, data } => {
            // Save existing content for undo (if file exists).
            let undo = match Vfs::read_file(path) {
                Ok(old_data) => UndoOp::RestoreFile {
                    path: path.clone(),
                    data: old_data,
                },
                Err(KernelError::NotFound) => UndoOp::RemoveFile {
                    path: path.clone(),
                },
                Err(e) => return Err(e),
            };

            Vfs::write_file(path, data)?;
            Ok(undo)
        }
        TxOp::Remove { path } => {
            // Save content for undo.
            let data = Vfs::read_file(path)?;
            Vfs::remove(path)?;
            Ok(UndoOp::WriteFile {
                path: path.clone(),
                data,
            })
        }
        TxOp::Mkdir { path } => {
            Vfs::mkdir(path)?;
            Ok(UndoOp::Rmdir { path: path.clone() })
        }
        TxOp::Rename { from, to } => {
            Vfs::rename(from, to)?;
            Ok(UndoOp::Rename {
                from: to.clone(),
                to: from.clone(),
            })
        }
        TxOp::Symlink { path, target } => {
            Vfs::symlink(path, target)?;
            Ok(UndoOp::RemoveSymlink { path: path.clone() })
        }
    }
}

/// Roll back a list of undo operations (in reverse order).
///
/// Returns true if all undos succeeded, false if any failed.
fn rollback_ops(undos: &[UndoOp]) -> bool {
    let mut all_ok = true;

    // Execute undos in reverse order.
    for undo in undos.iter().rev() {
        let result = match undo {
            UndoOp::RemoveFile { path } => Vfs::remove(path),
            UndoOp::RestoreFile { path, data } => Vfs::write_file(path, data),
            UndoOp::WriteFile { path, data } => Vfs::write_file(path, data),
            UndoOp::Rmdir { path } => Vfs::rmdir(path),
            UndoOp::Rename { from, to } => Vfs::rename(from, to),
            UndoOp::RemoveSymlink { path } => Vfs::remove(path),
        };

        if result.is_err() {
            all_ok = false;
        }
    }

    all_ok
}

fn check_active(tx: &Transaction) -> KernelResult<()> {
    if tx.state != TxState::Active {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

fn check_capacity(tx: &Transaction) -> KernelResult<()> {
    if tx.ops.len() >= MAX_OPS {
        return Err(KernelError::DiskFull); // Reuse for "transaction full"
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[transaction] Running self-test...");

    test_begin_commit();
    test_rollback();
    test_commit_failure_rollback();
    test_multi_op();
    test_info_list();
    test_remove();

    serial_println!("[transaction] Self-test passed (6 tests).");
    Ok(())
}

fn test_begin_commit() {
    // Simple: create a file within a transaction.
    let tx = begin_with_label("test-commit").expect("begin failed");
    tx_write(tx, "/tmp/tx_test_1.txt", b"hello").expect("tx_write failed");
    commit(tx).expect("commit failed");

    // Verify file exists.
    let data = Vfs::read_file("/tmp/tx_test_1.txt").expect("file should exist after commit");
    assert_eq!(&data, b"hello");

    // Cleanup.
    let _ = Vfs::remove("/tmp/tx_test_1.txt");
    let _ = remove(tx);

    serial_println!("[transaction]   begin+commit: ok");
}

fn test_rollback() {
    // Create a file, then rollback — file should not exist.
    let tx = begin().expect("begin failed");
    tx_write(tx, "/tmp/tx_test_rb.txt", b"should not persist").expect("tx_write failed");
    rollback(tx).expect("rollback failed");

    // File should not exist (operations were never executed on rollback).
    assert!(Vfs::read_file("/tmp/tx_test_rb.txt").is_err());

    let _ = remove(tx);

    serial_println!("[transaction]   rollback: ok");
}

fn test_commit_failure_rollback() {
    // Write a file, then try to remove a non-existent file.
    // The write should be rolled back.
    let tx = begin().expect("begin failed");
    tx_write(tx, "/tmp/tx_test_fail.txt", b"temporary").expect("tx_write failed");
    tx_remove(tx, "/tmp/tx_nonexistent_xyz.txt").expect("tx_remove queue ok");

    // Commit should fail (remove on non-existent file).
    let result = commit(tx);
    assert!(result.is_err(), "commit should fail");

    // The write should have been rolled back.
    assert!(
        Vfs::read_file("/tmp/tx_test_fail.txt").is_err(),
        "file should not exist after rollback"
    );

    let _ = remove(tx);

    serial_println!("[transaction]   commit failure rollback: ok");
}

fn test_multi_op() {
    // Multiple operations in one transaction.
    let tx = begin_with_label("multi-op").expect("begin failed");
    tx_mkdir(tx, "/tmp/tx_dir").expect("tx_mkdir failed");
    tx_write(tx, "/tmp/tx_dir/a.txt", b"file A").expect("write A failed");
    tx_write(tx, "/tmp/tx_dir/b.txt", b"file B").expect("write B failed");
    commit(tx).expect("commit failed");

    // Verify all operations.
    let a = Vfs::read_file("/tmp/tx_dir/a.txt").expect("a.txt missing");
    assert_eq!(&a, b"file A");
    let b = Vfs::read_file("/tmp/tx_dir/b.txt").expect("b.txt missing");
    assert_eq!(&b, b"file B");

    // Cleanup.
    let _ = Vfs::remove("/tmp/tx_dir/a.txt");
    let _ = Vfs::remove("/tmp/tx_dir/b.txt");
    let _ = Vfs::rmdir("/tmp/tx_dir");
    let _ = remove(tx);

    serial_println!("[transaction]   multi-op: ok");
}

fn test_info_list() {
    let tx = begin_with_label("info-test").expect("begin failed");
    tx_write(tx, "/tmp/tx_info_test.txt", b"x").expect("write failed");

    let i = info(tx).expect("info failed");
    assert_eq!(i.id, tx);
    assert_eq!(i.state, TxState::Active);
    assert_eq!(i.ops_count, 1);
    assert_eq!(i.label, "info-test");

    let all = list();
    assert!(all.iter().any(|t| t.id == tx));

    rollback(tx).expect("rollback failed");
    let _ = remove(tx);

    serial_println!("[transaction]   info+list: ok");
}

fn test_remove() {
    let tx = begin().expect("begin failed");
    rollback(tx).expect("rollback");

    // Can remove completed transactions.
    remove(tx).expect("remove failed");
    assert!(info(tx).is_err(), "should be gone after remove");

    // Cannot remove active transactions.
    let tx2 = begin().expect("begin");
    assert!(remove(tx2).is_err(), "should reject active tx removal");
    rollback(tx2).expect("rollback");
    let _ = remove(tx2);

    serial_println!("[transaction]   remove: ok");
}
