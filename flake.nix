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
            # The prebuilt OpenKernel stage1 rustc (downloaded as a raw
            # tarball in CI, not built by Nix) is dynamically linked
            # against libz.so.1. Nix's sandboxed environment has no FHS
            # library paths, so without this its loader can't find zlib
            # even though it's present via the `zlib` package above.
            shellHook = ''
              export LD_LIBRARY_PATH="${pkgs.zlib}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
            '';
          };
        });
    };
}
