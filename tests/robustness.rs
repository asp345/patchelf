//! Feed damaged ELF files through every pass and the layout, and check that
//! patchelf reports errors instead of panicking. Successful results must
//! parse and survive a second round of edits.
//!
//! `PATCHELF_ROBUSTNESS_ITERS` raises the number of mutations per file
//! (default 300), `PATCHELF_ROBUSTNESS_SEED` picks the random seed, and
//! `PATCHELF_ROBUSTNESS_DIR` adds the files of another directory.

use std::path::{Path, PathBuf};

use patchelf::cli::{ExecstackMode, Options, RpathOp};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    for dir in ["no-rpath-prebuild", "invalid-elf", "endianness", "empty-note"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in entries.flatten() {
            if e.path().is_file() {
                files.push(e.path());
            }
        }
    }
    if let Some(dir) = std::env::var_os("PATCHELF_ROBUSTNESS_DIR") {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_file() {
                files.push(e.path());
            }
        }
    }
    files.sort();
    assert!(!files.is_empty(), "no fixtures found");
    files
}

fn bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// One option set per kind of edit.
fn edits() -> Vec<Options> {
    let base = || Options { clobber_old_sections: true, ..Options::default() };
    let long = "/nix/store/0123456789abcdefghijklmnopqrstuv-some-long-package-name-1.0/lib";
    vec![
        Options {
            print_interpreter: true,
            print_os_abi: true,
            print_soname: true,
            print_rpath: true,
            print_needed: true,
            execstack: Some(ExecstackMode::Print),
            ..base()
        },
        Options { rpath: Some(RpathOp::Set(bytes(long))), ..base() },
        Options { rpath: Some(RpathOp::Add(bytes("/x"))), force_rpath: true, ..base() },
        Options { rpath: Some(RpathOp::Remove), ..base() },
        Options { set_interpreter: Some(bytes(long)), ..base() },
        Options { set_soname: Some(bytes("libsomething-much-longer.so.1")), ..base() },
        Options { add_needed: [bytes("libadded.so.1")].into(), remove_needed: [bytes("libc.so.6")].into(), ..base() },
        Options { replace_needed: [(bytes("libc.so.6"), bytes("libreplacement.so.6"))].into(), ..base() },
        Options { execstack: Some(ExecstackMode::Set), no_default_lib: true, add_debug_tag: true, ..base() },
        Options { rename_dynamic_symbols: [(bytes("printf"), bytes("printf_renamed"))].into(), ..base() },
        Options { clear_symbol_version: [bytes("printf")].into(), ..base() },
        Options { set_os_abi: Some(9), page_size: Some(0x10000), ..base() },
    ]
}

fn run(opts: &Options, input: &[u8]) -> Option<Vec<u8>> {
    let mut sink = Vec::new();
    patchelf::patch_bytes(opts, input, &mut sink).ok().flatten()
}

fn mutate(rng: &mut Rng, data: &mut [u8]) {
    let n = 1 + rng.below(6);
    for _ in 0..n {
        let len = data.len() as u64;
        // Bias towards the headers, where damage is most interesting.
        let pos = if rng.below(2) == 0 { rng.below(len.min(256)) } else { rng.below(len) } as usize;
        let width = [1usize, 2, 4, 8][rng.below(4) as usize].min(data.len() - pos);
        let value = match rng.below(5) {
            0 => 0,
            1 => u64::MAX,
            2 => rng.below(64),
            3 => 1u64 << rng.below(64),
            _ => rng.next(),
        };
        data[pos..pos + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
}

#[test]
fn damaged_files_never_panic() {
    let iters: u64 = std::env::var("PATCHELF_ROBUSTNESS_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let seed: u64 = std::env::var("PATCHELF_ROBUSTNESS_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x5eed);
    let edits = edits();
    let mut rng = Rng(seed | 1);
    for file in fixtures() {
        let original = std::fs::read(&file).unwrap();
        if original.len() < 64 {
            continue;
        }
        for i in 0..iters {
            let mut data = original.clone();
            if i > 0 {
                mutate(&mut rng, &mut data);
            }
            let opts = &edits[rng.below(edits.len() as u64) as usize];
            let result = std::panic::catch_unwind(|| {
                if let Some(out) = run(opts, &data) {
                    // A written result must parse and be editable again.
                    patchelf::elf::Elf::parse(&out).expect("output does not parse");
                    let again = &edits[(i as usize + 1) % edits.len()];
                    run(again, &out);
                }
            });
            if result.is_err() {
                let dump = std::env::temp_dir().join(format!("patchelf-panic-{i}"));
                std::fs::write(&dump, &data).unwrap();
                panic!("panic on {} (mutation {i}, seed {seed}); input saved to {}", file.display(), dump.display());
            }
        }
    }
}

#[test]
fn unmodified_fixtures_patch_cleanly() {
    // Every committed sample binary must accept every edit, except where an
    // edit legitimately does not apply to that file (reported as an error).
    for file in fixtures().into_iter().filter(|f| f.to_string_lossy().contains("no-rpath-prebuild")) {
        let data = std::fs::read(&file).unwrap();
        for opts in edits() {
            let mut sink = Vec::new();
            match patchelf::patch_bytes(&opts, &data, &mut sink) {
                Ok(Some(out)) => {
                    assert!(patchelf::validate::problems(&out).is_subset(&patchelf::validate::problems(&data)))
                }
                Ok(None) => {}
                Err(e) => {
                    let msg = e.to_string();
                    assert!(!msg.contains("internal error"), "{}: {msg}", file.display());
                }
            }
        }
    }
}
