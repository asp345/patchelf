//! `--print-os-abi`, `--set-os-abi`.

use std::io::Write;

use super::Pass;
use crate::debug;
use crate::elf::consts::EI_OSABI;
use crate::error::Result;
use crate::image::Image;

const NAMES: &[(u8, &str)] = &[
    (0, "System V"),
    (1, "HP-UX"),
    (2, "NetBSD"),
    (3, "Linux"),
    (4, "GNU Hurd"),
    (6, "Solaris"),
    (7, "AIX"),
    (8, "IRIX"),
    (9, "FreeBSD"),
    (10, "Tru64"),
    (12, "OpenBSD"),
    (13, "OpenVMS"),
];

/// Parse the names `--set-os-abi` accepts (case-insensitive, surrounding whitespace ignored).
pub fn parse(name: &str) -> Option<u8> {
    let abi = match name.trim().to_ascii_lowercase().as_str() {
        "system v" | "system-v" | "sysv" => 0,
        "hp-ux" => 1,
        "netbsd" => 2,
        "linux" | "gnu" => 3,
        "gnu hurd" | "gnu-hurd" | "hurd" => 4,
        "solaris" => 6,
        "aix" => 7,
        "irix" => 8,
        "freebsd" => 9,
        "tru64" => 10,
        "openbsd" => 12,
        "openvms" => 13,
        _ => return None,
    };
    Some(abi)
}

pub struct Print;

impl Pass for Print {
    fn name(&self) -> &'static str {
        "print-os-abi"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        let abi = img.ehdr.ident[EI_OSABI];
        match NAMES.iter().find(|(v, _)| *v == abi) {
            Some((_, name)) => writeln!(out, "{name}")?,
            None => writeln!(out, "0x{abi:02X}")?,
        }
        Ok(())
    }
}

pub struct Set(pub u8);

impl Pass for Set {
    fn name(&self) -> &'static str {
        "set-os-abi"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        if img.ehdr.ident[EI_OSABI] == self.0 {
            debug!("current and requested OS ABIs are equal");
            return Ok(());
        }
        img.ehdr.ident[EI_OSABI] = self.0;
        img.changed = true;
        Ok(())
    }
}
