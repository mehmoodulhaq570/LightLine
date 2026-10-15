use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// Version metadata shown in the file's Properties > Details tab. Code-signing
// programs require the product name and version to be embedded in the binary.
fn version_resource(version: &str) -> String {
    let mut numbers = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let mut next = || numbers.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    format!(
        "1 VERSIONINFO\n\
         FILEVERSION {major},{minor},{patch},0\n\
         PRODUCTVERSION {major},{minor},{patch},0\n\
         FILEOS 0x40004\n\
         FILETYPE 1\n\
         BEGIN\n\
         BLOCK \"StringFileInfo\"\n\
         BEGIN\n\
         BLOCK \"040904B0\"\n\
         BEGIN\n\
         VALUE \"CompanyName\", \"LightLine\"\n\
         VALUE \"FileDescription\", \"LightLine code editor\"\n\
         VALUE \"FileVersion\", \"{version}\"\n\
         VALUE \"InternalName\", \"lightline\"\n\
         VALUE \"LegalCopyright\", \"Copyright (c) 2026 Mehmood-Ul-Haq. MIT License.\"\n\
         VALUE \"OriginalFilename\", \"lightline.exe\"\n\
         VALUE \"ProductName\", \"LightLine\"\n\
         VALUE \"ProductVersion\", \"{version}\"\n\
         END\n\
         END\n\
         BLOCK \"VarFileInfo\"\n\
         BEGIN\n\
         VALUE \"Translation\", 0x0409, 0x04B0\n\
         END\n\
         END\n"
    )
}

fn main() {
    println!("cargo:rerun-if-changed=assets/lightline.ico");
    println!("cargo:rerun-if-changed=assets/lightline.rc");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-env-changed=LIGHTLINE_VERSION");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    let output = out_dir.join("lightline.res");
    // The release workflow sets LIGHTLINE_VERSION because Cargo.toml's version is not bumped per release.
    let version = env::var("LIGHTLINE_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("CARGO_PKG_VERSION").ok())
        .unwrap_or_else(|| "0.0.0".into());
    if let Err(error) = fs::write(
        out_dir.join("lightline_version.rc"),
        version_resource(&version),
    ) {
        println!("cargo:warning=Could not write version resource: {error}");
    }
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
            .arg(format!("/i{}", out_dir.display()))
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
