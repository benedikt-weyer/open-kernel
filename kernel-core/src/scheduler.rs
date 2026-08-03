use core::arch::global_asm;

const MAX_TASKS: usize = 8;
const STACK_SIZE: usize = 16 * 1024;
const NO_TASK: usize = usize::MAX;

pub type TaskEntry = extern "C" fn() -> !;

#[repr(C)]
#[derive(Clone, Copy)]
struct Context {
    stack_pointer: u64,
}
impl Context {
    const EMPTY: Self = Self { stack_pointer: 0 };
}
#[derive(Clone, Copy)]
struct Task {
    context: Context,
    entry: Option<TaskEntry>,
}
impl Task {
    const EMPTY: Self = Self {
        context: Context::EMPTY,
        entry: None,
    };
}
#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

static mut TASKS: [Task; MAX_TASKS] = [Task::EMPTY; MAX_TASKS];
static mut STACKS: [Stack; MAX_TASKS] = [const { Stack([0; STACK_SIZE]) }; MAX_TASKS];
static mut SCHEDULER_CONTEXT: Context = Context::EMPTY;
static mut CURRENT_TASK: usize = NO_TASK;
static mut PREEMPT_REQUESTED: bool = false;

unsafe extern "C" {
    fn scheduler_context_switch(from: *mut Context, to: *const Context);
}
global_asm!(
    r#"
.section .text
.global scheduler_context_switch
.type scheduler_context_switch, @function
scheduler_context_switch:
    mov %rsp, (%rdi)
    mov (%rsi), %rsp
    ret
"#,
    options(att_syntax)
);

pub fn spawn(entry: TaskEntry) -> Option<usize> {
    unsafe {
        for slot in 0..MAX_TASKS {
            if (*(&raw const TASKS))[slot].entry.is_none() {
                let stack = &mut (*(&raw mut STACKS))[slot].0;
                let top = stack.as_mut_ptr().add(STACK_SIZE) as usize & !0xF;
                let initial_stack = (top - 16) as *mut usize;
                initial_stack.write(0);
                initial_stack
                    .add(1)
                    .write(task_trampoline as *const () as usize);
                (*(&raw mut TASKS))[slot] = Task {
                    context: Context {
                        stack_pointer: initial_stack as u64,
                    },
                    entry: Some(entry),
                };
                return Some(slot);
            }
        }
    }
    None
}

pub fn start() -> ! {
    unsafe {
        let next = next_task(NO_TASK).expect("scheduler started without tasks");
        CURRENT_TASK = next;
        scheduler_context_switch(
            &raw mut SCHEDULER_CONTEXT,
            &raw const (*(&raw const TASKS))[next].context,
        );
    }
    loop {
        core::hint::spin_loop();
    }
}

pub fn yield_now() {
    unsafe {
        let current = CURRENT_TASK;
        if current == NO_TASK {
            return;
        }
        let Some(next) = next_task(current) else {
            return;
        };
        if next == current {
            return;
        }
        CURRENT_TASK = next;
        scheduler_context_switch(
            &raw mut (*(&raw mut TASKS))[current].context,
            &raw const (*(&raw const TASKS))[next].context,
        );
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

fn next_task(current: usize) -> Option<usize> {
    unsafe {
        for offset in 1..=MAX_TASKS {
            let slot = current.wrapping_add(offset) % MAX_TASKS;
            if (*(&raw const TASKS))[slot].entry.is_some() {
                return Some(slot);
            }
        }
    }
    None
}

extern "C" fn task_trampoline() -> ! {
    unsafe {
        let task = CURRENT_TASK;
        let entry = (*(&raw const TASKS))[task]
            .entry
            .expect("missing task entry");
        entry()
    }
}
