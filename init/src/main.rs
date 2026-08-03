use openkernel_rt as _;

mod syscall;

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Runs once to completion; dependents wait for it to exit before starting.
    Oneshot,
    /// Runs indefinitely; dependents only wait for it to have been launched.
    Daemon,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "a full restart-policy vocabulary for services declared in SERVICES")]
enum Restart {
    Never,
    OnFailure,
    Always,
}

struct Service {
    name: &'static str,
    path: &'static str,
    argv: &'static [&'static str],
    depends_on: &'static [&'static str],
    kind: Kind,
    restart: Restart,
}

/// The service table. Order here is declarative, not the start order:
/// `start_order` resolves `depends_on` into an actual sequence, so adding a
/// new daemon (e.g. a "network" service some other daemon should come
/// after) is just another entry with the right `depends_on`.
///
/// The three `console-ttyN` entries autostart a shell on every switchable
/// virtual terminal (Alt+F1..F3); their count must match
/// `kernel_core::console::TTY_COUNT`.
const SERVICES: &[Service] = &[
    Service {
        name: "selftest",
        path: "/std-smoke",
        argv: &[],
        depends_on: &[],
        kind: Kind::Oneshot,
        restart: Restart::OnFailure,
    },
    Service {
        name: "console-tty0",
        path: "/console",
        argv: &["tty:0"],
        depends_on: &["selftest"],
        kind: Kind::Daemon,
        restart: Restart::Always,
    },
    Service {
        name: "console-tty1",
        path: "/console",
        argv: &["tty:1"],
        depends_on: &["selftest"],
        kind: Kind::Daemon,
        restart: Restart::Always,
    },
    Service {
        name: "console-tty2",
        path: "/console",
        argv: &["tty:2"],
        depends_on: &["selftest"],
        kind: Kind::Daemon,
        restart: Restart::Always,
    },
];

struct Running {
    pid: u64,
}

fn main() {
    syscall::clear_screen();
    println!("openkernel init: starting");

    prepare_filesystem();
    discover_devices();

    let mut running: HashMap<&'static str, Running> = HashMap::new();
    for service in start_order() {
        start_service(service, &mut running);
    }

    supervise(running);
}

fn prepare_filesystem() {
    for path in ["/", "/tmp"] {
        let state = if syscall::vfs_directory_exists(path) { "ready" } else { "missing" };
        println!("init: filesystem {path} {state}");
    }
}

fn discover_devices() {
    println!("init: discovering devices");
    syscall::sata_status();
    syscall::pci_status();
    syscall::lsblk();
}

/// Topologically sorts `SERVICES` by `depends_on`. Falls back to declaration
/// order for anything left over if a dependency is missing or cyclic, so a
/// configuration mistake delays a service instead of hanging boot.
fn start_order() -> Vec<&'static Service> {
    let mut started: Vec<&'static str> = Vec::new();
    let mut order: Vec<&'static Service> = Vec::new();

    while order.len() < SERVICES.len() {
        let mut progressed = false;
        for service in SERVICES {
            if started.contains(&service.name) {
                continue;
            }
            if service.depends_on.iter().all(|dependency| started.contains(dependency)) {
                started.push(service.name);
                order.push(service);
                progressed = true;
            }
        }
        if !progressed {
            println!("init: unresolved service dependencies, starting the rest in declared order");
            for service in SERVICES {
                if !started.contains(&service.name) {
                    order.push(service);
                }
            }
            break;
        }
    }
    order
}

fn start_service(service: &'static Service, running: &mut HashMap<&'static str, Running>) {
    println!("init: starting {} ({})", service.name, service.path);
    match service.kind {
        Kind::Oneshot => run_oneshot(service),
        Kind::Daemon => start_daemon(service, running),
    }
}

/// Runs a oneshot to completion, retrying it (up to a small bound, so a
/// permanently broken service can't hang boot forever) when its restart
/// policy calls for it.
fn run_oneshot(service: &'static Service) {
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        let Some(pid) = syscall::spawn_process(service.path, service.argv) else {
            println!("init: failed to start {}", service.name);
            return;
        };
        let status = syscall::wait_process(pid).unwrap_or(u64::MAX);
        println!("init: {} finished (status={status})", service.name);
        let retry = status != 0
            && matches!(service.restart, Restart::OnFailure | Restart::Always)
            && attempt < MAX_ATTEMPTS;
        if !retry {
            return;
        }
        println!("init: retrying {} (attempt {})", service.name, attempt + 1);
    }
}

fn start_daemon(service: &'static Service, running: &mut HashMap<&'static str, Running>) {
    let Some(pid) = syscall::spawn_process(service.path, service.argv) else {
        println!("init: failed to start {}", service.name);
        return;
    };
    println!("init: {} running (pid={pid})", service.name);
    running.insert(service.name, Running { pid });
}

fn supervise(mut running: HashMap<&'static str, Running>) -> ! {
    println!("init: entering supervisor loop");
    loop {
        let event = syscall::poll_power_event();
        if event != syscall::POWER_EVENT_NONE {
            shutdown_sequence(running, event);
        }

        while let Some((pid, status)) = syscall::reap_any_child() {
            reap(pid, status, &mut running);
        }

        syscall::sleep_ms(200);
    }
}

/// Called for every exited child, whether it's a tracked service or an
/// orphan the kernel reparented to init after its original parent exited.
/// Either way, reaping here is what keeps it from lingering as a zombie.
fn reap(pid: u64, status: u64, running: &mut HashMap<&'static str, Running>) {
    let Some(name) = running.iter().find(|(_, entry)| entry.pid == pid).map(|(name, _)| *name) else {
        println!("init: reaped orphan pid={pid} status={status}");
        return;
    };
    println!("init: {name} exited (status={status})");
    running.remove(name);

    let service = SERVICES
        .iter()
        .find(|service| service.name == name)
        .expect("a running entry always names a declared service");
    let restart = match service.restart {
        Restart::Always => true,
        Restart::OnFailure => status != 0,
        Restart::Never => false,
    };
    if restart {
        start_service(service, running);
    }
}

fn shutdown_sequence(mut running: HashMap<&'static str, Running>, event: u64) -> ! {
    println!("init: shutdown requested, stopping services");
    for service in start_order().into_iter().rev() {
        let Some(entry) = running.remove(service.name) else {
            continue;
        };
        println!("init: stopping {}", service.name);
        syscall::terminate_process(entry.pid);
        syscall::wait_process(entry.pid);
    }
    println!("init: all services stopped");

    match event {
        syscall::POWER_EVENT_REBOOT => {
            println!("init: rebooting");
            syscall::reboot();
        }
        _ => {
            println!("init: powering off");
            syscall::shutdown();
        }
    }
}
