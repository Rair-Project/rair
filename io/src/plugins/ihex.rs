//! RIO plugin that opens intel hex files.

use super::defaultplugin;
use super::dummy::Dummy;
use crate::plugin::{RIOPlugin, RIOPluginDesc, RIOPluginMetadata, RIOPluginOperations};
use crate::utils::{IoError, IoMode};
use alloc::collections::BTreeMap;
use core::num::ParseIntError;
use core::{fmt::Write as _, str};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while_m_n},
    {combinator::map_res, sequence::tuple, IResult},
};
use std::{
    fs::{File, OpenOptions},
    io,
    io::Write as _,
    path::Path,
};
const METADATA: RIOPluginMetadata = RIOPluginMetadata {
    name: "IHex",
    desc: "This IO plugin is used to open Intel IHex files,\
           this plugin would fill sparce intel ihex files with\
           zeros when doing read operation but in case of writes,\
           unfilled bytes will remain unfilled",
    author: "Oddcoder",
    license: "LGPL",
    version: "0.0.1",
};

struct FileInternals {
    file: Box<dyn RIOPluginOperations + Sync + Send>, // defaultplugin
    uri: String,
    bytes: BTreeMap<u64, u8>, // sparce array of bytes
    prot: IoMode,
    ssa: Option<u32>, // used for Record 03
    sla: Option<u32>, // used for Record 05
}

fn parse_newline(buffer: &[u8]) -> IResult<&[u8], &[u8]> {
    alt((tag("\r\n"), tag("\n"), tag("\r")))(buffer)
}

enum Record {
    Data(u64, Vec<u8>), // Record 00 (base address, bytes)
    Eof,                // Record 01
    Ea(u64),            // Extended Address: Record 02, Record 04
    Ssa(u32),           //Record 03
    Sla(u32),           // record 05
}
fn from_hex(input: &[u8]) -> Result<u8, ParseIntError> {
    u8::from_str_radix(str::from_utf8(input).unwrap(), 16)
}

fn is_hex_digit(c: u8) -> bool {
    (c as char).is_ascii_hexdigit()
}

fn hex_byte(input: &[u8]) -> IResult<&[u8], u8> {
    map_res(take_while_m_n(2, 2, is_hex_digit), from_hex)(input)
}

fn hex_big_word(input: &[u8]) -> IResult<&[u8], u16> {
    let (input, (byte1, byte2)) = tuple((hex_byte, hex_byte))(input)?;
    let result = ((byte1 as u16) << 8i32) + byte2 as u16;
    Ok((input, result))
}
fn hex_big_dword(input: &[u8]) -> IResult<&[u8], u32> {
    let (input, (word1, word2)) = tuple((hex_big_word, hex_big_word))(input)?;
    let result = ((word1 as u32) << 16i32) + word2 as u32;
    Ok((input, result))
}
fn parse_record00(input: &[u8]) -> IResult<&[u8], Record> {
    // Data record
    let (input, _) = tag(":")(input)?;
    let (input, size) = hex_byte(input)?;
    let (input, addr) = hex_big_word(input)?;
    let (mut input, _) = tag("00")(input)?;
    let mut data = Vec::with_capacity(size as usize);
    for _ in 0..size {
        let x = hex_byte(input)?;
        input = x.0;
        data.push(x.1);
    }
    let (input, _) = hex_byte(input)?; //checksum
    let (input, _) = parse_newline(input)?; //newline
    Ok((input, Record::Data(addr as u64, data)))
}

fn parse_record01(input: &[u8]) -> IResult<&[u8], Record> {
    // EOF Record
    let (input, _) = tag(":00")(input)?; // size entry
    let (input, _) = hex_big_word(input)?; // addr entry
    let (input, _) = tag("01")(input)?; // record ID
    let (input, _) = hex_byte(input)?; // checksum
    Ok((input, Record::Eof))
}
fn parse_record02(input: &[u8]) -> IResult<&[u8], Record> {
    // Extended Segment Address Record
    let (input, _) = tag(":02")(input)?; // size entry
    let (input, _) = hex_big_word(input)?; // addr entry
    let (input, _) = tag("02")(input)?; // record ID
    let (input, addr) = hex_big_word(input)?; // data
    let (input, _) = hex_byte(input)?; // checksum
    let (input, _) = parse_newline(input)?; //newline
    Ok((input, Record::Ea((addr as u64) << 4)))
}

fn parse_record03(input: &[u8]) -> IResult<&[u8], Record> {
    // Start Segment Address Record
    let (input, _) = tag(":04")(input)?; // size entry
    let (input, _) = hex_big_word(input)?; // addr entry
    let (input, _) = tag("03")(input)?; // record ID
    let (input, addr) = hex_big_dword(input)?; // data
    let (input, _) = hex_byte(input)?; // checksum
    let (input, _) = parse_newline(input)?; //newline
    Ok((input, Record::Ssa(addr)))
}

fn parse_record04(input: &[u8]) -> IResult<&[u8], Record> {
    // Extended Segment Address Record
    let (input, _) = tag(":02")(input)?; // size entry
    let (input, _) = hex_big_word(input)?; // addr entry
    let (input, _) = tag("04")(input)?; // record ID
    let (input, addr) = hex_big_word(input)?; // data
    let (input, _) = hex_byte(input)?; // checksum
    let (input, _) = parse_newline(input)?; //newline
    Ok((input, Record::Ea((addr as u64) << 16)))
}

fn parse_record05(input: &[u8]) -> IResult<&[u8], Record> {
    // Start Linear Address Record
    let (input, _) = tag(":04")(input)?; // size entry
    let (input, _) = hex_big_word(input)?; // addr entry
    let (input, _) = tag("05")(input)?; // record ID
    let (input, addr) = hex_big_dword(input)?; // data
    let (input, _) = hex_byte(input)?; // checksum
    let (input, _) = parse_newline(input)?; //newline
    Ok((input, Record::Sla(addr)))
}

impl FileInternals {
    fn parse_record(input: &[u8]) -> IResult<&[u8], Record> {
        alt((
            parse_record00,
            parse_record01,
            parse_record02,
            parse_record03,
            parse_record04,
            parse_record05,
        ))(input)
    }
    fn parse_ihex(&mut self, mut input: &[u8]) -> Result<(), IoError> {
        let mut base = 0u64;
        let mut line = 1i32;
        loop {
            let Ok(x) = Self::parse_record(input) else {
                return Err(IoError::Custom(format!(
                    "Invalid Ihex entry at line: {line}"
                )));
            };
            input = x.0;
            match x.1 {
                Record::Eof => break,
                Record::Data(addr, data) => {
                    for i in 0..data.len() as u64 {
                        self.bytes.insert(i + addr + base, data[i as usize]);
                    }
                }
                Record::Ea(addr) => base = addr,
                Record::Ssa(addr) => self.ssa = Some(addr),
                Record::Sla(addr) => self.sla = Some(addr),
            }
            line += 1i32;
        }
        Ok(())
    }
    fn write_sa(&self, file: &mut File) -> Result<(), IoError> {
        if let Some(ssa) = self.ssa {
            let mut checksum: u16 = 4 + 3;
            for byte in &ssa.to_be_bytes() {
                checksum = (checksum + *byte as u16) & 0xFF;
            }
            checksum = 256 - checksum;
            writeln!(file, ":04000003{ssa:08x}{checksum:02x}")?;
        }
        if let Some(sla) = self.sla {
            let mut checksum: u16 = 4 + 5;
            for byte in &sla.to_be_bytes() {
                checksum = (checksum + *byte as u16) & 0xFF;
            }
            checksum = 256 - checksum;
            writeln!(file, ":04000005{sla:08x}{checksum:02x}")?;
        }
        Ok(())
    }
    fn write_record04(file: &mut File, addr: u64) -> Result<(), IoError> {
        let addr = (addr >> 16i32) as u16;
        let mut checksum = 6;
        for byte in &addr.to_be_bytes() {
            checksum = (checksum + *byte as u16) & 0xFF;
        }
        checksum = 256 - checksum;
        writeln!(file, ":02000004{addr:04x}{checksum:02x}")?;
        Ok(())
    }

    fn write_record02(file: &mut File, addr: u64) -> Result<(), IoError> {
        let addr = (addr >> 4i32) as u16 & 0xf000;
        let mut checksum = 4;
        for byte in &addr.to_be_bytes() {
            checksum = (checksum + *byte as u16) & 0xFF;
        }
        checksum = 256 - checksum;
        writeln!(file, ":02000002{addr:04x}{checksum:02x}")?;
        Ok(())
    }

    fn write_data(&self, file: &mut File) -> Result<(), IoError> {
        let mut checksum: u16 = 0x10;
        let mut addr = self.base();
        let mut data = String::new();
        let mut i = 0i32;
        for (k, v) in &self.bytes {
            if i != 0i32 {
                if i == 0x10i32 || *k != addr + 1 {
                    writeln!(file, ":{:02x}{}{:02x}", i, data, (256 - checksum) & 0xff)?;
                    data.clear();
                    checksum = 0x10;
                    i = 0i32;
                } else {
                    // we know that *k == addr + 1
                    addr = *k;
                    write!(data, "{:02x}", *v).unwrap();
                    checksum = (checksum + *v as u16) & 0xff;
                }
            }
            if i == 0i32 {
                if *k > 0xfffff {
                    // record 04
                    Self::write_record04(file, *k)?;
                } else if *k > 0xffff {
                    // record 02
                    Self::write_record02(file, *k)?;
                }
                let offset = (*k & 0xffff) as u16;
                for byte in &offset.to_be_bytes() {
                    checksum = (checksum + *byte as u16) & 0xff;
                }
                addr = *k;
                write!(data, "{:04x}00{:02x}", offset, *v).unwrap();
                checksum = (checksum + *v as u16) & 0xff;
            }
            i += 1i32;
        }
        if !data.is_empty() {
            writeln!(file, ":{:02x}{}{:02x}", i, data, 256 - checksum)?;
        }
        Ok(())
    }
    fn save_ihex(&self) -> Result<(), IoError> {
        // truncate the current file.
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(IHexPlugin::uri_to_path(&self.uri))?;
        //write ssa and sla
        self.write_sa(&mut file)?;
        //write data
        self.write_data(&mut file)?;
        // write EOF
        writeln!(file, ":00000001FF")?;
        Ok(())
    }
    fn size(&self) -> u64 {
        let Some((min, _)) = self.bytes.iter().next() else {
            return 0;
        };
        let Some((max, _)) = self.bytes.iter().next_back() else {
            return 0;
        };
        max - min + 1
    }
    fn base(&self) -> u64 {
        if let Some((k, _)) = self.bytes.iter().next() {
            *k
        } else {
            0
        }
    }
}

impl RIOPluginOperations for FileInternals {
    fn read(&mut self, raddr: usize, buffer: &mut [u8]) -> Result<(), IoError> {
        for (i, item) in buffer.iter_mut().enumerate() {
            let addr = (i + raddr) as u64;
            if let Some(v) = self.bytes.get(&addr) {
                *item = *v;
            } else {
                *item = 0;
            }
        }
        Ok(())
    }

    fn write(&mut self, raddr: usize, buffer: &[u8]) -> Result<(), IoError> {
        // if we are dealing with cow or write firs write data to the sparce array
        if !self.prot.contains(IoMode::COW) && !self.prot.contains(IoMode::WRITE) {
            return Err(IoError::Parse(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "File Not Writable",
            )));
        }
        for (i, item) in buffer.iter().enumerate() {
            self.bytes.insert((i + raddr) as u64, *item);
        }

        if self.prot.contains(IoMode::WRITE) {
            // drop old file descriptor
            self.file = Box::new(Dummy {});
            // write data to new file with old file name
            self.save_ihex()?;
            // mmap new file
            let mut plug = defaultplugin::plugin();
            let def_desc = plug.open(
                &IHexPlugin::uri_to_path(&self.uri).to_string_lossy(),
                IoMode::READ,
            )?;
            self.file = def_desc.plugin_operations;
        }
        Ok(())
    }
}

struct IHexPlugin {
    defaultplugin: Box<dyn RIOPlugin + Sync + Send>, // defaultplugin
}

impl IHexPlugin {
    fn uri_to_path(uri: &str) -> &Path {
        let path = uri.trim_start_matches("ihex://");
        Path::new(path)
    }
    fn new() -> IHexPlugin {
        IHexPlugin {
            defaultplugin: defaultplugin::plugin(),
        }
    }
}

impl RIOPlugin for IHexPlugin {
    fn get_metadata(&self) -> &'static RIOPluginMetadata {
        &METADATA
    }

    fn open(&mut self, uri: &str, flags: IoMode) -> Result<RIOPluginDesc, IoError> {
        if !self.accept_uri(uri) {
            return Err(IoError::Custom(format!("Invalid uri {uri}")));
        }
        let def_desc = self.defaultplugin.open(
            &IHexPlugin::uri_to_path(uri).to_string_lossy(),
            IoMode::READ,
        )?;
        let mut internal = FileInternals {
            file: def_desc.plugin_operations,
            bytes: BTreeMap::new(),
            ssa: None,
            sla: None,
            prot: flags,
            uri: uri.to_owned(),
        };
        let mut data = vec![0; def_desc.size as usize];
        internal.file.read(0x0, &mut data)?;
        internal.parse_ihex(&data)?;
        let desc = RIOPluginDesc {
            name: uri.to_owned(),
            perm: flags,
            raddr: internal.base(),
            size: internal.size(),
            plugin_operations: Box::new(internal),
        };
        Ok(desc)
    }

    fn accept_uri(&self, uri: &str) -> bool {
        let split: Vec<&str> = uri.split("://").collect();
        split.len() == 2 && split[0] == "ihex"
    }
}

pub fn plugin() -> Box<dyn RIOPlugin + Sync + Send> {
    Box::new(IHexPlugin::new())
}

#[cfg(test)]
mod test_ihex {
    use super::*;
    use test_file::*;

    #[test]
    fn test_accept_uri() {
        let p = plugin();
        assert!(p.accept_uri("ihex:///bin/ls"));
        assert!(!p.accept_uri("ihx:///bin/ls"));
        assert!(!p.accept_uri("/bin/ls"));
    }

    #[test]
    fn test_tiny_ihex_read() {
        // this is simple ihex file testing,
        // no sparce file with holes, no nothing but basic record 00 and record 01
        let mut p = plugin();
        let mut file = p
            .open("ihex://../testing_binaries/rio/ihex/tiny.hex", IoMode::READ)
            .unwrap();
        assert_eq!(file.size, 11);
        let mut buffer = vec![0; file.size as usize];
        file.plugin_operations.read(0x0, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [0x02, 0x00, 0x00, 0x02, 0x00, 0x09, 0x02, 0x00, 0x03, 0x80, 0xfe]
        );
    }
    fn tiny_ihex_write_cb(path: &Path) {
        let mut p = plugin();
        let mut uri = "ihex://".to_owned();
        uri.push_str(&path.to_string_lossy());
        let mut file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();

        file.plugin_operations
            .write(0x5, &[0x80, 0x90, 0xff])
            .unwrap();
        drop(file);
        file = p.open(&uri, IoMode::READ).unwrap();
        assert_eq!(file.size, 11);
        let mut buffer = vec![0; file.size as usize];
        file.plugin_operations.read(0x0, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [0x02, 0x00, 0x00, 0x02, 0x00, 0x80, 0x90, 0xff, 0x03, 0x80, 0xfe]
        );
    }
    #[test]
    fn test_tiny_ihex_write() {
        // this is simple ihex file testing,
        // no sparce file with holes, no nothing but basic record 00 and record 01
        operate_on_copy(&tiny_ihex_write_cb, "../testing_binaries/rio/ihex/tiny.hex");
    }
    #[test]
    fn test_tiny_sparce_ihex_read() {
        //sparce file with holes, no nothing but basic record 00 and record 01
        let mut p = plugin();
        let mut file = p
            .open(
                "ihex://../testing_binaries/rio/ihex/tiny_sparce.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, 0x20);
        let mut buffer = vec![0; file.size as usize];
        file.plugin_operations.read(0x50, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [
                0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x24, 0x68, 0xac, 0xef,
                0xaa, 0xbb, 0xee, 0xff
            ]
        );
    }
    fn tiny_sparce_ihex_write_cb(path: &Path) {
        let mut p = plugin();
        let mut uri = "ihex://".to_owned();
        uri.push_str(&path.to_string_lossy());
        let mut file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();

        file.plugin_operations
            .write(0x55, &[0x80, 0x90, 0xff])
            .unwrap();
        drop(file);
        file = p.open(&uri, IoMode::READ).unwrap();
        assert_eq!(file.size, 0x20);
        let mut buffer = vec![0; file.size as usize];
        file.plugin_operations.read(0x50, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [
                0x02, 0x00, 0x00, 0x00, 0x00, 0x80, 0x90, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x24, 0x68, 0xac, 0xef,
                0xaa, 0xbb, 0xee, 0xff
            ]
        );
    }
    #[test]
    fn test_tiny_sparce_ihex_write() {
        // this is simple ihex file testing,
        // no sparce file with holes, no nothing but basic record 00 and record 01
        operate_on_copy(
            &tiny_sparce_ihex_write_cb,
            "../testing_binaries/rio/ihex/tiny_sparce.hex",
        );
    }

    #[test]
    fn test_big_read() {
        //test reading from huge file with record 00 and record 01
        let mut p = plugin();
        let mut file = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_00_01.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, 0xead);
        let mut buffer = [0; 4];
        file.plugin_operations.read(0x520, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x40, 0x06, 0x7c]);
        file.plugin_operations.read(0xe87, &mut buffer).unwrap();
        assert_eq!(buffer, [0xd3, 0x22, 0x32, 0x32]);
        file.plugin_operations.read(0xea9, &mut buffer).unwrap();
        assert_eq!(buffer, [0x32, 0x32, 0x32, 0x32]);
    }

    fn big_write_cb(path: &Path) {
        let mut p = plugin();
        let mut uri = "ihex://".to_owned();
        uri.push_str(&path.to_string_lossy());
        let mut file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();

        file.plugin_operations
            .write(0x520, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();
        file.plugin_operations
            .write(0xe87, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();
        file.plugin_operations
            .write(0xea9, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();

        drop(file);
        file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();
        assert_eq!(file.size, 0xead);
        let mut buffer = [0; 4];
        file.plugin_operations.read(0x520, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);
        file.plugin_operations.read(0xe87, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);
        file.plugin_operations.read(0xea9, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);

        file.plugin_operations
            .write(0x520, &[0x80, 0x40, 0x06, 0x7c])
            .unwrap();
        file.plugin_operations
            .write(0xe87, &[0xd3, 0x22, 0x32, 0x32])
            .unwrap();
        file.plugin_operations
            .write(0xea9, &[0x32, 0x32, 0x32, 0x32])
            .unwrap();
        drop(file);
        file = p.open(&uri, IoMode::READ).unwrap();
        let mut file2 = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_00_01.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, file2.size);
        let mut data = vec![0; file.size as usize];
        let mut data2 = vec![0; file.size as usize];
        file.plugin_operations.read(0, &mut data).unwrap();
        file2.plugin_operations.read(0, &mut data2).unwrap();
        assert_eq!(data, data2);
    }

    #[test]
    fn test_big_write() {
        //test writing to huge file with record 00 and record 01
        operate_on_copy(
            &big_write_cb,
            "../testing_binaries/rio/ihex/record_00_01.hex",
        );
    }

    #[test]
    fn test_read_02_03() {
        let mut p = plugin();
        let mut file = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_02_03.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, 0x5a1ec);
        let mut buffer = [0; 0x20];
        file.plugin_operations.read(0x2ce34, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [
                0x54, 0x68, 0x69, 0x73, 0x20, 0x70, 0x61, 0x72, 0x74, 0x20, 0x69, 0x73, 0x20, 0x69,
                0x6e, 0x20, 0x61, 0x20, 0x6c, 0x6f, 0x77, 0x20, 0x73, 0x65, 0x67, 0x6d, 0x65, 0x6e,
                0x74, 0x00, 0x00, 0x00,
            ]
        );
        file.plugin_operations.read(0x87000, &mut buffer).unwrap();
        assert_eq!(
            buffer,
            [
                0x54, 0x68, 0x69, 0x73, 0x20, 0x70, 0x61, 0x72, 0x74, 0x20, 0x69, 0x73, 0x20, 0x69,
                0x6e, 0x20, 0x74, 0x68, 0x65, 0x20, 0x68, 0x69, 0x67, 0x68, 0x20, 0x73, 0x65, 0x67,
                0x6d, 0x65, 0x6e, 0x74,
            ]
        );
    }

    fn write_02_03_cb(path: &Path) {
        let mut p = plugin();
        let mut uri = "ihex://".to_owned();
        uri.push_str(&path.to_string_lossy());
        let mut file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();

        file.plugin_operations
            .write(0x2ce34, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();
        file.plugin_operations
            .write(0x2ce3c, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();
        file.plugin_operations
            .write(0x87000, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();

        drop(file);
        file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();
        assert_eq!(file.size, 0x5a1ec);
        let mut buffer = [0; 4];
        file.plugin_operations.read(0x2ce34, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);
        file.plugin_operations.read(0x2ce3c, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);
        file.plugin_operations.read(0x2ce3c, &mut buffer).unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);

        file.plugin_operations
            .write(0x2ce34, &[0x54, 0x68, 0x69, 0x73])
            .unwrap();
        file.plugin_operations
            .write(0x2ce3c, &[0x74, 0x20, 0x69, 0x73])
            .unwrap();
        file.plugin_operations
            .write(0x87000, &[0x54, 0x68, 0x69, 0x73])
            .unwrap();
        drop(file);
        file = p.open(&uri, IoMode::READ).unwrap();
        let mut file2 = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_02_03.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, file2.size);
        let mut data = vec![0; file.size as usize];
        let mut data2 = vec![0; file.size as usize];
        file.plugin_operations.read(0, &mut data).unwrap();
        file2.plugin_operations.read(0, &mut data2).unwrap();
        assert_eq!(data, data2);
    }

    #[test]
    fn test_write_02_03() {
        operate_on_copy(
            &write_02_03_cb,
            "../testing_binaries/rio/ihex/record_02_03.hex",
        );
    }

    #[test]
    fn test_read_04_05() {
        let mut p = plugin();
        let mut file = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_04_05.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, 0xEF60);
        let mut buffer = [0; 4];
        file.plugin_operations
            .read(0x123400C1, &mut buffer)
            .unwrap();
        assert_eq!(buffer, [0x48, 0x85, 0x46, 0x0C]);
    }

    fn write_04_05_cb(path: &Path) {
        let mut p = plugin();
        let mut uri = "ihex://".to_owned();
        uri.push_str(&path.to_string_lossy());
        let mut file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();

        file.plugin_operations
            .write(0x123400C1, &[0x80, 0x90, 0xff, 0xfe])
            .unwrap();

        drop(file);
        file = p.open(&uri, IoMode::READ | IoMode::WRITE).unwrap();
        assert_eq!(file.size, 0xEF60);
        let mut buffer = [0; 4];
        file.plugin_operations
            .read(0x123400C1, &mut buffer)
            .unwrap();
        assert_eq!(buffer, [0x80, 0x90, 0xff, 0xfe]);
        file.plugin_operations
            .write(0x123400C1, &[0x48, 0x85, 0x46, 0x0C])
            .unwrap();
        drop(file);
        file = p.open(&uri, IoMode::READ).unwrap();
        let mut file2 = p
            .open(
                "ihex://../testing_binaries/rio/ihex/record_04_05.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(file.size, file2.size);
        let mut data = vec![0; file.size as usize];
        let mut data2 = vec![0; file.size as usize];
        file.plugin_operations.read(0, &mut data).unwrap();
        file2.plugin_operations.read(0, &mut data2).unwrap();
        assert_eq!(data, data2);
    }

    #[test]
    fn test_write_04_05() {
        operate_on_copy(
            &write_04_05_cb,
            "../testing_binaries/rio/ihex/record_04_05.hex",
        );
    }

    #[test]
    fn test_broken() {
        let mut p = plugin();
        let err = p
            .open(
                "ihex://../testing_binaries/rio/ihex/broken.hex",
                IoMode::READ,
            )
            .err()
            .unwrap();
        assert_eq!(
            err,
            IoError::Custom("Invalid Ihex entry at line: 4".to_owned())
        );
    }
    #[test]
    fn test_empty() {
        let mut p = plugin();
        let f = p
            .open(
                "ihex://../testing_binaries/rio/ihex/empty.hex",
                IoMode::READ,
            )
            .unwrap();
        assert_eq!(f.size, 0);
    }
}
