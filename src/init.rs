/*
 * init.rs: Initializers for rair cli.
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
use lineformatter::LineFormatter;
use rcore::Core;
use rustyline::{CompletionType, Config, EditMode, Editor, OutputStreamType};
pub fn init_editor_from_core(core: &mut Core) -> Editor<LineFormatter> {
    let config = Config::builder()
        .completion_type(CompletionType::Circular)
        .edit_mode(EditMode::Emacs)
        .output_stream(OutputStreamType::Stdout)
        .build();
    let mut editor = Editor::with_config(config);
    editor.set_helper(Some(LineFormatter::new(core.commands())));
    return editor;
}
