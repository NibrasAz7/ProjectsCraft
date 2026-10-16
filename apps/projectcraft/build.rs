//! Windows only: embed the app icon and version info into `projectcraft.exe`. Elsewhere a no-op.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/projectcraft.ico");
    println!("cargo:rerun-if-env-changed=PROJECTCRAFT_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=PROJECTCRAFT_BUILD_SHA");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if std::path::Path::new("../../assets/app-icon/projectcraft.ico").exists() {
        res.set_icon("../../assets/app-icon/projectcraft.ico");
    }
    res.set("ProductName", "ProjectCraft")
        .set("FileDescription", "ProjectCraft project-management and scheduling suite")
        .set("CompanyName", "NibrasAz7")
        .set("LegalCopyright", "Copyright (c) 2026 NibrasAz7 and contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "projectcraft.exe")
        .set("InternalName", "projectcraft");
    if let Err(e) = res.compile() {
        if std::env::var_os("PROJECTCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=projectcraft.exe built without icon/version resources: {e}");
    }
}
