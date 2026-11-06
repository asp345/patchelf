//! Command-line parsing. The options and their spelling match earlier releases.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;

use crate::os::{bytes_of, path_of};

use crate::error::{Error, Result};
use crate::{bail, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpathOp {
    Shrink,
    Remove,
    Set(Vec<u8>),
    Add(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecstackMode {
    Print,
    Clear,
    Set,
}

#[derive(Debug, Default)]
pub struct Options {
    pub files: Vec<PathBuf>,
    pub output: Option<PathBuf>,
    pub debug: bool,
    pub page_size: Option<u64>,
    pub clobber_old_sections: bool,

    pub print_interpreter: bool,
    pub set_interpreter: Option<Vec<u8>>,
    pub print_os_abi: bool,
    pub set_os_abi: Option<u8>,
    pub print_soname: bool,
    pub set_soname: Option<Vec<u8>>,
    pub print_rpath: bool,
    pub rpath: Option<RpathOp>,
    pub force_rpath: bool,
    pub allowed_rpath_prefixes: Vec<Vec<u8>>,
    pub print_needed: bool,
    pub add_needed: BTreeSet<Vec<u8>>,
    pub remove_needed: BTreeSet<Vec<u8>>,
    pub replace_needed: BTreeMap<Vec<u8>, Vec<u8>>,
    pub clear_symbol_version: BTreeSet<Vec<u8>>,
    pub rename_dynamic_symbols: BTreeMap<Vec<u8>, Vec<u8>>,
    pub execstack: Option<ExecstackMode>,
    pub no_default_lib: bool,
    pub add_debug_tag: bool,
    pub build_resolution_cache: bool,
}

pub enum Command {
    Run(Box<Options>),
    Help { failed: bool },
    Version,
}

pub const USAGE: &str = "\
syntax: patchelf
  [--set-interpreter FILENAME]
  [--page-size SIZE]
  [--print-interpreter]
  [--print-os-abi]\t\tPrints 'EI_OSABI' field of ELF header
  [--set-os-abi ABI]\t\tSets 'EI_OSABI' field of ELF header to ABI.
  [--print-soname]\t\tPrints 'DT_SONAME' entry of .dynamic section. Raises an error if DT_SONAME doesn't exist
  [--set-soname SONAME]\t\tSets 'DT_SONAME' entry to SONAME.
  [--set-rpath RPATH]
  [--add-rpath RPATH]
  [--remove-rpath]
  [--shrink-rpath]
  [--allowed-rpath-prefixes PREFIXES]\t\tWith '--shrink-rpath', reject rpath entries not starting with the allowed prefix
  [--print-rpath]
  [--force-rpath]
  [--add-needed LIBRARY]
  [--remove-needed LIBRARY]
  [--replace-needed LIBRARY NEW_LIBRARY]
  [--print-needed]
  [--no-default-lib]
  [--no-sort]\t\tAccepted for compatibility; headers are never re-sorted.
  [--clear-symbol-version SYMBOL]
  [--add-debug-tag]
  [--build-resolution-cache]
  [--print-execstack]\t\tPrints whether the object requests an executable stack
  [--clear-execstack]
  [--set-execstack]
  [--rename-dynamic-symbols NAME_MAP_FILE]\tRenames dynamic symbols. The map file should contain two symbols (old_name new_name) per line
  [--no-clobber-old-sections]\t\tDo not clobber old section values - only use when the binary expects to find section info at the old location.
  [--output FILE]
  [--debug]
  [--version]
  FILENAME...
";

/// `@path` arguments are replaced by the contents of `path`.
fn resolve(arg: OsString) -> Result<Vec<u8>> {
    let bytes = bytes_of(&arg);
    match bytes.strip_prefix(b"@") {
        Some(path) if !path.is_empty() => std::fs::read(path_of(path))
            .map_err(|e| Error::new(format!("cannot read '{}': {e}", String::from_utf8_lossy(path)))),
        _ => Ok(bytes),
    }
}

fn split_colons(s: &[u8]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = s.split(|&b| b == b':').map(<[u8]>::to_vec).collect();
    if parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

fn read_symbol_map(path: &OsString, map: &mut BTreeMap<Vec<u8>, Vec<u8>>) -> Result<()> {
    let name = path.to_string_lossy();
    let text = std::fs::read(path).map_err(|e| Error::new(format!("cannot open map file {name}: {e}")))?;
    for (n, line) in text.split(|&b| b == b'\n').enumerate() {
        let mut words = line.split(|b| b.is_ascii_whitespace()).filter(|w| !w.is_empty());
        let Some(from) = words.next() else { break };
        let Some(to) = words.next() else {
            bail!("{name}:{}: map file line is missing the second element", n + 1);
        };
        ensure!(
            !map.contains_key(from),
            "{name}:{}: name '{}' appears twice in the map file",
            n + 1,
            String::from_utf8_lossy(from)
        );
        ensure!(
            !from.contains(&b'@') && !to.contains(&b'@'),
            "{name}:{}: name pair contains a version tag: {} {}",
            n + 1,
            String::from_utf8_lossy(from),
            String::from_utf8_lossy(to)
        );
        map.insert(from.to_vec(), to.to_vec());
    }
    Ok(())
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut args = args.into_iter().peekable();
    if args.peek().is_none() {
        return Ok(Command::Help { failed: true });
    }
    let mut o = Options { clobber_old_sections: true, ..Options::default() };
    let (mut shrink, mut remove, mut set, mut add) = (false, false, None, None);
    let (mut print_stack, mut clear_stack, mut set_stack) = (false, false, false);

    let next = |args: &mut std::iter::Peekable<_>| -> Result<OsString> {
        args.next().ok_or_else(|| Error::new("missing argument"))
    };
    while let Some(arg) = args.next() {
        match bytes_of(&arg).as_slice() {
            b"--set-interpreter" | b"--interpreter" => o.set_interpreter = Some(resolve(next(&mut args)?)?),
            b"--page-size" => {
                let v = next(&mut args)?;
                let size = v.to_str().and_then(|s| s.parse::<u64>().ok()).filter(|&s| s > 0 && s.is_power_of_two());
                o.page_size = Some(size.ok_or_else(|| Error::new("invalid argument to --page-size"))?);
            }
            b"--print-interpreter" => o.print_interpreter = true,
            b"--print-os-abi" => o.print_os_abi = true,
            b"--set-os-abi" => {
                let v = resolve(next(&mut args)?)?;
                let abi = crate::passes::parse_os_abi(&String::from_utf8_lossy(&v));
                o.set_os_abi = Some(abi.ok_or_else(|| Error::new("unrecognized OS ABI"))?);
            }
            b"--print-soname" => o.print_soname = true,
            b"--set-soname" => o.set_soname = Some(resolve(next(&mut args)?)?),
            b"--remove-rpath" => remove = true,
            b"--shrink-rpath" => shrink = true,
            b"--allowed-rpath-prefixes" => o.allowed_rpath_prefixes = split_colons(&bytes_of(&next(&mut args)?)),
            b"--set-rpath" => set = Some(resolve(next(&mut args)?)?),
            b"--add-rpath" => add = Some(resolve(next(&mut args)?)?),
            b"--print-rpath" => o.print_rpath = true,
            b"--force-rpath" => o.force_rpath = true,
            b"--print-needed" => o.print_needed = true,
            b"--no-sort" => {}
            b"--add-needed" => {
                o.add_needed.insert(resolve(next(&mut args)?)?);
            }
            b"--remove-needed" => {
                o.remove_needed.insert(resolve(next(&mut args)?)?);
            }
            b"--replace-needed" => {
                let (Some(from), Some(to)) = (args.next(), args.next()) else {
                    bail!("missing argument(s)");
                };
                o.replace_needed.insert(bytes_of(&from), bytes_of(&to));
            }
            b"--clear-symbol-version" => {
                o.clear_symbol_version.insert(resolve(next(&mut args)?)?);
            }
            b"--print-execstack" => print_stack = true,
            b"--clear-execstack" => clear_stack = true,
            b"--set-execstack" => set_stack = true,
            b"--output" => o.output = Some(path_of(&resolve(next(&mut args)?)?)),
            b"--debug" => o.debug = true,
            b"--no-default-lib" => o.no_default_lib = true,
            b"--add-debug-tag" => o.add_debug_tag = true,
            b"--build-resolution-cache" => o.build_resolution_cache = true,
            b"--rename-dynamic-symbols" => read_symbol_map(&next(&mut args)?, &mut o.rename_dynamic_symbols)?,
            b"--no-clobber-old-sections" => o.clobber_old_sections = false,
            b"--help" | b"-h" => return Ok(Command::Help { failed: false }),
            b"--version" => return Ok(Command::Version),
            a if a.starts_with(b"--") => bail!("unknown option '{}'", arg.to_string_lossy()),
            _ => o.files.push(PathBuf::from(arg)),
        }
    }

    ensure!(!o.files.is_empty(), "missing filename");
    ensure!(
        !(o.force_rpath && o.build_resolution_cache),
        "--build-resolution-cache cannot be combined with --force-rpath"
    );
    ensure!(o.output.is_none() || o.files.len() == 1, "--output option only allowed with single input file");
    ensure!(set.is_none() || add.is_none(), "--set-rpath option not allowed with --add-rpath");

    o.rpath = if shrink {
        Some(RpathOp::Shrink)
    } else if remove {
        Some(RpathOp::Remove)
    } else if let Some(v) = set {
        Some(RpathOp::Set(v))
    } else {
        add.map(RpathOp::Add)
    };
    o.execstack = if print_stack {
        Some(ExecstackMode::Print)
    } else if clear_stack {
        Some(ExecstackMode::Clear)
    } else if set_stack {
        Some(ExecstackMode::Set)
    } else {
        None
    };
    Ok(Command::Run(Box::new(o)))
}
