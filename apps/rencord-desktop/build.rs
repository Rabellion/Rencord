fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/hexwave.ico");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "windows" {
        // embed-resource is a no-op off Windows. Skip the rc so Linux CI
        // does not need windres or llvm-rc.
        return;
    }
    let ico =
        std::fs::canonicalize("../../assets/brand/hexwave.ico").expect("assets/brand/hexwave.ico");
    let mut ico_literal = ico.display().to_string();
    if let Some(rest) = ico_literal.strip_prefix(r"\\?\") {
        ico_literal = rest.to_string();
    }
    let ico_literal = ico_literal.replace('\\', "\\\\");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.6.0".into());
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_else(|_| "0".into());
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_else(|_| "6".into());
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap_or_else(|_| "0".into());
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let rc_path = std::path::Path::new(&out_dir).join("rencord.rc");
    let rc = format!(
        r#"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS 0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Rabellion"
      VALUE "FileDescription", "Rencord"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "rencord.exe"
      VALUE "OriginalFilename", "Rencord.exe"
      VALUE "ProductName", "Rencord"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
1 ICON "{ico_literal}"
"#
    );
    std::fs::write(&rc_path, rc).expect("write rencord.rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("embed Hexwave icon");
}
