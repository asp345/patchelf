//! patchelf: modify the dynamic loader, run path and dependencies of ELF files.
//!
//! The work for each file is split into three stages:
//! 1. [`image::Image::load`] parses the file into an editable model.
//! 2. [`passes`] apply the requested changes to the model, one feature per pass.
//! 3. [`layout::emit`] decides where everything goes and produces the bytes,
//!    and [`validate`] checks them before anything is written.

pub mod cli;
pub mod elf;
pub mod error;
pub mod image;
pub mod io;
pub mod layout;
pub mod log;
pub mod os;
pub mod passes;
pub mod validate;

use std::path::Path;

use cli::Options;
use error::{Error, Result};
use image::Image;

pub fn run(opts: &Options) -> Result<()> {
    for file in &opts.files {
        patch_file(opts, file).map_err(|e| e.context(file.display()))?;
    }
    Ok(())
}

fn patch_file(opts: &Options, file: &Path) -> Result<()> {
    let input = io::read(file)?;
    let stdout = std::io::stdout();
    let output = patch_bytes(opts, &input, &mut stdout.lock())?;
    let target = opts.output.as_deref().unwrap_or(file);
    match output {
        Some(bytes) => io::write(target, &bytes, file),
        None if opts.output.is_some() => {
            debug!("not modified, but --output was given");
            io::write(target, &input, file)
        }
        None => Ok(()),
    }
}

/// Apply `opts` to an ELF image in memory. Printed output goes to `out`.
/// Returns the new file contents, or `None` if nothing changed.
pub fn patch_bytes(opts: &Options, input: &[u8], out: &mut dyn std::io::Write) -> Result<Option<Vec<u8>>> {
    let mut img = Image::load(input.to_vec())?;
    for pass in passes::build(opts) {
        debug!("pass: {}", pass.name());
        pass.run(&mut img, out)?;
    }
    if !img.changed {
        return Ok(None);
    }
    let config = layout::Config { page_size: opts.page_size, clobber_old: opts.clobber_old_sections };
    let output = layout::emit(img, &config)?;

    let before = validate::problems(input);
    let introduced: Vec<String> = validate::problems(&output).difference(&before).cloned().collect();
    if !introduced.is_empty() {
        return Err(Error::new(format!(
            "internal error: the result failed validation, so nothing was written: {}",
            introduced.join("; ")
        )));
    }
    Ok(Some(output))
}
