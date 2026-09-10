//! Cooperative RT scan loop (architecture T1). No tokio.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::loader::Runtime;

/// Spawn the scan thread. It holds the runtime mutex only for `run_due`.
pub fn spawn_scan_thread(
    runtime: Arc<Mutex<Runtime>>,
    stop: Arc<AtomicBool>,
    cpu_affinity: Option<usize>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("plc-scan".into())
        .spawn(move || {
            apply_thread_rt(cpu_affinity);
            scan_loop(&runtime, &stop);
        })
        .expect("spawn plc-scan")
}

fn scan_loop(runtime: &Mutex<Runtime>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let sleep_ms = {
            let mut rt = runtime.lock().expect("runtime");
            match rt.run_due() {
                Ok(_) => {
                    let now = rt.now_ms();
                    let next = rt.next_wakeup_ms();
                    next.saturating_sub(now)
                }
                Err(_) => 1,
            }
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if sleep_ms == 0 {
            thread::sleep(Duration::from_micros(200));
        } else {
            thread::sleep(Duration::from_millis(sleep_ms.min(1_000)));
        }
    }
}

fn apply_thread_rt(cpu_affinity: Option<usize>) {
    #[cfg(target_os = "linux")]
    {
        linux_rt(cpu_affinity);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cpu_affinity;
    }
}

#[cfg(target_os = "linux")]
fn linux_rt(cpu_affinity: Option<usize>) {
    unsafe {
        let mut param: libc::sched_param = std::mem::zeroed();
        param.sched_priority = 10;
        let rc = libc::sched_setscheduler(0, libc::SCHED_FIFO, &param);
        if rc != 0 {
            // Soft RT: continue without FIFO when unprivileged.
            tracing::warn!(
                "SCHED_FIFO not granted (errno {}); continuing as CFS",
                std::io::Error::last_os_error()
            );
        }
        if let Some(cpu) = cpu_affinity {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut set);
            libc::CPU_SET(cpu, &mut set);
            let rc = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            if rc != 0 {
                tracing::warn!(
                    "CPU affinity {cpu} failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}
