use my_editor::document::{Document, Pos};
use my_editor::syntax::RustSyntax;
use std::time::{Duration, Instant};

fn wait_for_syntax(syntax: &mut RustSyntax, document: &Document, line: usize) {
    while !syntax.advance_to(document, line, 2_000) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn main() {
    let mut document = Document::new();
    let mut source = String::new();
    for index in 0..2_000 {
        source.push_str(&format!(
            "fn f{index}() {{ let value: i32 = 123; println!(\"{{}}\", value); }}\n"
        ));
    }
    document.replace(Pos::default(), Pos::default(), &source);
    let mut syntax = RustSyntax::new();
    let started = Instant::now();
    syntax.advance_to(&document, 1_000, 2_000);
    let scheduled = started.elapsed();
    wait_for_syntax(&mut syntax, &document, 1_000);
    let initial_ready = started.elapsed();

    let pos = Pos {
        line: 1_000,
        byte: 0,
    };
    document.replace(pos, pos, "// edit\n");
    syntax.invalidate_from(pos.line);
    let started = Instant::now();
    syntax.advance_to(&document, 1_001, 2_000);
    let edit_scheduled = started.elapsed();
    wait_for_syntax(&mut syntax, &document, 1_001);
    let edit_ready = started.elapsed();
    println!(
        "{} bytes: initial schedule {scheduled:?}, colors ready {initial_ready:?}; edit schedule {edit_scheduled:?}, colors ready {edit_ready:?}",
        source.len()
    );
}
