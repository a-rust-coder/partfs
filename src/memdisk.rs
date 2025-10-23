use crate::{Disk, DiskErr, DiskInfos, Permissions, SectorSize};
use alloc::vec::Vec;
use mutex::Mutex;

/// An in-memory instance of a `Disk`.
pub struct MemDisk {
    sector_size: SectorSize,
    permissions: Permissions,
    content: Mutex<Vec<u8>>,
}

impl Disk for MemDisk {
    fn disk_infos(&self) -> Result<DiskInfos, DiskErr> {
        Ok(DiskInfos {
            sector_size: self.sector_size.clone(),
            disk_size: self.content.lock().len(),
            permissions: self.permissions,
        })
    }

    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr> {
        if !self.permissions.read {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        if !self
            .sector_size
            .is_supported(buf.len(), self.content.lock().len())
        {
            return Err(DiskErr::InvalidSectorSize {
                found: buf.len(),
                supported: self.sector_size.clone(),
                start: 0,
            });
        }

        buf.copy_from_slice(&self.content.lock()[(sector * buf.len())..((sector + 1) * buf.len())]);
        Ok(())
    }

    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr> {
        if !self.permissions.write {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        if !self
            .sector_size
            .is_supported(buf.len(), self.content.lock().len())
        {
            return Err(DiskErr::InvalidSectorSize {
                found: buf.len(),
                supported: self.sector_size.clone(),
                start: 0,
            });
        }

        self.content.lock()[(sector * buf.len())..((sector + 1) * buf.len())].copy_from_slice(buf);
        Ok(())
    }
}
