use crate::{
    FUTURE_USER_SPACE_BASE, PAGE_SIZE, PageFlags, PagingError, allocate_physical_frame,
    allocate_user_stack_in, map_user_page_in,
    write_physical_frame, zero_physical_frame,
};

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const ELF_MACHINE_X86_64: u16 = 0x3E;
const ELF_TYPE_EXECUTABLE: u16 = 2;
const PROGRAM_HEADER_LOAD: u32 = 1;
const PROGRAM_HEADER_TLS: u32 = 7;
const PROGRAM_FLAG_EXECUTE: u32 = 1;
const PROGRAM_FLAG_WRITE: u32 = 2;
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;
const USER_TLS_BASE: u64 = 0x0000_3000_0000_0000;
const TLS_TCB_SIZE: u64 = 64;

#[derive(Clone, Copy)]
pub struct LoadedImage {
    pub entry: u64,
    pub stack_pointer: u64,
    pub tls: Option<TlsImage>,
    pub fs_base: u64,
}

#[derive(Clone, Copy)]
pub struct TlsImage {
    pub file_offset: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub align: u64,
}

#[derive(Clone, Copy)]
pub enum ElfError {
    InvalidHeader,
    UnsupportedExecutable,
    InvalidProgramHeader,
    InvalidSegment,
    NoExecutableSegment,
    Paging(PagingError),
}

impl From<PagingError> for ElfError {
    fn from(error: PagingError) -> Self {
        Self::Paging(error)
    }
}

pub fn load_user_elf(image: &[u8]) -> Result<LoadedImage, ElfError> {
    load_user_elf_into(image, crate::active_address_space(), 0)
}

/// Loads an ELF into an address space which need not be the active CR3.
pub fn load_user_elf_into(
    image: &[u8],
    address_space: u64,
    stack_slot: usize,
) -> Result<LoadedImage, ElfError> {
    if image.len() < ELF_HEADER_SIZE
        || image[..4] != *b"\x7FELF"
        || image[4] != 2
        || image[5] != 1
        || image[6] != 1
        || read_u16(image, 16)? != ELF_TYPE_EXECUTABLE
        || read_u16(image, 18)? != ELF_MACHINE_X86_64
        || read_u32(image, 20)? != 1
        || read_u16(image, 52)? != ELF_HEADER_SIZE as u16
        || read_u16(image, 54)? != PROGRAM_HEADER_SIZE as u16
    {
        return Err(ElfError::InvalidHeader);
    }

    let entry = read_u64(image, 24)?;
    let program_header_offset = read_u64(image, 32)? as usize;
    let program_header_count = read_u16(image, 56)? as usize;
    let table_size = program_header_count
        .checked_mul(PROGRAM_HEADER_SIZE)
        .ok_or(ElfError::InvalidHeader)?;
    let table_end = program_header_offset
        .checked_add(table_size)
        .ok_or(ElfError::InvalidHeader)?;
    if table_end > image.len() {
        return Err(ElfError::InvalidHeader);
    }

    let mut entry_is_executable = false;
    let mut tls = None;
    for index in 0..program_header_count {
        let header_offset = program_header_offset + index * PROGRAM_HEADER_SIZE;
        let header_type = read_u32(image, header_offset)?;
        if header_type == PROGRAM_HEADER_TLS {
            if tls.is_some() {
                return Err(ElfError::InvalidProgramHeader);
            }
            let file_offset = read_u64(image, header_offset + 8)?;
            let file_size = read_u64(image, header_offset + 32)?;
            let memory_size = read_u64(image, header_offset + 40)?;
            let align = read_u64(image, header_offset + 48)?;
            if memory_size < file_size
                || file_offset.checked_add(file_size).is_none_or(|end| end > image.len() as u64)
                || (align != 0 && !align.is_power_of_two())
            {
                return Err(ElfError::InvalidProgramHeader);
            }
            tls = Some(TlsImage { file_offset, file_size, memory_size, align });
            continue;
        }
        if header_type != PROGRAM_HEADER_LOAD {
            continue;
        }
        let flags = read_u32(image, header_offset + 4)?;
        let file_offset = read_u64(image, header_offset + 8)?;
        let virtual_address = read_u64(image, header_offset + 16)?;
        let file_size = read_u64(image, header_offset + 32)?;
        let memory_size = read_u64(image, header_offset + 40)?;
        if memory_size < file_size {
            return Err(ElfError::InvalidSegment);
        }
        let file_end = file_offset
            .checked_add(file_size)
            .ok_or(ElfError::InvalidSegment)?;
        let memory_end = virtual_address
            .checked_add(memory_size)
            .ok_or(ElfError::InvalidSegment)?;
        if memory_size == 0
            || file_end > image.len() as u64
            || virtual_address < FUTURE_USER_SPACE_BASE
            || memory_end > USER_SPACE_END
        {
            return Err(ElfError::InvalidSegment);
        }
        if flags & PROGRAM_FLAG_EXECUTE != 0 && entry >= virtual_address && entry < memory_end {
            entry_is_executable = true;
        }

        let page_flags = match (flags & PROGRAM_FLAG_WRITE != 0, flags & PROGRAM_FLAG_EXECUTE != 0)
        {
            (false, false) => PageFlags::UserReadOnly,
            (true, false) => PageFlags::UserReadWrite,
            (false, true) => PageFlags::UserReadExecute,
            (true, true) => PageFlags::UserReadWriteExecute,
        };
        let first_page = virtual_address & !(PAGE_SIZE - 1);
        let last_page = (memory_end - 1) & !(PAGE_SIZE - 1);
        let mut page = first_page;
        loop {
            let frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
            zero_physical_frame(frame);
            map_user_page_in(address_space, page, frame, page_flags)?;

            let copy_start = page.max(virtual_address);
            let copy_end = (page + PAGE_SIZE).min(virtual_address + file_size);
            if copy_start < copy_end {
                let source_offset = file_offset + copy_start - virtual_address;
                let byte_count = (copy_end - copy_start) as usize;
                write_physical_frame(
                    frame + copy_start - page,
                    &image[source_offset as usize..source_offset as usize + byte_count],
                );
            }
            if page == last_page {
                break;
            }
            page += PAGE_SIZE;
        }
    }

    if !entry_is_executable {
        return Err(ElfError::NoExecutableSegment);
    }
    let stack_top = allocate_user_stack_in(address_space, stack_slot)?;
    let fs_base = if let Some(tls_image) = tls {
        load_initial_tls(image, address_space, stack_slot, tls_image)?
    } else {
        0
    };
    Ok(LoadedImage {
        entry,
        // `_start` is a Rust function entry point, so emulate a normal call:
        // the System V ABI requires RSP to be 8 modulo 16 on entry.
        stack_pointer: stack_top - 8,
        tls,
        fs_base,
    })
}

fn load_initial_tls(
    image: &[u8],
    address_space: u64,
    slot: usize,
    tls: TlsImage,
) -> Result<u64, ElfError> {
    let align = tls.align.max(16);
    let tls_size = tls.memory_size.max(1);
    let block_size = tls_size.checked_add(align - 1).ok_or(ElfError::InvalidProgramHeader)? & !(align - 1);
    let base = USER_TLS_BASE + (slot as u64) * 0x1_0000;
    let fs_base = base.checked_add(block_size).ok_or(ElfError::InvalidProgramHeader)?;
    let end = fs_base.checked_add(TLS_TCB_SIZE).ok_or(ElfError::InvalidProgramHeader)?;
    let first_page = base & !(PAGE_SIZE - 1);
    let last_page = (end - 1) & !(PAGE_SIZE - 1);
    let mut page = first_page;
    loop {
        let frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
        zero_physical_frame(frame);
        map_user_page_in(address_space, page, frame, PageFlags::UserReadWrite)?;
        let copy_start = page.max(base);
        let copy_end = (page + PAGE_SIZE).min(base + tls.file_size);
        if copy_start < copy_end {
            let offset = (copy_start - base) as usize;
            let source = tls.file_offset as usize + offset;
            write_physical_frame(frame + copy_start - page, &image[source..source + (copy_end - copy_start) as usize]);
        }
        if page == last_page { break; }
        page += PAGE_SIZE;
    }
    // Variant II TCB: FS points immediately after static TLS and FS:0 is self.
    let tcb_page = fs_base & !(PAGE_SIZE - 1);
    let tcb_offset = fs_base - tcb_page;
    // The TCB lies in a page just mapped above; resolve it by temporarily using
    // the direct physical map through the page-table walk is unnecessary here:
    // write after activation in the process setup path.
    let _ = tcb_offset;
    Ok(fs_base)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    let data = bytes.get(offset..offset + 2).ok_or(ElfError::InvalidHeader)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    let data = bytes.get(offset..offset + 4).ok_or(ElfError::InvalidHeader)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfError> {
    let data = bytes.get(offset..offset + 8).ok_or(ElfError::InvalidHeader)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}
