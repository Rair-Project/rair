//! commands handling view mode (phy/vir).

use super::history::History;
use crate::core::Core;
use crate::helper::{error_msg, expect, AddrMode, MRc};
use crate::Cmd;
use yansi::Paint;
#[derive(Default)]
pub struct Mode {
    history: MRc<History>,
}

impl Mode {
    pub(super) fn with_history(history: MRc<History>) -> Self {
        Mode { history }
    }
}

impl Cmd for Mode {
    fn run(&mut self, core: &mut Core, args: &[String]) {
        if args.len() != 1 {
            expect(core, args.len() as u64, 1);
            return;
        }
        if args[0] == "vir" {
            self.history.lock().add(core);
            if core.mode == AddrMode::Phy {
                let vir = core.io.phy_to_vir(core.get_loc());
                if !vir.is_empty() {
                    core.set_loc(vir[0]);
                }
            }
            core.mode = AddrMode::Vir;
        } else if args[0] == "phy" {
            self.history.lock().add(core);
            if core.mode == AddrMode::Vir {
                if let Some(vir) = core.io.vir_to_phy(core.get_loc(), 1) {
                    core.set_loc(vir[0].paddr);
                }
            }
            core.mode = AddrMode::Phy;
        } else {
            let msg = format!(
                "Expected {} or {}, but found {}.",
                "vir".primary().italic().bold(),
                "phy".primary().italic().bold(),
                &args[0].primary().italic().bold(),
            );
            error_msg(core, "Invalid Mode", &msg);
        }
    }
    fn commands(&self) -> &'static [&'static str] {
        &["mode", "m"]
    }

    fn help_messages(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("vir", "Set view mode to virtual address space."),
            ("phy", "Set view mode to physical address space."),
        ]
    }
}

#[cfg(test)]
mod test_mode {
    use super::*;
    use crate::{writer::Writer, CmdOps};
    use rair_io::*;
    use std::path::Path;
    use test_file::*;

    #[test]
    fn test_docs() {
        let mut core = Core::new_no_colors();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mode = Mode::default();
        mode.help(&mut core);
        assert_eq!(
            core.stdout.utf8_string().unwrap(),
            "Commands: [mode | m]\n\
             Usage:\n\
             m vir\tSet view mode to virtual address space.\n\
             m phy\tSet view mode to physical address space.\n\
             "
        );
        assert_eq!(core.stderr.utf8_string().unwrap(), "");
    }

    fn test_mode_cb(path: &Path) {
        let mut core = Core::new_no_colors();
        let len = DATA.len() as u64;
        let mut mode = Mode::default();
        core.io.open(&path.to_string_lossy(), IoMode::READ).unwrap();
        core.io.map(0x0, 0x5000, len).unwrap();
        assert_eq!(core.get_loc(), 0x0);
        mode.run(&mut core, &["vir".to_owned()]);
        assert_eq!(core.get_loc(), 0x5000);
        assert_eq!(core.mode, AddrMode::Vir);
        core.set_loc(0x5001);
        mode.run(&mut core, &["phy".to_owned()]);
        assert_eq!(core.get_loc(), 1);
        assert_eq!(core.mode, AddrMode::Phy);
        core.set_loc(len + 10);
        mode.run(&mut core, &["vir".to_owned()]);
        assert_eq!(core.get_loc(), len + 10);
        assert_eq!(core.mode, AddrMode::Vir);
        mode.run(&mut core, &["phy".to_owned()]);
        assert_eq!(core.get_loc(), len + 10);
        assert_eq!(core.mode, AddrMode::Phy);
    }
    #[test]
    fn test_mode() {
        operate_on_file(&test_mode_cb, DATA);
    }

    #[test]
    fn test_mode_errors() {
        let mut core = Core::new_no_colors();
        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        let mut mode = Mode::default();
        mode.run(&mut core, &[]);
        assert_eq!(core.stdout.utf8_string().unwrap(), "");
        assert_eq!(
            core.stderr.utf8_string().unwrap(),
            "Arguments Error: Expected 1 argument(s), found 0.\n"
        );

        core.stderr = Writer::new_buf();
        core.stdout = Writer::new_buf();
        mode.run(&mut core, &["not_real_arg".to_owned()]);
        assert_eq!(core.stdout.utf8_string().unwrap(), "");
        assert_eq!(
            core.stderr.utf8_string().unwrap(),
            "Error: Invalid Mode\nExpected vir or phy, but found not_real_arg.\n"
        );
    }
}
