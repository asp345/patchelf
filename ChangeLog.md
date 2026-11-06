# Release History

## 0.20.0 (unreleased)

patchelf has been rewritten in Rust. It has the same options and is meant as
a drop-in replacement, but it is a new implementation rather than a port, and
the C++ sources and the Autotools, CMake and Meson builds are gone. Build with
`cargo build --release`; run the tests with `cargo test` and
`make -C tests check`.

### How files are rewritten

* Each option is a separate pass over an in-memory model of the file, and the
  output layout is computed once, after all passes.
* Code and data are never moved. Tables that grow move into a single new
  segment at the end of the file, and later runs rebuild that segment instead
  of adding more segments.
* The program header table stays right after the ELF header, so tools such as
  GNU `strip` keep working on patched files. When it needs more entries,
  loader metadata after it (interpreter, notes, symbol and hash tables, ...)
  moves out of the way, and unused `PT_NULL` entries are reused.
* Files are understood through the program headers and the dynamic section,
  like the dynamic loader does. Files without section headers can be patched.
* Every result is checked for overlapping segments, misplaced headers,
  dangling dynamic entries and similar problems before it is written; a file
  that would be broken is left untouched.

### Behaviour changes

* If the program header table has to move (rare), running the result needs
  Linux 5.18 or newer, or a recent qemu-user, unless it could be placed where
  older loaders look for it.
* `--add-needed` no longer adds a library that is already needed.
* `--force-rpath` on a file with both `DT_RPATH` and `DT_RUNPATH` drops the
  `DT_RUNPATH`, which would otherwise make the loader ignore `DT_RPATH`.
* `--shrink-rpath` keeps empty run path entries (the current directory) like
  other relative ones, and ignores unreadable or non-ELF files instead of
  failing.
* Strings that are no longer referenced (old run paths, sonames, library and
  symbol names) are overwritten, not only old run paths.
* Files whose dynamic section is not described by `PT_DYNAMIC` are rejected:
  the loader ignores such a section, so editing it had no effect.
* `--print-interpreter`, `--print-needed` and `--print-soname` handle
  `objcopy --only-keep-debug` files: the former reports that the interpreter
  is not in the file instead of printing garbage, the latter print nothing.
* `--page-size` must be a power of two, unknown `--options` are errors rather
  than file names, and `--no-sort` is accepted but has no effect.
* Error messages are reworded and name the file they refer to.
* Files are still edited in place, keeping links and ownership; a running
  executable is replaced through a rename.

### Bug fixes

* `--rename-dynamic-symbols` only renumbers relocations that refer to the
  dynamic symbol table (it used to also change `--emit-relocs` relocations)
  and keeps the System V hash table chains intact. On MIPS, where symbol
  order is tied to the GOT, it rebuilds `.hash` and `.MIPS.xhash` without
  reordering symbols (it used to reorder them, breaking the GOT).
* Unused `PT_NULL` program headers are reused when the table needs room.
* Malformed files can no longer make patchelf crash or allocate huge amounts
  of memory.

## 0.19.3 (October 30, 2026)

A bug fix release for executable rewrites, symbol version definitions, and zsh
completion.

### Bug fixes

* Fix overlapping load segment mappings when growing an executable's interpreter
  and then rewriting its RPATH. Preserve writable coverage of `.dynamic` so the
  dynamic loader can update `DT_DEBUG`, fixing crashes in musl bootstrap GCC and
  GNAT (#672).
* Reuse the header load segment for further growth and reserve enough file and
  virtual address space after `strip` removes padding, preventing later rewrites
  from overlapping mappings or corrupting section contents (#672).
* Keep the program header table covered by a load segment when repeated
  executable rewrites split note segments and grow the table. This fixes invalid
  PHDR coverage on x86_64 and an assertion failure on AArch64, including when
  patching Grpc.Tools binaries (#675, #679).
* Update `DT_VERDEF` when `.gnu.version_d` is relocated, preventing the dynamic
  loader from following stale symbol version definitions and crashing before
  the program starts (#664, #665).
* Fix zsh completion of options, OS ABI names, input files, library names, and
  colon-separated directory lists (#677).

## 0.19.2 (October 1, 2026)

A bug fix release for rewriting executables after stripping.

### Bug fixes

* Fix executable corruption when patching again after `strip` removes padding
  between load segments, as seen when bootstrapping GHC (#593, #667). Rewritten
  sections and program headers now use the address mapping of the load segment
  covering the ELF header.
* Avoid allocating an unnecessary extra page when growing executable sections,
  while keeping split load segments from overlapping after page alignment
  (#667).

## 0.19.1 (July 6, 2026)

A small bug fix release for `--build-resolution-cache`.

### Bug fixes

* `--build-resolution-cache`: run-path entries that only resolve at run time are
  now recorded as search hints in run-path order instead of being dropped or
  baked in as bogus exact entries, so cached resolution keeps the semantics of a
  plain run-path walk (#658, #659). This covers empty and relative components,
  absolute directories absent at patch time (such as `/run/opengl-driver/lib`
  on NixOS), trailing empty components, and entries that exist but are not
  searchable directories when patching.

## 0.19.0 (June 26, 2026)

This is the first feature release since 0.18.0 (April 2023) and collects three
years of fixes and hardening.

### New features

* Add `--build-resolution-cache` to write a per-DSO library resolution note (#647).
* Add `--no-clobber-old-sections` switch to keep the original section contents in place.

### Bug fixes

* Fix support for files larger than 4GB.
* Fix alignment problem when rewriting sections.
* Allocate the program and section header tables at the end of the file.
* Set the interpreter only when necessary.
* Fix `#639`: update `DT_INIT`/`DT_FINI` dynamic tags when their sections move, and stop
  rewriting them to the `.init`/`.fini` `sh_addr`.
* Fix `DT_MIPS_RLD_MAP_REL` on cross-endian patches and after `--remove-rpath`,
  `--remove-needed` and friends.
* Allow removing an RPATH on MIPS (#645).
* Fix the page size on Alpha, Itanium and SPARC.
* `roundUp`: reject a zero alignment instead of dividing by it.
* Preserve pointer provenance during string table offset arithmetic, fixing a
  `SIGSEGV` on capability-based architectures such as CHERI RISC-V (#641).
* Restrict system operations on OpenBSD.
* Optimize prefix stripping in `replaceNeeded` (#650).
* Fix compilation with GCC 7.

### Hardening

A round of fuzzing-driven robustness fixes so malformed inputs are rejected with
errors instead of crashing (#648):

* `setSubstr` errors out instead of asserting on an out-of-bounds write.
* Bound every `.dynamic` walk, `.dynstr` lookup and `.gnu.version_{d,r}` link walk by
  the owning section.
* Validate `e_shentsize`, hash table header counts and `getSectionSpan` offsets/sizes
  against the file before deriving spans.
* Treat `sh_addralign == 0` as 1 in `normalizeNoteSegments`.

### Build system and infra

* Add a Meson build system (#604) and improve the CMake build (#610).
* Add `package.nix` definitions for the autotools, CMake and Meson builds.
* Add property-based testing of binary-preserving rewrites (#651).
* CI: build static musl binaries for more architectures, test mips64le and FreeBSD,
  and add developer tooling and formatting.

## 0.16.1 (October 28, 2022)

This release only fixes the incorrect reported version by the 0.16.0 release.

## 0.16.0 (October 27, 2022)

Changes from 0.15.3:

* Add `--print-os-abi` and `--set-os-abi` options by @dmsck in https://github.com/NixOS/patchelf/pull/381
* Sync `.note.gnu.property` to `PT_GNU_PROPERTY` by @Bo98 in https://github.com/NixOS/patchelf/pull/414
* Rework file shifting to avoid sections crossing multiple segments by @Bo98 in https://github.com/NixOS/patchelf/pull/415

(These notes are the remainder of 0.16.0 that was not backported to 0.15.3.
Originally, 0.16.0 was branched from 0.15.0, and those changes were fresh in this release, having never been backported.)

## 0.15.5 (August 12, 2025)

Bugfix backports from 0.18.0:

* Adjust `roundUp` for 0 as input by @cgzones in https://github.com/NixOS/patchelf/pull/466

Bugfix backports from 0.17.{0, 1}:

* Fix page size constants for Itanium and SPARC.
* Fix Out-of-bounds read in the function `modifySoname` by @yairKoskas in https://github.com/NixOS/patchelf/pull/451

## 0.15.4 (August 12, 2025)

* Update to a later glibc `elf.h`.

  This should not result in any behavioral changes, except for possibly better architecture-specific support.

## 0.15.3 (August 12, 2025)

This is a backport release made from the simplest fixes from 0.16.0.

* Handle `glibc-hwcaps` on `ppc64le` on CentOS/RHEL/Rocky 8 for `tests/replace-add-needed.sh` (fixes #406) by @robert-scheck in https://github.com/NixOS/patchelf/pull/407
* Fix Out-of-bounds read in the function `modifyRPath` by @xiaoxiaoafeifei in https://github.com/NixOS/patchelf/pull/419
* Fix out of bounds access when increasing program header table by @Bo98 in https://github.com/NixOS/patchelf/pull/411

## 0.15.2 (August 12, 2025)

* In the autotools build system for the tests, pass a few more tool env vars in `TESTS_ENVIRONMENT`.

  Some of them are not used yet (at all, or at least not in this backport to the `0.15-maintenance` branch), but at least `STRIP` is.

## 0.15.1 (August 12, 2025)

The objective is to prepare a very conservative patch release which can be bumped in Nixpkgs immediately.

* Backport infra changes from later master (August 2025)
* Backport shellcheck fixes to test suite
* Backport C++ code quality fixes (all of https://github.com/NixOS/patchelf/pull/464 but the final two commits)

The behavior of this version should be essentially the same as 0.15.0.

## 0.15.0 (July 16, 2022)

* Add `--add-debug` option by @deadw00d in https://github.com/NixOS/patchelf/pull/367
* Add `O_BINARY flag` when opening files to allow compilation for Windows by @JagoGyselinck in https://github.com/NixOS/patchelf/pull/372
* Document `--print-needed` by @klemensn in https://github.com/NixOS/patchelf/pull/375
* modifyRPath: return early if new and old rpath are empty by @ehmry in https://github.com/NixOS/patchelf/pull/376
* Add comment explaining calculation for `DT_MIPS_RLD_MAP_REL` by @amjoseph-nixpkgs in https://github.com/NixOS/patchelf/pull/379
* Add `--no-sort option` by @amjoseph-nixpkgs in https://github.com/NixOS/patchelf/pull/378
* Handle `DT_MIPS_XHASH` and `.MIPS.xhash` by @amjoseph-nixpkgs in https://github.com/NixOS/patchelf/pull/380

## 0.14.5 (February 21, 2022)

* fix faulty version in 0.14.4

## 0.14.4 (February 21, 2022)

* Several test fixes to fix patchelf test suite on openbsd by @klemensn
* Allow multiple modifications in same call by @fzakaria in https://github.com/NixOS/patchelf/pull/361
* Add support to build with musl by @fzakaria in https://github.com/NixOS/patchelf/pull/362
* Fix typo: s/folllow/follow/ by @bjornfor in https://github.com/NixOS/patchelf/pull/366
* mips: fix incorrect polarity on `dyn_offset`; closes #364 by @a-m-joseph in https://github.com/NixOS/patchelf/pull/365

## 0.14.3 (December 05, 2021)

* this release adds support for static, pre-compiled patchelf binaries

## 0.14.2 (November 29, 2021)

* make version number in tarball easier to use for packagers

## 0.14.1 (November 28, 2021)

* build fix: add missing include

## 0.14 (November 27, 2021)

Changes compared to 0.13:

* Bug fixes:
  - Fix corrupted library names when using `--replace-needed` multiple times
  - Fix setting an empty rpath
  - Don't try to parse `.dynamic section` of type `NOBITS`
  - Fix use-after-free in `normalizeNoteSegments`
  - Correct `EINTR` handling in writeFile
  - MIPS: Adjust `PT_MIPS_ABIFLAGS` segment and `DT_MIPS_RLD_MAP_REL` dynamic section if present
  - Fix binaries without `.gnu.hash` section
* Support loongarch architecture
* Remove limits on output file size for elf files
* Allow reading rpath from file
* Requires now C++17 for building

## 0.13.1 (November 27, 2021)

* Bug fixes:
  - fix setting empty rpath
  - use memcpy instead of strcpy to set rpath
  - Don't try to parse `.dynamic section` of type `NOBITS`
  - fix use-after-free in `normalizeNoteSegments`
  - correct `EINTR` handling in writeFile
  - Adjust `PT_MIPS_ABIFLAGS` segment if present
  - Adjust `DT_MIPS_RLD_MAP_REL` dynamic section entry if present
  - fix binaries without `.gnu.hash` section

## 0.13 (August 5, 2021)

* New `--add-rpath` flag.

* Bug fixes.

## 0.12 (August 27, 2020)

* New `--clear-symbol-version` flag.

* Better support for relocating NOTE sections/segments.

* Improved the default section alignment choice.

* Bug fixes.

## 0.11 (June 9, 2020)

* New `--output` flag.

* Some bug fixes.

## 0.10 (March 28, 2019)

* Many bug fixes. Please refer to the Git commit log:

    https://github.com/NixOS/patchelf/commits/0.10

  This release has contributions from Adam Trhoň, Benjamin Hipple,
  Bernardo Ramos, Bjørn Forsman, Domen Kožar, Eelco Dolstra, Ezra
  Cooper, Felipe Sateler, Jakub Wilk, James Le Cuirot, Karl Millar,
  Linus Heckemann, Nathaniel J. Smith, Richard Purdie, Stanislav
  Markevich and Tuomas Tynkkynen.

## 0.9 (February 29, 2016)

* Lots of new features. Please refer to the Git commit log:

    https://github.com/NixOS/patchelf/commits/0.9

  This release has contributions from Aaron D. Marasco, Adrien
  Devresse, Alexandre Pretyman, Changli Gao, Chingis Dugarzhapov,
  darealshinji, David Sveningsson, Eelco Dolstra, Felipe Sateler,
  Jeremy Sanders, Jonas Kuemmerlin, Thomas Tuegel, Tuomas Tynkkynen,
  Vincent Danjean and Vladimír Čunát.

## 0.8 (January 15, 2014)

* Fix a segfault caused by certain illegal entries in symbol tables.

## 0.7 (January 7, 2014)

* Rewrite section indices in symbol tables. This for instance allows
  gdb to show proper backtraces.

* Added `--remove-needed` option.

## 0.6 (November 7, 2011)

* Hacky support for executables created by the Gold linker.

* Support segments with an alignment of 0 (contributed by Zack
  Weinberg).

* Added a manual page (contributed by Jeremy Sanders
  <jeremy@jeremysanders.net>).

## 0.5 (November 4, 2009)

* Various bugfixes.

* `--force-rpath` now deletes the `DT_RUNPATH` if it is present.

## 0.4 (June 4, 2008)

* Support for growing the RPATH on dynamic libraries.

* IA-64 support (not tested) and related 64-bit fixes.

* FreeBSD support.

* `--set-rpath`, `--shrink-rpath` and `--print-rpath` now prefer
  `DT_RUNPATH` over `DT_RPATH`, which is obsolete.  When updating, if both
  are present, both are updated.  If only `DT_RPATH` is present, it is
  converted to `DT_RUNPATH` unless `--force-rpath` is specified.  If
  neither is present, a `DT_RUNPATH` is added unless `--force-rpath` is
  specified, in which case a `DT_RPATH` is added.

## 0.3 (May 24, 2007)

* Support for 64-bit ELF binaries (such as on `x86_64-linux`).

* Support for big-endian ELF binaries (such as on `powerpc-linux`).

* Various bugfixes.

## 0.2 (January 15, 2007)

* Provides a hack to get certain programs (such as the
  Belastingaangifte 2005) to work.

## 0.1 (October 11, 2005)

* Initial release.
