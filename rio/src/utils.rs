/*
 * utils.rs: Utility data structures for managing RIO.
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
use std::fmt;
use std::io;

bitflags! {
    #[derive(Default)]
    pub struct IoMode: u64 {
    const WRITE = 2;
    const READ = 4;
    const COW = 8;
    }
}

#[derive(Debug)]
pub enum IoError {
    AddressNotFound,
    AddressesOverlapError,
    IoPluginNotFoundError,
    HndlNotFoundError,
    TooManyFilesError,
    Parse(io::Error),
}
impl PartialEq for IoError {
    fn eq(&self, rhs: &IoError) -> bool {
        match self {
            IoError::AddressNotFound => {
                if let IoError::AddressNotFound = rhs {
                    return true;
                }
            }
            IoError::AddressesOverlapError => {
                if let IoError::AddressesOverlapError = rhs {
                    return true;
                }
            }
            IoError::IoPluginNotFoundError => {
                if let IoError::IoPluginNotFoundError = rhs {
                    return true;
                }
            }
            IoError::TooManyFilesError => {
                if let IoError::TooManyFilesError = rhs {
                    return true;
                }
            }
            IoError::HndlNotFoundError => {
                if let IoError::HndlNotFoundError = rhs {
                    return true;
                }
            }
            IoError::Parse(_) => {
                if let IoError::Parse(_) = rhs {
                    return true;
                }
            }
        }
        return false;
    }
}
impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            IoError::AddressNotFound => write!(f, "Cannot resolve address."),
            IoError::AddressesOverlapError => write!(f, "Phyiscal addresses overlap"),
            IoError::IoPluginNotFoundError => write!(f, "Can not find Suitable IO Plugin"),
            IoError::TooManyFilesError => write!(f, "You have too many open files."),
            IoError::HndlNotFoundError => write!(f, "Handle Does not exist"),
            IoError::Parse(ref e) => e.fmt(f),
        }
    }
}

impl From<io::Error> for IoError {
    fn from(err: io::Error) -> IoError {
        IoError::Parse(err)
    }
}
