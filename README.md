# open-kernel

A minimal Rust x86_64 kernel that boots through GRUB using Multiboot2. It clears
the VGA text buffer and displays a short status message.

## Build a bootable ISO

Enter the Nix development shell, then run:

```sh
nix develop
scripts/build-iso
```

The ISO is written to `build/open-kernel.iso`.

## Run it

```sh
scripts/run
```

Additional QEMU options can be passed through, for example
`scripts/run -m 256M`.

The build compiles the kernel with `rustc` and the entry shim with `gcc`, then
uses GNU `ld`, GRUB's `grub-file` and `grub-mkrescue`, and `xorriso`. These are
provided by the development shell.
