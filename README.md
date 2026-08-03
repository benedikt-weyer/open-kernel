# open-kernel

A minimal Rust x86_64 kernel that boots through GRUB using Multiboot2. It uses
the `multiboot2` crate to validate GRUB's boot information, clears the VGA text
buffer, and displays a short status message.

The workspace separates bootloader adapters from the shared kernel:
`kernel-core` owns the bootloader-neutral kernel entry, display, serial, and
halt logic. `multiboot2-adapter` translates Multiboot2 data, while
`limine-adapter` translates Limine protocol data. `multiboot2-adapter-bootstrap`
contains the 32-bit Multiboot
entry code and long-mode transition needed only by the GRUB path.

## Build a bootable ISO

Enter the Nix development shell, then run:

```sh
nix develop
scripts/build-iso
```

The ISO is written to `build/open-kernel.iso`.

## Build a Limine ISO

```sh
nix develop
scripts/build-limine-iso
```

The Limine protocol boots `limine-adapter` directly in 64-bit long mode. Its ISO
is written to `build/open-kernel-limine.iso`.

## Run it

```sh
scripts/run
```

Additional QEMU options can be passed through, for example
`scripts/run -m 256M`.

The build uses Cargo to compile and link the kernel with GNU `ld`, then uses
GRUB's `grub-file` and
`grub-mkrescue`, and `xorriso`. These are provided by the development shell.
