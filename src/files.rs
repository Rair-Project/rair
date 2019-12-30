/*
 * files.rs: keep track of whatever files needs to be taken care of (config, hist ..etc).
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
use app_dirs::*;
use std::path::PathBuf;

const APPINFO: AppInfo = AppInfo { name: "rair", author: "RairDevs" };

pub fn hist_file() -> PathBuf {
    let mut history = app_dir(AppDataType::UserData, &APPINFO, "/").unwrap();
    history.push("history");
    return history;
}
