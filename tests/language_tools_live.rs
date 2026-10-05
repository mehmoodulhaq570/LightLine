//! Requires rustfmt, ruff, clang-format, clangd, typescript-language-server, and gopls.
//! Run with `cargo test --test language_tools_live -- --ignored --nocapture`.

use lightline::formatter::formatter_for;
use lightline::lsp::{Client, Command, Event, Language, Position, file_uri, same_file_uri};
use std::fs;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires installed external formatters"]
fn external_formatters_format_real_source() {
    for (file, source, expected) in [
        ("main.rs", "fn main(){let value=42;}", "let value = 42;"),
        ("main.py", "value=1+2\n", "value = 1 + 2"),
        ("main.c", "int main(){return 0;}", "return 0;"),
    ] {
        let output = formatter_for(file.as_ref())
            .unwrap()
            .format(source, file.as_ref())
            .unwrap();
        assert_ne!(output, source, "{file} must actually be reformatted");
        assert!(output.contains(expected), "{file}: {output}");
        println!("{file} formatted successfully");
    }
}

#[test]
#[ignore = "requires installed C, JavaScript/TypeScript, and Go language servers"]
fn language_servers_publish_diagnostics_and_hover() {
    let base = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("language-tools-live-{}", std::process::id()));
    for (language, file, source, line, character) in [
        (
            Language::C,
            "main.c",
            "#include <stdio.h>\nint value = \"bad\";\nint main(void) { printf(\"%d\", value); return value; }\n",
            1,
            5,
        ),
        (
            Language::C,
            "main.cpp",
            "#include <iostream>\nint value = \"bad\";\nint main() { std::cout << value; return value; }\n",
            1,
            5,
        ),
        (
            Language::TypeScript,
            "main.js",
            "// @ts-check\nconst value = 1;\nvalue.missing();\n",
            1,
            7,
        ),
        (
            Language::TypeScript,
            "main.ts",
            "const value: number = \"bad\";\nvalue.toFixed();\n",
            0,
            7,
        ),
        (
            Language::Go,
            "main.go",
            "package main\nfunc main() {\n var value int = \"bad\"\n println(value)\n}\n",
            2,
            6,
        ),
    ] {
        let root = base.join(file.replace('.', "-"));
        fs::create_dir_all(&root).unwrap();
        if language == Language::Go {
            fs::write(
                root.join("go.mod"),
                "module example.com/lightlineverify\n\ngo 1.23.0\n",
            )
            .unwrap();
        }
        if language == Language::TypeScript {
            fs::write(
                root.join("tsconfig.json"),
                "{\"compilerOptions\":{\"allowJs\":true,\"checkJs\":true,\"strict\":true}}",
            )
            .unwrap();
        }
        let path = root.join(file);
        fs::write(&path, source).unwrap();
        let uri = file_uri(&path);
        let (sender, receiver) = mpsc::channel();
        let client = Client::start(language, root, None, sender, Arc::new(|| {}));
        assert!(client.send(Command::Open {
            uri: uri.clone(),
            text: source.into(),
            version: 1
        }));
        let deadline = Instant::now() + Duration::from_secs(60);
        let (mut ready, mut diagnostics, mut hover) = (false, false, false);
        while Instant::now() < deadline && !(ready && diagnostics && hover) {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(Event::Ready { .. }) => {
                    ready = true;
                }
                Ok(Event::Diagnostics {
                    uri: actual, items, ..
                }) if same_file_uri(&actual, &uri) => {
                    assert!(
                        !items
                            .iter()
                            .any(|item| item.message.contains("file not found")),
                        "{file}: missing system headers: {:?}",
                        items.iter().map(|item| &item.message).collect::<Vec<_>>()
                    );
                    diagnostics |= items.iter().any(|item| item.severity == 1);
                    if diagnostics && !hover {
                        client.send(Command::Hover {
                            id: 1000,
                            uri: uri.clone(),
                            version: 1,
                            position: Position { line, character },
                        });
                    }
                }
                Ok(Event::Hover {
                    id: 1000,
                    text: Some(text),
                    ..
                }) => hover = !text.trim().is_empty(),
                Ok(Event::Stopped { message, .. }) => panic!("{file}: {message}"),
                _ => {}
            }
        }
        assert!(
            ready && diagnostics && hover,
            "{file}: ready={ready}, diagnostics={diagnostics}, hover={hover}"
        );
        println!("{file}: server ready, error diagnostics and hover verified");
        drop(client);
    }
}
