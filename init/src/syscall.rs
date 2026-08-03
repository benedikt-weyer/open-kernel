//! Direct OpenKernel syscalls PID 1 needs and that `std` does not (yet)
//! cover: process spawn/wait/terminate, the reap-any-child primitive,
//! the shutdown/reboot power syscalls and the mailbox other processes use
//! to request them, and the handful of VFS/device calls used for the
//! "prepare filesystem" and "discover devices" boot phases.

use core::arch::asm;

pub const OPEN_DIRECTORY: u64 = 4;

pub const POWER_EVENT_NONE: u64 = 0;
pub const POWER_EVENT_REBOOT: u64 = 2;

pub fn clear_screen() {
    syscall0(7);
}

pub fn sata_status() {
    syscall0(10);
}

pub fn pci_status() {
    syscall0(13);
}

pub fn lsblk() {
    syscall0(14);
}

pub fn sleep_ms(milliseconds: u64) -> bool {
    syscall1(5, milliseconds) == 0
}

/// Spawns `path` with up to 4 arguments. Each argument is copied into a
/// small NUL-terminated stack buffer, since the spawn syscall reads argv
/// as an array of pointers to NUL-terminated strings.
pub fn spawn_process(path: &str, argv: &[&str]) -> Option<u64> {
    let mut buffers = [[0_u8; 32]; 4];
    let mut pointers = [0_u64; 4];
    for (index, argument) in argv.iter().take(4).enumerate() {
        let bytes = argument.as_bytes();
        let length = bytes.len().min(buffers[index].len() - 1);
        buffers[index][..length].copy_from_slice(&bytes[..length]);
        pointers[index] = buffers[index].as_ptr() as u64;
    }
    let argv_pointer = if argv.is_empty() { 0 } else { pointers.as_ptr() as u64 };
    let id = syscall3(33, path.as_ptr() as u64, path.len() as u64, argv_pointer);
    (id != u64::MAX).then_some(id)
}

/// Blocks until `pid` exits. Used only for oneshot boot steps that later
/// services genuinely depend on having finished.
pub fn wait_process(pid: u64) -> Option<u64> {
    let status = syscall1(34, pid);
    (status != u64::MAX).then_some(status)
}

pub fn reboot() -> ! {
    syscall0(36);
    hang()
}

pub fn shutdown() -> ! {
    syscall0(9);
    hang()
}

/// Reads and clears the pending shutdown/reboot request, if any. Any
/// process can raise one via syscall 37; this is how PID 1 receives it.
pub fn poll_power_event() -> u64 {
    syscall0(38)
}

/// Ends one of init's own child processes. There is no general signal
/// delivery in this kernel, so this is the only way to stop a service that
/// hasn't exited on its own during shutdown.
pub fn terminate_process(pid: u64) -> bool {
    syscall1(39, pid) != 0
}

/// Reaps one exited child (a tracked service or an orphan reparented to
/// init), returning its pid and exit status. `None` if nothing has exited.
pub fn reap_any_child() -> Option<(u64, u64)> {
    let mut status = 0_u64;
    let pid = syscall1(40, (&raw mut status) as u64);
    (pid != u64::MAX).then_some((pid, status))
}

pub fn vfs_directory_exists(path: &str) -> bool {
    let fd = syscall3(19, path.as_ptr() as u64, path.len() as u64, OPEN_DIRECTORY);
    if fd == u64::MAX {
        return false;
    }
    syscall1(22, fd);
    true
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
