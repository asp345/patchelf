//! Each command-line feature is one pass over the [`Image`].
//!
//! Passes only edit the in-memory model. They run in the fixed order of
//! [`build`], and the output layout is computed once, after the last pass.

mod execstack;
mod flags;
mod interp;
mod needed;
mod osabi;
mod rescache;
mod rpath;
mod soname;
mod symbols;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

pub use osabi::parse as parse_os_abi;

use crate::cli::Options;
use crate::error::Result;
use crate::image::Image;

pub trait Pass {
    fn name(&self) -> &'static str;
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()>;
}

/// The passes requested by `opts`, in the order they run.
pub fn build(opts: &Options) -> Vec<Box<dyn Pass>> {
    let mut passes: Vec<Box<dyn Pass>> = Vec::new();
    if opts.print_interpreter {
        passes.push(Box::new(interp::Print));
    }
    if opts.print_os_abi {
        passes.push(Box::new(osabi::Print));
    }
    if let Some(abi) = opts.set_os_abi {
        passes.push(Box::new(osabi::Set(abi)));
    }
    if opts.print_soname {
        passes.push(Box::new(soname::Print));
    }
    if let Some(name) = &opts.set_soname {
        passes.push(Box::new(soname::Set(name.clone())));
    }
    if let Some(path) = &opts.set_interpreter {
        passes.push(Box::new(interp::Set(path.clone())));
    }
    if opts.print_rpath {
        passes.push(Box::new(rpath::Print));
    }
    if let Some(mode) = opts.execstack {
        passes.push(Box::new(execstack::Execstack(mode)));
    }
    if let Some(op) = &opts.rpath {
        passes.push(Box::new(rpath::Modify {
            op: op.clone(),
            force_rpath: opts.force_rpath,
            allowed_prefixes: opts.allowed_rpath_prefixes.clone(),
        }));
    }
    if opts.print_needed {
        passes.push(Box::new(needed::Print));
    }
    if !opts.remove_needed.is_empty() {
        passes.push(Box::new(needed::Remove(opts.remove_needed.clone())));
    }
    if !opts.replace_needed.is_empty() {
        passes.push(Box::new(needed::Replace(opts.replace_needed.clone())));
    }
    if !opts.add_needed.is_empty() {
        passes.push(Box::new(needed::Add(opts.add_needed.clone())));
    }
    if !opts.clear_symbol_version.is_empty() {
        passes.push(Box::new(symbols::ClearVersion(opts.clear_symbol_version.clone())));
    }
    if opts.no_default_lib {
        passes.push(Box::new(flags::NoDefaultLib));
    }
    if opts.add_debug_tag {
        passes.push(Box::new(flags::AddDebugTag));
    }
    if opts.build_resolution_cache {
        passes.push(Box::new(rescache::Build));
    }
    if !opts.rename_dynamic_symbols.is_empty() {
        passes.push(Box::new(symbols::Rename(opts.rename_dynamic_symbols.clone())));
    }
    passes
}

pub type Names = BTreeSet<Vec<u8>>;
pub type NameMap = BTreeMap<Vec<u8>, Vec<u8>>;
