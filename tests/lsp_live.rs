//! Run explicitly with `cargo test --offline --test lsp_live -- --ignored`.

use lightline::lsp::{Client, Command, Event, Position, Range, file_uri};
use std::fs;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a local rust-analyzer binary"]
fn rust_analyzer_publishes_diagnostics_and_hover() {
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("lsp-live-{}", std::process::id()));
    let source = root.join("src").join("main.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-live\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let source_text =
        "fn main() {\n    let answer: i32 = \"wrong\";\n    println!(\"{answer}\");\n}\n";
    fs::write(&source, source_text).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let uri = file_uri(&fs::canonicalize(source).unwrap());
    let (sender, receiver) = mpsc::channel();
    let client = Client::start(root.clone(), sender, Arc::new(|| {}));
    assert!(client.send(Command::Open {
        uri: uri.clone(),
        text: source_text.into(),
        version: 1
    }));
    let deadline = Instant::now() + Duration::from_secs(50);
    let mut ready = false;
    let mut diagnosed = false;
    let mut hovered = false;
    let mut hover_requested = false;
    let mut changed = false;
    let mut cleared = false;
    while Instant::now() < deadline && !(ready && diagnosed && hovered && cleared) {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(Event::Ready) => {
                println!("rust-analyzer ready");
                ready = true;
            }
            Ok(Event::Diagnostics {
                uri: actual,
                version,
                items,
            }) => {
                println!(
                    "diagnostics for {actual} version {version:?}: {:?}",
                    items.iter().map(|item| &item.message).collect::<Vec<_>>()
                );
                if !actual.eq_ignore_ascii_case(&uri) {
                    continue;
                }
                if changed
                    && version == Some(2)
                    && items
                        .iter()
                        .all(|item| !item.message.contains("mismatched types"))
                {
                    cleared = true;
                }
                if items
                    .iter()
                    .any(|item| item.message.contains("mismatched types"))
                {
                    diagnosed = true;
                    if !hover_requested {
                        client.send(Command::Hover {
                            id: 1000,
                            uri: uri.clone(),
                            version: 1,
                            position: Position {
                                line: 1,
                                character: 8,
                            },
                        });
                        hover_requested = true;
                    }
                }
            }
            Ok(Event::Hover {
                id: 1000,
                text: Some(text),
                ..
            }) => {
                println!("hover: {text}");
                hovered = text.contains("answer") || text.contains("i32");
                if hovered && !changed {
                    let source_line = "    let answer: i32 = \"wrong\";";
                    let start = source_line.find("\"wrong\"").unwrap() as u32;
                    assert!(client.send(Command::Change {
                        uri: uri.clone(),
                        version: 2,
                        range: Range {
                            start: Position {
                                line: 1,
                                character: start,
                            },
                            end: Position {
                                line: 1,
                                character: start + 7,
                            },
                        },
                        text: "7".into(),
                    }));
                    println!("sent incremental edit");
                    client.send(Command::Hover {
                        id: 1001,
                        uri: uri.clone(),
                        version: 2,
                        position: Position {
                            line: 1,
                            character: 8,
                        },
                    });
                    fs::write(
                        root.join("src").join("main.rs"),
                        source_text.replace("\"wrong\"", "7"),
                    )
                    .unwrap();
                    client.send(Command::Save { uri: uri.clone() });
                    changed = true;
                }
            }
            Ok(Event::Hover { id: 1001, text, .. }) => println!("hover after edit: {text:?}"),
            Ok(Event::Hover {
                id: 1000,
                text: None,
                ..
            }) => println!("hover was empty"),
            Ok(Event::Stopped(error)) => panic!("rust-analyzer stopped: {error}"),
            _ => {}
        }
    }
    assert!(ready, "rust-analyzer did not initialize");
    assert!(diagnosed, "expected a type diagnostic");
    assert!(hovered, "expected hover information for answer");
    assert!(cleared, "expected diagnostics for the incremental edit");
}
