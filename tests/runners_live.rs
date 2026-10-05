//! Run with `cargo test --test runners_live -- --ignored --nocapture`.
//! Requires Node.js, tsx, Go, and Rust on PATH, plus Windows ConPTY.
#![cfg(windows)]

use lightline::runner::{RunCommand, RunPlan, prepare};
use lightline::terminal::{
    LaunchRequest, SessionKind, SessionStatus, TerminalService, TerminalSize,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("runners-live-{}", std::process::id()))
        .join(name);
    fs::create_dir_all(&root).unwrap();
    root
}

fn write(root: &Path, name: &str, text: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn start(plan: RunPlan) -> (TerminalService, lightline::terminal::SessionId) {
    let mut service = TerminalService::default();
    let id = service
        .start(
            SessionKind::ManagedRun,
            LaunchRequest::Run {
                command: plan.powershell_command().unwrap(),
                cwd: plan.cwd,
            },
            TerminalSize::new(24, 120).unwrap(),
        )
        .unwrap();
    (service, id)
}

fn finish(
    service: &mut TerminalService,
    id: lightline::terminal::SessionId,
) -> (SessionStatus, String) {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut output = String::new();
    while Instant::now() < deadline {
        for event in service.poll(16) {
            output = event.snapshot.text();
            if event.snapshot.status.is_final() {
                return (event.snapshot.status.clone(), output);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = service.stop(id);
    panic!("Run timed out: {output}");
}

fn verify(file: &Path, marker: &str) {
    let (mut service, id) = start(prepare(file).unwrap());
    let (status, output) = finish(&mut service, id);
    assert_eq!(status, SessionStatus::Exited { code: 0 }, "{output}");
    assert!(output.contains(marker), "{file:?}: {output}");
    println!("{}: {marker}, exit 0", file.display());
    service.remove(id).unwrap();
}

#[test]
#[ignore = "requires installed language tools and live Windows ConPTY"]
fn runs_javascript_typescript_go_and_rust_projects_and_files() {
    let root = fixture("spaces ' $(Write-Output INJECTED) test");
    let js = write(&root, "javascript/main.js", "console.log('NODE_RUN_OK');\n");
    verify(&js, "NODE_RUN_OK");

    let ts = write(
        &root,
        "typescript/main.ts",
        "const value: number = 42; console.log('TS_RUN_OK', value);\n",
    );
    verify(&ts, "TS_RUN_OK 42");
    write(
        &root,
        "tsx/tsconfig.json",
        "{\"compilerOptions\":{\"jsx\":\"react\"}}",
    );
    let tsx = write(
        &root,
        "tsx/main.tsx",
        "const React = { createElement: (tag: string) => tag }; const value = <div/>; console.log('TSX_RUN_OK', value);\n",
    );
    verify(&tsx, "TSX_RUN_OK div");

    write(
        &root,
        "npm/package.json",
        "{\"scripts\":{\"start\":\"node entry.js\"}}",
    );
    write(&root, "npm/entry.js", "console.log('NPM_PROJECT_OK');\n");
    let helper = write(
        &root,
        "npm/src/helper.js",
        "throw new Error('must run the configured project script');\n",
    );
    verify(&helper, "NPM_PROJECT_OK");

    let go = write(
        &root,
        "go-file/main.go",
        "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"GO_FILE_OK\") }\n",
    );
    verify(&go, "GO_FILE_OK");
    write(
        &root,
        "go-project/go.mod",
        "module example.com/runnerverify\n\ngo 1.23.0\n",
    );
    write(
        &root,
        "go-project/helper.go",
        "package main\nfunc message() string { return \"GO_PACKAGE_OK\" }\n",
    );
    let go = write(
        &root,
        "go-project/main.go",
        "package main\nimport \"fmt\"\nfunc main() { fmt.Println(message()) }\n",
    );
    verify(&go, "GO_PACKAGE_OK");

    let rust = write(
        &root,
        "rust-file/loose file.rs",
        "fn main() { println!(\"RUST_FILE_OK\"); }\n",
    );
    verify(&rust, "RUST_FILE_OK");
    write(
        &root,
        "rust-project/Cargo.toml",
        "[package]\nname = 'runner-verify'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n",
    );
    let rust = write(
        &root,
        "rust-project/src/main.rs",
        "fn main() { println!(\"CARGO_RUN_OK\"); }\n",
    );
    verify(&rust, "CARGO_RUN_OK");
}

#[test]
#[ignore = "requires Node.js and live Windows ConPTY"]
fn forwards_input_preserves_exit_codes_and_stops_runs() {
    let root = fixture("interactive");
    let file = write(
        &root,
        "input.js",
        "const readline = require('node:readline'); const rl = readline.createInterface({ input: process.stdin, output: process.stdout }); rl.question('INPUT_READY\\n', value => { console.log('INPUT_OK:' + value); rl.close(); });\n",
    );
    let (mut service, id) = start(prepare(&file).unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        ready = service
            .poll(16)
            .iter()
            .any(|event| event.snapshot.text().contains("INPUT_READY"));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready);
    service.input(id, b"hello runner\r\n").unwrap();
    let (status, output) = finish(&mut service, id);
    assert_eq!(status, SessionStatus::Exited { code: 0 }, "{output}");
    assert!(output.contains("INPUT_OK:hello runner"), "{output}");
    service.remove(id).unwrap();

    let node = lightline::workflow::resolve_command("node").unwrap();
    let plan = RunPlan {
        cwd: root.clone(),
        commands: vec![
            RunCommand {
                program: node.clone(),
                arguments: vec!["-e".into(), "process.exit(7)".into()],
            },
            RunCommand {
                program: node,
                arguments: vec!["-e".into(), "console.log('STALE_BINARY_RAN')".into()],
            },
        ],
    };
    let (mut service, id) = start(plan);
    let (status, output) = finish(&mut service, id);
    assert_eq!(status, SessionStatus::Exited { code: 7 }, "{output}");
    assert!(!output.contains("STALE_BINARY_RAN"));
    service.remove(id).unwrap();

    let file = write(
        &root,
        "wait.js",
        "console.log('STOP_READY'); setInterval(() => {}, 1000);\n",
    );
    let (mut service, id) = start(prepare(&file).unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        ready = service
            .poll(16)
            .iter()
            .any(|event| event.snapshot.text().contains("STOP_READY"));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready);
    service.stop(id).unwrap();
    let (status, _) = finish(&mut service, id);
    assert_eq!(status, SessionStatus::Stopped);
    service.remove(id).unwrap();
}
