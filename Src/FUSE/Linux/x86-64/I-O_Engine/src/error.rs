//! Public error type. Every fallible engine operation returns
//! `Result<T, CafsError>`.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CafsError {
    Open,
    Io,
    TooSmall,
    BadMagic,
    Checksum,
    Readonly,
    Misaligned,
    NotMounted,
    InvalidArg,
    Unsupported,
}

impl fmt::Display for CafsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CafsError::Open => "device open failed",
            CafsError::Io => "a read/write syscall failed",
            CafsError::TooSmall => "device smaller than the anchor region needs",
            CafsError::BadMagic => "a structure's magic didn't match",
            CafsError::Checksum => "a structure's checksum didn't match",
            CafsError::Readonly => "write attempted against a readonly mount",
            CafsError::Misaligned => "unaligned I/O and misaligned_action=reject",
            CafsError::NotMounted => "called out of order, e.g. confirm before mount",
            CafsError::InvalidArg => "invalid argument",
            CafsError::Unsupported => "operation unsupported in this configuration",
        };
        f.write_str(s)
    }
}

impl std::error::Error for CafsError {}

impl From<rustix::io::Errno> for CafsError {
    fn from(_: rustix::io::Errno) -> Self {
        CafsError::Io
    }
}

impl From<std::io::Error> for CafsError {
    fn from(_: std::io::Error) -> Self {
        CafsError::Io
    }
}

pub type CafsResult<T> = Result<T, CafsError>;
