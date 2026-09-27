//! The table against the kernel's own source, `kernel/src/error.rs`: every
//! entry here must be a variant there, with the same code and word for word
//! the same message. A variant renumbered or reworded fails here -- and should,
//! since `KernelError::message` is declared stable ABI.
//!
//! A variant *added* there does not fail: `kernel/` is lane A's, this crate is
//! lane B's, and a new kernel error must not break another lane's test run.
//! Until this table has it, a tool says `error N` for it, which is honest.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::{ALL, INVALID_ARGUMENT, NOT_SUPPORTED, PERMISSION_DENIED, describe, lookup, message};

const KERNEL_ERROR_RS: &str = include_str!("../../../kernel/src/error.rs");

/// `Name = -123,` lines inside `pub enum KernelError { ... }`.
fn kernel_codes() -> Vec<(String, i64)> {
    let body = KERNEL_ERROR_RS
        .split("pub enum KernelError {")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .unwrap();
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, value) = line.split_once(" = ")?;
            let value = value.strip_suffix(',')?;
            Some((name.to_string(), value.parse().ok()?))
        })
        .collect()
}

/// `Self::Name => "message",` lines inside `fn message`.
fn kernel_messages() -> Vec<(String, String)> {
    let body = KERNEL_ERROR_RS
        .split("pub const fn message(self)")
        .nth(1)
        .and_then(|rest| rest.split("\n    }").next())
        .unwrap();
    body.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Self::")?;
            let (name, msg) = rest.split_once(" => \"")?;
            let msg = msg.strip_suffix("\",")?;
            Some((name.to_string(), msg.to_string()))
        })
        .collect()
}

#[test]
fn every_entry_is_a_kernel_variant_with_its_code() {
    let kernel = kernel_codes();
    // The parse found the enum: a table this crate could not read would pass
    // the loop below vacuously.
    assert!(kernel.len() >= 50, "read {} variants", kernel.len());
    for k in &ALL {
        let theirs = kernel
            .iter()
            .find(|(name, _)| name == k.name)
            .unwrap_or_else(|| panic!("{} is not in kernel/src/error.rs", k.name));
        assert_eq!(theirs.1, k.code, "{}", k.name);
    }
}

#[test]
fn every_message_is_the_kernel_s_word_for_word() {
    let messages = kernel_messages();
    assert!(messages.len() >= 50, "read {} messages", messages.len());
    for k in &ALL {
        let theirs = messages
            .iter()
            .find(|(name, _)| name == k.name)
            .unwrap_or_else(|| panic!("{} has no message in kernel/src/error.rs", k.name));
        assert_eq!(theirs.1, k.message, "{}", k.name);
    }
}

#[test]
fn every_code_is_listed_once() {
    for k in &ALL {
        assert_eq!(
            ALL.iter().filter(|o| o.code == k.code).count(),
            1,
            "{}",
            k.name
        );
    }
}

#[test]
fn the_codes_a_refusal_is_made_of() {
    assert_eq!(message(PERMISSION_DENIED), Some("permission denied"));
    assert_eq!(message(NOT_SUPPORTED), Some("operation not supported"));
    assert_eq!(message(INVALID_ARGUMENT), Some("invalid argument"));
    // Linux's EPERM, -1, is the kernel's InternalError: a tool that reads -1
    // as "need root" names an internal error as a refusal. EACCES, -13, is
    // no kernel code at all.
    assert_eq!(message(-1), Some("internal kernel error"));
    assert_eq!(lookup(-13), None);
}

#[test]
fn describe_falls_back_to_the_number() {
    assert_eq!(describe(-400), "permission denied");
    assert_eq!(describe(-9999), "error -9999");
}
