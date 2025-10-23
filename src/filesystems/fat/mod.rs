use crate::{
    Disk, DiskErr, Permissions,
    filesystems::fat::dir::{DirEntryRaw, Directory},
    wrappers::{DiskWrapper, FragmentedSubDisk, SubDisk},
};
use alloc::{sync::Arc, vec, vec::Vec};

pub mod dir;
pub mod fat12;

pub trait FatFS {
    /// # Errors
    ///
    /// TODO:
    fn get_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
    ) -> Result<Result<FatEntry, FatError>, DiskErr>;

    /// # Errors
    ///
    /// TODO:
    fn set_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
        value: FatEntry,
    ) -> Result<Result<(), FatError>, DiskErr>;

    /// # Errors
    ///
    /// TODO:
    fn get_cluster(
        &self,
        index: usize,
        permissions: Permissions,
    ) -> Result<Result<SubDisk, FatError>, DiskErr>;

    fn sector_size(&self) -> usize;

    /// # Errors
    ///
    /// TODO:
    fn get_root_dir(&self, permissions: Permissions) -> Result<SubDisk, DiskErr>;

    /// # Errors
    ///
    /// TODO:
    fn create_frangemented_subdisk(
        &self,
        clusters: Vec<usize>,
        permissions: Permissions,
    ) -> Result<Result<FragmentedSubDisk, FatError>, DiskErr>;

    /// # Errors
    ///
    /// TODO:
    fn get_file(
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

    /// # Errors
    ///
    /// TODO:
    fn get_dir(
        &self,
        directory: &Directory,
        permissions: Permissions,
    ) -> Result<Result<Arc<DiskWrapper>, FatError>, DiskErr> {
        let root_dir = self.get_root_dir(Permissions::read_only())?;
        let mut dir = DiskWrapper::new(root_dir);
        let sector_size = self.sector_size();
        let mut sector = vec![0; sector_size];
        let mut entry = [0; 32];

        for index in directory.rev_path().into_iter().rev() {
            let offset = index * 32;
            let sector_index = offset / sector_size;
            let offset_in_sector = offset % sector_size;

            dir.read_sector(sector_index, &mut sector)?;
            entry.copy_from_slice(&sector[offset_in_sector..offset_in_sector + 32]);

            let dir_entry_raw = DirEntryRaw::from(entry);

            // TODO: check if this is a long file name entry

            dir = DiskWrapper::new(
                match self.get_file(dir_entry_raw.first_cluster(), permissions)? {
                    Ok(v) => v,
                    Err(e) => return Ok(Err(e)),
                },
            );
        }

        Ok(Ok(dir))
    }

    /// # Errors
    ///
    /// TODO:
    fn ls_dir(&self, directory: Directory) -> Result<Result<Vec<Directory>, FatError>, DiskErr> {
        let dir = match self.get_dir(&directory, Permissions::read_only())? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
        let infos = dir.disk_infos()?;

        // The following may be used to check if the sector size is valid:
        //
        // ```
        // if !infos
        //     .sector_size
        //     .is_supported(self.sector_size(), infos.disk_size)
        // {
        //     return Err(DiskErr::UnsupportedDiskSectorSize);
        // }
        // ```
        //
        // But `self.disk_size` is supposed to be correct, so we'll simply try and return an error
        // in case it doesn't work. We should also check if `sector_size % 32 == 0`, but again,
        // it's assumed to be true.

        let mut sector = vec![0; self.sector_size()];
        let mut entry = [0; 32];

        for i in 0..(infos.disk_size / self.sector_size() - 1) {
            dir.read_sector(i, &mut sector)?;

            for i in 0..(self.sector_size() / 32 - 1) {
                entry.copy_from_slice(&sector[i * 32..(i + 1) * 32]);
                // TODO:
            }
        }

        todo!()
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
    /// # Errors
    ///
    /// TODO:
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

    /// # Errors
    ///
    /// TODO:
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
}
