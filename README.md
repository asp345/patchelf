# PatchELF

PatchELF is a simple utility for modifying existing ELF executables and
libraries.  In particular, it can do the following:

* Change the dynamic loader ("ELF interpreter") of executables:

  ```console
  $ patchelf --set-interpreter /lib/my-ld-linux.so.2 my-program
  ```

* Change the `RPATH` of executables and libraries:

  ```console
  $ patchelf --set-rpath /opt/my-libs/lib:/other-libs my-program
  ```

* Shrink the `RPATH` of executables and libraries:

  ```console
  $ patchelf --shrink-rpath my-program
  ```

  This removes from the `RPATH` all directories that do not contain a
  library referenced by `DT_NEEDED` fields of the executable or library.
  For instance, if an executable references one library `libfoo.so`, has
  an RPATH `/lib:/usr/lib:/foo/lib`, and `libfoo.so` can only be found
  in `/foo/lib`, then the new `RPATH` will be `/foo/lib`.

  In addition, the `--allowed-rpath-prefixes` option can be used for
  further rpath tuning. For instance, if an executable has an `RPATH`
  `/tmp/build-foo/.libs:/foo/lib`, it is probably desirable to keep
  the `/foo/lib` reference instead of the `/tmp` entry. To accomplish
  that, use:

  ```console
  $ patchelf --shrink-rpath --allowed-rpath-prefixes /usr/lib:/foo/lib my-program
  ```

* Remove declared dependencies on dynamic libraries (`DT_NEEDED`
  entries):

  ```console
  $ patchelf --remove-needed libfoo.so.1 my-program
  ```

  This option can be given multiple times.

* Add a declared dependency on a dynamic library (`DT_NEEDED`):

  ```console
  $ patchelf --add-needed libfoo.so.1 my-program
  ```

  This option can be give multiple times.

* Replace a declared dependency on a dynamic library with another one
  (`DT_NEEDED`):

  ```console
  $ patchelf --replace-needed liboriginal.so.1 libreplacement.so.1 my-program
  ```

  This option can be give multiple times.

* Change `SONAME` of a dynamic library:

  ```console
  $ patchelf --set-soname libnewname.so.3.4.5 path/to/libmylibrary.so.1.2.3
  ```

* Pre-resolve each `DT_NEEDED` dependency against the run path and record the
  result in a `.note.nixos.ldcache` note, so a loader that understands the note
  can skip the run-path search at startup:

  ```console
  $ patchelf --build-resolution-cache my-program
  ```


## How it works

patchelf is written in Rust and has no dependencies beyond the standard
library. Each file goes through three stages:

1. **Load** (`src/image.rs`): the file is parsed into an editable model.
   Everything is found the way the dynamic loader finds it, through the
   program headers and the dynamic section. Section headers are kept
   consistent but are never needed.
2. **Passes** (`src/passes/`): each command-line feature is a separate pass
   that edits only the model. Passes run in a fixed order.
3. **Layout** (`src/layout.rs`): once, after all passes, patchelf decides
   where everything goes. Code and data never move. Tables that grew
   (interpreter, dynamic section, string table, ...) move into one new
   segment at the end of the file, and a later run rebuilds that segment
   instead of adding another. The program header table stays right after the
   ELF header, as tools such as `strip` expect; when it needs more entries,
   the loader metadata that follows it is moved out of the way.

Before anything is written, the result is checked (`src/validate.rs`) for
overlapping segments, misplaced headers, dangling dynamic entries and
similar problems. If the edit introduced any, the file is left untouched.

Old copies of moved tables are zeroed, so stale paths don't stay in the file
(Nix scans files for store paths). `--no-clobber-old-sections` turns that off.

If the program header table itself ever has to move, Linux 5.18 or newer
(or a recent qemu-user) is needed to run the result; patchelf places it so
that older loaders find it too when that costs little padding.

## Compiling and Testing

```console
cargo build --release
cargo test
make -C tests check
sudo install -m755 target/release/patchelf /usr/local/bin/
```

`make -C tests check` builds small C programs with the system compiler and
runs the shell tests in `tests/` against `target/release/patchelf`
(override with `PATCHELF=...`; run a subset with `TESTS="soname.sh ..."`).
`tests/cross-arch.sh` also runs patched programs for other architectures under
qemu-user when cross compilers (e.g. `aarch64-linux-gnu-gcc`) and qemu are
installed.

`cargo test` includes a robustness test that feeds randomly damaged ELF
files through every pass. `PATCHELF_ROBUSTNESS_ITERS`,
`PATCHELF_ROBUSTNESS_SEED` and `PATCHELF_ROBUSTNESS_DIR` make it run longer,
with another seed, or on more files.

### Via Nix

1. `nix build` produces `./result/bin/patchelf` and runs the test suite.
   `nix build .#patchelf-musl` and `.#patchelf-static` build musl and static
   binaries, and `.#patchelf-win64` cross-compiles for Windows.

2. `nix develop` opens a shell with the Rust toolchain and the test
   dependencies.

## Help and resources

- Matrix: [#patchelf:nixos.org](https://matrix.to/#/#patchelf:nixos.org)

## Author

Copyright 2004-2019 Eelco Dolstra <edolstra@gmail.com>.

## License

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or (at
your option) any later version.

This program is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <http://www.gnu.org/licenses/>.
