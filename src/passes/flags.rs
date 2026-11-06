//! `--no-default-lib` and `--add-debug-tag`.

use std::io::Write;

use super::Pass;
use crate::debug;
use crate::elf::consts::{DF_1_NODEFLIB, DT_DEBUG, DT_FLAGS_1};
use crate::error::Result;
use crate::image::Image;

pub struct NoDefaultLib;

impl Pass for NoDefaultLib {
    fn name(&self) -> &'static str {
        "no-default-lib"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let dynamic = img.dynamic_mut()?;
        match dynamic.position(DT_FLAGS_1) {
            Some(i) if dynamic.entries[i].val & DF_1_NODEFLIB != 0 => {
                debug!("DF_1_NODEFLIB is already set");
                return Ok(());
            }
            Some(i) => {
                dynamic.entries[i].val |= DF_1_NODEFLIB;
                dynamic.dirty = true;
            }
            None => dynamic.insert_front(DT_FLAGS_1, DF_1_NODEFLIB),
        }
        img.changed = true;
        Ok(())
    }
}

/// Shared libraries normally lack `DT_DEBUG`; a debugger needs it to find
/// the loader's data when such a library is run as a program.
pub struct AddDebugTag;

impl Pass for AddDebugTag {
    fn name(&self) -> &'static str {
        "add-debug-tag"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let dynamic = img.dynamic_mut()?;
        if dynamic.position(DT_DEBUG).is_some() {
            debug!("DT_DEBUG is already present");
            return Ok(());
        }
        dynamic.insert_front(DT_DEBUG, 0);
        img.changed = true;
        Ok(())
    }
}
