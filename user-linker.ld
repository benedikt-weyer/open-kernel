OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
  tls PT_TLS FLAGS(4);
}

SECTIONS
{
  . = 0x0000000040000000;

  .text : { *(.text*) } :text
  . = ALIGN(4K);
  .rodata : { *(.rodata*) } :rodata
  . = ALIGN(4K);
  .lbss : { *(.lbss .lbss.*) } :data
  .data : { *(.data*) *(.bss*) *(.lbss*) *(COMMON) } :data
  .tdata : { *(.tdata*) *(.tbss*) } :data :tls

  /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
