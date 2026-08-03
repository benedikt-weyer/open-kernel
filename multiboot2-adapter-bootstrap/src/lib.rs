#![no_std]

use core::arch::global_asm;

#[unsafe(no_mangle)]
pub static BOOTSTRAP_LINK: u8 = 0;

global_asm!(
    r#"
.section .multiboot, "a"
.align 8

multiboot_header_start:
.long 0xE85250D6
.long 0
.long multiboot_header_end - multiboot_header_start
.long -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header_start))
# Request a 32-bit RGB framebuffer from GRUB.
.short 5
.short 0
.long 20
.long 1024
.long 768
.long 32
.align 8

.short 0
.short 0
.long 8

multiboot_header_end:

.section .bss
.align 4096
p4_table:
.skip 4096
p3_table:
.skip 4096
p2_tables:
.skip 16384
boot_magic:
.skip 4
boot_info:
.skip 4

.align 16
stack_bottom:
.skip 16384
stack_top:

.section .text
.code32
.global _start
.type _start, @function
.extern kernel_main
_start:
    cli
    mov $stack_top, %esp

    # Copy GRUB's handoff values before using any scratch registers.
    mov %eax, boot_magic
    mov %ebx, boot_info

    mov $p4_table, %edi
    xor %eax, %eax
    mov $3072, %ecx
    rep stosl

    mov $p3_table, %eax
    or $0x3, %eax
    mov %eax, p4_table

    mov $p2_tables, %eax
    or $0x3, %eax
    mov %eax, p3_table
    add $0x1000, %eax
    mov %eax, p3_table + 8
    add $0x1000, %eax
    mov %eax, p3_table + 16
    add $0x1000, %eax
    mov %eax, p3_table + 24

    # Identity map the first 4 GiB, including QEMU's framebuffer aperture.
    mov $p2_tables, %edi
    mov $0x83, %eax
    mov $2048, %ecx
1:
    mov %eax, (%edi)
    add $8, %edi
    add $0x200000, %eax
    loop 1b

    mov $p4_table, %eax
    mov %eax, %cr3

    mov %cr4, %eax
    or $0x620, %eax
    mov %eax, %cr4

    mov $0xC0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr

    mov %cr0, %eax
    and $0xFFFFFFFB, %eax
    or $0x80000002, %eax
    mov %eax, %cr0

    lgdt gdt64_pointer
    ljmp $0x08, $long_mode_start

.code64
long_mode_start:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $stack_top, %rsp
    mov boot_magic(%rip), %edi
    mov boot_info(%rip), %esi
    call kernel_main

halt:
    hlt
    jmp halt

.align 8
gdt64:
    .quad 0
    .quad 0x00AF9A000000FFFF
    .quad 0x00AF92000000FFFF
gdt64_end:

gdt64_pointer:
    .word gdt64_end - gdt64 - 1
    .long gdt64

.section .note.GNU-stack,"",@progbits
"#,
    options(att_syntax)
);
