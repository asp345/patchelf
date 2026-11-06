//! `--print-rpath`, `--set-rpath`, `--add-rpath`, `--remove-rpath`,
//! `--shrink-rpath` (with `--allowed-rpath-prefixes`) and `--force-rpath`.
//!
//! `DT_RUNPATH` is preferred: unless `--force-rpath` is given, a `DT_RPATH`
//! is turned into `DT_RUNPATH` when the run path is changed, and a new entry
//! is a `DT_RUNPATH`. With `--force-rpath` it is the other way around.

use std::io::{Read, Write};

use super::{rescache, Pass};
use crate::cli::RpathOp;
use crate::elf::consts::{DT_NEEDED, DT_RPATH, DT_RUNPATH};
use crate::error::Result;
use crate::image::Image;
use crate::{debug, log::show};

/// The effective run path: `DT_RUNPATH` wins over `DT_RPATH`, as in the loader.
fn current(img: &Image) -> Result<Option<Vec<u8>>> {
    let d = img.dynamic()?;
    match d.get(DT_RUNPATH).or_else(|| d.get(DT_RPATH)) {
        Some(off) => Ok(Some(img.string(off)?)),
        None => Ok(None),
    }
}

pub struct Print;

impl Pass for Print {
    fn name(&self) -> &'static str {
        "print-rpath"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        if img.dynamic_stripped {
            debug!("no dynamic section");
            return Ok(());
        }
        out.write_all(&current(img)?.unwrap_or_default())?;
        out.write_all(b"\n")?;
        Ok(())
    }
}

pub struct Modify {
    pub op: RpathOp,
    pub force_rpath: bool,
    pub allowed_prefixes: Vec<Vec<u8>>,
}

impl Pass for Modify {
    fn name(&self) -> &'static str {
        "modify-rpath"
    }

    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        if img.dynamic_stripped {
            debug!("no dynamic section");
            return Ok(());
        }
        let old = current(img)?;
        let new = match (&self.op, &old) {
            (RpathOp::Remove, None) => {
                rescache::remove(img)?;
                debug!("no RPATH to delete");
                return Ok(());
            }
            (RpathOp::Remove, Some(_)) => {
                rescache::remove(img)?;
                remove_entries(img)?;
                return Ok(());
            }
            (RpathOp::Shrink, None) => {
                rescache::remove(img)?;
                debug!("no RPATH to shrink");
                return Ok(());
            }
            (RpathOp::Shrink, Some(old)) => {
                let needed = img.dynamic_strings(DT_NEEDED)?;
                shrink(old, &needed, &self.allowed_prefixes, img.ehdr.machine)
            }
            (RpathOp::Add(extra), _) => {
                let mut v = old.clone().unwrap_or_default();
                if !v.is_empty() {
                    v.push(b':');
                }
                v.extend_from_slice(extra);
                v
            }
            (RpathOp::Set(v), _) => v.clone(),
        };

        self.convert_tags(img)?;
        if old.as_ref() == Some(&new) {
            return Ok(());
        }
        rescache::remove(img)?;
        debug!("new rpath is '{}'", show(&new));

        // Release the old string first, so a shorter path can reuse its space.
        let olds: Vec<u64> = entries(img)?.iter().map(|&i| img.dynamic.as_ref().unwrap().entries[i].val).collect();
        for off in olds {
            img.release_string(off)?;
        }
        let off = img.intern(&new)?;
        let positions = entries(img)?;
        let force = self.force_rpath;
        let dynamic = img.dynamic_mut()?;
        if positions.is_empty() {
            dynamic.insert_front(if force { DT_RPATH } else { DT_RUNPATH }, off);
        }
        for i in positions {
            dynamic.entries[i].val = off;
        }
        dynamic.dirty = true;
        img.changed = true;
        Ok(())
    }
}

impl Modify {
    /// Switch between `DT_RPATH` and `DT_RUNPATH` as described in the module docs.
    fn convert_tags(&self, img: &mut Image) -> Result<()> {
        let force = self.force_rpath;
        let dynamic = img.dynamic_mut()?;
        let rpath = dynamic.position(DT_RPATH);
        let runpath = dynamic.position(DT_RUNPATH);
        match (force, rpath, runpath) {
            (false, Some(i), None) => {
                debug!("converting DT_RPATH to DT_RUNPATH");
                dynamic.entries[i].tag = DT_RUNPATH;
            }
            (true, None, Some(i)) => {
                debug!("converting DT_RUNPATH to DT_RPATH");
                dynamic.entries[i].tag = DT_RPATH;
            }
            (true, Some(_), Some(i)) => {
                // The loader ignores DT_RPATH while DT_RUNPATH is present.
                debug!("removing DT_RUNPATH in favour of DT_RPATH");
                let val = dynamic.entries.remove(i).val;
                img.release_string(val)?;
                img.dynamic_mut()?.dirty = true;
                img.changed = true;
                return Ok(());
            }
            _ => return Ok(()),
        }
        dynamic.dirty = true;
        img.changed = true;
        Ok(())
    }
}

/// Indices of the `DT_RPATH` and `DT_RUNPATH` entries.
fn entries(img: &Image) -> Result<Vec<usize>> {
    let d = img.dynamic()?;
    Ok((0..d.entries.len()).filter(|&i| matches!(d.entries[i].tag, DT_RPATH | DT_RUNPATH)).collect())
}

fn remove_entries(img: &mut Image) -> Result<()> {
    for i in entries(img)?.into_iter().rev() {
        let d = img.dynamic_mut()?.entries.remove(i);
        debug!("removing {} entry", if d.tag == DT_RPATH { "DT_RPATH" } else { "DT_RUNPATH" });
        img.release_string(d.val)?;
    }
    img.dynamic_mut()?.dirty = true;
    img.changed = true;
    Ok(())
}

/// Keep the run path entries that provide a needed library (with the right
/// `e_machine`), plus relative entries such as `$ORIGIN`, which can't be
/// checked here.
fn shrink(rpath: &[u8], needed: &[Vec<u8>], allowed: &[Vec<u8>], machine: u16) -> Vec<u8> {
    let mut found = vec![false; needed.len()];
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut dirs: Vec<&[u8]> = rpath.split(|&b| b == b':').collect();
    if dirs.last().is_some_and(|d| d.is_empty()) {
        dirs.pop();
    }
    for dir in dirs {
        if !dir.starts_with(b"/") {
            kept.push(dir);
            continue;
        }
        if !allowed.is_empty() && !allowed.iter().any(|p| dir.starts_with(p)) {
            debug!("removing directory '{}' from RPATH because of non-allowed prefix", show(dir));
            continue;
        }
        let mut provides = false;
        for (j, lib) in needed.iter().enumerate() {
            if found[j] {
                continue;
            }
            let mut path = dir.to_vec();
            path.push(b'/');
            path.extend_from_slice(lib);
            let path = crate::os::path_of(&path);
            let mut header = [0u8; 64];
            let Ok(n) = std::fs::File::open(&path).and_then(|mut f| f.read(&mut header)) else {
                continue;
            };
            if crate::elf::machine_of(&header[..n]) == Some(machine) {
                found[j] = true;
                provides = true;
            } else {
                debug!("ignoring library '{}' because its machine type differs", path.display());
            }
        }
        if provides {
            kept.push(dir);
        } else {
            debug!("removing directory '{}' from RPATH", show(dir));
        }
    }
    kept.join(&b':')
}
