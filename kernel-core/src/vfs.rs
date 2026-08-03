use crate::{
    FileSystem, InitRamFs, RamFsError,
};
use crate::storage::{
    boot_file_name, create as create_ram_file, ram_file_name, read, write_at as write_ram_file_at,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    AlreadyMounted,
    InvalidMountPoint,
    NotMounted,
    NotFound,
    ReadOnly,
    InvalidPath,
    NameTooLong,
    NoSpace,
}

#[derive(Clone, Copy)]
struct Mount {
    mount_point: &'static str,
    file_system: &'static dyn FileSystem,
}

static INITRAMFS: InitRamFs = InitRamFs;
static mut ROOT_MOUNT: Option<Mount> = None;

pub fn mount_initramfs() -> Result<(), VfsError> {
    unsafe {
        if (*(&raw const ROOT_MOUNT)).is_some() {
            return Err(VfsError::AlreadyMounted);
        }
        (*(&raw mut ROOT_MOUNT)) = Some(Mount {
            mount_point: "/",
            file_system: &INITRAMFS,
        });
    }
    Ok(())
}

pub fn open(path: &str) -> Option<&'static [u8]> {
    let relative_path = path.strip_prefix('/')?;
    unsafe {
        let mount = (*(&raw const ROOT_MOUNT)).as_ref()?;
        if mount.mount_point != "/" {
            return None;
        }
        if let Some(name) = relative_path.strip_prefix("tmp/") {
            if name.is_empty() || name.contains('/') {
                return None;
            }
            return read(name);
        }
        if relative_path.is_empty() || relative_path.contains('/') {
            return None;
        }
        mount.file_system.open(relative_path)
    }
}

pub fn open_file(path: &str, writable: bool, create: bool) -> Result<(), VfsError> {
    if path == "/" || path == "/tmp" {
        return Err(VfsError::InvalidPath);
    }
    if let Some(name) = path.strip_prefix("/tmp/") {
        if name.is_empty() || name.contains('/') {
            return Err(VfsError::InvalidPath);
        }
        if read(name).is_none() {
            if !create {
                return Err(VfsError::NotFound);
            }
            create_ram_file(name).map_err(ram_error)?;
        }
        return Ok(());
    }
    if writable || create {
        return Err(VfsError::ReadOnly);
    }
    open(path).map(|_| ()).ok_or(VfsError::NotFound)
}

pub fn write(path: &str, data: &[u8]) -> Result<(), VfsError> {
    write_at(path, 0, data)
}

pub fn write_at(path: &str, offset: usize, data: &[u8]) -> Result<(), VfsError> {
    let Some(name) = path.strip_prefix("/tmp/") else {
        return Err(VfsError::ReadOnly);
    };
    write_ram_file_at(name, offset, data).map_err(ram_error)
}

pub fn directory_entry(path: &str, index: usize, output: &mut [u8]) -> Option<usize> {
    match path {
        "/" => {
            if index == 0 {
                copy_name(output, b"tmp")
            } else {
                let name = boot_file_name(index - 1)?;
                copy_name(output, name.as_bytes())
            }
        }
        "/tmp" => ram_file_name(index, output),
        _ => None,
    }
}

pub fn mounted() -> bool {
    unsafe { (*(&raw const ROOT_MOUNT)).is_some() }
}

pub fn mount_count() -> usize {
    usize::from(mounted())
}

fn copy_name(output: &mut [u8], name: &[u8]) -> Option<usize> {
    if output.len() < name.len() {
        return None;
    }
    for index in 0..name.len() {
        unsafe {
            core::ptr::write_volatile(output.as_mut_ptr().add(index), name[index]);
        }
    }
    Some(name.len())
}

fn ram_error(error: RamFsError) -> VfsError {
    match error {
        RamFsError::NotFound => VfsError::NotFound,
        RamFsError::NameTooLong => VfsError::NameTooLong,
        RamFsError::NoSpace | RamFsError::FileTooLarge => VfsError::NoSpace,
        RamFsError::AlreadyExists => VfsError::InvalidPath,
    }
}
