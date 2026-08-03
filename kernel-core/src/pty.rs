//! Pseudo-terminal pairs: a fixed pool of master/slave byte-stream pairs.
//!
//! A process opens a pty and holds its master end; another process (or
//! itself) attaches to the slave end via `crate::user::bind_pty`, after
//! which its console write/poll-key syscalls read and write the pty's
//! ring buffers instead of a real framebuffer tty. There is no line
//! discipline or screen model on the slave side (no clear/backspace
//! rendering) since a pty is just a byte pipe in both directions, exactly
//! like a real one before a terminal emulator adds editing on top.

const PTY_COUNT: usize = 4;
const RING_CAPACITY: usize = 256;

#[derive(Clone, Copy)]
struct Ring {
    buffer: [u8; RING_CAPACITY],
    head: usize,
    count: usize,
}
impl Ring {
    const EMPTY: Self = Self { buffer: [0; RING_CAPACITY], head: 0, count: 0 };

    fn push(&mut self, byte: u8) -> bool {
        if self.count == RING_CAPACITY {
            return false;
        }
        self.buffer[(self.head + self.count) % RING_CAPACITY] = byte;
        self.count += 1;
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.count == 0 {
            return None;
        }
        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % RING_CAPACITY;
        self.count -= 1;
        Some(byte)
    }
}

#[derive(Clone, Copy)]
struct Pty {
    allocated: bool,
    owner: Option<usize>,
    /// Bytes the slave-attached process wrote, waiting for the master to read.
    to_master: Ring,
    /// Bytes the master injected, waiting for the slave-attached process to poll.
    to_slave: Ring,
}
impl Pty {
    const EMPTY: Self = Self { allocated: false, owner: None, to_master: Ring::EMPTY, to_slave: Ring::EMPTY };
}

static mut PTYS: [Pty; PTY_COUNT] = [Pty::EMPTY; PTY_COUNT];

fn owner_process() -> Option<usize> {
    crate::current_thread_id().and_then(crate::thread_process_id)
}

pub fn open() -> Option<usize> {
    let owner = owner_process()?;
    unsafe {
        for (index, pty) in (*(&raw mut PTYS)).iter_mut().enumerate() {
            if !pty.allocated {
                *pty = Pty { allocated: true, owner: Some(owner), ..Pty::EMPTY };
                return Some(index);
            }
        }
    }
    None
}

pub fn exists(id: usize) -> bool {
    unsafe { (*(&raw const PTYS)).get(id).is_some_and(|pty| pty.allocated) }
}

pub fn close(id: usize) -> bool {
    let Some(owner) = owner_process() else {
        return false;
    };
    unsafe {
        let Some(pty) = (*(&raw mut PTYS)).get_mut(id) else {
            return false;
        };
        if !pty.allocated || pty.owner != Some(owner) {
            return false;
        }
        *pty = Pty::EMPTY;
    }
    true
}

/// Reads whatever the slave-attached process has written so far. Only the
/// pty's owner (whoever opened it) may read its master side.
pub fn read_master(id: usize, output: &mut [u8]) -> Option<usize> {
    let owner = owner_process()?;
    unsafe {
        let pty = (*(&raw mut PTYS)).get_mut(id)?;
        if !pty.allocated || pty.owner != Some(owner) {
            return None;
        }
        let mut count = 0;
        while count < output.len() {
            let Some(byte) = pty.to_master.pop() else {
                break;
            };
            output[count] = byte;
            count += 1;
        }
        Some(count)
    }
}

/// Injects bytes as though they were typed into the slave side. Only the
/// pty's owner may write its master side.
pub fn write_master(id: usize, input: &[u8]) -> Option<usize> {
    let owner = owner_process()?;
    unsafe {
        let pty = (*(&raw mut PTYS)).get_mut(id)?;
        if !pty.allocated || pty.owner != Some(owner) {
            return None;
        }
        let mut count = 0;
        for byte in input {
            if !pty.to_slave.push(*byte) {
                break;
            }
            count += 1;
        }
        Some(count)
    }
}

/// Called by the slave-attached process's write syscall.
pub fn slave_write(id: usize, bytes: &[u8]) {
    unsafe {
        let Some(pty) = (*(&raw mut PTYS)).get_mut(id) else {
            return;
        };
        if !pty.allocated {
            return;
        }
        for byte in bytes {
            if !pty.to_master.push(*byte) {
                break;
            }
        }
    }
}

/// Called by the slave-attached process's poll-key syscall.
pub fn slave_poll_key(id: usize) -> Option<u8> {
    unsafe {
        let pty = (*(&raw mut PTYS)).get_mut(id)?;
        if !pty.allocated {
            return None;
        }
        pty.to_slave.pop()
    }
}
