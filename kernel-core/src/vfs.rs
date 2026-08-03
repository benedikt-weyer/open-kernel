use crate::{FileSystem, InitRamFs};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    AlreadyMounted,
    InvalidMountPoint,
    NotMounted,
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
    if relative_path.is_empty() || relative_path.contains('/') {
        return None;
    }
    unsafe {
        let mount = (*(&raw const ROOT_MOUNT)).as_ref()?;
        if mount.mount_point != "/" {
            return None;
        }
        mount.file_system.open(relative_path)
    }
}

pub fn mounted() -> bool {
    unsafe { (*(&raw const ROOT_MOUNT)).is_some() }
}

pub fn mount_count() -> usize {
    usize::from(mounted())
}
