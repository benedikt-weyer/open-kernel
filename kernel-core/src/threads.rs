use core::arch::{asm, global_asm};

const MAX_THREADS: usize = 8;
const NO_THREAD: usize = usize::MAX;
const IDLE_THREAD: usize = 0;

pub type ThreadId = usize;
pub type ProcessId = usize;
pub type TaskEntry = extern "C" fn();
pub const USER_PROCESS_ID: ProcessId = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserContext {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub fs_base: u64,
}
impl UserContext {
    pub const EMPTY: Self = Self {
        rip: 0, rsp: 0, rflags: 0x202, rax: 0, rbx: 0, rcx: 0, rdx: 0,
        rsi: 0, rdi: 0, rbp: 0, r8: 0, r9: 0, r10: 0, r11: 0, r12: 0,
        r13: 0, r14: 0, r15: 0, fs_base: 0,
    };
}

#[derive(Clone, Copy)]
pub struct Process {
    pub id: ProcessId,
    pub address_space: u64,
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
    process: Option<ProcessId>,
    user_context: UserContext,
    is_user: bool,
    state: ThreadState,
    syscall_state: SyscallState,
    stack_top: u64,
    syscall_stack_top: u64,
    stack_allocated: bool,
    owns_user_stack: bool,
    reapable: bool,
    exit_status: u64,
    join_waiters: [usize; MAX_THREADS],
    join_waiter_count: usize,
}
impl Thread {
    const EMPTY: Self = Self {
        context: Context::EMPTY,
        entry: None,
        process: None,
        user_context: UserContext::EMPTY,
        is_user: false,
        state: ThreadState::Exited,
        syscall_state: SyscallState::EMPTY,
        stack_top: 0,
        syscall_stack_top: 0,
        stack_allocated: false,
        owns_user_stack: false,
        reapable: true,
        exit_status: 0,
        join_waiters: [NO_THREAD; MAX_THREADS],
        join_waiter_count: 0,
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
static mut USER_PROCESS: Process = Process {
    id: USER_PROCESS_ID,
    address_space: 0,
};

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

unsafe fn release_resources(thread: &mut Thread, slot: usize) {
    if !thread.stack_allocated {
        return;
    }
    crate::release_kernel_stack(slot);
    crate::release_kernel_stack(slot + MAX_THREADS);
    if thread.owns_user_stack {
        crate::release_user_stack(slot);
    }
    thread.stack_top = 0;
    thread.syscall_stack_top = 0;
    thread.stack_allocated = false;
    thread.owns_user_stack = false;
}

pub fn spawn(entry: TaskEntry) -> Option<ThreadId> {
    unsafe {
        for slot in 1..MAX_THREADS {
            let thread = &mut (*(&raw mut THREADS))[slot];
            if thread.state != ThreadState::Exited || !thread.reapable {
                continue;
            }
            release_resources(thread, slot);
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
                process: None,
                user_context: UserContext::EMPTY,
                is_user: false,
                state: ThreadState::Ready,
                syscall_state: SyscallState::EMPTY,
                stack_top,
                syscall_stack_top,
                stack_allocated: true,
                owns_user_stack: false,
                reapable: false,
                exit_status: 0,
                join_waiters: [NO_THREAD; MAX_THREADS],
                join_waiter_count: 0,
            };
            enqueue(slot);
            return Some(slot);
        }
    }
    None
}

pub fn initialize_user_process() {
    unsafe {
        let address_space: u64;
        asm!("mov {}, cr3", out(reg) address_space, options(nomem, nostack));
        (*(&raw mut USER_PROCESS)).address_space = address_space;
    }
}

pub fn spawn_user(
    entry: u64,
    argument: u64,
    tls_base: u64,
    initial_stack: Option<u64>,
) -> Option<ThreadId> {
    if !crate::is_user_executable(entry) || (tls_base != 0 && !crate::is_user_mapped(tls_base)) {
        return None;
    }
    unsafe {
        for slot in 1..MAX_THREADS {
            let thread = &mut (*(&raw mut THREADS))[slot];
            if thread.state != ThreadState::Exited || !thread.reapable {
                continue;
            }
            release_resources(thread, slot);
            if !thread.stack_allocated {
                thread.stack_top = crate::allocate_kernel_stack(slot).ok()?;
                thread.syscall_stack_top = crate::allocate_kernel_stack(slot + MAX_THREADS).ok()?;
                thread.stack_allocated = true;
            }
            let (user_stack, owns_user_stack) = match initial_stack {
                Some(stack) => (stack, false),
                None => (crate::allocate_user_stack(slot).ok()?.checked_sub(8)?, true),
            };
            let stack_top = thread.stack_top;
            let syscall_stack_top = thread.syscall_stack_top;
            let initial_kernel_stack = (stack_top - 16) as *mut u64;
            initial_kernel_stack.write(user_thread_trampoline as *const () as u64);
            *thread = Thread {
                context: Context { stack_pointer: initial_kernel_stack as u64, ..Context::EMPTY },
                entry: None,
                process: Some(USER_PROCESS_ID),
                user_context: UserContext {
                    rip: entry,
                    rsp: user_stack,
                    rdi: argument,
                    fs_base: tls_base,
                    ..UserContext::EMPTY
                },
                is_user: true,
                state: ThreadState::Ready,
                syscall_state: SyscallState::EMPTY,
                stack_top,
                syscall_stack_top,
                stack_allocated: true,
                owns_user_stack,
                reapable: false,
                exit_status: 0,
                join_waiters: [NO_THREAD; MAX_THREADS],
                join_waiter_count: 0,
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
    exit_current_with_status(0)
}

pub fn exit_current_with_status(status: u64) -> ! {
    unsafe {
        let current = CURRENT_THREAD;
        if current != NO_THREAD && current != IDLE_THREAD {
            let task = &mut (*(&raw mut THREADS))[current];
            task.state = ThreadState::Exited;
            task.entry = None;
            task.exit_status = status;
            task.reapable = !task.is_user;
            for waiter_index in 0..task.join_waiter_count {
                let waiter = task.join_waiters[waiter_index];
                let waiting = &mut (*(&raw mut THREADS))[waiter];
                if waiting.state == ThreadState::Blocked {
                    waiting.state = ThreadState::Ready;
                    enqueue(waiter);
                }
            }
            schedule_from_current(current);
        }
    }
    idle_loop()
}

pub fn join(thread: ThreadId) -> Option<u64> {
    unsafe {
        let current = CURRENT_THREAD;
        if current == NO_THREAD || current == thread || thread >= MAX_THREADS {
            return None;
        }
        let target = &mut (*(&raw mut THREADS))[thread];
        if target.state == ThreadState::Exited {
            if target.join_waiter_count == 0 {
                target.reapable = true;
            }
            return Some(target.exit_status);
        }
        if target.join_waiter_count == MAX_THREADS {
            return None;
        }
        target.join_waiters[target.join_waiter_count] = current;
        target.join_waiter_count += 1;
        block_current();
        let target = &mut (*(&raw mut THREADS))[thread];
        if target.state != ThreadState::Exited {
            return None;
        }
        if target.join_waiter_count != 0 {
            target.join_waiter_count -= 1;
        }
        if target.join_waiter_count == 0 {
            target.reapable = true;
        }
        Some(target.exit_status)
    }
}

pub fn state(thread: ThreadId) -> Option<ThreadState> {
    if thread >= MAX_THREADS {
        return None;
    }
    unsafe { Some((*(&raw const THREADS))[thread].state) }
}

pub fn current_id() -> Option<ThreadId> {
    unsafe {
        if CURRENT_THREAD == NO_THREAD {
            None
        } else {
            Some(CURRENT_THREAD)
        }
    }
}

pub fn process_id(thread: ThreadId) -> Option<ProcessId> {
    if thread >= MAX_THREADS {
        return None;
    }
    unsafe { (*(&raw const THREADS))[thread].process }
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
            let context = &mut (*(&raw mut THREADS))[thread].user_context;
            context.rsp = state.stack_pointer;
            context.rip = state.instruction_pointer;
            context.rflags = state.flags;
        }
    }
}

pub fn save_user_context(context: UserContext) {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread != NO_THREAD && (*(&raw const THREADS))[thread].is_user {
            (*(&raw mut THREADS))[thread].user_context = context;
        }
    }
}

pub fn current_user_context() -> *const UserContext {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread == NO_THREAD || !(*(&raw const THREADS))[thread].is_user {
            core::ptr::null()
        } else {
            &raw const (*(&raw const THREADS))[thread].user_context
        }
    }
}

pub fn current_user_fs_base() -> u64 {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread == NO_THREAD || !(*(&raw const THREADS))[thread].is_user {
            0
        } else {
            (*(&raw const THREADS))[thread].user_context.fs_base
        }
    }
}

pub fn current_kernel_stack_top() -> u64 {
    unsafe {
        let thread = CURRENT_THREAD;
        if thread == NO_THREAD { 0 } else { (*(&raw const THREADS))[thread].stack_top }
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

extern "C" fn user_thread_trampoline() {
    unsafe {
        crate::arch::resume_user_context(current_user_context());
    }
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
