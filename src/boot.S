.section .multiboot, "a"
.align 8

# Multiboot2 header: magic, architecture, header length, checksum, end tag.
multiboot_header_start:
.long 0xE85250D6
.long 0
.long multiboot_header_end - multiboot_header_start
.long -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header_start))
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
p2_table:
.skip 4096

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

    # Identity-map the first GiB with 2 MiB pages, then enable long mode.
    mov $p4_table, %edi
    xor %eax, %eax
    mov $3072, %ecx
    rep stosl

    mov $p3_table, %eax
    or $0x3, %eax
    mov %eax, p4_table

    mov $p2_table, %eax
    or $0x3, %eax
    mov %eax, p3_table

    mov $p2_table, %edi
    mov $0x83, %eax
    mov $512, %ecx
1:
    mov %eax, (%edi)
    add $8, %edi
    add $0x200000, %eax
    loop 1b

    mov $p4_table, %eax
    mov %eax, %cr3

    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4

    mov $0xC0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr

    mov %cr0, %eax
    or $0x80000000, %eax
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
