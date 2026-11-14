use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    let rc_paths = [
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe",
        "rc.exe",
    ];

    let mut compiled = false;
    for rc in &rc_paths {
        if let Ok(status) = Command::new(rc).args(&["/fo", "resources.res", "resources.rc"]).status() {
            if status.success() {
                compiled = true;
                break;
            }
        }
    }

    if compiled || std::path::Path::new("resources.res").exists() {
        let res_path = std::env::current_dir().unwrap().join("resources.res");
        println!("cargo:rustc-link-arg={}", res_path.display());
    }
}
