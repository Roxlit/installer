//! Memory repro for the viewer's live captures: N concurrent `view::shot` calls at
//! w=2560 against a real window, R rounds, the way `/shot/<token>` runs them (one OS
//! thread per request). Prints peak and final private bytes of this (fresh) process.
//!
//!   $env:SHOTBENCH_PID = <studioPid from GET /sessions, or any visible window's pid>
//!   cargo test --release --bin roxlit-studios shotbench -- --ignored --nocapture
//!
//! Optional: SHOTBENCH_THREADS (12), SHOTBENCH_ROUNDS (20), SHOTBENCH_DISTINCT=1 (each
//! thread asks a different width, so no two requests can share a frame),
//! SHOTBENCH_SAVE=<file.png> (writes the first captured frame).

use crate::view;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Default)]
struct MemCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged_pool: usize,
    quota_paged_pool: usize,
    quota_peak_non_paged_pool: usize,
    quota_non_paged_pool: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut MemCounters, cb: u32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
}

/// (private bytes now, peak commit of the process).
fn mem() -> (usize, usize) {
    let mut c = MemCounters {
        cb: std::mem::size_of::<MemCounters>() as u32,
        ..Default::default()
    };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.private_usage, c.peak_pagefile_usage)
}

fn mb(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn env<T: std::str::FromStr>(k: &str, d: T) -> T {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

#[test]
#[ignore]
fn shotbench() {
    let pid: u32 = env("SHOTBENCH_PID", 0);
    assert!(pid != 0, "set SHOTBENCH_PID");
    let threads: usize = env("SHOTBENCH_THREADS", 12);
    let rounds: usize = env("SHOTBENCH_ROUNDS", 20);
    let distinct = env("SHOTBENCH_DISTINCT", 0) == 1;
    // Same as main(): without it rects are scaled to 96 dpi and frames come out smaller.
    unsafe { SetProcessDpiAwarenessContext(-4) };

    let (start, _) = mem();
    let stop = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicUsize::new(start));
    let sampler = {
        let (stop, peak) = (stop.clone(), peak.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                peak.fetch_max(mem().0, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };

    let t0 = Instant::now();
    let (mut ok, mut bytes) = (0usize, 0usize);
    for round in 0..rounds {
        let hs: Vec<_> = (0..threads)
            .map(|i| {
                let w = if distinct { 2560 - i as i32 } else { 2560 };
                // Like a /shot reply: holds the frame while sending it, then drops it.
                std::thread::spawn(move || {
                    view::shot(pid, true, w).map(|png| {
                        if round == 0 && i == 0 {
                            if let Ok(path) = std::env::var("SHOTBENCH_SAVE") {
                                let _ = std::fs::write(path, &png[..]);
                            }
                        }
                        png.len()
                    })
                })
            })
            .collect();
        for h in hs {
            if let Some(len) = h.join().unwrap() {
                ok += 1;
                bytes = len;
            }
        }
        let (now, _) = mem();
        println!(
            "round {:>2}: private {:>7.1} MB, peak so far {:>7.1} MB",
            round + 1,
            mb(now),
            mb(peak.load(Ordering::Relaxed))
        );
    }
    stop.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    let (after, commit_peak) = mem();
    let peak = peak.load(Ordering::Relaxed).max(after);
    println!(
        "SHOTBENCH pid={pid} threads={threads} rounds={rounds} distinct={distinct}: \
         {ok}/{} frames ok, last png {:.1} MB, {:.1}s\n\
         SHOTBENCH private start {:.1} MB, peak {:.1} MB (peak commit {:.1} MB), after {:.1} MB",
        threads * rounds,
        mb(bytes),
        t0.elapsed().as_secs_f64(),
        mb(start),
        mb(peak),
        mb(commit_peak),
        mb(after)
    );
    assert!(ok > 0, "no frame captured: is pid {pid} a visible window?");
}
