use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "Debug".to_string());
    let current_dir = std::env::current_dir().unwrap();

    let target = if profile == "Release" {
        Path::new(&current_dir).join("target/release")
    } else {
        Path::new(&current_dir).join("target/debug")
    };

    Command::new("rustc")
        .arg("src/test_shared.rs")
        .arg("--crate-name")
        .arg("test_shared")
        .arg("--crate-type")
        .arg("dylib")
        .arg("--out-dir")
        .arg(target)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
}
