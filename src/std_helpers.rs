use crate::{Disk, DiskErr, DiskInfos, Permissions, SectorSize};
use mutex::Mutex;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

/// Wrapper type for a disk image
#[derive(Debug)]
pub struct DiskFile {
    sector_size: SectorSize,
    /// Size of the file/disk, in bytes
    size: usize,
    /// Permissions of the disk. The file is opened with the same permissions.
    permissions: Permissions,
    file: Mutex<File>,
}

impl Disk for DiskFile {
    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr> {
        // ### CHECKS FOR INVALID REQUEST ###

        if !self.permissions.read {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        let sector_size = buf.len();

        if !self.sector_size.is_supported(sector_size, self.size) {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: 0,
            });
        }

        let offset = sector_size * sector;

        if offset + sector_size > self.size {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: self.size / sector_size,
            });
        }

        // ### PERFORMS THE READ OPERATION ON THE FILE ###

        if self
            .file
            .lock()
            .seek(SeekFrom::Start(offset as u64))
            .is_err()
        {
            return Err(DiskErr::IOErr);
        }

        if self.file.lock().read_exact(buf).is_err() {
            return Err(DiskErr::IOErr);
        }

        Ok(())
    }

    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr> {
        // ### CHECKS FOR INVALID REQUEST ###

        if !self.permissions.write {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        let sector_size = buf.len();

        if !self.sector_size.is_supported(sector_size, self.size) {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: 0,
            });
        }

        let offset = sector_size * sector;

        if offset + sector_size > self.size {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: self.size / sector_size,
            });
        }

        // ### PERFORMS THE WRITE OPERATION ON THE FILE ###

        if self
            .file
            .lock()
            .seek(SeekFrom::Start(offset as u64))
            .is_err()
        {
            return Err(DiskErr::IOErr);
        }

        if self.file.lock().write_all(buf).is_err() {
            return Err(DiskErr::IOErr);
        }

        Ok(())
    }

    fn disk_infos(&self) -> Result<DiskInfos, DiskErr> {
        Ok(DiskInfos {
            sector_size: self.sector_size.clone(),
            disk_size: self.size,
            permissions: self.permissions,
        })
    }
}

impl DiskFile {
    /// Creates a new file to handle the disk image. Will result in an error if the file already
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns the errors produced when creating a file, opening a file, or trying to get its
    /// metadata.
    pub fn new(
        path: PathBuf,
        size: usize,
        sector_conf: SectorSize,
        permission: Permissions,
    ) -> io::Result<Self> {
        let file = File::create_new(path.clone())?;
        file.set_len(size as u64)?;
        drop(file);

        Self::from_file(path, sector_conf, permission)
    }

    /// Opens an existing file. Will result in an error if the file doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns the errors produced when opening a file or trying to get its metadata.
    pub fn from_file(
        file: PathBuf,
        sector_conf: SectorSize,
        permission: Permissions,
    ) -> io::Result<Self> {
        let file = File::options()
            .create_new(false)
            .write(permission.write)
            .read(permission.read)
            .open(file)?;
        let size = usize::try_from(file.metadata()?.len()).unwrap_or(0);

        Ok(Self {
            sector_size: sector_conf,
            size,
            permissions: permission,
            file: Mutex::new(file),
        })
    }
}
