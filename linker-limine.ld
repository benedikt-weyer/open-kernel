OUTPUT_FORMAT(elf64-x86-64)
ENTRY(limine_entry)

PHDRS
{
  limine_requests PT_LOAD FLAGS(6);
  text PT_LOAD FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
}

SECTIONS
{
  . = 0xffffffff80000000;

  .limine_requests_start ALIGN(8) :
  {
    KEEP(*(.limine_requests_start))
  } :limine_requests

  .limine_requests ALIGN(8) :
  {
    KEEP(*(.limine_requests))
  } :limine_requests

  .limine_requests_end ALIGN(8) :
  {
    KEEP(*(.limine_requests_end))
  } :limine_requests

  . = ALIGN(4K);
  .text : { *(.text*) } :text
  . = ALIGN(4K);
  .rodata : { *(.rodata*) } :rodata
  . = ALIGN(4K);
  .data : { *(.data*) } :data
  . = ALIGN(4K);
  .bss :
  {
    *(COMMON)
    *(.bss*)
  } :data

  /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
