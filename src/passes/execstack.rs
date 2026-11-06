//! `--print-execstack`, `--set-execstack`, `--clear-execstack`.

use std::io::Write;

use super::Pass;
use crate::cli::ExecstackMode;
use crate::debug;
use crate::elf::consts::{PF_R, PF_W, PF_X, PT_GNU_STACK, PT_NULL};
use crate::elf::types::Phdr;
use crate::error::Result;
use crate::image::Image;

pub struct Execstack(pub ExecstackMode);

impl Pass for Execstack {
    fn name(&self) -> &'static str {
        "execstack"
    }
    fn run(&self, img: &mut Image, out: &mut dyn Write) -> Result<()> {
        let stack = img.phdrs.iter().position(|p| p.p_type == PT_GNU_STACK);
        let want_exec = match self.0 {
            ExecstackMode::Print => {
                let state = match stack {
                    None => '?',
                    Some(i) if img.phdrs[i].flags & PF_X != 0 => 'X',
                    Some(_) => '-',
                };
                writeln!(out, "execstack: {state}")?;
                return Ok(());
            }
            ExecstackMode::Set => true,
            ExecstackMode::Clear => false,
        };
        match stack {
            Some(i) => {
                let p = &mut img.phdrs[i];
                if (p.flags & PF_X != 0) == want_exec {
                    debug!("execstack already in requested state");
                    return Ok(());
                }
                p.flags ^= PF_X;
            }
            None => {
                let header = Phdr {
                    p_type: PT_GNU_STACK,
                    flags: PF_R | PF_W | if want_exec { PF_X } else { 0 },
                    align: 0x10,
                    ..Phdr::default()
                };
                // Reuse an unused slot if there is one; otherwise the table grows.
                match img.phdrs.iter().position(|p| p.p_type == PT_NULL) {
                    Some(i) => img.phdrs[i] = header,
                    None => img.phdrs.push(header),
                }
            }
        }
        img.changed = true;
        Ok(())
    }
}
