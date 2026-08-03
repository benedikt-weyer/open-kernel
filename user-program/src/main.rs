#![no_std]
#![no_main]
// The writable .data/.bss ELF segment holds allocator state.

extern crate alloc;
mod sync;

use alloc::vec::Vec;
use core::{
    alloc::{GlobalAlloc, Layout},
    arch::asm,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};
use sync::{UserCondvar, UserMutex, UserThread};
use core::panic::PanicInfo;

static BANNER: [u8; 37] = *b"OPEN KERNEL USER CONSOLE\r\nTYPE HELP\r\n";
static PROMPT: [u8; 2] = *b"> ";
static HELP: &[u8] = b"COMMANDS: HELP CLEAR EXIT SHUTDOWN SATA IDENTIFY READ PCI LSBLK THREADS HEAP VFS ENV TIME SYNC\r\n";
static UNKNOWN: [u8; 17] = *b"UNKNOWN COMMAND\r\n";
static EXITING: [u8; 9] = *b"GOODBYE\r\n";
const COMMANDS: [&[u8]; 15] = [
    b"help", b"clear", b"exit", b"shutdown", b"sata", b"identify", b"read", b"pci", b"lsblk",
    b"threads",
    b"heap",
    b"vfs",
    b"env",
    b"time",
    b"sync",
];

struct BrkAllocator {
    locked: AtomicBool,
}

impl BrkAllocator {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    fn lock(&self) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

static mut HEAP_NEXT: usize = 0;

unsafe impl GlobalAlloc for BrkAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.lock();
        let heap_start = if unsafe { HEAP_NEXT } == 0 {
            syscall1(18, 0) as usize
        } else {
            unsafe { HEAP_NEXT }
        };
        let aligned_start = match heap_start.checked_add(layout.align() - 1) {
            Some(value) => value & !(layout.align() - 1),
            None => {
                self.unlock();
                return core::ptr::null_mut();
            }
        };
        let Some(new_break) = aligned_start.checked_add(layout.size().max(1)) else {
            self.unlock();
            return core::ptr::null_mut();
        };
        if syscall1(18, new_break as u64) == u64::MAX {
            self.unlock();
            return core::ptr::null_mut();
        }
        unsafe {
            HEAP_NEXT = new_break;
        }
        self.unlock();
        aligned_start as *mut u8
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BrkAllocator = BrkAllocator::new();
static SYNC_MUTEX: UserMutex = UserMutex::new();
static SYNC_CONDITION: UserCondvar = UserCondvar::new();
static SYNC_VALUE: AtomicU32 = AtomicU32::new(0);
static TLS_WORD: u64 = 0x544C_535F_4F4B_0001;

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    syscall0(7);
    write(&BANNER);
    write(&PROMPT);
    let mut input = core::mem::MaybeUninit::<[u8; 64]>::uninit();
    let mut length = 0;

    loop {
        let character = syscall0(6) as u8;
        if character == 0 {
            syscall1(5, 1);
            continue;
        }
        if character == b'\n' || character == b'\r' {
            write(b"\r\n");
            let input = input.as_ptr().cast::<u8>();
            if equals(input, length, b"help") {
                write(&HELP);
            } else if equals(input, length, b"clear") {
                syscall0(7);
            } else if equals(input, length, b"exit") {
                write(&EXITING);
                syscall0(3);
            } else if equals(input, length, b"shutdown") {
                write(&EXITING);
                syscall0(9);
            } else if equals(input, length, b"sata") {
                syscall0(10);
            } else if equals(input, length, b"identify") {
                syscall0(11);
            } else if equals(input, length, b"read") {
                syscall0(12);
            } else if equals(input, length, b"pci") {
                syscall0(13);
            } else if equals(input, length, b"lsblk") {
                syscall0(14);
            } else if equals(input, length, b"threads") {
                user_thread_demo();
            } else if equals(input, length, b"heap") {
                heap_test();
            } else if equals(input, length, b"vfs") {
                vfs_test();
            } else if equals(input, length, b"env") {
                environment_test();
            } else if equals(input, length, b"time") {
                time_test();
            } else if equals(input, length, b"sync") {
                synchronization_test();
            } else if length != 0 {
                write(&UNKNOWN);
            }
            length = 0;
            write(&PROMPT);
            continue;
        }
        if character == 8 {
            if length != 0 {
                length -= 1;
                syscall0(8);
            }
            continue;
        }
        if character == b'\t' {
            let input_bytes = input.as_ptr().cast::<u8>();
            if let Some(command) = complete(input_bytes, length) {
                for (index, byte) in command.iter().enumerate() {
                    unsafe {
                        input.as_mut_ptr().cast::<u8>().add(index).write(*byte);
                    }
                }
                write(&command[length..]);
                length = command.len();
            }
            continue;
        }
        if length < 64 {
            unsafe {
                input.as_mut_ptr().cast::<u8>().add(length).write(character);
            }
            length += 1;
            write(core::slice::from_ref(&character));
        }
    }
}

fn synchronization_test() {
    SYNC_VALUE.store(0, Ordering::Release);
    let Some(worker) = UserThread::spawn_with_tls(
        synchronization_worker,
        0,
        (&raw const TLS_WORD) as *const u64 as u64,
    ) else {
        write(b"SYNC THREAD FAILED\r\n");
        return;
    };
    SYNC_MUTEX.lock();
    while SYNC_VALUE.load(Ordering::Acquire) == 0 {
        SYNC_CONDITION.wait(&SYNC_MUTEX);
    }
    SYNC_MUTEX.unlock();
    if worker.join() == 0 {
        write(b"SYNC OK\r\n");
    } else {
        write(b"SYNC JOIN FAILED\r\n");
    }
}

extern "C" fn synchronization_worker(_: u64) {
    let first_tls_word = read_tls_word();
    SYNC_MUTEX.lock();
    SYNC_VALUE.store(1, Ordering::Release);
    SYNC_CONDITION.notify_one();
    SYNC_MUTEX.unlock();
    syscall0(2);
    let status = u64::from(first_tls_word != TLS_WORD || read_tls_word() != TLS_WORD);
    syscall1(16, status);
    loop {
        core::hint::spin_loop();
    }
}

fn read_tls_word() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, qword ptr fs:[0]", out(reg) value, options(nostack, readonly));
    }
    value
}

fn time_test() {
    let before = syscall0(27);
    if syscall1(5, 30) != 0 {
        write(b"SLEEP FAILED\r\n");
        return;
    }
    let after = syscall0(27);
    if after.wrapping_sub(before) >= 30 {
        write(b"TIME OK\r\n");
    } else {
        write(b"TIME FAILED\r\n");
    }
}

fn environment_test() {
    let mut cwd = [0_u8; 8];
    let mut executable = [0_u8; 16];
    let entry = syscall2(26, executable.as_mut_ptr() as u64, executable.len() as u64);
    if entry == u64::MAX
        || syscall2(25, cwd.as_mut_ptr() as u64, cwd.len() as u64) != 1
        || cwd[0] != b'/'
        || &executable[..5] != b"/init"
    {
        write(b"ENVIRONMENT FAILED\r\n");
        return;
    }
    if syscall2(24, b"/tmp".as_ptr() as u64, 4) != 0
        || syscall2(25, cwd.as_mut_ptr() as u64, cwd.len() as u64) != 4
        || &cwd[..4] != b"/tmp"
    {
        write(b"CWD FAILED\r\n");
        return;
    }
    write(b"ENVIRONMENT OK\r\n");
}

fn vfs_test() {
    const OPEN_WRITE: u64 = 1;
    const OPEN_CREATE: u64 = 2;
    const OPEN_DIRECTORY: u64 = 4;
    let fd = syscall3(19, b"/tmp/demo".as_ptr() as u64, b"/tmp/demo".len() as u64, OPEN_WRITE | OPEN_CREATE);
    if fd == u64::MAX || syscall3(21, fd, b"hello".as_ptr() as u64, 5) != 5 {
        write(b"VFS WRITE FAILED\r\n");
        return;
    }
    if syscall3(23, fd, 0, 0) != 0 {
        write(b"VFS SEEK FAILED\r\n");
        return;
    }
    let mut content = [0_u8; 8];
    if syscall3(20, fd, content.as_mut_ptr() as u64, 5) != 5 || &content[..5] != b"hello" {
        write(b"VFS READ FAILED\r\n");
        return;
    }
    if syscall1(22, fd) != 0 {
        write(b"VFS CLOSE FAILED\r\n");
        return;
    }
    let directory = syscall3(19, b"/tmp".as_ptr() as u64, b"/tmp".len() as u64, OPEN_DIRECTORY);
    let mut entry = [0_u8; 64];
    if directory == u64::MAX || syscall3(20, directory, entry.as_mut_ptr() as u64, entry.len() as u64) == 0 {
        write(b"VFS DIRECTORY FAILED\r\n");
        return;
    }
    let _ = syscall1(22, directory);
    write(b"VFS OK\r\n");
}

fn heap_test() {
    let mut bytes = Vec::with_capacity(8192);
    for index in 0..8192 {
        bytes.push((index as u8) ^ 0xA5);
    }
    if bytes.len() == 8192 && bytes[0] == 0xA5 && bytes[8191] == 0x5A {
        write(b"ALLOC VEC OK\r\n");
    } else {
        write(b"ALLOC VEC CORRUPTED\r\n");
    }
}

fn user_thread_demo() {
    let worker = syscall2(15, user_thread_worker as *const () as u64, 0);
    if worker == u64::MAX {
        write(b"THREAD CREATE FAILED\r\n");
        return;
    }
    write(b"A\r\n");
    syscall0(2);
    write(b"A\r\n");
    syscall1(17, worker);
}

extern "C" fn user_thread_worker(_: u64) {
    write(b"B\r\n");
    syscall0(2);
    write(b"B\r\n");
    syscall1(16, 0);
    loop {
        core::hint::spin_loop();
    }
}

fn complete(input: *const u8, length: usize) -> Option<&'static [u8]> {
    let mut match_command = None;
    for command in COMMANDS {
        if has_prefix(input, length, command) {
            if match_command.is_some() {
                return None;
            }
            match_command = Some(command);
        }
    }
    match_command.filter(|command| command.len() > length)
}

fn has_prefix(input: *const u8, length: usize, command: &[u8]) -> bool {
    if length > command.len() {
        return false;
    }
    for index in 0..length {
        if unsafe { input.add(index).read() } != command[index] {
            return false;
        }
    }
    true
}

fn equals(input: *const u8, length: usize, expected: &[u8]) -> bool {
    if length != expected.len() {
        return false;
    }
    for index in 0..length {
        if unsafe { input.add(index).read() } != expected[index] {
            return false;
        }
    }
    true
}

fn write(bytes: &[u8]) {
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") 1_u64 => _,
            in("rdi") bytes.as_ptr() as u64,
            in("rsi") bytes.len() as u64,
            clobber_abi("sysv64"),
        );
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

fn syscall1(number: u64, argument: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") argument,
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

// `alloc` was built for the host target, so provide the small freestanding ABI
// surface it references when the ELF is linked with `-nostdlib`.
#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

#[unsafe(no_mangle)]
unsafe extern "C" fn memcpy(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    for offset in 0..length {
        unsafe {
            destination
                .add(offset)
                .write_volatile(source.add(offset).read_volatile());
        }
    }
    destination
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memmove(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    if (destination as usize) <= (source as usize) {
        unsafe { memcpy(destination, source, length) }
    } else {
        for offset in (0..length).rev() {
            unsafe {
                destination
                    .add(offset)
                    .write_volatile(source.add(offset).read_volatile());
            }
        }
        destination
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memset(destination: *mut u8, value: i32, length: usize) -> *mut u8 {
    for offset in 0..length {
        unsafe {
            destination.add(offset).write_volatile(value as u8);
        }
    }
    destination
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
