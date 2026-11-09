# Text model benchmark

`Document` keeps a file as one `String` per line (`Vec<String>`). The technical direction called this a prototype, "kept until benchmarks justify a rope". This page measures it on the workloads the editor actually puts on it, lists every place the app touches the whole document, and makes a recommendation.

**Short answer:** keep `Vec<String>` for now. Typing is fast at every size, and after the fix below it is fast on very long lines too. The one structural cost is Enter, Backspace across a line break, and multi-line paste: they move every line below the edit, which passes the 2–3 ms keystroke budget near the top of files over about 650,000 lines (it grows by about 4.5 µs per 1,000 lines below the edit). When that matters, store lines in chunks (a B-tree of line chunks) instead of switching to a byte rope. Chunks keep the `line(i) -> &str` API that 114 call sites use, and the measured cost of inserting a line drops from 25 ms to 4 µs at 3.5 million lines.

## Method

- Machine: Intel Core i5-8365U (4 cores, 1.6 GHz base), 16 GB RAM, Windows 11, x64. Release build, Rust 1.97.1.
- Code: `cargo run --release --example measure_text_model -- [--dir <folder>] [--max-mb <n>] [--only <text>]`. It writes each fixture to disk, opens it with `Document::open`, runs each operation through the same `Document` calls the app makes, and deletes the files afterwards.
- Fixtures: lines of real Rust (`src/lsp.rs`) repeated to 3,148 lines, 100k lines, 1M lines (LF, all-CRLF and mixed CRLF/LF), and 3.5M lines (121 MiB, just under the 128 MiB open limit); plus 20 lines of 512 KiB each, like minified JavaScript.
- Timing: median (p50) and 95th percentile over repeated runs, with the thread's CPU cycles from `QueryThreadCycleTime` (about 1.9 billion a second on this machine). Other agents were compiling on the same PC during the runs, so wall-clock times vary by up to 2× between runs. Comparisons below were run alternately in the same conditions.
- Open times are CPU time from the cycle counts: wall time also included Windows Defender scanning each freshly written fixture (for example 690 ms wall against 29 ms of CPU at 100k lines).
- Limits: Replace All and the gutter diff were measured up to 1M lines, because on the 121 MiB file Replace All holds about six copies of the text and the PC had under 2 GB of RAM free. Both grow linearly.

## Results

Times are p50 wall clock from one full run, before the fix described below; "type a character" rows after the fix are in the next section.

| Operation | 3k lines (0.1 MiB) | 100k lines (3.5 MiB) | 1M lines (35 MiB) | 3.5M lines (121 MiB) |
| --- | --- | --- | --- | --- |
| Open (CPU) | 1.4 ms | 29 ms | 175 ms | 680 ms |
| Memory held (× file size) | 1.3× | 2.5× | 2.1× | 2.3× (272 MiB) |
| Type or delete a character, anywhere | 0.5–0.9 µs | 0.8–1.7 µs | 0.7–0.9 µs | 0.9–1.3 µs |
| Enter / Backspace joining lines, first line | 3.5 / 2.7 µs | 0.58 / 0.61 ms | 4.5 / 4.8 ms | 17.8 / 17.0 ms |
| Enter / Backspace joining lines, middle | 1.9 / 1.4 µs | 70 / 54 µs | 2.4 / 2.6 ms | 8.0 / 8.4 ms |
| Enter / Backspace joining lines, last line | 0.7 / 0.5 µs | 0.9 / 0.8 µs | 1.3 / 1.2 µs | 1.8 / 1.7 µs |
| Paste 50 lines in the middle (undo, redo) | 17 µs (7, 13) | 97 µs (71, 92) | 2.4 ms (2.3, 2.0) | 9.9 ms (9.6, 10.0) |
| 100 carets typing a character (undo) | 149 µs (69) | 114 µs (65) | 77 µs (51) | 146 µs (78) |
| 100 carets pressing Enter (undo) | 0.30 ms (0.17) | 21 ms (21) | 250 ms (221) | 661 ms (676) |
| `text()`: recovery snapshot, opening in a language server | 0.12 ms | 4.2 ms | 22 ms | 89 ms |
| `text_range(start, end)`: syntax worker reset | 0.19 ms | 6.3 ms | 43 ms | 146 ms |
| `byte_len()`: LSP size check | 1.8 µs | 0.26 ms | 1.8 ms | 8.0 ms |
| Count matches of `self.` (find box) | 0.23 ms | 6.7 ms | 59 ms | 297 ms |
| Replace All `self.` → `this.` (undo) | 1.7 ms (0.74) | 43 ms (22) | 382 ms (247) | not run |
| Memory kept for Replace All's undo | 0.4 MiB | 14 MiB | 122 MiB | not run |
| Save (atomic write and flush) | 4.2 ms | 33 ms | 272 ms | 911 ms |
| Gutter diff against HEAD, one line changed | 0.40 ms | 16 ms | 128 ms | not run |

Line endings make little difference: at 1M lines, all-CRLF and mixed CRLF/LF files measured within the run-to-run spread of LF files (Enter at the first line 3.5 and 3.9 ms against 4.5 ms; memory 2.1× for all three). The per-line ending list a mixed file keeps is one byte a line.

Memory is 40–53 bytes a line on top of the text: each line's 24-byte `String` plus the heap allocation's own overhead and rounding. For typical code (35 bytes a line) that is just over twice the file size.

### Inserting a line: one `Vec` against chunks

Inserting and removing a line at the top of the file, with lines kept in one `Vec` (today) or in chunks of at most 1,024 lines:

| Lines | One `Vec` | Chunks of 1,024 |
| --- | --- | --- |
| 100,000 | 648 µs | 2.4 µs |
| 1,000,000 | 7.4 ms | 3.9 µs |
| 3,495,253 | 25.3 ms | 3.9 µs |

## Fixed in this change: typing on a very long line

Typing on a 512 KiB minified line cost about 1 ms a keystroke, growing with the line's length, for two reasons:

- `record_change` works out the edit's UTF-16 columns for the language server by encoding everything before the cursor as UTF-16, twice per edit.
- `replace_raw` copied the line's text before and after the edit and built a new line, even for one character.

Now `utf16_column` returns the byte offset when the text before the cursor is ASCII (checking that is much quicker than encoding), and an edit within one line changes the `String` in place with `replace_range`, which moves only the text after the edit. Line endings, folds and breakpoints are handled as before.

Before and after, run alternately twice under the same load (p50):

| Operation | Before | After |
| --- | --- | --- |
| Type a character, 512 KiB line | 0.98 ms, 2.31 ms | 29 µs, 51 µs |
| Delete a character, 512 KiB line | 0.95 ms, 2.29 ms | 26 µs, 45 µs |
| 100 carets typing, 512 KiB lines | 19.1 ms, 24.5 ms | 4.8 ms, 6.1 ms |
| Type a character, 3k-line file | 1.2 µs, 1.9 µs | 0.5 µs, 0.5 µs |
| 100 carets typing, 3k-line file | 352 µs, 177 µs | 41 µs, 89 µs |

Enter, Backspace across lines and paste were unchanged: they take the multi-line path, which this change doesn't touch.

## Where the app touches the whole document

Checked in `src/windows_app/` against the costs above:

| Work | When | Cost on the UI thread |
| --- | --- | --- |
| Enter, Backspace joining lines, multi-line paste, multi-caret Enter, and their undo | Every such keystroke | O(lines below the edit): see the table. Over 1 ms near the top of files above ~220k lines; 18 ms at 3.5M lines |
| Recovery snapshot (`Tab::recovery_text`, `text()`) | At most once per change, from a timer that fires 5 s after a repaint | 22 ms at 1M lines, 89 ms at 121 MiB: a pause every 5 s while typing in a very large file |
| Gutter diff (`start_gutter_diff`) | 120 ms after typing stops; skipped above 50,000 lines | Up to about 8 ms at the 50,000-line cap; each run also re-splits HEAD's text into lines |
| Find box match count (`update_find_count`) | Every character typed into the find box | 59 ms at 1M lines, 297 ms at 121 MiB. Since v0.9.0, files over 50,000 lines are counted 50,000 lines per timer message between keystrokes |
| Replace All (`replace_all_edit`) | On request | 382 ms at 1M lines, and its undo keeps old and new text: 122 MiB for a 35 MiB file |
| Save | On request | 272 ms at 1M lines, 0.9 s at 121 MiB, including the flush to disk |
| Opening in a language server (`text()`) | Once per file | Files over 2 MiB skip the language server |
| Syntax worker reset (`text_range`) | On open or reload, within the parse limit (4 MB by default) | About 7 ms at 4 MB |

Each keystroke sends only the edit to the language server (incremental `didChange`) and to the syntax worker, so neither copies the document while typing. The syntax worker does copy work on its own thread: it edits its own copy of the text and clones the line spans for each reparse, both bounded by the parse limit. Bracket colors rescan from the edited line down to the visible lines only.

## Recommendation

1. **Keep `Vec<String>` now.** Within a line, typing is under 1 µs on normal files and about 30 µs on a 512 KiB line. Enter stays under 0.1 ms in the middle of a 100k-line file. The editor's 2–3 ms keystroke budget is met everywhere except line-count-changing edits in very large files.
2. **When a structural change is needed, use chunks of lines, not a byte rope.** Paint, syntax, search, folding and brackets work line by line through `line(i)` (114 call sites); only the gutter diff takes the whole `lines()` slice. A B-tree of line chunks keeps that API, makes Enter about 4 µs at any size, and lets a snapshot share unchanged chunks (`Arc`) instead of copying 89 ms of text. A byte rope (ropey, crop) would also bound Enter, but every `line(i)` would need a line-index lookup and a copy or a chunked `&str`, a much larger change for no measured gain on typing.
3. **Smaller fixes that don't need a new model**, each measured above:
   - Cache HEAD's lines once per file for the gutter diff instead of splitting HEAD's text on every recompute: part of the up to ~8 ms each typing pause costs at the 50,000-line cap.
   - ~~Count find-box matches on a worker, or stop counting past a limit, for files over a few MB (297 ms per typed character at 121 MiB).~~ Done after v0.9.0: counted in slices between keystrokes; a typed character on a 1M-line file holds the window for 4–18 ms instead of 80–150 ms.
   - Write the recovery snapshot of very large files less often, or from a shared snapshot once lines are chunked (89 ms every 5 s while typing at 121 MiB).
   - `slice` could reserve its result's size up front: `text_range` takes 1.5–2× as long as `text()` for the same bytes.

### What would change the answer

- **Switch to chunked lines** if people edit files over about 650,000 lines (Enter near the top passes 3 ms), use multi-caret Enter on files over 50,000 lines (100 carets cost 21 ms at 100k lines), or if the 128 MiB open limit is raised (Enter is 18 ms at 121 MiB, and grows with it).
- **Consider a rope or piece table** only if memory becomes the constraint: today a file needs about 2.1–2.5× its size in RAM. A piece table opened over the file's own bytes would be close to 1×, but would give up the per-line `&str` that the renderer and highlighter rely on.
