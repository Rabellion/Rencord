fn main() {
    // OpenH264 is C++. rust-lld does not search the GCC directory on its own.
    let output = std::process::Command::new("gcc")
        .args(["-print-file-name=libstdc++.so"])
        .output();
    if let Ok(output) = output {
        let path = String::from_utf8_lossy(&output.stdout);
        let path = path.trim();
        if let Some(parent) = std::path::Path::new(path).parent() {
            if parent.as_os_str().is_empty() {
                return;
            }
            println!("cargo:rustc-link-search={}", parent.display());
        }
    }
}
