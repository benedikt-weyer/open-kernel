//! Direct OpenKernel syscalls for operations `std` does not yet cover:
//! console control, raw keyboard polling, raw thread create/join, block
//! device access, process spawn/wait, and the VFS/cwd calls that stand in
//! for `std::fs` until it is implemented for this target.

use core::arch::asm;

pub const OPEN_WRITE: u64 = 1;
pub const OPEN_CREATE: u64 = 2;
pub const OPEN_DIRECTORY: u64 = 4;

pub fn clear_screen() {
    syscall0(7);
}

pub fn erase_char() {
    syscall0(8);
}

pub fn shutdown() -> ! {
    syscall0(9);
    hang()
}

pub fn process_exit() -> ! {
    syscall0(3);
    hang()
}

pub fn sata_status() {
    syscall0(10);
}

pub fn sata_identify() {
    syscall0(11);
}

pub fn sata_read() {
    syscall0(12);
}

pub fn pci_status() {
    syscall0(13);
}

pub fn lsblk() {
    syscall0(14);
}

/// Polls for a pending keypress without blocking; the kernel returns `0`
/// when no key is ready.
pub fn poll_key() -> Option<u8> {
    let key = syscall0(6) as u8;
    (key != 0).then_some(key)
}

pub fn sleep_ms(milliseconds: u64) -> bool {
    syscall1(5, milliseconds) == 0
}

pub fn yield_now() {
    syscall0(2);
}

pub fn now_ms() -> u64 {
    syscall0(27)
}

/// Spawns a bare function on a new kernel thread. `entry` must call
/// [`exit_thread`] itself; `std::thread` cannot be used here because this
/// target has no `sys::thread` implementation yet.
pub fn spawn_thread(entry: extern "C" fn(u64) -> !, argument: u64, tls_base: u64) -> Option<u64> {
    let id = syscall3(15, entry as *const () as u64, argument, tls_base);
    (id != u64::MAX).then_some(id)
}

pub fn exit_thread(status: u64) -> ! {
    syscall1(16, status);
    hang()
}

pub fn join_thread(id: u64) -> Option<u64> {
    let status = syscall1(17, id);
    (status != u64::MAX).then_some(status)
}

pub fn spawn_process(path: &str) -> Option<u64> {
    let id = syscall3(33, path.as_ptr() as u64, path.len() as u64, 0);
    (id != u64::MAX).then_some(id)
}

pub fn wait_process(id: u64) -> Option<u64> {
    let status = syscall1(34, id);
    (status != u64::MAX).then_some(status)
}

pub fn vfs_open(path: &str, flags: u64) -> Option<u64> {
    let fd = syscall3(19, path.as_ptr() as u64, path.len() as u64, flags);
    (fd != u64::MAX).then_some(fd)
}

pub fn vfs_read(fd: u64, buffer: &mut [u8]) -> Option<usize> {
    let count = syscall3(20, fd, buffer.as_mut_ptr() as u64, buffer.len() as u64);
    (count != u64::MAX).then_some(count as usize)
}

pub fn vfs_write(fd: u64, bytes: &[u8]) -> Option<usize> {
    let count = syscall3(21, fd, bytes.as_ptr() as u64, bytes.len() as u64);
    (count != u64::MAX).then_some(count as usize)
}

pub fn vfs_close(fd: u64) -> bool {
    syscall1(22, fd) == 0
}

pub fn vfs_seek(fd: u64, offset: i64, whence: u64) -> bool {
    syscall3(23, fd, offset as u64, whence) == 0
}

pub fn set_cwd(path: &str) -> bool {
    syscall2(24, path.as_ptr() as u64, path.len() as u64) == 0
}

pub fn get_cwd(buffer: &mut [u8]) -> Option<usize> {
    let count = syscall2(25, buffer.as_mut_ptr() as u64, buffer.len() as u64);
    (count != u64::MAX).then_some(count as usize)
}

pub fn executable_path(buffer: &mut [u8]) -> Option<usize> {
    let count = syscall2(26, buffer.as_mut_ptr() as u64, buffer.len() as u64);
    (count != u64::MAX).then_some(count as usize)
}

fn hang() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn syscall0(number: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            clobber_abi("sysv64"),
        );
    }
    result
}

fn syscall1(number: u64, first: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") first,
            clobber_abi("sysv64"),
        );
    }
    result
}

fn syscall2(number: u64, first: u64, second: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") first,
            in("rsi") second,
            clobber_abi("sysv64"),
        );
    }
    result
}

fn syscall3(number: u64, first: u64, second: u64, third: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") first,
            in("rsi") second,
            in("rdx") third,
            clobber_abi("sysv64"),
        );
    }
    result
}
