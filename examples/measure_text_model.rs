//! Text model benchmark: the costs of `Document` (one `String` per line) on
//! the workloads the editor puts on it, from a 3,000-line file up to the
//! largest file LightLine opens.
//!
//!     cargo run --release --example measure_text_model -- [--dir <folder>] [--max-mb <n>] [--only <text>]
//!
//! Fixtures are written to `--dir` (default: the temp folder) and deleted
//! afterwards. `--max-mb` sizes the largest fixture (default 120, just under
//! the 128 MiB open limit). `--only` runs just the fixtures whose name
//! contains that text (case-insensitive), and skips the line-insert
//! comparison unless the text is "insert". CPU cost is the thread's cycle count
//! (`QueryThreadCycleTime`), which other busy processes affect much less
//! than wall-clock time; both are reported. Results are in
//! docs/TEXT_MODEL_BENCHMARK.md.
use lightline::document::{Document, Pos};
use lightline::text_search::{Matcher, SearchOptions};
use lightline::workflow;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Sample {
    wall: Duration,
    cycles: u64,
}

// Declared here: the crate doesn't enable the windows-sys feature that has it.
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryThreadCycleTime(
        thread: windows_sys::Win32::Foundation::HANDLE,
        cycles: *mut u64,
    ) -> i32;
}

fn thread_cycles() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetCurrentThread;
        let mut cycles = 0u64;
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
        cycles
    }
    #[cfg(not(windows))]
    0
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Sample) {
    let cycles = thread_cycles();
    let start = Instant::now();
    let value = f();
    let wall = start.elapsed();
    let sample = Sample {
        wall,
        cycles: thread_cycles() - cycles,
    };
    (value, sample)
}

fn private_bytes() -> (u64, u64) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of_val(&counters) as u32;
        let ok = unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                counters.cb,
            )
        };
        if ok != 0 {
            return (
                counters.PrivateUsage as u64,
                counters.PeakPagefileUsage as u64,
            );
        }
    }
    (0, 0)
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn show(duration: Duration) -> String {
    let micros = duration.as_secs_f64() * 1e6;
    if micros < 1000.0 {
        format!("{micros:.1} µs")
    } else {
        format!("{:.2} ms", micros / 1000.0)
    }
}

// Median and 95th percentile, wall and cycles, plus the fastest and slowest
// wall time so the spread under load is visible.
fn report(label: &str, samples: &mut [Sample]) {
    samples.sort_by_key(|s| s.wall);
    let wall = |p: usize| samples[(samples.len() - 1) * p / 100].wall;
    let mut cycles: Vec<u64> = samples.iter().map(|s| s.cycles).collect();
    cycles.sort_unstable();
    let cyc = |p: usize| cycles[(cycles.len() - 1) * p / 100];
    println!(
        "  {label:<38} p50 {:>10}  p95 {:>10}  [{} .. {}]  cycles p50 {:>12}  p95 {:>12}  (n={})",
        show(wall(50)),
        show(wall(95)),
        show(samples[0].wall),
        show(samples[samples.len() - 1].wall),
        cyc(50),
        cyc(95),
        samples.len()
    );
}

fn repeat(label: &str, n: usize, mut f: impl FnMut() -> Sample) {
    let mut samples: Vec<Sample> = (0..n).map(|_| f()).collect();
    report(label, &mut samples);
}

// Lines of real Rust (src/lsp.rs), repeated until `lines` long, with `eol`
// between lines; with `mixed`, every tenth line ends in LF instead.
fn code_fixture(lines: usize, eol: &str, mixed: bool) -> String {
    let source: Vec<&str> = include_str!("../src/lsp.rs").lines().collect();
    let mut text = String::new();
    for index in 0..lines {
        if index > 0 {
            text.push_str(if mixed && index % 10 == 0 { "\n" } else { eol });
        }
        text.push_str(source[index % source.len()]);
    }
    text
}

// Minified-JavaScript-like: a few very long lines.
fn long_line_fixture(lines: usize, bytes_per_line: usize) -> String {
    let unit = "function a(b,c){return b+c};var d=a(1,2);";
    let line = unit.repeat(bytes_per_line / unit.len());
    vec![line; lines].join("\n")
}

fn bytes_for_lines(lines: usize) -> usize {
    let source: Vec<&str> = include_str!("../src/lsp.rs").lines().collect();
    let total: usize = source.iter().map(|l| l.len() + 1).sum();
    total * lines / source.len()
}

struct Fixture {
    name: String,
    text: String,
    // Whether to run the operations that hold several copies of the text
    // (Replace All and its undo, gutter diff).
    heavy: bool,
}

fn edit_points(doc: &Document) -> [(&'static str, usize); 3] {
    let last = doc.line_count() - 1;
    [("first line", 0), ("middle", last / 2), ("last line", last)]
}

fn run_fixture(fixture: Fixture, dir: &Path) {
    let path = dir.join(format!("lightline-bench-{}.txt", std::process::id()));
    std::fs::write(&path, &fixture.text).expect("write fixture");
    let bytes = fixture.text.len();
    let head_text = fixture.heavy.then(|| fixture.text.clone());
    drop(fixture.text);

    let (before, _) = private_bytes();
    let (doc, open) = measure(|| Document::open(path.clone()).expect("open fixture"));
    let (after, _) = private_bytes();
    let mut doc = doc;
    let lines = doc.line_count();
    println!(
        "\n{} — {} lines, {:.1} MiB",
        fixture.name,
        lines,
        mib(bytes as u64)
    );
    report("open", &mut [open]);
    let held = after.saturating_sub(before);
    println!(
        "  memory held by the document: {:.1} MiB = {:.2}x the file, {:.0} bytes/line over the text",
        mib(held),
        held as f64 / bytes as f64,
        (held as f64 - bytes as f64) / lines as f64
    );

    // Keystrokes: type a character and delete it again.
    for (label, line) in edit_points(&doc) {
        let byte = doc.line(line).len() / 2;
        let byte = (0..=byte)
            .rev()
            .find(|b| doc.line(line).is_char_boundary(*b))
            .unwrap_or(0);
        let at = Pos { line, byte };
        let after_x = Pos {
            line,
            byte: byte + 1,
        };
        let mut typed = Vec::new();
        let mut deleted = Vec::new();
        for _ in 0..201 {
            typed.push(measure(|| black_box(doc.replace(at, at, "x"))).1);
            deleted.push(measure(|| black_box(doc.replace(at, after_x, ""))).1);
        }
        report(&format!("type a character, {label}"), &mut typed);
        report(&format!("delete a character, {label}"), &mut deleted);
    }

    // Enter splits a line (one more line); Backspace at the start of the
    // next line joins them again (one fewer).
    for (label, line) in edit_points(&doc) {
        let at = Pos {
            line,
            byte: doc.line(line).len() / 2,
        };
        let at = doc.clamp(at);
        let mut enter = Vec::new();
        let mut join = Vec::new();
        for _ in 0..51 {
            enter.push(measure(|| black_box(doc.replace(at, at, "\n"))).1);
            let next = Pos {
                line: line + 1,
                byte: 0,
            };
            join.push(measure(|| black_box(doc.replace(at, next, ""))).1);
        }
        report(&format!("Enter, {label}"), &mut enter);
        report(&format!("Backspace joining lines, {label}"), &mut join);
    }

    // Paste a 50-line block in the middle, then undo and redo it.
    let block: String = (0..50)
        .map(|i| format!("    let pasted_{i} = compute({i}, \"value\");\n"))
        .collect();
    let middle = Pos {
        line: lines / 2,
        byte: 0,
    };
    let (mut paste, mut undo, mut redo) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..21 {
        paste.push(measure(|| black_box(doc.replace(middle, middle, &block))).1);
        undo.push(measure(|| black_box(doc.undo())).1);
        redo.push(measure(|| black_box(doc.redo())).1);
        doc.undo();
    }
    report("paste 50 lines, middle", &mut paste);
    report("undo the paste", &mut undo);
    report("redo the paste", &mut redo);

    // 100 carets spread through the file, all typing (one undo group), then
    // Enter at all of them; each undone as Ctrl+Z does, step by step.
    let carets: Vec<usize> = (0..100).map(|i| i * (lines - 1) / 99).collect();
    for (label, text) in [("type", "x"), ("Enter", "\n")] {
        let mut edit = Vec::new();
        let mut undo_group = Vec::new();
        for _ in 0..5 {
            edit.push(
                measure(|| {
                    doc.begin_group();
                    // Bottom to top, as multi-cursor editing does, so earlier
                    // edits don't move the later carets.
                    for &line in carets.iter().rev() {
                        let at = Pos { line, byte: 0 };
                        doc.replace(at, at, text);
                    }
                    doc.end_group();
                })
                .1,
            );
            undo_group.push(measure(|| while doc.undo().is_some() && doc.undo_continues() {}).1);
        }
        report(&format!("100 carets: {label}"), &mut edit);
        report(&format!("undo 100-caret {label}"), &mut undo_group);
    }

    // Whole-document copies the app makes: the recovery snapshot, opening
    // the file in a language server (`text()`), the syntax worker's reset
    // (`text_range`), and the LSP size check (`byte_len`).
    let n = if bytes > 50_000_000 { 3 } else { 9 };
    repeat("text() (recovery snapshot, LSP open)", n, || {
        measure(|| black_box(doc.text())).1
    });
    repeat("text_range(start, end) (syntax reset)", n, || {
        measure(|| black_box(doc.text_range(Pos::default(), doc.end()))).1
    });
    repeat("byte_len() (LSP size check)", n, || {
        measure(|| black_box(doc.byte_len())).1
    });

    // Find: the match count the find box shows, over every line.
    let matcher = Matcher::new("self.", SearchOptions::default()).expect("matcher");
    let mut count = 0;
    repeat("count matches of \"self.\"", n, || {
        let (found, sample) = measure(|| doc.find_matches(&matcher).len());
        count = found;
        sample
    });

    // Replace All, as the find box does it: every match found first, then
    // one edit from the first match to the end of the last.
    if fixture.heavy && count > 0 {
        let (before, peak_before) = private_bytes();
        let (_, replace) = measure(|| {
            let matches = doc.find_matches(&matcher);
            let (first, last) = (matches[0].0, matches[matches.len() - 1].1);
            let mut text = String::new();
            let mut from = first;
            for &(start, end) in &matches {
                text.push_str(&doc.text_range(from, start));
                text.push_str("this.");
                from = end;
            }
            doc.replace(first, last, &text);
        });
        let (after, peak_after) = private_bytes();
        report(&format!("Replace All ({count} matches)"), &mut [replace]);
        println!(
            "    memory kept for its undo: {:.1} MiB; peak rose {:.1} MiB",
            mib(after.saturating_sub(before)),
            mib(peak_after.saturating_sub(peak_before))
        );
        let (_, undo) = measure(|| black_box(doc.undo()));
        report("undo Replace All", &mut [undo]);
        let (_, redo) = measure(|| black_box(doc.redo()));
        report("redo Replace All", &mut [redo]);
        doc.undo();
    }

    // Save, through the same atomic write the app uses.
    let save_path = dir.join(format!("lightline-bench-save-{}.txt", std::process::id()));
    repeat("save", 3, || {
        measure(|| doc.save_over_disk_changes(&save_path).expect("save")).1
    });
    let _ = std::fs::remove_file(&save_path);

    // Gutter change marks: HEAD's text against the buffer with one line
    // changed in the middle (the app does this only up to 50,000 lines).
    if let Some(head) = head_text.as_deref() {
        let at = Pos {
            line: lines / 2,
            byte: 0,
        };
        doc.replace(at, at, "x");
        repeat("gutter diff, one line changed", n, || {
            measure(|| black_box(workflow::compute_gutter_diff(head, doc.lines()))).1
        });
    }

    drop(doc);
    let _ = std::fs::remove_file(&path);
}

// The cost Enter pays for keeping lines in one Vec: moving every line after
// it. Compared with lines kept in chunks of at most CHUNK, which is what a
// rope or B-tree of lines does to bound it. Strings are empty here because
// only the line headers move.
fn line_insert_comparison(lines: usize) {
    const CHUNK: usize = 1024;
    let mut flat: Vec<String> = vec![String::new(); lines];
    let mut chunks: Vec<Vec<String>> = (0..lines.div_ceil(CHUNK))
        .map(|i| vec![String::new(); CHUNK.min(lines - i * CHUNK)])
        .collect();
    let mut flat_samples = Vec::new();
    let mut chunk_samples = Vec::new();
    for _ in 0..51 {
        flat_samples.push(
            measure(|| {
                flat.insert(0, String::new());
                flat.remove(0);
            })
            .1,
        );
        let line = black_box(0usize);
        chunk_samples.push(
            measure(|| {
                // Find the chunk by walking lengths, as a flat chunk list does.
                let mut index = 0;
                let mut first = 0;
                while index + 1 < chunks.len() && first + chunks[index].len() <= line {
                    first += chunks[index].len();
                    index += 1;
                }
                chunks[index].insert(line - first, String::new());
                if chunks[index].len() > CHUNK * 2 {
                    let tail = chunks[index].split_off(CHUNK);
                    chunks.insert(index + 1, tail);
                }
                chunks[index].remove(line - first);
                black_box(&chunks);
            })
            .1,
        );
    }
    println!("\nInsert and remove a line at the top of {lines} lines");
    report("one Vec of lines (today)", &mut flat_samples);
    report(&format!("chunks of {CHUNK} lines"), &mut chunk_samples);
}

fn main() {
    let mut dir = std::env::temp_dir();
    let mut max_mb = 120usize;
    let mut only: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = PathBuf::from(args.next().expect("--dir needs a folder")),
            "--max-mb" => {
                max_mb = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--max-mb needs a number")
            }
            "--only" => only = Some(args.next().expect("--only needs some text").to_lowercase()),
            other => panic!("unknown argument {other}"),
        }
    }
    lightline::local_history::set_enabled(false);
    println!(
        "Text model benchmark: {} / {}, release={}, fixtures in {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        !cfg!(debug_assertions),
        dir.display()
    );

    let source_lines = include_str!("../src/lsp.rs").lines().count();
    let max_lines = max_mb * 1_048_576 / (bytes_for_lines(1000) / 1000).max(1);
    let fixtures = [
        (
            format!("Rust, {source_lines} lines (src/lsp.rs)"),
            source_lines,
            "\n",
            false,
            true,
        ),
        ("Rust, 100k lines".to_owned(), 100_000, "\n", false, true),
        ("Rust, 1M lines".to_owned(), 1_000_000, "\n", false, true),
        (
            "Rust, 1M lines, CRLF".to_owned(),
            1_000_000,
            "\r\n",
            false,
            false,
        ),
        (
            "Rust, 1M lines, mixed CRLF/LF".to_owned(),
            1_000_000,
            "\r\n",
            true,
            false,
        ),
        // Replace All here would hold about six copies of the text at once;
        // its cost is measured on the 1M-line file and grows linearly.
        (
            format!("Rust, ~{max_mb} MiB"),
            max_lines,
            "\n",
            false,
            false,
        ),
    ];
    let wanted = |name: &str| {
        only.as_ref()
            .is_none_or(|only| name.to_lowercase().contains(only.as_str()))
    };
    for (name, lines, eol, mixed, heavy) in fixtures {
        if !wanted(&name) {
            continue;
        }
        run_fixture(
            Fixture {
                name,
                text: code_fixture(lines, eol, mixed),
                heavy,
            },
            &dir,
        );
    }
    let minified = "Minified, 20 lines of 512 KiB";
    if wanted(minified) {
        run_fixture(
            Fixture {
                name: minified.to_owned(),
                text: long_line_fixture(20, 512 * 1024),
                heavy: false,
            },
            &dir,
        );
    }

    if only.is_none() || wanted("insert") {
        for lines in [100_000, 1_000_000, max_lines] {
            line_insert_comparison(lines);
        }
    }
}
