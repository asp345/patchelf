//! Behaviour checks through the library API, using committed sample binaries.

use std::ffi::OsString;
use std::path::Path;

use patchelf::cli::{self, Command, ExecstackMode, Options, RpathOp};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/no-rpath-prebuild").join(name)).unwrap()
}

fn opts() -> Options {
    Options { clobber_old_sections: true, ..Options::default() }
}

fn apply(o: &Options, input: &[u8]) -> Vec<u8> {
    patchelf::patch_bytes(o, input, &mut Vec::new()).unwrap().expect("file was not changed")
}

fn print(o: Options, input: &[u8]) -> String {
    let mut out = Vec::new();
    patchelf::patch_bytes(&o, input, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn loads(data: &[u8]) -> usize {
    let elf = patchelf::elf::Elf::parse(data).unwrap();
    elf.phdrs.iter().filter(|p| p.p_type == patchelf::elf::consts::PT_LOAD).count()
}

fn parse(args: &[&str]) -> patchelf::error::Result<Command> {
    cli::parse(args.iter().map(OsString::from))
}

#[test]
fn rpath_round_trip_scrubs_old_values_and_reuses_the_segment() {
    for name in ["no-rpath-amd64", "no-rpath-i386", "no-rpath-powerpc", "no-rpath-s390"] {
        let input = sample(name);
        let long = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-first/lib";
        let first = apply(&Options { rpath: Some(RpathOp::Set(long.into())), ..opts() }, &input);
        assert_eq!(print(Options { print_rpath: true, ..opts() }, &first), format!("{long}\n"), "{name}");
        let grown = loads(&first);
        assert_eq!(grown, loads(&input) + 1, "{name}: one segment added");

        // A second, longer value is placed in the same segment and the old one disappears.
        let longer = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-second/lib:/nix/store/cccc-third/lib";
        let second = apply(&Options { rpath: Some(RpathOp::Set(longer.into())), ..opts() }, &first);
        assert_eq!(print(Options { print_rpath: true, ..opts() }, &second), format!("{longer}\n"), "{name}");
        assert_eq!(loads(&second), grown, "{name}: segment reused");
        assert!(!contains(&second, b"aaaaaaaaaaaaaaaa"), "{name}: old run path left in the file");

        // Removing it scrubs the string as well.
        let removed = apply(&Options { rpath: Some(RpathOp::Remove), ..opts() }, &second);
        assert_eq!(print(Options { print_rpath: true, ..opts() }, &removed), "\n");
        assert!(!contains(&removed, b"bbbbbbbbbbbbbbbb"), "{name}: removed run path left in the file");
    }
}

#[test]
fn shorter_values_are_written_in_place() {
    let input = sample("no-rpath-amd64");
    let with_rpath = apply(&Options { rpath: Some(RpathOp::Set("/a/long/directory:/b".into())), ..opts() }, &input);
    let shorter = apply(&Options { rpath: Some(RpathOp::Set("/c".into())), ..opts() }, &with_rpath);
    assert_eq!(shorter.len(), with_rpath.len());
    assert_eq!(print(Options { print_rpath: true, ..opts() }, &shorter), "/c\n");
}

#[test]
fn needed_libraries_are_added_first_and_only_once() {
    let input = sample("no-rpath-amd64");
    let before = print(Options { print_needed: true, ..opts() }, &input);
    let added = apply(&Options { add_needed: ["libb.so".into(), "liba.so".into()].into(), ..opts() }, &input);
    assert_eq!(print(Options { print_needed: true, ..opts() }, &added), format!("liba.so\nlibb.so\n{before}"));
    let again =
        patchelf::patch_bytes(&Options { add_needed: ["liba.so".into()].into(), ..opts() }, &added, &mut Vec::new());
    assert!(matches!(again, Ok(None)), "adding an existing library changes nothing");
}

#[test]
fn execstack_round_trip() {
    for name in ["no-rpath-amd64", "no-rpath-sparc", "no-rpath-armhf"] {
        let input = sample(name);
        let set = apply(&Options { execstack: Some(ExecstackMode::Set), ..opts() }, &input);
        assert_eq!(print(Options { execstack: Some(ExecstackMode::Print), ..opts() }, &set), "execstack: X\n");
        let clear = apply(&Options { execstack: Some(ExecstackMode::Clear), ..opts() }, &set);
        assert_eq!(print(Options { execstack: Some(ExecstackMode::Print), ..opts() }, &clear), "execstack: -\n");
    }
}

#[test]
fn command_line() {
    let Ok(Command::Run(o)) =
        parse(&["--shrink-rpath", "--set-rpath", "/x", "--print-execstack", "--set-execstack", "f"])
    else {
        panic!()
    };
    assert_eq!(o.rpath, Some(RpathOp::Shrink), "--shrink-rpath wins over --set-rpath");
    assert_eq!(o.execstack, Some(ExecstackMode::Print));
    assert_eq!(o.files.len(), 1);

    let Ok(Command::Run(o)) = parse(&["--replace-needed", "a", "b", "--set-os-abi", " FreeBSD ", "f"]) else {
        panic!()
    };
    assert_eq!(o.replace_needed.get(b"a".as_slice()).map(Vec::as_slice), Some(b"b".as_slice()));
    assert_eq!(o.set_os_abi, Some(9));

    for (args, msg) in [
        (&["--set-rpath", "/a", "--add-rpath", "/b", "f"][..], "not allowed with --add-rpath"),
        (&["--output", "o", "f", "g"], "single input file"),
        (&["--page-size", "3000", "f"], "invalid argument"),
        (&["--set-os-abi", "beos", "f"], "unrecognized OS ABI"),
        (&["--frobnicate", "f"], "unknown option"),
        (&["--set-soname"], "missing argument"),
        (&["--print-rpath"], "missing filename"),
    ] {
        let Err(e) = parse(args) else { panic!("{args:?} should fail") };
        assert!(e.to_string().contains(msg), "{args:?}: {e}");
    }
    assert!(matches!(parse(&[]), Ok(Command::Help { failed: true })));
    assert!(matches!(parse(&["--version"]), Ok(Command::Version)));
}
