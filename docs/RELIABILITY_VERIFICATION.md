# Reliability changes — 2026-10-07

Implemented unsaved-work recovery, staged extension replacement, asynchronous Explorer loading and grapheme-aware caret movement/deletion. The text buffer remains `Vec<String>`; performance measurements were added before considering a buffer rewrite.

## Behavior

- Changed sessions checkpoint approximately every five seconds through one background writer. The mailbox retains only the newest pending snapshot. Checkpoint replacement is atomic; unchanged sessions do not repeatedly copy text or write to disk.
- Startup offers Restore/Discard for unsaved text, including when a file argument is supplied. Recovery preserves untitled buffers, cursors and selection. If the original's timestamp/size no longer matches the checkpoint, recovered text becomes untitled, requiring Save As. Original files remain unchanged until saved.
- Explicitly discarding a document on close excludes its unsaved text from the final checkpoint. If a later close prompt is cancelled, earlier Discard choices have not yet changed the open documents. A failed final checkpoint keeps the app open.
- Extension downloads use staging. Matching manifests, supported theme JSON and referenced icon assets are validated before promotion. Previous copies move to a stable backup and are restored if promotion fails. Windows exclusive file handles serialize updates across instances; staging/backup folders are excluded from theme discovery.
- Explorer directory reads and sorting run in workers. Request IDs reject obsolete folder refreshes; workspace generations reject results from a closed or replaced workspace. Branch detection reuses the existing Git worker, which now also checks the workspace generation.
- Left/right movement and ordinary Backspace/Delete follow Unicode grapheme boundaries. UI cursor placement snaps to boundaries; protocol byte/UTF-16 positions retain their precision. Rendering, bidirectional layout and font shaping were not rewritten.

## Verification

- `cargo test --offline --locked --all-targets`: 177 library tests, 64 binary tests and 9 integration tests passed; 16 explicitly ignored live/network tests were skipped.
- `cargo clippy --offline --locked --all-targets -- -D warnings` and `cargo build --offline --locked --bin lightline` passed using the standard registry dependency, without the temporary cached-source override.
- Native GUI smoke test used a separate `%APPDATA%` profile. Verified actual Backspace removes a complete joined emoji, periodic checkpointing, unchanged idle checkpoints, forced termination/restart, subsequent typing at the restored cursor, No on close, and Discard at startup. Test-created processes were closed.
- Regression tests cover combining marks, joined emoji, flags, skin tones and Indic clusters; deletion/undo; long-line navigation; missing/changed-original recovery; old/new session formats; extension ID validation; promotion rollback; failed local Git cloning; invalid theme validation; and exclusion of staged theme copies.
- The initial dependency download failed through Cargo. The registry archive was subsequently downloaded, its SHA-256 checked against the cached registry index, and copied into Cargo's cache. `Cargo.lock` retains the ordinary registry source/checksum; no machine-specific dependency path is committed.
- A pre-existing Python-root fixture assumed that its temporary directory had no enclosing repository. Its expectation now accounts for the existing marker-precedence rule when temporary files are created inside this workspace.

## Document measurements

Command: `cargo run --offline --locked --release --example measure_editing`.

Environment: Windows x86_64, Intel64 Family 6 Model 142 Stepping 12. This is a synthetic document benchmark, not an end-to-end UI benchmark or a before/after speedup claim. Timings vary with system load.

| Generated lines | Source bytes | Initialization | Recovery text copy | Process private committed after initialization |
| --- | ---: | ---: | ---: | ---: |
| 1,000 | 33,000 | 0.218 ms | 0.017 ms | 0.76 MiB |
| 100,000 | 3,300,000 | 23.54 ms | 4.28 ms | 8.81 MiB |
| 1,000,000 | 33,000,000 | 173.02 ms | 32.74 ms | 74.12 MiB |

Single-character insert p95 was approximately 1.1–3.2 microseconds across these fixtures/locations. Left/right navigation p95 was 0.3 microseconds on a one-million-byte ASCII line and 0.7 microseconds on a 440,000-byte Unicode line. The benchmark also reports undo/redo percentiles, working set and peak working set.

Memory counters include the benchmark process and allocator retention; they are not exact document allocation counts or LightLine GUI memory usage. Recovery text copying still happens on the UI thread before the worker serializes/writes it, so very large modified documents can cause a checkpoint pause. Snapshot deduplication avoids that work when state has not changed. Multiline editing, GUI typing/scrolling latency and a replacement text buffer remain separate measurements.

Local artifacts: `target/live-verification/reliability/results.txt` and `measure-editing.txt`. The app executable was built in debug mode; only the benchmark was built in release mode. Nothing was committed or published.
