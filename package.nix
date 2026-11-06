{
  lib,
  stdenv,
  rustPlatform,
  installShellFiles,
  version,
  src,
}:

rustPlatform.buildRustPackage {
  pname = "patchelf";
  inherit version src;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ installShellFiles ];

  postInstall = ''
    installManPage patchelf.1
    installShellCompletion --zsh completions/zsh/_patchelf
  '';

  # The shell test suite builds native programs and runs them, so it only
  # works when the build machine can run the result.
  doInstallCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform && !stdenv.hostPlatform.isWindows;
  installCheckPhase = ''
    runHook preInstallCheck
    make -C tests check PATCHELF=$out/bin/patchelf
    runHook postInstallCheck
  '';

  meta = {
    homepage = "https://github.com/NixOS/patchelf";
    description = "Small utility to modify the dynamic linker and RPATH of ELF executables";
    license = lib.licenses.gpl3Plus;
    mainProgram = "patchelf";
    platforms = lib.platforms.all;
  };
}
