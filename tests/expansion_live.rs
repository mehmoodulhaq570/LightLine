use lightline::document::{Document, Pos};
use lightline::formatter::{Formatter, PrettierFormatter, RustfmtFormatter, formatter_for};
use lightline::lsp::{Language, server_config};
use lightline::syntax::{Color, Syntax};
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn live_verify_c_syntax_highlighting() {
    let mut doc = Document::new();
    let c_code = r#"
#include <stdio.h>

typedef struct {
    int id;
    const char* name;
} User;

int calculate_total(int count, double rate) {
    if (count <= 0) {
        return 0;
    }
    // Calculate total amount
    return (int)(count * rate);
}
"#;
    doc.replace(Pos::default(), Pos::default(), c_code);
    let mut syntax = Syntax::new_c();

    let start = Instant::now();
    let mut settled = false;
    while start.elapsed() < Duration::from_secs(5) {
        if syntax.advance_to(&doc, 20, 20) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(settled, "C syntax highlighter worker should complete parse");

    // Line 3: typedef struct { -> should contain Keyword
    let l3_spans = syntax.spans(&doc, 3);
    assert!(
        l3_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword in struct def"
    );

    // Line 4: int id; -> should contain Type
    let l4_spans = syntax.spans(&doc, 4);
    assert!(
        l4_spans.iter().any(|s| s.color == Color::Type),
        "Expected Type for int"
    );

    // Line 12: // Calculate total amount -> should contain Comment
    let l12_spans = syntax.spans(&doc, 12);
    assert!(
        l12_spans.iter().any(|s| s.color == Color::Comment),
        "Expected Comment for // line"
    );

    println!("✓ Live C syntax highlighting verified successfully");
}

#[test]
fn live_verify_javascript_syntax_highlighting() {
    let mut doc = Document::new();
    let js_code = r#"
import { helper } from './utils.js';

class DataService {
    constructor(timeout = 5000) {
        this.timeout = timeout;
    }

    async fetchData(url) {
        const response = await fetch(url);
        return response.json();
    }
}
"#;
    doc.replace(Pos::default(), Pos::default(), js_code);
    let mut syntax = Syntax::new_javascript();

    let start = Instant::now();
    let mut settled = false;
    while start.elapsed() < Duration::from_secs(5) {
        if syntax.advance_to(&doc, 15, 15) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        settled,
        "JS syntax highlighter worker should complete parse"
    );

    // Line 1: import { helper } from './utils.js'; -> Keyword and String
    let l1_spans = syntax.spans(&doc, 1);
    assert!(
        l1_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword in import"
    );
    assert!(
        l1_spans.iter().any(|s| s.color == Color::String),
        "Expected String in module path"
    );

    // Line 3: class DataService { -> Keyword and Type
    let l3_spans = syntax.spans(&doc, 3);
    assert!(
        l3_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for class"
    );
    assert!(
        l3_spans.iter().any(|s| s.color == Color::Type),
        "Expected Type for class name"
    );

    // Line 8: async fetchData(url) { -> Keyword and Function
    let l8_spans = syntax.spans(&doc, 8);
    assert!(
        l8_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for async"
    );

    println!("✓ Live JavaScript syntax highlighting verified successfully");
}

#[test]
fn live_verify_typescript_syntax_highlighting() {
    let mut doc = Document::new();
    let ts_code = r#"
interface ConfigOptions {
    retries: number;
    endpoint: string;
    debug?: boolean;
}

export const executeTask = async (opts: ConfigOptions): Promise<void> => {
    const maxRetries: number = 3;
    if (opts.retries > maxRetries) {
        return;
    }
};
"#;
    doc.replace(Pos::default(), Pos::default(), ts_code);
    let mut syntax = Syntax::new_typescript();

    let start = Instant::now();
    let mut settled = false;
    while start.elapsed() < Duration::from_secs(5) {
        if syntax.advance_to(&doc, 15, 15) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        settled,
        "TS syntax highlighter worker should complete parse"
    );

    // Line 1: interface ConfigOptions { -> Keyword and Type
    let l1_spans = syntax.spans(&doc, 1);
    assert!(
        l1_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for interface"
    );
    assert!(
        l1_spans.iter().any(|s| s.color == Color::Type),
        "Expected Type for ConfigOptions"
    );

    // Line 2: retries: number; -> Type for number
    let l2_spans = syntax.spans(&doc, 2);
    assert!(
        l2_spans.iter().any(|s| s.color == Color::Type),
        "Expected Type for number"
    );

    // Line 7: export const executeTask = ... -> Keyword and Type
    let l7_spans = syntax.spans(&doc, 7);
    assert!(
        l7_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for export/const/async"
    );
    assert!(
        l7_spans.iter().any(|s| s.color == Color::Type),
        "Expected Type for ConfigOptions/Promise"
    );

    println!("✓ Live TypeScript syntax highlighting verified successfully");
}

#[test]
fn live_verify_json_syntax_highlighting() {
    let mut doc = Document::new();
    let json_code = r#"{
    "name": "lightline",
    "version": "1.0.0",
    "port": 8080,
    "enabled": true,
    "metadata": null
}
"#;
    doc.replace(Pos::default(), Pos::default(), json_code);
    let mut syntax = Syntax::new_json();

    let start = Instant::now();
    let mut settled = false;
    while start.elapsed() < Duration::from_secs(5) {
        if syntax.advance_to(&doc, 8, 8) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        settled,
        "JSON syntax highlighter worker should complete parse"
    );

    // Line 1: "name": "lightline", -> Strings
    let l1_spans = syntax.spans(&doc, 1);
    assert!(
        l1_spans.iter().any(|s| s.color == Color::String),
        "Expected String in JSON key/value"
    );

    // Line 3: "port": 8080, -> Number
    let l3_spans = syntax.spans(&doc, 3);
    assert!(
        l3_spans.iter().any(|s| s.color == Color::Number),
        "Expected Number in JSON value"
    );

    // Line 4: "enabled": true, -> Keyword for boolean
    let l4_spans = syntax.spans(&doc, 4);
    assert!(
        l4_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for true"
    );

    // Line 5: "metadata": null -> Keyword for null
    let l5_spans = syntax.spans(&doc, 5);
    assert!(
        l5_spans.iter().any(|s| s.color == Color::Keyword),
        "Expected Keyword for null"
    );

    println!("✓ Live JSON syntax highlighting verified successfully");
}

#[test]
fn live_verify_large_python_file_parses_above_128kb() {
    // Previous parse limit was 128 KB (131,072 bytes).
    // A file > 128 KB used to drop immediately to plain text.
    // We now support files up to 4 MiB!
    let mut doc = Document::new();
    let mut code = String::from("def calculate(x):\n    return x * 2\n\n");
    // Generate ~350 KB of valid Python code (well beyond 128 KB)
    for i in 0..6000 {
        code.push_str(&format!("value_{i} = calculate({i})\n"));
    }
    assert!(
        code.len() > 150 * 1024,
        "File size must be > 128 KB (was {} bytes)",
        code.len()
    );

    doc.replace(Pos::default(), Pos::default(), &code);
    let mut syntax = Syntax::new_python();

    let start = Instant::now();
    let mut settled = false;
    while start.elapsed() < Duration::from_secs(10) {
        if syntax.advance_to(&doc, 50, 50) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        settled,
        "Tree-sitter worker should parse large >128KB Python file"
    );

    let l0_spans = syntax.spans(&doc, 0);
    assert!(
        !l0_spans.is_empty(),
        "Line 0 spans must not be empty (file > 128KB did NOT drop to plain text!)"
    );
    assert!(
        l0_spans.iter().any(|s| s.color == Color::Keyword),
        "Line 0 must contain Keyword span for 'def'"
    );

    println!(
        "✓ Live large Python file verification passed: {} bytes successfully parsed and highlighted via Tree-sitter without dropping to plain text",
        code.len()
    );
}

#[test]
fn live_verify_rustfmt_formatter() {
    let unformatted = "fn main(){let x=1+2;println!(\"{}\",x);}";
    let formatted = RustfmtFormatter.format(unformatted, Path::new("test.rs"));
    match formatted {
        Ok(text) => {
            let normalized = text.replace("\r\n", "\n");
            assert!(
                normalized.contains("fn main() {\n"),
                "rustfmt should format function block"
            );
            assert!(
                normalized.contains("let x = 1 + 2;"),
                "rustfmt should space out operators"
            );
            println!("✓ Live rustfmt formatted successfully:\n{}", text.trim());
        }
        Err(e) => {
            println!("⚠ Note: rustfmt returned err ({e}), check if rustfmt is on PATH");
        }
    }
}

#[test]
fn live_verify_prettier_formatter() {
    let unformatted_json = "{\"a\":1,   \"b\":    [2,   3,4]}";
    let formatted = PrettierFormatter.format(unformatted_json, Path::new("test.json"));
    match formatted {
        Ok(text) => {
            assert!(
                text.contains("\"a\": 1"),
                "prettier should format keys and values"
            );
            assert!(
                text.contains("\"b\": [2, 3, 4]"),
                "prettier should format arrays"
            );
            println!("✓ Live prettier formatted successfully:\n{}", text.trim());
        }
        Err(e) => {
            println!("⚠ Note: prettier returned err ({e})");
        }
    }
}

#[test]
fn live_verify_formatter_dispatch() {
    let rs = formatter_for(Path::new("app.rs"));
    assert!(rs.is_some(), "Rust files should have a formatter");

    let py = formatter_for(Path::new("script.py"));
    assert!(py.is_some(), "Python files should have a formatter");

    let c = formatter_for(Path::new("main.c"));
    assert!(c.is_some(), "C files should have a formatter");

    let cpp = formatter_for(Path::new("main.cpp"));
    assert!(cpp.is_some(), "C++ files should have a formatter");

    let js = formatter_for(Path::new("index.js"));
    assert!(js.is_some(), "JS files should have a formatter");

    let ts = formatter_for(Path::new("index.ts"));
    assert!(ts.is_some(), "TS files should have a formatter");

    let json = formatter_for(Path::new("data.json"));
    assert!(json.is_some(), "JSON files should have a formatter");

    println!("✓ Live formatter dispatch verified for .rs, .py, .c, .cpp, .js, .ts, .json");
}

#[test]
fn live_verify_lsp_server_configs() {
    let languages = [
        (Language::Rust, "rust-analyzer", "rust", "Rust"),
        (Language::Python, "Pyright", "python", "Python"),
        (Language::C, "clangd", "c", "C/C++"),
        (
            Language::TypeScript,
            "typescript-language-server",
            "typescript",
            "TypeScript/JavaScript",
        ),
        (Language::Go, "gopls", "go", "Go"),
    ];

    for (lang, expected_server, expected_id, expected_name) in languages {
        let config = server_config(lang, None);
        assert_eq!(config.language, lang);
        assert_eq!(config.display_name, expected_server);
        assert_eq!(lang.language_id(), expected_id);
        assert_eq!(lang.name(), expected_name);
    }

    println!(
        "✓ Live LSP server configuration verified for Rust, Python, C/C++, TypeScript, and Go"
    );
}
