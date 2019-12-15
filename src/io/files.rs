/*
 * files.rs: commands for opening, closing and listing files.
 * Copyright (C) 2019  Oddcoder
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Lesser General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

use core::*;
use helper::*;
use rio::*;
use std::io::Write;

#[derive(Default)]
pub struct ListFiles {}

impl ListFiles {
    pub fn new() -> Self {
        Default::default()
    }
}

impl Cmd for ListFiles {
    fn run(&mut self, core: &mut Core, args: &[String]) {
        if !args.is_empty() {
            expect(core, args.len() as u64, 0);
            return;
        }
        writeln!(core.stdout, "Handle\tStart address\tsize\t\tPermissions\tURI").unwrap();
        for file in core.io.uri_iter() {
            let perm = format!("{:?}", file.perm());
            write!(core.stdout, "{}\t0x{:08x}\t0x{:08x}\t{}", file.hndl(), file.paddr_base(), file.size(), perm).unwrap();
            if perm.len() < 6 {
                write!(core.stdout, "\t").unwrap();
            }
            writeln!(core.stdout, "\t{}", file.name()).unwrap();
        }
    }
    fn help(&self, core: &mut Core) {
        help(core, &"files", &"", vec![("", "List all open files.")]);
    }
}

#[derive(Default)]
pub struct OpenFile {}

impl OpenFile {
    pub fn new() -> Self {
        Default::default()
    }
}
fn parse_perm(p: &str) -> Result<IoMode, String> {
    let mut perm = Default::default();
    for c in p.to_lowercase().chars() {
        match c {
            'r' => perm |= IoMode::READ,
            'w' => perm |= IoMode::WRITE,
            'c' => perm |= IoMode::COW,
            _ => return Err(format!("Unknown Permission: `{}`", c)),
        }
    }
    return Ok(perm);
}
impl Cmd for OpenFile {
    fn run(&mut self, core: &mut Core, args: &[String]) {
        if args.len() > 3 || args.is_empty() {
            expect_range(core, args.len() as u64, 1, 2);
            return;
        }
        let uri;
        let mut perm = IoMode::READ;
        let mut addr = None;
        if args.len() == 3 {
            uri = &args[1];
            perm = match parse_perm(&args[0]) {
                Ok(perm) => perm,
                Err(e) => return error_msg(core, "Failed to parse permission", &e),
            };
            addr = match str_to_num(&args[2]) {
                Ok(addr) => Some(addr),
                Err(e) => {
                    let err_str = format!("{}", e);
                    error_msg(core, "Failed to parse address", &err_str);
                    return;
                }
            }
        } else if args.len() == 2 {
            if let Ok(a) = str_to_num(&args[1]) {
                addr = Some(a);
                uri = &args[0];
            } else {
                uri = &args[1];
                perm = match parse_perm(&args[0]) {
                    Ok(perm) => perm,
                    Err(e) => return error_msg(core, "Failed to parse permission", &e),
                };
            }
        } else {
            uri = &args[0];
        }

        let result = match addr {
            Some(addr) => core.io.open_at(uri, perm, addr),
            None => core.io.open(uri, perm),
        };
        if let Err(e) = result {
            let err_str = format!("{}", e);
            error_msg(core, "Failed to open file", &err_str);
        }
    }
    fn help(&self, core: &mut Core) {
        help(
            core,
            &"open",
            &"o",
            vec![("<Perm> [URI] <Addr>", "Open given URI using given optional permission (default to readonly) at given optional address.")],
        );
    }
}

#[derive(Default)]
pub struct CloseFile {}

impl CloseFile {
    pub fn new() -> Self {
        Default::default()
    }
}

impl Cmd for CloseFile {
    fn run(&mut self, core: &mut Core, args: &[String]) {
        if args.len() != 1 {
            expect(core, args.len() as u64, 1);
            return;
        }
        let hndl = match str_to_num(&args[0]) {
            Ok(hndl) => hndl,
            Err(e) => {
                let err_str = format!("{}", e);
                error_msg(core, "Invalid hndl", &err_str);
                return;
            }
        };
        if let Err(e) = core.io.close(hndl) {
            let err_str = format!("{}", e);
            error_msg(core, "Failed to close file", &err_str);
        }
    }
    fn help(&self, core: &mut Core) {
        help(core, &"close", &"", vec![("[hndl]", "Close file with given hndl.")]);
    }
}

#[cfg(test)]
mod test_files {
    use super::*;
    use writer::Writer;
    use yansi::Paint;
    #[test]
    fn test_docs() {
        Paint::disable();
        let mut core = Core::new();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let files = ListFiles::new();
        let open = OpenFile::new();
        let close = CloseFile::new();
        files.help(&mut core);
        open.help(&mut core);
        close.help(&mut core);
        assert_eq!(
            core.stdout.utf8_string().unwrap(),
            "Command: [files]\n\n\
             Usage:\n\
             files\tList all open files.\n\
             Commands: [open | o]\n\n\
             Usage:\n\
             o <Perm> [URI] <Addr>\tOpen given URI using given optional permission (default to readonly) at given optional address.\n\
             Command: [close]\n\n\
             Usage:\n\
             close [hndl]\tClose file with given hndl.\n"
        );
        assert_eq!(core.stderr.utf8_string().unwrap(), "");
    }

    #[test]
    fn test_open_close_files() {
        Paint::disable();
        let mut core = Core::new();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mut files = ListFiles::new();
        let mut open = OpenFile::new();
        let mut close = CloseFile::new();
        open.run(&mut core, &["b64://../testing_binaries/rio/base64/no_padding.b64".to_string()]);
        open.run(&mut core, &["rw".to_string(), "malloc://0x50".to_string()]);
        open.run(&mut core, &["c".to_string(), "../testing_binaries/rio/base64/one_pad.b64".to_string(), "0x5000".to_string()]);
        open.run(&mut core, &["b64://../testing_binaries/rio/base64/no_padding.b64".to_string(), "0xa000".to_string()]);

        files.run(&mut core, &[]);
        assert_eq!(
            core.stdout.utf8_string().unwrap(),
            "Handle\tStart address\tsize\t\tPermissions\tURI\n\
             0\t0x00000000\t0x0000002d\tREAD\t\tb64://../testing_binaries/rio/base64/no_padding.b64\n\
             1\t0x0000002d\t0x00000050\tWRITE | READ\tmalloc://0x50\n\
             2\t0x00005000\t0x00000005\tCOW\t\t../testing_binaries/rio/base64/one_pad.b64\n\
             3\t0x0000a000\t0x0000002d\tREAD\t\tb64://../testing_binaries/rio/base64/no_padding.b64\n"
        );
        assert_eq!(core.stderr.utf8_string().unwrap(), "");
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        close.run(&mut core, &["1".to_string()]);
        files.run(&mut core, &[]);
        assert_eq!(
            core.stdout.utf8_string().unwrap(),
            "Handle\tStart address\tsize\t\tPermissions\tURI\n\
             0\t0x00000000\t0x0000002d\tREAD\t\tb64://../testing_binaries/rio/base64/no_padding.b64\n\
             2\t0x00005000\t0x00000005\tCOW\t\t../testing_binaries/rio/base64/one_pad.b64\n\
             3\t0x0000a000\t0x0000002d\tREAD\t\tb64://../testing_binaries/rio/base64/no_padding.b64\n"
        );
        assert_eq!(core.stderr.utf8_string().unwrap(), "");
    }

    #[test]
    fn test_failing_parsing() {
        Paint::disable();
        let mut core = Core::new();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mut open = OpenFile::new();
        open.run(&mut core, &["z".to_string(), "malloc://0x50".to_string()]);
        open.run(&mut core, &["z".to_string(), "malloc://0x50".to_string(), "0x500".to_string()]);
        open.run(&mut core, &["rw".to_string(), "malloc://0x50".to_string(), "0b500".to_string()]);

        assert_eq!(core.stdout.utf8_string().unwrap(), "");
        assert_eq!(
            core.stderr.utf8_string().unwrap(),
            "Error: Failed to parse permission\n\
             Unknown Permission: `z`\n\
             Error: Failed to parse permission\n\
             Unknown Permission: `z`\n\
             Error: Failed to parse address\n\
             invalid digit found in string\n"
        );
    }

    #[test]
    fn test_arguments_count() {
        Paint::disable();
        let mut core = Core::new();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mut files = ListFiles::new();
        let mut open = OpenFile::new();
        let mut close = CloseFile::new();
        open.run(&mut core, &[]);
        files.run(&mut core, &["test".to_string()]);
        close.run(&mut core, &[]);
        assert_eq!(core.stdout.utf8_string().unwrap(), "");
        assert_eq!(
            core.stderr.utf8_string().unwrap(),
            "Arguments Error: Expected between 1 and 2 arguments, found 0.\n\
             Arguments Error: Expected 0 argument(s), found 1.\n\
             Arguments Error: Expected 1 argument(s), found 0.\n"
        );
    }

    #[test]
    fn test_failed_open_close() {
        Paint::disable();
        let mut core = Core::new();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mut open = OpenFile::new();
        let mut close = CloseFile::new();
        open.run(&mut core, &["file_that_doesnt_exist".to_string()]);
        close.run(&mut core, &["5".to_string()]);
        assert_eq!(core.stdout.utf8_string().unwrap(), "");
        let err = core.stderr.utf8_string().unwrap();
        assert!(err.starts_with("Error: Failed to open file\n"));
        // what in between is different between Windows and *Nix
        assert!(err.ends_with("Error: Failed to close file\nHandle Does not exist.\n"));
    }
}
