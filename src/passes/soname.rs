//! `--print-soname`, `--set-soname`.

use std::io::Write;

use super::Pass;
use crate::elf::consts::{DT_SONAME, ET_DYN};
use crate::error::Result;
use crate::image::Image;
use crate::{debug, log::show};

pub struct Print;

impl Pass for Print {
    fn name(&self) -> &'static str {
        "print-soname"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        if img.ehdr.e_type != ET_DYN {
            debug!("this is not a dynamic library");
            return Ok(());
        }
        if img.dynamic_stripped {
            debug!("the dynamic section's contents are not in this file");
            return Ok(());
        }
        match img.dynamic()?.get(DT_SONAME) {
            None => debug!("no DT_SONAME found"),
            Some(off) => {
                let name = img.string(off)?;
                if name.is_empty() {
                    debug!("DT_SONAME is empty");
                } else {
                    out.write_all(&name)?;
                    out.write_all(b"\n")?;
                }
            }
        }
        Ok(())
    }
}

pub struct Set(pub Vec<u8>);

impl Pass for Set {
    fn name(&self) -> &'static str {
        "set-soname"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        if img.ehdr.e_type != ET_DYN {
            debug!("this is not a dynamic library");
            return Ok(());
        }
        let old = img.dynamic()?.get(DT_SONAME);
        if let Some(off) = old {
            if img.string(off)? == self.0 {
                debug!("current and proposed new SONAMEs are equal");
                return Ok(());
            }
            img.release_string(off)?;
        }
        debug!("new SONAME is '{}'", show(&self.0));
        let off = img.intern(&self.0)?;
        let dynamic = img.dynamic_mut()?;
        match dynamic.position(DT_SONAME) {
            Some(i) => dynamic.entries[i].val = off,
            None => dynamic.insert_front(DT_SONAME, off),
        }
        dynamic.dirty = true;
        img.changed = true;
        Ok(())
    }
}
