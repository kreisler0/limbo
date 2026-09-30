// Gives LimboPortable.exe Limbo's icon and version info. The resource script
// is generated with an absolute icon path, so it doesn't depend on the
// directory `rc.exe` runs in. Only Windows-hosted Windows builds do this.

use std::path::Path;

fn main() {
    let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if !(target_windows && cfg!(windows)) {
        return;
    }
    let icon = icon.canonicalize().expect("src-tauri/icons/icon.ico");
    // `canonicalize` gives a `\\?\` path, which rc.exe doesn't understand.
    let icon = icon.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "\\\\");
    let version = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = version.split('.').collect();
    let (major, minor, patch) = (parts[0], parts.get(1).unwrap_or(&"0"), parts.get(2).unwrap_or(&"0"));
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Limbo"
      VALUE "FileDescription", "Limbo Portable"
      VALUE "FileVersion", "{version}.0"
      VALUE "InternalName", "LimboPortable"
      VALUE "OriginalFilename", "LimboPortable.exe"
      VALUE "ProductName", "Limbo Portable"
      VALUE "ProductVersion", "{version}.0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("launcher.rc");
    std::fs::write(&out, rc).expect("write launcher.rc");
    let _ = embed_resource::compile(&out, embed_resource::NONE);
}
