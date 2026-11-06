//! `--build-resolution-cache`: record where each `DT_NEEDED` library is
//! found on the run path, in a note that a patched loader (Nixpkgs' glibc)
//! reads to skip the search at startup.
//!
//! The descriptor is a sequence of NUL-terminated `(library, path-list)`
//! pairs, followed by one more NUL. A path-list entry is `=<path>` for a
//! library found at patch time, or `?<dir>` for a directory the loader still
//! has to search (relative or `$`-token entries, directories missing at
//! patch time, and directories with `glibc-hwcaps` subdirectories).
//!
//! Any change to the run path or the needed libraries makes the cache stale,
//! so those passes call [`remove`].

use std::collections::BTreeMap;
use std::io::Write;

use super::Pass;
use crate::elf::consts::*;
use crate::elf::note;
use crate::error::Result;
use crate::image::{Image, NewNote};
use crate::os::{access, Access};
use crate::{bail, debug, log::show};

const NOTE_NAME: &[u8] = b"NixOS";
const SECTION_NAME: &[u8] = b".note.nixos.ldcache";
const NT_NIXOS_LD_CACHE: u32 = 0x63a8_6cb6;

/// An existing cache note: its file range and descriptor.
struct Existing {
    offset: u64,
    size: u64,
    desc: Vec<u8>,
}

fn find(img: &Image) -> Option<Existing> {
    for p in img.phdrs.iter().filter(|p| p.p_type == PT_NOTE) {
        for n in note::parse(img.ctx, &img.data, p.offset, p.filesz, p.align) {
            if n.name == NOTE_NAME && n.n_type == NT_NIXOS_LD_CACHE {
                return Some(Existing { offset: n.offset, size: n.size, desc: n.desc.to_vec() });
            }
        }
    }
    // A section with the cache's name counts even if its contents are not a
    // valid note: it is stale either way.
    let i = img.find_section(SECTION_NAME)?;
    let s = &img.shdrs[i];
    let size = if s.has_file_data() { s.size } else { 0 };
    let desc = note::parse(img.ctx, &img.data, s.offset, size, s.addralign)
        .into_iter()
        .next()
        .map(|n| n.desc.to_vec())
        .unwrap_or_default();
    Some(Existing { offset: s.offset, size, desc })
}

/// Drop the cache note, its headers and its bytes (it contains store paths).
pub fn remove(img: &mut Image) -> Result<()> {
    let Some(e) = find(img) else { return Ok(()) };
    debug!("removing the resolution cache note");
    img.zero_ranges.push((e.offset, e.size));
    img.phdrs.retain(|p| !(matches!(p.p_type, PT_NOTE | PT_LOAD) && p.offset == e.offset && p.filesz == e.size));
    if let Some(i) = img.find_section(SECTION_NAME) {
        img.remove_section(i)?;
    }
    img.changed = true;
    Ok(())
}

/// Whether `dir` can only be searched at run time.
fn needs_search_hint(dir: &[u8]) -> bool {
    if !dir.starts_with(b"/") || dir.contains(&b'$') {
        return true;
    }
    let is_dir = std::fs::metadata(crate::os::path_of(dir)).is_ok_and(|m| m.is_dir());
    if !is_dir || !access(dir, Access::Search) {
        return true;
    }
    let mut hwcaps = dir.to_vec();
    hwcaps.extend_from_slice(b"/glibc-hwcaps");
    access(&hwcaps, Access::Exists)
}

pub struct Build;

impl Pass for Build {
    fn name(&self) -> &'static str {
        "build-resolution-cache"
    }

    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let existing = find(img);
        let fail_if_stale = |existing: &Option<Existing>| -> Result<()> {
            if existing.is_some() {
                bail!("--build-resolution-cache: a resolution cache note is already present; rebuild from an unpatched binary");
            }
            Ok(())
        };
        if img.dynamic.is_none() || img.dynstr.is_none() {
            fail_if_stale(&existing)?;
            eprintln!("warning: --build-resolution-cache: no dynamic section (statically linked?); no cache written");
            return Ok(());
        }
        let needed = img.dynamic_strings(DT_NEEDED)?;
        let d = img.dynamic()?;
        let run_path = match d.get(DT_RUNPATH).or_else(|| d.get(DT_RPATH)) {
            Some(off) => img.string(off)?,
            None => Vec::new(),
        };
        // A trailing empty component means the current directory and is kept.
        let dirs: Vec<&[u8]> = if run_path.is_empty() { Vec::new() } else { run_path.split(|&b| b == b':').collect() };
        if needed.is_empty() || dirs.is_empty() {
            fail_if_stale(&existing)?;
            eprintln!(
                "warning: --build-resolution-cache: no DT_NEEDED entries or run path to resolve; no cache written"
            );
            return Ok(());
        }

        // Libraries named by path are opened directly, without a search.
        let searched: Vec<&Vec<u8>> = needed.iter().filter(|l| !l.contains(&b'/')).collect();
        let mut cache: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let mut add = |lib: &[u8], entry: Vec<u8>| {
            let list = cache.entry(lib.to_vec()).or_default();
            if !list.is_empty() {
                list.push(b':');
            }
            list.extend_from_slice(&entry);
        };
        for dir in &dirs {
            if needs_search_hint(dir) {
                for lib in &searched {
                    add(lib, [b"?", *dir].concat());
                }
                continue;
            }
            for lib in &searched {
                let path = [*dir, b"/", lib.as_slice()].concat();
                if access(&path, Access::Read) {
                    add(lib, [b"=", path.as_slice()].concat());
                }
            }
        }
        if cache.is_empty() {
            fail_if_stale(&existing)?;
            eprintln!(
                "warning: --build-resolution-cache: no libraries resolved against the run path; no cache written"
            );
            return Ok(());
        }

        let mut desc = Vec::new();
        for (lib, paths) in &cache {
            debug!("resolved {} to {}", show(lib), show(paths));
            desc.extend_from_slice(lib);
            desc.push(0);
            desc.extend_from_slice(paths);
            desc.push(0);
        }
        desc.push(0);

        if let Some(e) = &existing {
            if e.desc == desc {
                debug!("resolution cache already up to date");
                return Ok(());
            }
            fail_if_stale(&existing)?;
        }
        img.new_notes.push(NewNote {
            section_name: SECTION_NAME,
            bytes: note::encode(img.ctx, NOTE_NAME, NT_NIXOS_LD_CACHE, &desc),
        });
        img.changed = true;
        Ok(())
    }
}
