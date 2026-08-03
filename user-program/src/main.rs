use openkernel_rt as _;

mod syscall;

use std::arch::asm;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};

const HELP: &str =
    "COMMANDS: HELP CLEAR EXIT SHUTDOWN REBOOT SATA IDENTIFY READ PCI LSBLK THREADS HEAP VFS ENV TIME SYNC RUN LS\r\n";
const COMMANDS: [&str; 18] = [
    "help", "clear", "exit", "shutdown", "reboot", "sata", "identify", "read", "pci", "lsblk",
    "threads", "heap", "vfs", "env", "time", "sync", "run", "ls",
];

static SYNC_MUTEX: Mutex<()> = Mutex::new(());
static SYNC_CONDVAR: Condvar = Condvar::new();
static SYNC_READY: AtomicU32 = AtomicU32::new(0);
static TLS_WORD: u64 = 0x544C_535F_4F4B_0001;

fn main() {
    let tty = std::env::args().nth(1).and_then(|argument| argument.parse().ok()).unwrap_or(0);
    syscall::bind_tty(tty);
    syscall::clear_screen();
    eprint!("OPEN KERNEL USER CONSOLE (TTY {tty})\r\nTYPE HELP\r\n> ");

    let mut input = String::new();
    loop {
        let Some(character) = read_char() else { continue };
        match character {
            b'\r' | b'\n' => {
                eprint!("\r\n");
                run_command(input.trim());
                input.clear();
                eprint!("> ");
            }
            0x08 => {
                if input.pop().is_some() {
                    syscall::erase_char();
                }
            }
            b'\t' => {
                if let Some(completion) = complete(&input) {
                    eprint!("{}", &completion[input.len()..]);
                    input.push_str(&completion[input.len()..]);
                }
            }
            byte if input.len() < 64 => {
                input.push(byte as char);
                eprint!("{}", byte as char);
            }
            _ => {}
        }
    }
}

/// Blocks until a key is pressed, yielding the CPU between polls.
fn read_char() -> Option<u8> {
    loop {
        if let Some(key) = syscall::poll_key() {
            return Some(key);
        }
        syscall::sleep_ms(1);
    }
}

fn run_command(command: &str) {
    match command {
        "" => {}
        "help" => eprint!("{HELP}"),
        "clear" => syscall::clear_screen(),
        "exit" => {
            eprint!("GOODBYE\r\n");
            syscall::process_exit();
        }
        "shutdown" => {
            if syscall::request_power_event(syscall::POWER_EVENT_SHUTDOWN) {
                eprint!("SHUTDOWN REQUESTED\r\n");
            } else {
                eprint!("SHUTDOWN REQUEST FAILED\r\n");
            }
        }
        "reboot" => {
            if syscall::request_power_event(syscall::POWER_EVENT_REBOOT) {
                eprint!("REBOOT REQUESTED\r\n");
            } else {
                eprint!("REBOOT REQUEST FAILED\r\n");
            }
        }
        "sata" => syscall::sata_status(),
        "identify" => syscall::sata_identify(),
        "read" => syscall::sata_read(),
        "pci" => syscall::pci_status(),
        "lsblk" => syscall::lsblk(),
        "threads" => user_thread_demo(),
        "heap" => heap_test(),
        "vfs" => vfs_test(),
        "env" => environment_test(),
        "time" => time_test(),
        "sync" => synchronization_test(),
        "run" => run_std_smoke(),
        "ls" => list_root(),
        _ => eprint!("UNKNOWN COMMAND\r\n"),
    }
}

fn complete(input: &str) -> Option<&'static str> {
    let mut matched = None;
    for command in COMMANDS {
        if command.starts_with(input) {
            if matched.is_some() {
                return None;
            }
            matched = Some(command);
        }
    }
    matched.filter(|command| command.len() > input.len())
}

fn run_std_smoke() {
    let Some(process) = syscall::spawn_process("/std-smoke") else {
        eprint!("SPAWN FAILED\r\n");
        return;
    };
    if syscall::wait_process(process).is_none() {
        eprint!("WAIT FAILED\r\n");
    } else {
        eprint!("CHILD EXITED\r\n");
    }
}

fn list_root() {
    let Some(directory) = syscall::vfs_open("/", syscall::OPEN_DIRECTORY) else {
        eprint!("LS FAILED\r\n");
        return;
    };
    let mut entry = [0_u8; 64];
    loop {
        match syscall::vfs_read(directory, &mut entry) {
            Some(0) => break,
            Some(count) => eprint!("{}\r\n", String::from_utf8_lossy(&entry[..count - 1])),
            None => {
                eprint!("LS FAILED\r\n");
                break;
            }
        }
    }
    syscall::vfs_close(directory);
}

fn synchronization_test() {
    SYNC_READY.store(0, Ordering::Release);
    let Some(worker) = syscall::spawn_thread(
        synchronization_worker,
        0,
        (&raw const TLS_WORD) as u64,
    ) else {
        eprint!("SYNC THREAD FAILED\r\n");
        return;
    };
    {
        let mut guard = SYNC_MUTEX.lock().unwrap();
        while SYNC_READY.load(Ordering::Acquire) == 0 {
            guard = SYNC_CONDVAR.wait(guard).unwrap();
        }
    }
    match syscall::join_thread(worker) {
        Some(0) => eprint!("SYNC OK\r\n"),
        _ => eprint!("SYNC JOIN FAILED\r\n"),
    }
}

extern "C" fn synchronization_worker(_: u64) -> ! {
    let first_tls_word = read_tls_word();
    {
        let _guard = SYNC_MUTEX.lock().unwrap();
        SYNC_READY.store(1, Ordering::Release);
        SYNC_CONDVAR.notify_one();
    }
    syscall::yield_now();
    let status = u64::from(first_tls_word != TLS_WORD || read_tls_word() != TLS_WORD);
    syscall::exit_thread(status);
}

fn read_tls_word() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, qword ptr fs:[0]", out(reg) value, options(nostack, readonly));
    }
    value
}

fn time_test() {
    let before = syscall::now_ms();
    if !syscall::sleep_ms(30) {
        eprint!("SLEEP FAILED\r\n");
        return;
    }
    let after = syscall::now_ms();
    if after.wrapping_sub(before) >= 30 {
        eprint!("TIME OK\r\n");
    } else {
        eprint!("TIME FAILED\r\n");
    }
}

fn environment_test() {
    let mut cwd = [0_u8; 8];
    let mut executable = [0_u8; 16];
    let Some(_) = syscall::executable_path(&mut executable) else {
        eprint!("ENVIRONMENT FAILED\r\n");
        return;
    };
    if syscall::get_cwd(&mut cwd) != Some(1) || cwd[0] != b'/' || &executable[..5] != b"/init" {
        eprint!("ENVIRONMENT FAILED\r\n");
        return;
    }
    if !syscall::set_cwd("/tmp") || syscall::get_cwd(&mut cwd) != Some(4) || &cwd[..4] != b"/tmp" {
        eprint!("CWD FAILED\r\n");
        return;
    }
    let variables: HashMap<String, String> = std::env::vars().collect();
    eprint!("ENVIRONMENT OK ({} env vars)\r\n", variables.len());
}

fn vfs_test() {
    const FLAGS: u64 = syscall::OPEN_WRITE | syscall::OPEN_CREATE;
    let Some(fd) = syscall::vfs_open("/tmp/demo", FLAGS) else {
        eprint!("VFS WRITE FAILED\r\n");
        return;
    };
    if syscall::vfs_write(fd, b"hello") != Some(5) {
        eprint!("VFS WRITE FAILED\r\n");
        return;
    }
    if !syscall::vfs_seek(fd, 0, 0) {
        eprint!("VFS SEEK FAILED\r\n");
        return;
    }
    let mut content = [0_u8; 8];
    if syscall::vfs_read(fd, &mut content[..5]) != Some(5) || &content[..5] != b"hello" {
        eprint!("VFS READ FAILED\r\n");
        return;
    }
    if !syscall::vfs_close(fd) {
        eprint!("VFS CLOSE FAILED\r\n");
        return;
    }
    let Some(directory) = syscall::vfs_open("/tmp", syscall::OPEN_DIRECTORY) else {
        eprint!("VFS DIRECTORY FAILED\r\n");
        return;
    };
    let mut entry = [0_u8; 64];
    if syscall::vfs_read(directory, &mut entry) == Some(0) {
        eprint!("VFS DIRECTORY FAILED\r\n");
        return;
    }
    syscall::vfs_close(directory);
    eprint!("VFS OK\r\n");
}

fn heap_test() {
    let mut bytes = Vec::with_capacity(8192);
    for index in 0..8192 {
        bytes.push((index as u8) ^ 0xA5);
    }
    if bytes.len() == 8192 && bytes[0] == 0xA5 && bytes[8191] == 0x5A {
        eprint!("ALLOC VEC OK\r\n");
    } else {
        eprint!("ALLOC VEC CORRUPTED\r\n");
    }
}

fn user_thread_demo() {
    let Some(worker) = syscall::spawn_thread(user_thread_worker, 0, 0) else {
        eprint!("THREAD CREATE FAILED\r\n");
        return;
    };
    eprint!("A\r\n");
    syscall::yield_now();
    eprint!("A\r\n");
    syscall::join_thread(worker);
}

extern "C" fn user_thread_worker(_: u64) -> ! {
    eprint!("B\r\n");
    syscall::yield_now();
    eprint!("B\r\n");
    syscall::exit_thread(0);
}
