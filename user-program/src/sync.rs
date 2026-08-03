use core::arch::asm;
use core::sync::atomic::{AtomicU32, Ordering};

const FUTEX_WAIT: u64 = 28;
const FUTEX_WAKE: u64 = 29;
const NO_TIMEOUT: u64 = u64::MAX;

pub struct UserMutex {
    state: AtomicU32,
}

impl UserMutex {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
        }
    }

    pub fn lock(&self) {
        loop {
            if self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            let _ = futex_wait(&self.state, 1, NO_TIMEOUT);
        }
    }

    pub fn unlock(&self) {
        self.state.store(0, Ordering::Release);
        let _ = futex_wake(&self.state, 1);
    }
}

pub struct UserCondvar {
    sequence: AtomicU32,
}

impl UserCondvar {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
        }
    }

    pub fn wait(&self, mutex: &UserMutex) {
        let sequence = self.sequence.load(Ordering::Acquire);
        mutex.unlock();
        let _ = futex_wait(&self.sequence, sequence, NO_TIMEOUT);
        mutex.lock();
    }

    pub fn notify_one(&self) {
        self.sequence.fetch_add(1, Ordering::Release);
        let _ = futex_wake(&self.sequence, 1);
    }

    #[allow(dead_code)]
    pub fn notify_all(&self) {
        self.sequence.fetch_add(1, Ordering::Release);
        let _ = futex_wake(&self.sequence, u64::MAX);
    }
}

pub struct UserThread {
    id: u64,
}

impl UserThread {
    // The entry function owns its lifecycle and must invoke syscall 16 when done.
    pub fn spawn(entry: extern "C" fn(u64), argument: u64) -> Option<Self> {
        let id = syscall3(15, entry as *const () as u64, argument, 0);
        (id != u64::MAX).then_some(Self { id })
    }

    pub fn join(self) -> u64 {
        syscall1(17, self.id)
    }

}

fn futex_wait(word: &AtomicU32, expected: u32, timeout_ms: u64) -> u64 {
    syscall3(FUTEX_WAIT, word as *const AtomicU32 as u64, expected as u64, timeout_ms)
}

fn futex_wake(word: &AtomicU32, count: u64) -> u64 {
    syscall3(FUTEX_WAKE, word as *const AtomicU32 as u64, count, 0)
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
