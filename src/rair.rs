#![warn(clippy::cargo)]
#![allow(clippy::multiple_crate_versions)]
#![allow(clippy::needless_return)]
/*
 * rair.rs: rair CLI.
 * Copyright (C) 2019  Oddcoder
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */
#[macro_use]
extern crate clap;
extern crate app_dirs;
extern crate rair_io;
extern crate rcmd;
extern crate rcore;
extern crate rtrees;
extern crate rustyline;
extern crate rustyline_derive;
extern crate yansi;

mod files;
mod init;
mod lineformatter;
mod rpel;

use clap::App;
use init::*;
use rair_io::IoMode;
use rcore::{panic_msg, str_to_num, Core};
use rpel::*;

fn main() {
    let yaml = load_yaml!("cli.yaml");
    let matches = App::from_yaml(yaml).version(crate_version!()).version_short("v").get_matches();

    let mut core = Core::new();
    let editor = init_editor_from_core(&mut core);

    let paddr = match matches.value_of("Paddr") {
        Some(addr) => Some(str_to_num(addr).unwrap_or_else(|e| panic_msg(&mut core, &e.to_string(), ""))),
        None => None,
    };
    let uri = matches.value_of("File").unwrap();
    let mut perm: IoMode = IoMode::READ;
    if let Some(p) = matches.value_of("Permission") {
        perm = Default::default();
        for c in p.to_lowercase().chars() {
            match c {
                'r' => perm |= IoMode::READ,
                'w' => perm |= IoMode::WRITE,
                'c' => perm |= IoMode::COW,
                _ => panic_msg(&mut core, &format!("Unknown Permission: `{}`", c), ""),
            }
        }
    }
    if let Some(paddr) = paddr {
        core.io.open_at(uri, perm, paddr).unwrap_or_else(|e| panic_msg(&mut core, &e.to_string(), ""));
        core.set_loc(paddr);
    } else {
        let hndl = core.io.open(uri, perm).unwrap_or_else(|e| panic_msg(&mut core, &e.to_string(), ""));
        core.set_loc(core.io.hndl_to_desc(hndl).unwrap().paddr_base());
    }
    prompt_read_parse_evaluate_loop(core, editor);
}
