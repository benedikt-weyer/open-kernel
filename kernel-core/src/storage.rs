const MAX_FILES: usize = 32;

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
static mut FILES: [Option<File>; MAX_FILES] = [None; MAX_FILES];
static mut FILE_COUNT: usize = 0;

impl FileSystem for InitRamFs {
    fn open(&self, name: &str) -> Option<&'static [u8]> {
        open(name)
    }
    fn files(&self) -> usize {
        file_count()
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
            if file.name == name {
                return Some(file.data);
            }
        }
    }
    None
}
pub fn file_count() -> usize {
    unsafe { FILE_COUNT }
}
