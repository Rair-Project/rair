//! RIO main implementation.

use crate::desc::RIODesc;
use crate::descquery::RIODescQuery;
use crate::mapsquery::{RIOMap, RIOMapQuery};
use crate::plugin::RIOPlugin;
use crate::plugins;
use crate::utils::{IoError, IoMode};
use alloc::{collections::BTreeMap, sync::Arc};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

// Credits goes to @Talchas#7429 for the idea of using remote
// to create something that behaves as finalize_hook() for
// the deserializing subroutine.
// Talchas: #[serde(remote="Foo") struct Bar makes it generate a
//          mod Bar { fn deserialize... -> Result<Foo ...> } instead of
//          impl Deserialize for Bar { fn deserialize... -> Result<Bar ...>
//          it's designed to let you copypaste the definition of a type in
//          some other crate that doesn't implement (De)serialize, and get
//          methods that you can use with #[serde(with = ..)]. here I'm using
//          it to just move the methods from a trait impl you can't wrap or
//          override to separate functions you can
//
/// [RIO] is abstraction over IO, It allows you to open (more than one) file each with its own
/// encoding in one address space, which you can read / write or map parts of without knowing
/// which file is really being accessed
#[derive(Default, Serialize, Deserialize)]
#[serde(remote = "RIO")]
pub struct RIO {
    descs: RIODescQuery,
    maps: RIOMapQuery,
    #[serde(skip)]
    plugins: Vec<Box<dyn RIOPlugin + Sync + Send>>,
}

impl Serialize for RIO {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RIO::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RIO {
    fn deserialize<D>(deserializer: D) -> Result<RIO, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut io = RIO::deserialize(deserializer)?;
        plugins::load_plugins(&mut io);
        for desc in &mut io.descs {
            let mut found = false;
            for plugin in &mut io.plugins {
                if plugin.accept_uri(&desc.name) {
                    desc.reopen(&mut **plugin).map_err(de::Error::custom)?;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(de::Error::custom(IoError::IoPluginNotFoundError));
            }
        }
        Ok(io)
    }
}

impl RIO {
    /// Returns new Input/Output interface to be used
    ///
    /// # Example
    /// ````
    /// use rair_io::RIO;
    /// let mut io = RIO::new();
    /// ````
    #[must_use]
    pub fn new() -> RIO {
        let mut io: RIO = RIO::default();
        plugins::load_plugins(&mut io);
        io
    }

    /// THIS FUNCTION IS NOT SUPPOSED TO BE THAT TRIVIAL
    /// I WANT IT TO LITERALLY OPEN A PLUGIN FILE
    pub fn load_plugin(&mut self, plugin: Box<dyn RIOPlugin + Sync + Send>) {
        self.plugins.push(plugin);
    }
    /// Allows us to open file and have it accessable from out physical address space,
    /// *open* will automatically load the file in the smallest available physical address while
    /// [`RIO::open_at`] will allow user to determine what physical address to use. `uri` is
    /// used to describe file path as well as data encoding if needed. `flags` is used to
    /// describe permision used while opening file.
    ///
    /// # Return value
    /// the unique file handler represented by [u64] is returned. In case of error, an [`IoError`]
    /// is returned explaining why opening file failed.
    ///
    /// # Example
    ///
    /// ```
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// let mut io = RIO::new();
    /// io.open("hello.txt", IoMode::READ);
    /// ```
    pub fn open(&mut self, uri: &str, flags: IoMode) -> Result<u64, IoError> {
        for plugin in &mut self.plugins {
            if plugin.accept_uri(uri) {
                if let Ok(hndl) = self.descs.register_open_default(&mut **plugin, uri, flags) {
                    return Ok(hndl);
                }
                return self.descs.register_open(&mut **plugin, uri, flags);
            }
        }
        Err(IoError::IoPluginNotFoundError)
    }

    /// Allows us to open file and have it accessable from out physical address space
    /// at physicall address of out choice, `uri` is used to describe file path as
    /// well as data encoding if needed. `flags` is used to describe permision used
    /// while opening file.
    ///
    /// # Return value
    /// the unique file handler represented by [u64] is returned. In case of error, an [`IoError`]
    /// is returned explaining why opening file failed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rair_io::{RIO, IoMode, IoError};
    /// fn main() -> Result<(), IoError> {
    ///     let mut io = RIO::new();
    ///     io.open_at("hello.txt", IoMode::READ | IoMode::WRITE, 0x4000)?;
    ///     return Ok(());
    /// }
    /// ```
    pub fn open_at(&mut self, uri: &str, flags: IoMode, at: u64) -> Result<u64, IoError> {
        for plugin in &mut self.plugins {
            if plugin.accept_uri(uri) {
                return self.descs.register_open_at(&mut **plugin, uri, flags, at);
            }
        }
        Err(IoError::IoPluginNotFoundError)
    }

    /// Close an opened file, delete its physical and virtual address space.
    /// In case of Error, an [`IoError`] is returned explaining why *close* failed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// use rair_io::IoError;
    /// fn main() -> Result<(), IoError> {
    ///     let mut io = RIO::new();
    ///     let hndl = io.open("hello.txt", IoMode::READ)?;
    ///     io.close(hndl)?;
    ///     return Ok(());
    /// }
    /// ```

    pub fn close(&mut self, hndl: u64) -> Result<(), IoError> {
        // delete all memory mappings related to the closed handle
        self.descs.close(hndl)?;
        Ok(())
    }

    /// Close all open files, and reset all virtual and physical address spaces.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// use rair_io::IoError;
    /// fn main() -> Result<(), IoError> {
    ///     let mut io = RIO::new();
    ///     io.open("foo.txt", IoMode::READ)?;
    ///     io.open("bar.txt", IoMode::READ)?;
    ///     io.close_all();
    ///     return Ok(());
    /// }
    /// ```

    pub fn close_all(&mut self) {
        self.maps = RIOMapQuery::new();
        self.descs = RIODescQuery::new();
    }

    /// Read from the physical address space of current [RIO] object. If there is no enough
    /// data to fill *buf* an error is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// let mut io = RIO::new();
    /// io.open_at("foo.txt", IoMode::READ, 0x20);
    /// let mut fillme: Vec<u8> = vec![0; 8];
    /// io.pread(0x20, &mut fillme);
    /// ```
    pub fn pread(&mut self, paddr: u64, buf: &mut [u8]) -> Result<(), IoError> {
        let result = self.descs.paddr_range_to_hndl(paddr, buf.len() as u64);
        if let Some(operations) = result {
            let mut start = 0;
            for (hndl, paddr, size) in operations {
                let desc = self.descs.hndl_to_mut_desc(hndl).unwrap();
                desc.read(
                    paddr as usize,
                    &mut buf[start as usize..(start + size) as usize],
                )?;
                start += size;
            }
            Ok(())
        } else {
            Err(IoError::AddressNotFound)
        }
    }
    /// Read from the physical address space of current [RIO] object. Data is stored in a sparce
    /// vector represented by [`BTreeMap`]. Error is returned only in case of internal IO errors.
    ///
    /// # Example
    ///
    /// ```
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// let mut io = RIO::new();
    /// io.open_at("foo.txt", IoMode::READ, 0x20);
    /// let data = io.pread_sparce(0x20, 0x50); //reads at most 0x50 bytes from foo.txt
    ///```  
    pub fn pread_sparce(&mut self, paddr: u64, size: u64) -> Result<BTreeMap<u64, u8>, IoError> {
        let mut result = BTreeMap::new();
        let ranges = self.descs.paddr_sparce_range_to_hndl(paddr, size);
        for (hndl, paddr, size) in ranges {
            let desc = self.descs.hndl_to_mut_desc(hndl).unwrap();
            let mut buffer = vec![0; size as usize];
            desc.read(paddr as usize, &mut buffer)?;
            for (i, v) in buffer.iter().enumerate() {
                result.insert(paddr + i as u64, *v);
            }
        }
        Ok(result)
    }
    /// Write into the physical address space of current [RIO] object. If there is no enough
    /// space to accomodate *buf* an error is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use rair_io::RIO;
    /// use rair_io::IoMode;
    /// let mut io = RIO::new();
    /// io.open_at("foo.txt", IoMode::READ, 0x20);
    /// let fillme: Vec<u8> = vec![0; 8];
    /// io.pwrite(0x20, &fillme);
    /// ```
    pub fn pwrite(&mut self, paddr: u64, buf: &[u8]) -> Result<(), IoError> {
        let result = self.descs.paddr_range_to_hndl(paddr, buf.len() as u64);
        if let Some(operations) = result {
            let mut start = 0;
            for (hndl, paddr, size) in operations {
                let desc = self.descs.hndl_to_mut_desc(hndl).unwrap();
                desc.write(
                    paddr as usize,
                    &buf[start as usize..(start + size) as usize],
                )?;
                start += size;
            }
            Ok(())
        } else {
            Err(IoError::AddressNotFound)
        }
    }
    ///  Map memory regions from physical address space to virtual address space
    pub fn map(&mut self, paddr: u64, vaddr: u64, size: u64) -> Result<(), IoError> {
        if self.descs.paddr_range_to_hndl(paddr, size).is_none() {
            return Err(IoError::AddressNotFound);
        }
        self.maps.map(paddr, vaddr, size)
    }

    /// unmap already mapped regions
    pub fn unmap(&mut self, vaddr: u64, size: u64) -> Result<(), IoError> {
        self.maps.unmap(vaddr, size)
    }

    /// read memory from virtual address space. If there is no enough
    /// data to fill *buf* an error is returned.
    pub fn vread(&mut self, vaddr: u64, buf: &mut [u8]) -> Result<(), IoError> {
        let result = self.maps.split_vaddr_range(vaddr, buf.len() as u64);
        if let Some(maps) = result {
            let mut start = 0;
            for map in maps {
                self.pread(
                    map.paddr,
                    &mut buf[start as usize..(start + map.size) as usize],
                )?;
                start += map.size;
            }
            Ok(())
        } else {
            Err(IoError::AddressNotFound)
        }
    }
    /// read memory from virtual address space. Data is stored in a sparce
    /// vector represented by [`BTreeMap`]. Error is returned only in case of
    /// internal IO errors.
    pub fn vread_sparce(&mut self, vaddr: u64, size: u64) -> Result<BTreeMap<u64, u8>, IoError> {
        let mut result = BTreeMap::new();
        let maps = self.maps.split_vaddr_sparce_range(vaddr, size);
        for map in maps {
            let mut buf = vec![0; map.size as usize];
            self.pread(map.paddr, &mut buf)?;
            for (i, v) in buf.iter().enumerate() {
                result.insert(map.vaddr + i as u64, *v);
            }
        }
        Ok(result)
    }
    /// write memory into virtual address space
    pub fn vwrite(&mut self, vaddr: u64, buf: &[u8]) -> Result<(), IoError> {
        let result = self.maps.split_vaddr_range(vaddr, buf.len() as u64);
        if let Some(maps) = result {
            let mut start = 0;
            for map in maps {
                self.pwrite(map.paddr, &buf[start as usize..(start + map.size) as usize])?;
                start += map.size;
            }
            Ok(())
        } else {
            Err(IoError::AddressNotFound)
        }
    }

    /// convert virtual address to physical address
    #[must_use]
    pub fn vir_to_phy(&self, vaddr: u64, size: u64) -> Option<Vec<RIOMap>> {
        self.maps.split_vaddr_range(vaddr, size)
    }
    /// This funciton reverse-queries individual physical addresses. It convert
    /// physical address to virtual address. The return value is a vector of
    /// virtual addresses, all of which would map to the provided physical address
    #[must_use]
    pub fn phy_to_vir(&self, phy: u64) -> Vec<u64> {
        self.maps.rev_query(phy)
    }

    /// Iterate over open URIs
    #[must_use]
    pub fn uri_iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a RIODesc> + 'a> {
        self.descs.into_iter()
    }

    /// Iterate over memory maps
    #[must_use]
    pub fn map_iter<'a>(&'a self) -> Box<dyn Iterator<Item = Arc<RIOMap>> + 'a> {
        self.maps.into_iter()
    }
    /// Return equivalent [`RIODesc`] structure for the given *hndl*
    #[must_use]
    pub fn hndl_to_desc(&self, hndl: u64) -> Option<&RIODesc> {
        self.descs.hndl_to_desc(hndl)
    }
}

#[cfg(test)]
mod rio_tests {

    use super::*;
    use std::io;
    use std::path::Path;
    use test_file::*;
    fn test_failing_open_cb(path: &[&Path]) {
        let mut io = RIO::new();
        let mut bad_path = "badformat://".to_owned();
        bad_path.push_str(&path[0].to_string_lossy());
        let mut e = io.open(&bad_path, IoMode::READ);
        assert_eq!(e.err().unwrap(), IoError::IoPluginNotFoundError);
        e = io.open_at(&bad_path, IoMode::READ, 0x500);
        assert_eq!(e.err().unwrap(), IoError::IoPluginNotFoundError);
        io.open(&path[0].to_string_lossy(), IoMode::READ).unwrap();
        e = io.open_at(&path[1].to_string_lossy(), IoMode::READ, 0);
        assert_eq!(e.err().unwrap(), IoError::AddressesOverlapError);
        io.open(&path[1].to_string_lossy(), IoMode::READ).unwrap();
        e = io.open_at(&path[1].to_string_lossy(), IoMode::READ, 0);
        assert_eq!(e.err().unwrap(), IoError::AddressesOverlapError);
        io.close_all();
    }
    #[test]
    fn test_failing_open() {
        operate_on_files(&test_failing_open_cb, &[DATA, DATA]);
    }
    fn test_pread_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut fillme: Vec<u8> = vec![0; 8];

        for path in paths {
            io.open(&path.to_string_lossy(), IoMode::READ).unwrap();
        }
        // First normal read
        io.pread(0, &mut fillme).unwrap();
        assert_eq!(fillme, &DATA[0..8]);
        // Second we read through 1 desc into another desc
        fillme = vec![0; DATA.len() * 3 / 2];
        io.pread(0, &mut fillme).unwrap();
        let mut sanity_data: Vec<u8> = vec![0; DATA.len() * 3 / 2];
        sanity_data[0..DATA.len()].copy_from_slice(DATA);
        let l = sanity_data.len() - DATA.len();
        sanity_data[DATA.len()..DATA.len() * 3 / 2].copy_from_slice(&DATA[0..l]);
        assert_eq!(fillme, sanity_data);
        // Now we make sure that we can read through all three descs
        fillme = vec![0; DATA.len() * 5 / 2];
        io.pread(0, &mut fillme).unwrap();
        sanity_data = vec![0; DATA.len() * 5 / 2];
        sanity_data[0..DATA.len()].copy_from_slice(DATA);
        sanity_data[DATA.len()..DATA.len() * 2].copy_from_slice(DATA);
        let l = sanity_data.len() - DATA.len() * 2;
        sanity_data[DATA.len() * 2..DATA.len() * 5 / 2].copy_from_slice(&DATA[0..l]);
        assert_eq!(fillme, sanity_data);
    }
    #[test]
    fn test_pread() {
        operate_on_files(&test_pread_cb, &[DATA, DATA, DATA]);
    }
    fn test_fail_pread_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut fillme: Vec<u8> = vec![0; 8];
        io.open(&paths[0].to_string_lossy(), IoMode::READ).unwrap();
        let mut e = io.pread(0x500, &mut fillme);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
        fillme = vec![0; DATA.len() + 1];
        e = io.pread(0, &mut fillme);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
        io.open(&paths[1].to_string_lossy(), IoMode::READ).unwrap();
        io.open_at(
            &paths[2].to_string_lossy(),
            IoMode::READ,
            DATA.len() as u64 * 2 + 1,
        )
        .unwrap();
        fillme = vec![0; DATA.len() * 3];
        e = io.pread(0, &mut fillme);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
    }
    #[test]
    fn test_fail_pread() {
        operate_on_files(&test_fail_pread_cb, &[DATA, DATA, DATA]);
    }

    fn test_pwrite_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut fillme: Vec<u8> = vec![0; 8];

        for path in paths {
            io.open(&path.to_string_lossy(), IoMode::READ | IoMode::WRITE)
                .unwrap();
        }
        // First normal write
        io.pwrite(0, &fillme).unwrap();
        io.pread(0, &mut fillme).unwrap();
        assert_eq!(fillme, &[0; 8]);
        // Second we write through 1 desc into another desc
        fillme = vec![1; DATA.len() * 3 / 2];
        io.pwrite(0, &fillme).unwrap();
        io.pread(0, &mut fillme).unwrap();
        assert_eq!(fillme, vec![1; DATA.len() * 3 / 2]);
        // Now we make sure that we can write through all three descs
        fillme = vec![2; DATA.len() * 5 / 2];
        io.pwrite(0, &fillme).unwrap();
        io.pread(0, &mut fillme).unwrap();
        assert_eq!(fillme, vec![2; DATA.len() * 5 / 2]);
    }
    #[test]
    fn test_pwrite() {
        operate_on_files(&test_pwrite_cb, &[DATA, DATA, DATA]);
    }
    fn test_fail_pwrite_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let permission_denied = IoError::Parse(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "File Not Writable",
        ));
        let mut write_me: Vec<u8> = vec![0; 8];
        io.open(&paths[0].to_string_lossy(), IoMode::READ).unwrap();
        let mut e = io.pwrite(0, &write_me);
        assert_eq!(e.err().unwrap(), permission_denied);
        io.close(0).unwrap();
        io.open(&paths[0].to_string_lossy(), IoMode::READ | IoMode::WRITE)
            .unwrap();
        e = io.pwrite(0x500, &write_me);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
        write_me = vec![0; DATA.len() + 1];
        e = io.pwrite(0, &write_me);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
        io.open(&paths[1].to_string_lossy(), IoMode::READ | IoMode::WRITE)
            .unwrap();
        io.open_at(
            &paths[2].to_string_lossy(),
            IoMode::READ | IoMode::WRITE,
            DATA.len() as u64 * 2 + 1,
        )
        .unwrap();
        write_me = vec![0; DATA.len() * 3];
        e = io.pwrite(0, &write_me);
        assert_eq!(e.err().unwrap(), IoError::AddressNotFound);
    }
    #[test]
    fn test_fail_pwrite() {
        operate_on_files(&test_fail_pwrite_cb, &[DATA, DATA, DATA]);
    }

    fn test_map_unmap_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0x1000)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x2000)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x3000)
            .unwrap();
        io.map(0x1000, 0x400, DATA.len() as u64).unwrap();
        io.map(0x2000, 0x400 + DATA.len() as u64, DATA.len() as u64)
            .unwrap();
        io.map(0x3000, 0x400 + DATA.len() as u64 * 2, DATA.len() as u64)
            .unwrap();
        let mut maps = vec![
            RIOMap {
                paddr: 0x1000,
                vaddr: 0x400,
                size: DATA.len() as u64,
            },
            RIOMap {
                paddr: 0x2000,
                vaddr: 0x400 + DATA.len() as u64,
                size: DATA.len() as u64,
            },
            RIOMap {
                paddr: 0x3000,
                vaddr: 0x400 + DATA.len() as u64 * 2,
                size: DATA.len() as u64,
            },
        ];
        assert_eq!(io.vir_to_phy(0x400, DATA.len() as u64 * 3).unwrap(), maps);
        io.unmap(0x400 + DATA.len() as u64, DATA.len() as u64 / 2)
            .unwrap();
        assert_eq!(io.vir_to_phy(0x400, DATA.len() as u64 * 3), None);
        maps = vec![RIOMap {
            paddr: 0x1000,
            vaddr: 0x400,
            size: DATA.len() as u64,
        }];
        assert_eq!(io.vir_to_phy(0x400, DATA.len() as u64).unwrap(), maps);
        maps = vec![
            RIOMap {
                paddr: 0x2000 + DATA.len() as u64 / 2,
                vaddr: 0x400 + DATA.len() as u64 * 3 / 2,
                size: DATA.len() as u64 - DATA.len() as u64 / 2,
            },
            RIOMap {
                paddr: 0x3000,
                vaddr: 0x400 + DATA.len() as u64 * 2,
                size: DATA.len() as u64,
            },
        ];
        assert_eq!(
            io.vir_to_phy(
                0x400 + DATA.len() as u64 * 3 / 2,
                DATA.len() as u64 * 2 - DATA.len() as u64 / 2
            )
            .unwrap(),
            maps
        );
        assert_eq!(
            io.map(0x200, 0x7000, 0x50).err().unwrap(),
            IoError::AddressNotFound
        );
    }
    #[test]
    fn test_map_unmap() {
        operate_on_files(&test_map_unmap_cb, &[DATA, DATA, DATA]);
    }

    fn test_vread_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut fillme: Vec<u8> = vec![0; 8];
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0x1000)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x2000)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x3000)
            .unwrap();
        io.map(0x1000, 0x400, DATA.len() as u64).unwrap();
        io.map(0x2000, 0x400 + DATA.len() as u64, DATA.len() as u64)
            .unwrap();
        io.map(0x3000, 0x400 + DATA.len() as u64 * 2, DATA.len() as u64)
            .unwrap();
        io.vread(0x400, &mut fillme).unwrap();
        assert_eq!(fillme, &DATA[0..8]);

        // second we read from one map into another
        io.vread(0x400 + DATA.len() as u64 - 4, &mut fillme)
            .unwrap();
        let mut sanity_data: Vec<u8> = Vec::with_capacity(8);
        sanity_data.extend(&DATA[DATA.len() - 4..DATA.len()]);
        sanity_data.extend(&DATA[0..4]);
        assert_eq!(fillme, sanity_data);

        // Now we make sure that we can read through all maps
        sanity_data = Vec::with_capacity(DATA.len());
        fillme = vec![1; DATA.len() * 3];
        for _ in 0i32..3i32 {
            sanity_data.extend(DATA);
        }
        io.vread(0x400, &mut fillme).unwrap();
        assert_eq!(fillme, sanity_data);
        assert_eq!(
            io.vread(0x300, &mut fillme).err().unwrap(),
            IoError::AddressNotFound
        );
    }
    #[test]
    fn test_vread() {
        operate_on_files(&test_vread_cb, &[DATA, DATA, DATA]);
    }

    fn test_vwrite_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut fillme: Vec<u8> = vec![0; 8];
        io.open_at(
            &paths[0].to_string_lossy(),
            IoMode::READ | IoMode::WRITE,
            0x1000,
        )
        .unwrap();
        io.open_at(
            &paths[1].to_string_lossy(),
            IoMode::READ | IoMode::WRITE,
            0x2000,
        )
        .unwrap();
        io.open_at(
            &paths[2].to_string_lossy(),
            IoMode::READ | IoMode::WRITE,
            0x3000,
        )
        .unwrap();
        io.map(0x1000, 0x400, DATA.len() as u64).unwrap();
        io.map(0x2000, 0x400 + DATA.len() as u64, DATA.len() as u64)
            .unwrap();
        io.map(0x3000, 0x400 + DATA.len() as u64 * 2, DATA.len() as u64)
            .unwrap();
        io.vwrite(0x400, &fillme).unwrap();
        io.vread(0x400, &mut fillme).unwrap();
        assert_eq!(fillme, &[0; 8]);

        // second we read from one map into another
        io.vwrite(0x400 + DATA.len() as u64 - 4, &fillme).unwrap();
        io.vread(0x400 + DATA.len() as u64 - 4, &mut fillme)
            .unwrap();
        assert_eq!(fillme, &[0; 8]);

        // Now we make sure that we can read through all maps

        fillme = vec![1; DATA.len() * 3];
        io.vwrite(0x400, &fillme).unwrap();
        io.vread(0x400, &mut fillme).unwrap();
        assert_eq!(fillme, vec![1; DATA.len() * 3]);
        assert_eq!(
            io.vwrite(0x300, &fillme).err().unwrap(),
            IoError::AddressNotFound
        );
    }
    #[test]
    fn test_vwrite() {
        operate_on_files(&test_vwrite_cb, &[DATA, DATA, DATA]);
    }

    fn uri_iter_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        for path in paths {
            io.open(&path.to_string_lossy(), IoMode::READ).unwrap();
        }
        let mut paddr = 0;
        for desc in io.uri_iter() {
            assert_eq!(paddr, desc.paddr_base());
            assert_eq!(DATA.len() as u64, desc.size());
            assert_eq!(IoMode::READ, desc.perm());
            paddr += desc.size();
        }
    }
    #[test]
    fn test_uri_iter() {
        operate_on_files(&uri_iter_cb, &[DATA, DATA, DATA, DATA]);
    }

    fn map_iter_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let size = DATA.len() as u64;
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x100)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x200)
            .unwrap();
        io.open_at(&paths[3].to_string_lossy(), IoMode::READ, 0x300)
            .unwrap();
        io.map(0, 0x4000, DATA.len() as u64).unwrap();
        io.map(0x100, 0x5000, size).unwrap();
        io.map(0x200, 0x2000, size).unwrap();
        io.map(0x300, 0x3000, size).unwrap();
        let mut iter = io.map_iter();
        assert_eq!(
            iter.next().unwrap(),
            RIOMap {
                paddr: 0x200,
                vaddr: 0x2000,
                size
            }
        );
        assert_eq!(
            iter.next().unwrap(),
            RIOMap {
                paddr: 0x300,
                vaddr: 0x3000,
                size
            }
        );
        assert_eq!(
            iter.next().unwrap(),
            RIOMap {
                paddr: 0,
                vaddr: 0x4000,
                size
            }
        );
        assert_eq!(
            iter.next().unwrap(),
            RIOMap {
                paddr: 0x100,
                vaddr: 0x5000,
                size
            }
        );
        assert_eq!(iter.next(), None);
    }
    #[test]
    fn test_map_iter() {
        operate_on_files(&map_iter_cb, &[DATA, DATA, DATA, DATA]);
    }

    fn pread_sparce_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let mut start = 0;
        for path in paths {
            io.open_at(&(path).to_string_lossy(), IoMode::READ, start)
                .unwrap();
            start += DATA.len() as u64 + 0x10;
        }
        let len = DATA.len() as u64;
        let mut data: BTreeMap<u64, u8> = BTreeMap::new();
        assert_eq!(io.pread_sparce(len, 0x10).unwrap(), data);
        for i in 0..len {
            data.insert(i, DATA[i as usize]);
        }
        assert_eq!(io.pread_sparce(0, len + 0x10).unwrap(), data);
        for i in 0..len {
            data.insert(len + 0x10 + i, DATA[i as usize]);
        }
        for i in 0..len - 0x20 {
            data.insert((len + 0x10) * 2 + i, DATA[i as usize]);
        }
        assert_eq!(io.pread_sparce(0, len * 3).unwrap(), data);
    }

    #[test]
    fn test_pread_sparce() {
        operate_on_files(&pread_sparce_cb, &[DATA, DATA, DATA, DATA]);
    }
    fn vread_sparce_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let len = DATA.len() as u64;
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0x1000)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x2000)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x3000)
            .unwrap();
        io.map(0x1000, 0x400, len).unwrap();
        io.map(0x2000, 0x400 + len + 0x10, len).unwrap();
        io.map(0x3000, 0x400 + (len + 0x10) * 2, len).unwrap();
        let mut data: BTreeMap<u64, u8> = BTreeMap::new();
        assert_eq!(io.vread_sparce(0x400 + len, 0x10).unwrap(), data);
        for i in 0..len {
            data.insert(0x400 + i, DATA[i as usize]);
        }
        assert_eq!(io.vread_sparce(0x400, len + 0x10).unwrap(), data);
        for i in 0..len {
            data.insert(0x400 + len + 0x10 + i, DATA[i as usize]);
        }
        for i in 0..len - 0x20 {
            data.insert(0x400 + (len + 0x10) * 2 + i, DATA[i as usize]);
        }
        assert_eq!(io.vread_sparce(0x400, len * 3).unwrap(), data);
    }
    #[test]
    fn test_vread_sparce() {
        operate_on_files(&vread_sparce_cb, &[DATA, DATA, DATA]);
    }

    fn phy_to_vir_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        let len = DATA.len() as u64;
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0x0)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x200)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x400)
            .unwrap();
        io.map(0, 0x4000, len).unwrap();
        io.map(0x200, 0x5000, len).unwrap();
        io.map(0x400, 0x2000, len).unwrap();
        io.map(0, 0x6000, len).unwrap();
        io.map(0, 0x7000, len).unwrap();
        io.map(0, 0x8000, len).unwrap();
        io.map(0, 0x9000, len).unwrap();
        io.map(0, 0x10000, len).unwrap();
        assert_eq!(
            io.phy_to_vir(0x45),
            vec![0x4045, 0x6045, 0x7045, 0x8045, 0x9045, 0x10045]
        );
        assert_eq!(io.phy_to_vir(0x245), vec![0x5045]);
        assert_eq!(io.phy_to_vir(700), Vec::<u64>::new());
    }
    #[test]

    fn test_phy_to_vir() {
        operate_on_files(&phy_to_vir_cb, &[DATA, DATA, DATA]);
    }
    fn hndl_to_desc_cb(path: &Path) {
        let mut io = RIO::new();
        let hndl = io
            .open_at(&path.to_string_lossy(), IoMode::READ, 0x0)
            .unwrap();
        assert!(io.hndl_to_desc(hndl + 1).is_none());
        let desc = io.hndl_to_desc(hndl).unwrap();
        assert_eq!(desc.size() as usize, DATA.len());
        assert_eq!(desc.paddr_base(), 0);
        assert_eq!(desc.name(), &path.to_string_lossy());
    }
    #[test]
    fn test_hndl_to_desc() {
        operate_on_file(&hndl_to_desc_cb, DATA);
    }
    fn serde_cb(paths: &[&Path]) {
        let mut io = RIO::new();
        io.open_at(&paths[0].to_string_lossy(), IoMode::READ, 0x1000)
            .unwrap();
        io.open_at(&paths[1].to_string_lossy(), IoMode::READ, 0x2000)
            .unwrap();
        io.open_at(&paths[2].to_string_lossy(), IoMode::READ, 0x3000)
            .unwrap();
        io.map(0x1000, 0x400, DATA.len() as u64).unwrap();
        io.map(0x2000, 0x400 + DATA.len() as u64, DATA.len() as u64)
            .unwrap();
        io.map(0x3000, 0x400 + DATA.len() as u64 * 2, DATA.len() as u64)
            .unwrap();
        let serialized = serde_json::to_string(&io).unwrap();
        drop(io);
        io = serde_json::from_str(&serialized).unwrap();
        let mut fillme: Vec<u8> = vec![0; 8];
        io.vread(0x400, &mut fillme).unwrap();
        assert_eq!(fillme, &DATA[0..8]);
    }
    #[test]
    fn test_serde() {
        operate_on_files(&serde_cb, &[DATA, DATA, DATA]);
    }
}
