use core::arch::{asm, global_asm};

const MAX_THREADS: usize = 8;
const NO_THREAD: usize = usize::MAX;
const IDLE_THREAD: usize = 0;

pub type ThreadId = usize;
pub type TaskEntry = extern "C" fn();

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Context {
    stack_pointer: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}
impl Context {
    const EMPTY: Self = Self {
        stack_pointer: 0,
        rbx: 0,
        rbp: 0,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SyscallState {
    pub stack_pointer: u64,
    pub instruction_pointer: u64,
    pub flags: u64,
}
impl SyscallState {
    const EMPTY: Self = Self {
        stack_pointer: 0,
        instruction_pointer: 0,
        flags: 0,
    };
}

#[derive(Clone, Copy)]
struct Thread {
    context: Context,
    entry: Option<TaskEntry>,
    state: ThreadState,
    syscall_state: SyscallState,
    stack_top: u64,
    syscall_stack_top: u64,
    stack_allocated: bool,
    reapable: bool,
}
impl Thread {
    const EMPTY: Self = Self {
        context: Context::EMPTY,
        entry: None,
        state: ThreadState::Exited,
        syscall_state: SyscallState::EMPTY,
        stack_top: 0,
        syscall_stack_top: 0,
        stack_allocated: false,
        reapable: false,
    };
}

static mut THREADS: [Thread; MAX_THREADS] = [Thread::EMPTY; MAX_THREADS];
static mut RUN_QUEUE: [ThreadId; MAX_THREADS] = [NO_THREAD; MAX_THREADS];
static mut RUN_HEAD: usize = 0;
static mut RUN_TAIL: usize = 0;
static mut RUN_COUNT: usize = 0;
static mut SCHEDULER_CONTEXT: Context = Context::EMPTY;
static mut CURRENT_THREAD: usize = NO_THREAD;
static mut PREEMPT_REQUESTED: bool = false;
static mut IDLE_CREATED: bool = false;

unsafe extern "C" {
    fn scheduler_context_switch(from: *mut Context, to: *const Context);
}
global_asm!(
    r#"
.section .text
.global scheduler_context_switch
.type scheduler_context_switch, @function
scheduler_context_switch:
    mov %rsp, 0(%rdi)
    mov %rbx, 8(%rdi)
    mov %rbp, 16(%rdi)
    mov %r12, 24(%rdi)
    mov %r13, 32(%rdi)
    mov %r14, 40(%rdi)
    mov %r15, 48(%rdi)
    mov 8(%rsi), %rbx
    mov 16(%rsi), %rbp
    mov 24(%rsi), %r12
    mov 32(%rsi), %r13
    mov 40(%rsi), %r14
    mov 48(%rsi), %r15
    mov 0(%rsi), %rsp
    ret
"#,
    options(att_syntax)
);

pub fn spawn(entry: TaskEntry) -> Option<ThreadId> {
    unsafe {
        for slot in 1..MAX_THREADS {
            let thread = &mut (*(&raw mut THREADS))[slot];
            if thread.state != ThreadState::Exited || (!thread.reapable && thread.entry.is_some()) {
                continue;
            }
            if !thread.stack_allocated {
                thread.stack_top = crate::allocate_kernel_stack(slot).ok()?;
                thread.syscall_stack_top = crate::allocate_kernel_stack(slot + MAX_THREADS).ok()?;
                thread.stack_allocated = true;
            }
            let stack_top = thread.stack_top;
            let syscall_stack_top = thread.syscall_stack_top;
            let initial_stack = (stack_top - 16) as *mut u64;
            initial_stack.write(thread_trampoline as *const () as u64);
            *thread = Thread {
                context: Context {
                    stack_pointer: initial_stack as u64,
                    ..Context::EMPTY
                },
                entry: Some(entry),
                state: ThreadState::Ready,
                syscall_state: SyscallState::EMPTY,
                stack_top,
                syscall_stack_top,
                stack_allocated: true,
                reapable: false,
            };
            enqueue(slot);
            return Some(slot);
        }
    }
    None
}

pub fn start() -> ! {
    unsafe {
        ensure_idle();
        schedule_from_scheduler();
    }
    idle_loop()
}

pub fn yield_now() {
    unsafe {
        let current = CURRENT_THREAD;
        if current == NO_THREAD {
            return;
        }
        if current != IDLE_THREAD {
            (*(&raw mut THREADS))[current].state = ThreadState::Ready;
            enqueue(current);
        }
        schedule_from_current(current);
    }
}

pub fn block_current() {
    unsafe {
        let current = CURRENT_THREAD;
        if current == NO_THREAD || current == IDLE_THREAD {
            return;
        }
        (*(&raw mut THREADS))[current].state = ThreadState::Blocked;
        schedule_from_current(current);
    }
}

pub fn wake(thread: ThreadId) -> bool {
    if thread >= MAX_THREADS {
        return false;
    }
    unsafe {
        let task = &mut (*(&raw mut THREADS))[thread];
        if task.state != ThreadState::Blocked {
            return false;
        }
        task.state = ThreadState::Ready;
        enqueue(thread);
    }
    true
}

pub fn exit_current() -> ! {
    unsafe {
        let current = CURRENT_THREAD;
        if current != NO_THREAD && current != IDLE_THREAD {
            let task = &mut (*(&raw mut THREADS))[current];
            task.state = ThreadState::Exited;
            task.entry = None;
            task.reapable = true;
            schedule_from_current(current);
        }
    }
    idle_loop()
}

pub fn state(thread: ThreadId) -> Option<ThreadState> {
    if thread >= MAX_THREADS {
        return None;
    }
    unsafe { Some((*(&raw const THREADS))[thread].state) }
}

pub fn syscall_stack_top() -> u64 {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread == NO_THREAD {
            return 0;
        }
        (*(&raw const THREADS))[thread].syscall_stack_top
    }
}

pub fn save_syscall_state(state: SyscallState) {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread != NO_THREAD {
            (*(&raw mut THREADS))[thread].syscall_state = state;
        }
    }
}

pub fn current_syscall_state() -> *const SyscallState {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread == NO_THREAD {
            core::ptr::null()
        } else {
            &raw const (*(&raw const THREADS))[thread].syscall_state
        }
    }
}

pub fn request_preemption() {
    unsafe {
        core::ptr::write_volatile(&raw mut PREEMPT_REQUESTED, true);
    }
}

pub fn yield_if_preempted() {
    let requested = unsafe { core::ptr::read_volatile(&raw const PREEMPT_REQUESTED) };
    if requested {
        unsafe {
            core::ptr::write_volatile(&raw mut PREEMPT_REQUESTED, false);
        }
        yield_now();
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn ensure_idle() {
    if IDLE_CREATED {
        return;
    }
    let task = &mut (*(&raw mut THREADS))[IDLE_THREAD];
    task.stack_top = match crate::allocate_kernel_stack(IDLE_THREAD) {
        Ok(stack_top) => stack_top,
        Err(_) => crate::halt(),
    };
    task.syscall_stack_top = match crate::allocate_kernel_stack(IDLE_THREAD + MAX_THREADS) {
        Ok(stack_top) => stack_top,
        Err(_) => crate::halt(),
    };
    task.stack_allocated = true;
    let initial_stack = (task.stack_top - 16) as *mut u64;
    initial_stack.write(idle_thread as *const () as u64);
    task.context.stack_pointer = initial_stack as u64;
    task.entry = Some(idle_thread);
    task.state = ThreadState::Ready;
    enqueue(IDLE_THREAD);
    IDLE_CREATED = true;
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn schedule_from_scheduler() {
    let next = dequeue().unwrap_or(IDLE_THREAD);
    CURRENT_THREAD = next;
    (*(&raw mut THREADS))[next].state = ThreadState::Running;
    scheduler_context_switch(
        &raw mut SCHEDULER_CONTEXT,
        &raw const (*(&raw const THREADS))[next].context,
    );
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn schedule_from_current(current: ThreadId) {
    let next = dequeue().unwrap_or(IDLE_THREAD);
    if next == current {
        (*(&raw mut THREADS))[current].state = ThreadState::Running;
        return;
    }
    CURRENT_THREAD = next;
    (*(&raw mut THREADS))[next].state = ThreadState::Running;
    scheduler_context_switch(
        &raw mut (*(&raw mut THREADS))[current].context,
        &raw const (*(&raw const THREADS))[next].context,
    );
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn enqueue(thread: ThreadId) {
    if RUN_COUNT == MAX_THREADS {
        return;
    }
    (*(&raw mut RUN_QUEUE))[RUN_TAIL] = thread;
    RUN_TAIL = (RUN_TAIL + 1) % MAX_THREADS;
    RUN_COUNT += 1;
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn dequeue() -> Option<ThreadId> {
    if RUN_COUNT == 0 {
        return None;
    }
    let thread = (*(&raw const RUN_QUEUE))[RUN_HEAD];
    RUN_HEAD = (RUN_HEAD + 1) % MAX_THREADS;
    RUN_COUNT -= 1;
    Some(thread)
}

extern "C" fn thread_trampoline() {
    unsafe {
        let entry = (*(&raw const THREADS))[CURRENT_THREAD]
            .entry
            .expect("missing thread entry");
        entry();
    }
    exit_current()
}

extern "C" fn idle_thread() {
    idle_loop()
}

fn idle_loop() -> ! {
    loop {
        yield_now();
        unsafe {
            asm!("sti", "hlt", "cli", options(nomem, nostack));
        }
    }
}
