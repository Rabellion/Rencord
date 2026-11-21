fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/hexwave.ico");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "windows" {
        // embed-resource is a no-op off Windows. Skip the rc so Linux CI
        // does not need windres or llvm-rc.
        return;
    }
    let ico =
        std::fs::canonicalize("../../assets/brand/hexwave.ico").expect("assets/brand/hexwave.ico");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let rc_path = std::path::Path::new(&out_dir).join("rencord.rc");
    let ico_literal = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc_path, format!("1 ICON \"{ico_literal}\"\n")).expect("write rencord.rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("embed Hexwave icon");
}
