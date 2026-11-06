{
  description = "A tool for modifying ELF executables and libraries";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  # dev tooling
  inputs.flake-parts.url = "github:hercules-ci/flake-parts";
  inputs.git-hooks-nix.url = "github:cachix/git-hooks.nix";
  # work around https://github.com/NixOS/nix/issues/7730
  inputs.flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
  inputs.git-hooks-nix.inputs.nixpkgs.follows = "nixpkgs";
  inputs.git-hooks-nix.inputs.nixpkgs-stable.follows = "nixpkgs";
  # work around 7730 and https://github.com/NixOS/nix/issues/7807
  inputs.git-hooks-nix.inputs.flake-compat.follows = "";
  inputs.git-hooks-nix.inputs.gitignore.follows = "";

  outputs =
    inputs@{ self, nixpkgs, ... }:

    let
      inherit (nixpkgs) lib;

      supportedSystems = [
        "x86_64-linux"
        "i686-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs supportedSystems;

      version = (lib.importTOML ./Cargo.toml).package.version;
      pkgs = nixpkgs.legacyPackages.x86_64-linux;

      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./COPYING
          ./Cargo.lock
          ./Cargo.toml
          ./README.md
          ./completions
          ./patchelf.1
          ./src
          ./tests
        ];
      };

      patchelfFor = pkgs: pkgs.callPackage ./package.nix { inherit version src; };

      # We don't apply flake-parts to the whole flake so that non-development attributes
      # load without fetching any development inputs.
      devFlake = inputs.flake-parts.lib.mkFlake { inherit inputs; } {
        imports = [ ./maintainers/flake-module.nix ];
        systems = supportedSystems;
        perSystem =
          { system, ... }:
          {
            _module.args.pkgs = nixpkgs.legacyPackages.${system};
          };
      };

    in

    {
      overlays.default = final: prev: {
        patchelf-new-musl = patchelfFor final.pkgsMusl;
        patchelf-new = patchelfFor final;
      };

      hydraJobs = {
        tarball = pkgs.runCommand "patchelf-tarball-${version}" { } ''
          mkdir -p $out/tarballs $out/nix-support
          cp -r ${src} patchelf-${version}
          chmod -R u+w patchelf-${version}
          tar -czf $out/tarballs/patchelf-${version}.tar.gz patchelf-${version}
          tar -cjf $out/tarballs/patchelf-${version}.tar.bz2 patchelf-${version}
          cp ${./README.md} $out/README.md
          for f in $out/tarballs/*; do echo "file source-dist $f" >> $out/nix-support/hydra-build-products; done
          echo "doc readme $out/README.md" >> $out/nix-support/hydra-build-products
        '';

        build = forAllSystems (system: self.packages.${system}.patchelf);

        # To get the mingw compiler from the hydra cache
        inherit (self.packages.x86_64-linux) patchelf-win64;

        release = pkgs.releaseTools.aggregate {
          name = "patchelf-${version}";
          constituents = [
            self.hydraJobs.tarball
            self.hydraJobs.build.x86_64-linux
            self.hydraJobs.build.i686-linux
            # FIXME: add aarch64 emulation to our github action...
            #self.hydraJobs.build.aarch64-linux
          ];
          meta.description = "Release-critical builds";
        };
      };

      checks = forAllSystems (
        system:
        {
          build = self.hydraJobs.build.${system};
        }
        // devFlake.checks.${system} or { }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          modular = devFlake.getSystem pkgs.stdenv.buildPlatform.system;
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.patchelf ];
            packages = [
              pkgs.clippy
              pkgs.rustfmt
              pkgs.lld
              pkgs.mold
              modular.pre-commit.settings.package
              (pkgs.writeScriptBin "pre-commit-hooks-install" modular.pre-commit.settings.installationScript)
            ];
            env._NIX_PRE_COMMIT_HOOKS_CONFIG = "${(pkgs.formats.yaml { }).generate "pre-commit-config.yaml"
              modular.pre-commit.settings.rawConfig
            }";
          };
        }
      );

      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          patchelf = patchelfFor pkgs;
          default = self.packages.${system}.patchelf;

          # Statically linked, for use outside Nix.
          patchelf-static = patchelfFor pkgs.pkgsStatic;

          # Tests whether the package cross-compiles.
          patchelf-musl-cross = patchelfFor pkgs.pkgsCross.musl64;
          patchelf-win64 = patchelfFor pkgs.pkgsCross.mingwW64;
        }
        // lib.optionalAttrs (system != "i686-linux") {
          patchelf-musl = patchelfFor pkgs.pkgsMusl;
        }
      );

    };
}
