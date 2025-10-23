#![cfg_attr(not(feature = "std"), no_std)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]

extern crate alloc;

use alloc::vec::Vec;

/// Provides an implementation of the `Disk` trait for `std::fs`
#[cfg(feature = "std")]
pub mod std_helpers;

#[cfg(feature = "std")]
pub use std_helpers::*;

/// Provides support for the following filesystems:
///  - FAT12
pub mod filesystems;
/// An in-memory instance of `Disk`
pub mod memdisk;
/// Provides support for the following partition tables:
///  - MBR
pub mod partition_tables;
/// Provides disk wrappers to allow subdisk creation. `SubDisk`s are useful when working with
/// partitions or filesystems for example.
pub mod wrappers;

/// The main trait representing any disk. It provides only minimal methods: read, write, and infos.
///
/// The disks work with sectors, but there are various possible sector sizes. To allow more
/// flexibility, this library is not directly dependent of the sector size of the disk, even if it
/// can be the case in some partition tables, partition types, or filesystems. It also allows a
/// same disk to support multiple sector sizes (typically when working with disk image).
pub trait Disk {
    /// The size of the buffer is implicitly the sector size (in bytes). `sector` is the LBA of the
    /// sector. It's the implementation responsibility to check the sector and the disk sizes, the
    /// caller may produce invalid requests.
    ///
    /// # Errors
    ///
    /// The possibility to return an error is at the discretion of the implementation. Usually, it
    /// can be a permission error, an invalid buffer size, an invalid sector index, or an
    /// unavailable disk.
    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr>;

    /// The size of the buffer is implicitly the sector size (in bytes). `sector` is the LBA of the
    /// sector. It's the implementation responsibility to check the sector and the disk sizes, the
    /// caller may produce invalid requests.
    ///
    /// # Errors
    ///
    /// The possibility to return an error is at the discretion of the implementation. Usually, it
    /// can be a permission error, an invalid buffer size, an invalid sector index, or an
    /// unavailable disk.
    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr>;

    /// # Errors
    ///
    /// The possibility to return an error is at the discretion of the implementation.
    fn disk_infos(&self) -> Result<DiskInfos, DiskErr>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskErr {
    /// Will trigger if the size of the buffer isn't supported.
    ///
    /// `found` is the size of the provided buffer (`buf.len()`)
    ///
    /// `supported` is the supported sector size(s)
    ///
    /// `start % sector_size` should be zero (used with subdisks).
    /// Note that `start` is not guaranteed to be relative to the root disk, meaning it's only
    /// relative to the direct parent disk. It can explain why an error triggers even if the sector
    /// size os supported.
    InvalidSectorSize {
        found: usize,
        supported: SectorSize,
        start: usize,
    },

    /// Will trigger if the sector index is out of the range of the disk.
    ///
    /// `found` is the provided sector index (lba)
    ///
    /// `max` is the last existing sector index **with the size of the given buffer**.
    InvalidSectorIndex { found: usize, max: usize },

    /// Will trigger if a write is performed on a read-only disk or if the program tries to read a
    /// write-only disk.
    InvalidPermission { disk_permissions: Permissions },

    /// Will trigger if, for any reason, the disk can not be found.
    UnreachableDisk,

    /// Will trigger when attempting to create a subdisk out of the range of the original disk
    /// size.
    InvalidDiskSize,

    /// Will trigger if a read/write/subdisk creation is requested when the disk is already in
    /// use/on a space already borrowed
    Busy,

    /// Will trigger for all the unknown errors coming from IO processes (for example with `std`).
    IOErr,

    /// The disk doesn't support the sector size requested by the partition table or the
    /// filesystem.
    UnsupportedDiskSectorSize,

    /// The partition index is out of the range of the existing partitions on the disk.
    InvalidPartitionIndex,

    /// The requested space is already used by an other process or already borrowed by an other
    /// subdisk. The borrowing rules for subdisks follow the Rust rules: exactly one read/write or
    /// write-only borrow or any number of read-only borrows.
    SpaceAlreadyInUse,

    /// The requested index is out of the maximum range.
    IndexOutOfRange,

    /// The value is out of the range of the value type internally used (e.g., 0x10000 is requested
    /// while the number is stored on 16 bits).
    OutOfRangeValue,
}

/// Represents the informations provided by a `Disk` instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskInfos {
    /// The supported sector size(s).
    pub sector_size: SectorSize,

    /// The disk size in bytes.
    pub disk_size: usize,

    /// Specially useful when working with disk images, or without `sudo` privileges.
    pub permissions: Permissions,
}

/// Informs the supported sector sizes. A sector size superior to the disk size is always invalid
/// and should trigger an error `DiskErr::InvalidSectorSize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectorSize {
    /// All sector sizes are supported
    Any,

    /// All sector sizes in the list are supported
    AllOf(Vec<usize>),
    /// All sector sizes are supported expected the ones in the list
    AnyExcept(Vec<usize>),

    /// All sector sizes in one of the ranges are supported. min <= size < max
    InRanges(Vec<(usize, usize)>),
    /// All sector sizes are supported expected the ones in one of these ranges. min <= size < max
    AnyExceptRanges(Vec<(usize, usize)>),
}

/// These permissions are only intented for disk usage, **not** for filesystems.
///
/// They only are a clue about the context in which the program is called, and to avoid
/// accidental writes. IF THE DISK CAN BE WRITTEN, A READ ONLY FILESYSTEM OR PARTITION IS
/// NOT A GUARANTEE. THIS IS THE `Disk` IMPLEMENTATION RESPONSIBILTY TO CHECK THE
/// PERMISSIONS, THE CALLER MAY TRY ILLEGAL OPERATIONS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
}

impl Permissions {
    /// Only allows to read the disk.
    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
        }
    }

    /// Only allows to write the disk.
    #[must_use]
    pub const fn write_only() -> Self {
        Self {
            read: false,
            write: true,
        }
    }

    /// Allows both to read and write the disk.
    #[must_use]
    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
        }
    }
}

impl SectorSize {
    /// Checks if a given sector size is supported.
    #[must_use]
    pub fn is_supported(&self, sector_size: usize, disk_size: usize) -> bool {
        (match self {
            Self::Any => true,
            Self::AllOf(l) => l.contains(&sector_size),
            Self::AnyExcept(l) => !l.contains(&sector_size),
            Self::InRanges(rs) => rs.iter().any(|r| r.0 <= sector_size && sector_size < r.1),
            Self::AnyExceptRanges(rs) => {
                !rs.iter().any(|r| r.0 <= sector_size && sector_size < r.1)
            }
        }) && (sector_size <= disk_size)
    }

    /// Returns the minimal supported sector size greater than or equal to `sector_size` if any
    /// exists.
    #[must_use]
    pub fn minimal_ge(&self, sector_size: usize) -> Option<usize> {
        match self {
            Self::Any => Some(sector_size),
            Self::AllOf(l) => {
                let mut min = None;
                for &i in l {
                    if i == sector_size {
                        min = Some(sector_size);
                        break;
                    }

                    if i > sector_size && min.is_none_or(|m| i < m) {
                        min = Some(i);
                    }
                }
                min
            }
            Self::AnyExcept(l) => {
                let mut min = sector_size;
                let mut v = l.clone();
                v.sort_unstable();

                for i in v {
                    if i == min {
                        min += 1;
                    } else if i > min {
                        break;
                    }
                }

                Some(min)
            }
            Self::InRanges(ranges) => {
                let mut min = None;

                for &(start, end) in ranges {
                    if end <= sector_size {
                        continue;
                    }

                    let v = if sector_size < start {
                        start
                    } else {
                        sector_size
                    };

                    if min.is_none_or(|m| v < m) {
                        min = Some(v);
                    }
                }

                min
            }
            Self::AnyExceptRanges(ranges) => {
                let mut min = sector_size;

                loop {
                    let mut inside = false;
                    let mut bump_to = min + 1;

                    for &(start, end) in ranges {
                        if start <= min && min < end {
                            inside = true;
                            bump_to = bump_to.max(end);
                        }
                    }

                    if inside {
                        min = bump_to;
                    } else {
                        break Some(min);
                    }
                }
            }
        }
    }
}
