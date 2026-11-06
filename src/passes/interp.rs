//! `--print-interpreter`, `--set-interpreter`.

use std::io::Write;

use super::Pass;
use crate::error::{Error, Result};
use crate::image::Image;
use crate::{debug, log::show};

fn missing(img: &Image) -> Error {
    if img.interp_stripped {
        Error::new("the interpreter's contents are not in this file")
    } else {
        Error::new(
            "cannot find the interpreter (no PT_INTERP segment); the input file is most likely statically linked",
        )
    }
}

pub struct Print;

impl Pass for Print {
    fn name(&self) -> &'static str {
        "print-interpreter"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        let interp = img.interp.as_ref().ok_or_else(|| missing(img))?;
        out.write_all(&interp.value)?;
        out.write_all(b"\n")?;
        Ok(())
    }
}

pub struct Set(pub Vec<u8>);

impl Pass for Set {
    fn name(&self) -> &'static str {
        "set-interpreter"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        crate::ensure!(!self.0.contains(&0), "the interpreter path contains a NUL byte");
        if img.interp.is_none() {
            return Err(missing(img));
        }
        let interp = img.interp.as_mut().unwrap();
        if interp.value == self.0 {
            debug!("given interpreter is already set");
            return Ok(());
        }
        debug!("new interpreter is '{}'", show(&self.0));
        interp.value = self.0.clone();
        interp.dirty = true;
        img.changed = true;
        Ok(())
    }
}
