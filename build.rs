use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/lightline.ico");
    println!("cargo:rerun-if-changed=assets/lightline.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"))
        .join("lightline.res");
    let mut candidates = Vec::new();
    if let Some(rc) = env::var_os("RC") {
        candidates.push(PathBuf::from(rc));
    }
    candidates.push(PathBuf::from("rc.exe"));

    let sdk_root = env::var_os("WindowsSdkDir").map(PathBuf::from).or_else(|| {
        env::var_os("ProgramFiles(x86)")
            .map(|base| PathBuf::from(base).join("Windows Kits").join("10"))
    });
    if let Some(root) = sdk_root {
        let bin = root.join("bin");
        if let Ok(versions) = fs::read_dir(bin) {
            let mut versions: Vec<_> = versions
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect();
            versions.sort();
            versions.reverse();
            let host = match env::consts::ARCH {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                _ => "x86",
            };
            for version in versions {
                candidates.push(version.join(host).join("rc.exe"));
            }
        }
    }

    for rc in candidates {
        let result = Command::new(&rc)
            .current_dir(manifest.join("assets"))
            .arg("/nologo")
            .arg(format!("/fo{}", output.display()))
            .arg("lightline.rc")
            .output();
        match result {
            Ok(result) if result.status.success() => {
                println!("cargo:rustc-link-arg-bin=lightline={}", output.display());
                return;
            }
            Ok(result) => {
                println!(
                    "cargo:warning=Icon resource compiler failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            Err(_) => {}
        }
    }
    println!(
        "cargo:warning=Windows SDK rc.exe not found; window icon will work, but the executable icon is unavailable"
    );
}
