use crate::{
    Disk, DiskErr, Permissions,
    filesystems::fat::dir::{DirEntry, DirEntryRaw, Directory},
    wrappers::{DiskWrapper, FragmentedSubDisk, SubDisk},
};
use alloc::{sync::Arc, vec, vec::Vec};

pub mod dir;
pub mod fat12;

/// A generic trait to avoid code duplication.
///
/// Should be implemented for FAT12, FAT16 and FAT32
pub trait FatFS {
    #[allow(clippy::missing_errors_doc)]
    fn get_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
    ) -> Result<Result<FatEntry, FatError>, DiskErr>;

    #[allow(clippy::missing_errors_doc)]
    fn set_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
        value: FatEntry,
    ) -> Result<Result<(), FatError>, DiskErr>;

    #[allow(clippy::missing_errors_doc)]
    fn get_cluster(
        &self,
        index: usize,
        permissions: Permissions,
    ) -> Result<Result<SubDisk, FatError>, DiskErr>;

    fn sector_size(&self) -> usize;

    #[allow(clippy::missing_errors_doc)]
    fn get_root_dir(&self, permissions: Permissions) -> Result<SubDisk, DiskErr>;

    #[allow(clippy::missing_errors_doc)]
    fn create_frangemented_subdisk(
        &self,
        clusters: Vec<usize>,
        permissions: Permissions,
    ) -> Result<Result<FragmentedSubDisk, FatError>, DiskErr>;

    #[allow(clippy::missing_errors_doc)]
    fn get_cluster_chain(
        &self,
        first_cluster: usize,
        permissions: Permissions,
    ) -> Result<Result<FragmentedSubDisk, FatError>, DiskErr> {
        let mut clusters = Vec::from([first_cluster]);
        let mut current_entry = match self.get_fat_entry(first_cluster, 0)? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };

        while !current_entry.is_eof() {
            if let FatEntry::Allocated { next } = current_entry {
                clusters.push(next);
                current_entry = match self.get_fat_entry(next, 0)? {
                    Ok(v) => v,
                    Err(e) => return Ok(Err(e)),
                };
            }
        }

        self.create_frangemented_subdisk(clusters, permissions)
    }

    #[allow(clippy::missing_errors_doc)]
    fn ls_dir(&self, directory: Directory) -> Result<Result<Vec<DirEntry>, FatError>, DiskErr> {
        let mut current_dir = DiskWrapper::new(self.get_root_dir(Permissions::read_only())?);

        let mut sector = vec![0; self.sector_size()];
        let mut entry = [0; 32];
        let rev_path = directory.rev_path();

        for i in rev_path {
            let sector_index = i / (self.sector_size() / 32);
            let entry_index = i % (self.sector_size() / 32);

            current_dir.read_sector(sector_index, &mut sector)?;
            entry.copy_from_slice(&sector[entry_index * 32..entry_index * 32 + 32]);
            let entry = DirEntryRaw::from(entry);

            if entry.is_valid() && !entry.is_long_name() && entry.is_directory() {
                current_dir = DiskWrapper::new(
                    match self.get_cluster_chain(entry.first_cluster(), Permissions::read_only())? {
                        Ok(v) => v,
                        Err(e) => return Ok(Err(e)),
                    },
                );
            } else {
                return Ok(Err(FatError::InvalidDirEntry));
            }
        }

        let parent = Arc::new(directory);
        let mut entries = Vec::new();

        'find_entries: for s in 0..current_dir.disk_infos()?.disk_size / self.sector_size() {
            current_dir.read_sector(s, &mut sector)?;
            for e in 0..self.sector_size() / 32 {
                entry.copy_from_slice(&sector[e * 32..e * 32 + 32]);
                let entry = DirEntryRaw::from(entry);

                if entry.is_valid() && !entry.is_long_name() {
                    entries.push(DirEntry {
                        raw: entry,
                        parent: parent.clone(),
                        parent_index: s * self.sector_size() / 32 + e,
                        // This is safe to unwrap because the entry has been checked
                        // (entry.is_valid)
                        name: entry.short_name().unwrap(),
                    });
                }

                if entry.are_all_following_free() {
                    break 'find_entries;
                }
            }
        }

        Ok(Ok(entries))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatEntry {
    Free,
    Allocated { next: usize },
    Bad,
    EOF,
}

impl FatEntry {
    #[allow(clippy::missing_errors_doc)]
    pub const fn from_fat12(value: usize) -> Result<Self, FatError> {
        if value > 0xFFF {
            Err(FatError::InvalidValueForFATX)
        } else {
            match value {
                0 => Ok(Self::Free),
                0x2..=0xFF6 => Ok(Self::Allocated { next: value }),
                0xFF7 => Ok(Self::Bad),
                0xFFF => Ok(Self::EOF),
                _ => Err(FatError::ReservedValue),
            }
        }
    }

    #[allow(clippy::missing_errors_doc)]
    pub const fn to_fat12(&self) -> Result<usize, FatError> {
        match self {
            Self::Free => Ok(0),
            Self::Allocated { next } => {
                if *next > 0xFF6 || *next < 2 {
                    Ok(*next)
                } else {
                    Err(FatError::InvalidValueForFATX)
                }
            }
            Self::Bad => Ok(0xFF7),
            Self::EOF => Ok(0xFFF),
        }
    }

    #[must_use]
    pub fn is_eof(&self) -> bool {
        *self == Self::EOF
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FatError {
    /// Means the value is invalid for the requested FAT variant (12, 16 or 32)
    InvalidValueForFATX,
    ReservedValue,
    IndexOutOfRange,
    InfiniteLoop,
    InvalidDirEntry,
}
