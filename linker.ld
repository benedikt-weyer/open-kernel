OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

SECTIONS
{
  . = 1M;

  .multiboot ALIGN(8) :
  {
    KEEP(*(.multiboot))
  }

  . = ALIGN(4K);
  .text : { *(.text*) }
  . = ALIGN(4K);
  .rodata : { *(.rodata*) }
  . = ALIGN(4K);
  .data : { *(.data*) }
  . = ALIGN(4K);
  .bss :
  {
    *(COMMON)
    *(.bss*)
  }

  /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
