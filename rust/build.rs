use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=appswitcher.rc");
    println!("cargo:rerun-if-changed=assets/appswitcher.ico");
    if !env::var("TARGET").unwrap_or_default().contains("windows-msvc") {
        return;
    }

    let sdk_bin = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let arch = if env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let mut versions: Vec<_> = fs::read_dir(&sdk_bin)
        .expect("Windows SDK is required to embed the app icon")
        .flatten()
        .map(|entry| entry.path().join(arch).join("rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    versions.sort();
    let rc = versions.pop().expect("Windows SDK rc.exe was not found");
    let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("appswitcher.res");
    let status = Command::new(rc)
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg("appswitcher.rc")
        .status()
        .expect("failed to run Windows resource compiler");
    assert!(status.success(), "failed to compile app icon resource");
    println!("cargo:rustc-link-arg={}", resource.display());
}
