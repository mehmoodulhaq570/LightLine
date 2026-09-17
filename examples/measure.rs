use my_editor::document::{Document, Pos};
use std::fs;
use std::time::Instant;

fn main() -> std::io::Result<()> {
    let path = std::env::temp_dir().join(format!("my-editor-measure-{}.txt", std::process::id()));
    let mut data = String::with_capacity(3_000_000);
    for i in 0..100_000 {
        data.push_str(&format!("line {i}: fn example() {{}}\n"));
    }
    fs::write(&path, data)?;

    let start = Instant::now();
    let mut document = Document::open(path.clone())?;
    let open = start.elapsed();

    let pos = Pos {
        line: 50_000,
        byte: 0,
    };
    let start = Instant::now();
    document.replace(pos, pos, "x");
    let edit = start.elapsed();

    let start = Instant::now();
    document.save(&path)?;
    let save = start.elapsed();

    println!("100,000-line file: open {open:?}, insert {edit:?}, save {save:?}");
    fs::remove_file(path)?;
    Ok(())
}
