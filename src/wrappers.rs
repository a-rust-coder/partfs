use crate::{Disk, DiskErr, DiskInfos, Permissions, SectorSize};
use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
use mutex::Mutex;

/// A global wrapper that can be created from any `Disk`. Is used only for creating `SubDisk`s or
/// `FragmentedSubDisk`s.
pub struct DiskWrapper {
    /// The disk from where it has been created
    disk: Mutex<Box<dyn Disk>>,
    /// The space borrowed for reading [start, end[
    r_borrows: Mutex<Vec<(usize, usize)>>,
    /// The space borrowed for writing [start, end[
    w_borrows: Mutex<Vec<(usize, usize)>>,
    /// A weak reference to self, used to give access to the `DiskWrapper` to all the `SubDisk`s
    /// created from it
    weak_self: Mutex<Weak<Self>>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for DiskWrapper {}
unsafe impl Sync for DiskWrapper {}

impl DiskWrapper {
    /// Creates a new wrapper from a disk
    pub fn new<T: Disk + 'static>(disk: T) -> Arc<Self> {
        let slf = Arc::new(Self {
            disk: Mutex::new(Box::new(disk)),
            r_borrows: Mutex::new(Vec::new()),
            w_borrows: Mutex::new(Vec::new()),
            weak_self: Mutex::new(Weak::new()),
        });
        let weak = Arc::downgrade(&slf);
        *slf.weak_self.lock() = weak;
        slf
    }

    /// Checks if a specific range of space is borrowed for reading. If any part of the range is
    /// borrowed for reading, returns true. There is not possibility to fail for this function, so
    /// it will correctly return even if the disk is not available.
    pub fn is_r_borrowed(&self, start: usize, end: usize) -> bool {
        for i in &*self.r_borrows.lock() {
            if (i.0 <= start && start < i.1) || (i.0 < end && end <= i.1) {
                return true;
            }
        }
        false
    }

    /// Checks if a specific range of space is borrowed for writing. If any part of the range is
    /// borrowed for reading, returns true. There is not possibility to fail for this function, so
    /// it will correctly return even if the disk is not available.
    pub fn is_w_borrowed(&self, start: usize, end: usize) -> bool {
        for i in &*self.w_borrows.lock() {
            if (i.0 <= start && start < i.1) || (i.0 < end && end <= i.1) {
                return true;
            }
        }
        false
    }

    /// Creates a new subdisk in a given range of space, with the specified permissions. This
    /// function ensures the space is not already borrowed for read/write (depending on the
    /// permissions). The read/write borrows follow the Rust borrowing rule: one mutable (write or
    /// read/write) borrow or unlimited immutable borrows (read only).
    ///
    /// # Errors
    ///
    /// `Busy` if the space is already borrowed.
    ///
    /// `InvalidDiskSize` if `end` is after the end of `self`.
    ///
    /// Any error that `self.disk.disk_infos()` may return.
    pub fn subdisk(
        &self,
        start: usize,
        end: usize,
        permissions: Permissions,
    ) -> Result<SubDisk, DiskErr> {
        // ### CHECKS IF THE SPACE IS AVAILABLE ###

        if self.is_w_borrowed(start, end) || (self.is_r_borrowed(start, end) && permissions.write) {
            return Err(DiskErr::Busy);
        }

        if end > self.disk.lock().disk_infos()?.disk_size {
            return Err(DiskErr::InvalidDiskSize);
        }

        // ### REGISTERS THE SPACE AS USED ###

        if permissions.read {
            self.r_borrows.lock().push((start, end));
        }

        if permissions.write {
            self.w_borrows.lock().push((start, end));
        }

        // ### CREATES THE SUBDISK ###

        let sector_size = self.disk_infos()?.sector_size;
        let parent = self.weak_self.lock().clone();

        Ok(SubDisk {
            parent,
            start,
            end,
            sector_size,
            permissions,
        })
    }

    /// # Errors
    ///
    /// Returns an error if a part is out of the disk space, or the space is already borrowed.
    pub fn fragmented_subdisk(
        &self,
        parts: Vec<(usize, usize)>,
        permissions: Permissions,
    ) -> Result<FragmentedSubDisk, DiskErr> {
        let mut w_borrows = self.w_borrows.lock();
        let mut r_borrows = self.r_borrows.lock();
        let disk_size = self.disk_infos()?.disk_size;

        let mut size = 0;

        for &(start, end) in &parts {
            if start > end || end > disk_size {
                return Err(DiskErr::InvalidDiskSize);
            }

            if permissions.read
                && (|| {
                    for i in r_borrows.clone() {
                        if (i.0 <= start && start < i.1) || (i.0 < end && end <= i.1) {
                            return true;
                        }
                    }
                    false
                })()
            {
                return Err(DiskErr::SpaceAlreadyInUse);
            }

            if permissions.write
                && (|| {
                    for i in w_borrows.clone() {
                        if (i.0 <= start && start < i.1) || (i.0 < end && end <= i.1) {
                            return true;
                        }
                    }
                    false
                })()
            {
                return Err(DiskErr::SpaceAlreadyInUse);
            }

            size += end - start;
        }

        if permissions.read {
            r_borrows.extend_from_slice(&parts);
        }

        if permissions.write {
            w_borrows.extend_from_slice(&parts);
        }

        let sector_size = self.disk_infos()?.sector_size;
        let parent = self.weak_self.lock().clone();

        Ok(FragmentedSubDisk {
            parent,
            parts,
            size,
            sector_size,
            permissions,
        })
    }
}

impl Disk for DiskWrapper {
    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr> {
        // ### VERIFIES IF THE SECTION IS CURRENTLY BORROWED ###

        let start = sector * buf.len();
        let end = start + buf.len();

        if self.is_w_borrowed(start, end) {
            return Err(DiskErr::Busy);
        }

        self.disk.lock().read_sector(sector, buf)
    }

    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr> {
        // ### VERIFIES IF THE SECTION IS CURRENTLY BORROWED ###

        let start = sector * buf.len();
        let end = start + buf.len();

        if self.is_w_borrowed(start, end) || self.is_r_borrowed(start, end) {
            return Err(DiskErr::Busy);
        }

        self.disk.lock().write_sector(sector, buf)
    }

    fn disk_infos(&self) -> Result<crate::DiskInfos, DiskErr> {
        self.disk.lock().disk_infos()
    }
}

#[derive(Debug)]
pub struct SubDisk {
    parent: Weak<DiskWrapper>,
    start: usize,
    end: usize,
    sector_size: SectorSize,
    permissions: Permissions,
}

impl Disk for SubDisk {
    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr> {
        // ### GETS THE PARENT ###

        let Some(parent) = self.parent.upgrade() else {
            return Err(DiskErr::UnreachableDisk);
        };

        // ### CHECKS THE PERMISSIONS ###

        if !self.permissions.read {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        // ### VERIFIES THE SECTOR SIZE ###

        let sector_size = buf.len();

        if !self
            .sector_size
            .is_supported(sector_size, self.end - self.start)
            || !self.start.is_multiple_of(sector_size)
        {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: self.start,
            });
        }

        // ### VERIFIES IF THE SECTOR IS IN THE SUBDISK RANGE ###

        let offset = self.start + sector_size * sector;

        if offset >= self.end {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: (self.end - self.start) / sector_size,
            });
        }

        let sector = offset / sector_size;

        parent.disk.lock().read_sector(sector, buf)
    }

    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr> {
        // ### GETS THE PARENT ###

        let Some(parent) = self.parent.upgrade() else {
            return Err(DiskErr::UnreachableDisk);
        };

        // ### CHECKS THE PERMISSIONS ###

        if !self.permissions.write {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        // ### VERIFIES THE SECTOR SIZE ###

        let sector_size = buf.len();

        if !self
            .sector_size
            .is_supported(sector_size, self.end - self.start)
            || !self.start.is_multiple_of(sector_size)
        {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: self.start,
            });
        }

        // ### VERIFIES IF THE SECTOR IS IN THE SUBDISK RANGE ###

        let offset = self.start + sector_size * sector;

        if offset >= self.end {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: (self.end - self.start) / sector_size,
            });
        }

        let sector = offset / sector_size;

        parent.disk.lock().write_sector(sector, buf)
    }

    fn disk_infos(&self) -> Result<DiskInfos, DiskErr> {
        Ok(DiskInfos {
            sector_size: self.sector_size.clone(),
            disk_size: self.end - self.start,
            permissions: self.permissions,
        })
    }
}

impl Drop for SubDisk {
    fn drop(&mut self) {
        // Gets the parent. If the parent has been dropped, no need to do nothing.
        if let Some(parent) = self.parent.upgrade() {
            // Remove the subdisk range from the read borrows if needed
            if self.permissions.read {
                let mut r_borrows = parent.r_borrows.lock();
                let idx = r_borrows
                    .iter()
                    .position(|&x| x == (self.start, self.end))
                    .unwrap();
                r_borrows.swap_remove(idx);
            }
            // Remove the subdisk range from the write borrows if needed
            if self.permissions.write {
                let mut w_borrows = parent.w_borrows.lock();
                let idx = w_borrows
                    .iter()
                    .position(|&x| x == (self.start, self.end))
                    .unwrap();
                w_borrows.swap_remove(idx);
            }
        }
    }
}

/// Acts like a `SubDisk` but can handle non-continuous space.
#[derive(Debug)]
pub struct FragmentedSubDisk {
    parent: Weak<DiskWrapper>,
    /// In sectors
    parts: Vec<(usize, usize)>,
    size: usize,
    sector_size: SectorSize,
    permissions: Permissions,
}

impl Disk for FragmentedSubDisk {
    fn read_sector(&self, sector: usize, buf: &mut [u8]) -> Result<(), DiskErr> {
        // ### GETS THE PARENT ###

        let Some(parent) = self.parent.upgrade() else {
            return Err(DiskErr::UnreachableDisk);
        };

        // ### CHECKS THE PERMISSIONS ###

        if !self.permissions.read {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        if self.parts.is_empty() {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: 0,
            });
        }

        // ### VERIFIES THE SECTOR SIZE ###

        let sector_size = buf.len();

        if !self.sector_size.is_supported(sector_size, self.size) {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: self.parts[0].0,
            });
        }

        let mut offset = 0;
        let mut current_sector = 0;

        for &(start, end) in &self.parts {
            let size = end - start;
            if size % sector_size != 0 || start % sector_size != 0 {
                return Err(DiskErr::InvalidSectorSize {
                    found: sector_size,
                    supported: self.sector_size.clone(),
                    start: self.parts[0].0,
                });
            }

            if current_sector + size / sector_size > sector {
                offset = start + (sector - current_sector) * sector_size;
                break;
            }

            current_sector += size / sector_size;
        }

        parent.disk.lock().read_sector(offset / sector_size, buf)
    }

    fn write_sector(&self, sector: usize, buf: &[u8]) -> Result<(), DiskErr> {
        // ### GETS THE PARENT ###

        let Some(parent) = self.parent.upgrade() else {
            return Err(DiskErr::UnreachableDisk);
        };

        // ### CHECKS THE PERMISSIONS ###

        if !self.permissions.write {
            return Err(DiskErr::InvalidPermission {
                disk_permissions: self.permissions,
            });
        }

        if self.parts.is_empty() {
            return Err(DiskErr::InvalidSectorIndex {
                found: sector,
                max: 0,
            });
        }

        // ### VERIFIES THE SECTOR SIZE ###

        let sector_size = buf.len();

        if !self.sector_size.is_supported(sector_size, self.size) {
            return Err(DiskErr::InvalidSectorSize {
                found: sector_size,
                supported: self.sector_size.clone(),
                start: self.parts[0].0,
            });
        }

        let mut offset = 0;
        let mut current_sector = 0;

        for &(start, end) in &self.parts {
            let size = end - start;
            if size % sector_size != 0 || start % sector_size != 0 {
                return Err(DiskErr::InvalidSectorSize {
                    found: sector_size,
                    supported: self.sector_size.clone(),
                    start: self.parts[0].0,
                });
            }

            if current_sector + size / sector_size > sector {
                offset = start + (sector - current_sector) * sector_size;
                break;
            }

            current_sector += size / sector_size;
        }

        parent.disk.lock().write_sector(offset / sector_size, buf)
    }

    fn disk_infos(&self) -> Result<DiskInfos, DiskErr> {
        Ok(DiskInfos {
            sector_size: self.sector_size.clone(),
            disk_size: self.size,
            permissions: self.permissions,
        })
    }
}

impl Drop for FragmentedSubDisk {
    fn drop(&mut self) {
        // Gets the parent. If the parent has been dropped, no need to do nothing.
        if let Some(parent) = self.parent.upgrade() {
            // Remove the subdisk range from the read borrows if needed
            if self.permissions.read {
                let mut r_borrows = parent.r_borrows.lock();

                for &(start, end) in &self.parts {
                    let idx = r_borrows.iter().position(|&x| x == (start, end)).unwrap();
                    r_borrows.swap_remove(idx);
                }
            }
            // Remove the subdisk range from the write borrows if needed
            if self.permissions.write {
                let mut w_borrows = parent.w_borrows.lock();

                for &(start, end) in &self.parts {
                    let idx = w_borrows.iter().position(|&x| x == (start, end)).unwrap();
                    w_borrows.swap_remove(idx);
                }
            }
        }
    }
}
