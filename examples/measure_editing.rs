//! Repeatable document measurements; these are not UI frame-time measurements.
use lightline::document::{Document, Pos};
use std::hint::black_box;
use std::time::{Duration, Instant};

fn percentile(samples: &mut [Duration], percent: usize) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() - 1) * percent / 100]
}

fn memory(label: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of_val(&counters) as u32;
        if unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                counters.cb,
            )
        } != 0
        {
            println!(
                "  {label}: process working set {:.2} MiB, private committed {:.2} MiB, peak working set {:.2} MiB",
                counters.WorkingSetSize as f64 / 1_048_576.0,
                counters.PrivateUsage as f64 / 1_048_576.0,
                counters.PeakWorkingSetSize as f64 / 1_048_576.0
            );
        }
    }
    #[cfg(not(windows))]
    let _ = label;
}

fn main() {
    println!(
        "Document benchmark: {} / {}, release={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        !cfg!(debug_assertions)
    );
    memory("baseline");
    for lines in [1_000, 100_000, 1_000_000] {
        let source = "fn example() { let value = 42; }\n".repeat(lines);
        let mut document = Document::new();
        let start = Instant::now();
        document.seed(&source);
        println!(
            "{lines} lines, {} bytes: initialize {:?}",
            source.len(),
            start.elapsed()
        );
        drop(source);
        memory("after initialization, fixture source released");
        for line in [0, lines / 2, lines - 1] {
            let pos = Pos { line, byte: 0 };
            let mut edits = Vec::new();
            let mut undo = Vec::new();
            let mut redo = Vec::new();
            for _ in 0..101 {
                let start = Instant::now();
                black_box(document.replace(pos, pos, "x"));
                edits.push(start.elapsed());
                let start = Instant::now();
                black_box(document.undo());
                undo.push(start.elapsed());
                let start = Instant::now();
                black_box(document.redo());
                redo.push(start.elapsed());
                document.undo();
            }
            println!(
                "  line {line}: insert p50 {:?}, p95 {:?}; undo p95 {:?}; redo p95 {:?}",
                percentile(&mut edits, 50),
                percentile(&mut edits, 95),
                percentile(&mut undo, 95),
                percentile(&mut redo, 95)
            );
        }
        let start = Instant::now();
        black_box(document.text());
        println!("  recovery text snapshot {:?}", start.elapsed());
        memory("after snapshot");
    }
    for text in ["x".repeat(1_000_000), "👩‍💻e\u{301}🇵🇰".repeat(20_000)] {
        let mut document = Document::new();
        document.seed(&text);
        let end = document.end();
        let mut samples = Vec::new();
        for _ in 0..1_001 {
            let start = Instant::now();
            let previous = document.previous(end);
            black_box(document.next(previous));
            samples.push(start.elapsed());
        }
        println!(
            "long line {} bytes: left/right p50 {:?}, p95 {:?}",
            text.len(),
            percentile(&mut samples, 50),
            percentile(&mut samples, 95)
        );
    }
}
