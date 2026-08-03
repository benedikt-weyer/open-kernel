use crate::drivers::{Driver, DriverError};

const MAX_FILES: usize = 32;
const MAX_RAM_FILES: usize = 32;
const MAX_FILE_NAME: usize = 64;
const MAX_RAM_FILE_SIZE: usize = 4096;

#[derive(Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub data: &'static [u8],
}

pub trait FileSystem {
    fn open(&self, name: &str) -> Option<&'static [u8]>;
    fn files(&self) -> usize;
}
pub struct InitRamFs;
pub struct RamFs;

#[derive(Clone, Copy)]
pub enum RamFsError {
    AlreadyExists,
    NotFound,
    NameTooLong,
    FileTooLarge,
    NoSpace,
}

#[derive(Clone, Copy)]
struct RamFile {
    used: bool,
    name: [u8; MAX_FILE_NAME],
    name_length: usize,
    data: [u8; MAX_RAM_FILE_SIZE],
    length: usize,
}
impl RamFile {
    const EMPTY: Self = Self {
        used: false,
        name: [0; MAX_FILE_NAME],
        name_length: 0,
        data: [0; MAX_RAM_FILE_SIZE],
        length: 0,
    };
}

static mut FILES: [Option<File>; MAX_FILES] = [None; MAX_FILES];
static mut FILE_COUNT: usize = 0;
static mut RAM_FILES: [RamFile; MAX_RAM_FILES] = [RamFile::EMPTY; MAX_RAM_FILES];

impl FileSystem for InitRamFs {
    fn open(&self, name: &str) -> Option<&'static [u8]> {
        open(name)
    }
    fn files(&self) -> usize {
        file_count()
    }
}
impl FileSystem for RamFs {
    fn open(&self, name: &str) -> Option<&'static [u8]> {
        read(name)
    }
    fn files(&self) -> usize {
        ram_file_count()
    }
}
impl Driver for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
    }
    fn initialize(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}
pub fn register_boot_file(name: &'static str, data: &'static [u8]) -> bool {
    unsafe {
        let count = FILE_COUNT;
        if count == MAX_FILES {
            return false;
        }
        FILES[count] = Some(File { name, data });
        FILE_COUNT = count + 1;
    }
    true
}
pub fn open(name: &str) -> Option<&'static [u8]> {
    unsafe {
        for file in FILES[..FILE_COUNT].iter().flatten() {
            if names_equal(file.name.as_bytes(), name.as_bytes()) {
                return Some(file.data);
            }
        }
    }
    read(name)
}
pub fn file_count() -> usize {
    unsafe { FILE_COUNT }
}

pub fn create(name: &str) -> Result<(), RamFsError> {
    let name = validate_name(name)?;
    unsafe {
        let files = &raw mut RAM_FILES;
        if find_ram_file(name).is_some() {
            return Err(RamFsError::AlreadyExists);
        }
        for slot in 0..MAX_RAM_FILES {
            let file = &mut (*files)[slot];
            if !file.used {
                file.used = true;
                file.name_length = name.len();
                file.length = 0;
                for (index, byte) in name.iter().enumerate() {
                    file.name[index] = *byte;
                }
                return Ok(());
            }
        }
    }
    Err(RamFsError::NoSpace)
}

pub fn write(name: &str, data: &[u8]) -> Result<(), RamFsError> {
    let name = validate_name(name)?;
    if data.len() > MAX_RAM_FILE_SIZE {
        return Err(RamFsError::FileTooLarge);
    }
    unsafe {
        let Some(slot) = find_ram_file(name) else {
            return Err(RamFsError::NotFound);
        };
        let file = &mut (*(&raw mut RAM_FILES))[slot];
        for (index, byte) in data.iter().enumerate() {
            file.data[index] = *byte;
        }
        file.length = data.len();
    }
    Ok(())
}

pub fn delete(name: &str) -> Result<(), RamFsError> {
    let name = validate_name(name)?;
    unsafe {
        let Some(slot) = find_ram_file(name) else {
            return Err(RamFsError::NotFound);
        };
        (*(&raw mut RAM_FILES))[slot] = RamFile::EMPTY;
    }
    Ok(())
}

pub fn read(name: &str) -> Option<&'static [u8]> {
    let name = name.as_bytes();
    unsafe {
        let slot = find_ram_file(name)?;
        let file = &(*(&raw const RAM_FILES))[slot];
        Some(core::slice::from_raw_parts(file.data.as_ptr(), file.length))
    }
}

pub fn ram_file_count() -> usize {
    unsafe {
        (*(&raw const RAM_FILES))
            .iter()
            .filter(|file| file.used)
            .count()
    }
}

fn validate_name(name: &str) -> Result<&[u8], RamFsError> {
    if name.is_empty() || name.len() >= MAX_FILE_NAME {
        return Err(RamFsError::NameTooLong);
    }
    Ok(name.as_bytes())
}

unsafe fn find_ram_file(name: &[u8]) -> Option<usize> {
    let files = unsafe { &*(&raw const RAM_FILES) };
    for (index, file) in files.iter().enumerate() {
        if file.used
            && file.name_length == name.len()
            && names_equal(&file.name[..name.len()], name)
        {
            return Some(index);
        }
    }
    None
}

fn names_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    for index in 0..left.len() {
        if left[index] != right[index] {
            return false;
        }
    }
    true
}
