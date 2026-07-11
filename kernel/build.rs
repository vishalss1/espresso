use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    println!("cargo:rustc-link-search={}", manifest_dir);
    println!("cargo:rustc-link-arg=-Tlinker.ld");
    println!("cargo:rustc-link-arg=-nostartfiles");
    println!("cargo:rustc-link-arg=-nostdlib");

    let asm_path = PathBuf::from(&manifest_dir).join("asm").join("switch.S");
    let out_path = PathBuf::from(&out_dir).join("switch.o");

    let cc = find_xtensa_as();

    let status = Command::new(&cc)
        .args(&[
            "-c",
            "-o",
            out_path.to_str().unwrap(),
            asm_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to assemble switch.S");

    if !status.success() {
        panic!("Failed to assemble switch.S");
    }

    println!("cargo:rustc-link-arg={}", out_path.to_str().unwrap());
    println!("cargo:rerun-if-changed=asm/switch.S");
}

fn find_xtensa_as() -> String {
    if let Ok(cc) = env::var("CC_xtensa-esp32-none-elf") {
        return cc;
    }

    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    let esp_bin = format!("{}/.rustup/toolchains/esp/xtensa-esp-elf/bin", home);
    let as_path = format!("{}/xtensa-esp32-elf-as.exe", esp_bin);

    if std::path::Path::new(&as_path).exists() {
        return as_path;
    }

    "xtensa-esp32-elf-as".to_string()
}
