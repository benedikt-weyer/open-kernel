OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
}

SECTIONS
{
  . = 0x0000400000000000;

  .text : { *(.text*) } :text
  . = ALIGN(4K);
  .rodata : { *(.rodata*) } :rodata
  . = ALIGN(4K);
  .data : { *(.data*) *(.bss*) *(.lbss*) *(.tdata*) *(.tbss*) *(COMMON) } :data

  /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
