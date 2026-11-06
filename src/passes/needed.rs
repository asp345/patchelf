//! `--print-needed`, `--remove-needed`, `--replace-needed`, `--add-needed`.

use std::io::Write;

use super::{rescache, NameMap, Names, Pass};
use crate::elf::consts::DT_NEEDED;
use crate::error::Result;
use crate::image::Image;
use crate::{debug, log::show};

pub struct Print;

impl Pass for Print {
    fn name(&self) -> &'static str {
        "print-needed"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        if img.dynamic_stripped {
            debug!("the dynamic section's contents are not in this file");
            return Ok(());
        }
        for name in img.dynamic_strings(DT_NEEDED)? {
            out.write_all(&name)?;
            out.write_all(b"\n")?;
        }
        Ok(())
    }
}

pub struct Remove(pub Names);

impl Pass for Remove {
    fn name(&self) -> &'static str {
        "remove-needed"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let mut removed = Vec::new();
        let entries = img.dynamic()?.entries.clone();
        let mut kept = Vec::with_capacity(entries.len());
        for d in entries {
            if d.tag == DT_NEEDED {
                let name = img.string(d.val)?;
                if self.0.contains(&name) {
                    debug!("removing DT_NEEDED entry '{}'", show(&name));
                    removed.push(d.val);
                    continue;
                }
                debug!("keeping DT_NEEDED entry '{}'", show(&name));
            }
            kept.push(d);
        }
        if removed.is_empty() {
            return Ok(());
        }
        let dynamic = img.dynamic_mut()?;
        dynamic.entries = kept;
        dynamic.dirty = true;
        for off in removed {
            img.release_string(off)?;
        }
        img.changed = true;
        rescache::remove(img)
    }
}

pub struct Replace(pub NameMap);

impl Pass for Replace {
    fn name(&self) -> &'static str {
        "replace-needed"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let mut replaced = false;
        let n = img.dynamic()?.entries.len();
        for i in 0..n {
            let d = img.dynamic()?.entries[i];
            if d.tag != DT_NEEDED {
                continue;
            }
            let name = img.string(d.val)?;
            match self.0.get(&name) {
                Some(new) if *new != name => {
                    debug!("replacing DT_NEEDED entry '{}' with '{}'", show(&name), show(new));
                    img.release_string(d.val)?;
                    let off = img.intern(new)?;
                    let dynamic = img.dynamic_mut()?;
                    dynamic.entries[i].val = off;
                    dynamic.dirty = true;
                    replaced = true;
                }
                _ => debug!("keeping DT_NEEDED entry '{}'", show(&name)),
            }
        }

        // Version requirements name the library they come from (`vn_file`).
        for (field, is_file) in img.version_string_fields()? {
            if !is_file {
                continue;
            }
            let off = u64::from(img.ctx.u32(&img.data, field)?);
            let name = img.string(off)?;
            if let Some(new) = self.0.get(&name).filter(|new| **new != name) {
                debug!("replacing version requirement file '{}' with '{}'", show(&name), show(new));
                img.release_string(off)?;
                let new_off = img.intern(new)?;
                let ctx = img.ctx;
                ctx.put_u32(&mut img.data, field, new_off as u32)?;
                replaced = true;
            }
        }

        if replaced {
            img.changed = true;
            rescache::remove(img)?;
        }
        Ok(())
    }
}

pub struct Add(pub Names);

impl Pass for Add {
    fn name(&self) -> &'static str {
        "add-needed"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let existing = img.dynamic_strings(DT_NEEDED)?;
        let mut offsets = Vec::new();
        for lib in &self.0 {
            if existing.contains(lib) {
                debug!("'{}' is already a DT_NEEDED entry", show(lib));
                continue;
            }
            debug!("adding DT_NEEDED entry '{}'", show(lib));
            offsets.push(img.intern(lib)?);
        }
        if offsets.is_empty() {
            return Ok(());
        }
        let dynamic = img.dynamic_mut()?;
        // New entries go first, in sorted order.
        for off in offsets.into_iter().rev() {
            dynamic.insert_front(DT_NEEDED, off);
        }
        img.changed = true;
        rescache::remove(img)
    }
}
