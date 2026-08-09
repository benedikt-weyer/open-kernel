{
  description = "Development environment for a minimal Multiboot2 kernel";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          limineIso = pkgs.limine.overrideAttrs (old: {
            configureFlags = old.configureFlags ++ [
              "--enable-bios-cd"
              "--enable-uefi-cd"
            ];
            nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.mtools ];
          });
          # The prebuilt OpenKernel stage1 rustc (downloaded as a raw
          # tarball in CI, not built by Nix) is dynamically linked against
          # libz.so.1 and libstdc++.so.6. Nix's sandboxed environment has
          # no FHS library paths, so without these its loader can't find
          # them even though they're present via Nix packages.
          runtimeLibraryPath = pkgs.lib.makeLibraryPath [
            pkgs.stdenv.cc.cc.lib
            pkgs.zlib
          ];
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              binutils
              cargo
              grub2
              limineIso
              rustc
              xorriso
              zlib
            ];
            shellHook = ''
              export LD_LIBRARY_PATH="${runtimeLibraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
            '';
          };
        });
    };
}
