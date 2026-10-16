//! Windows only: embed the app icon and version info (VERSIONINFO) into `redrob-query.exe`, so the
//! Start menu, Settings > Apps and Redrob Launcher show the Redrob mark and the product name.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `REDROB_QUERY_REQUIRE_WINRES=1` (set by the
//! release workflow) turns it into an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../resources/icons/redrob-query.ico");
    println!("cargo:rerun-if-env-changed=REDROB_QUERY_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../resources/icons/redrob-query.ico")
        .set("ProductName", "Redrob Query")
        .set("FileDescription", "Redrob Query")
        .set("CompanyName", "Redrob")
        .set(
            "LegalCopyright",
            "Copyright (c) 2026 Janghoon Lee (Redrob) and contributors. GPL-3.0-or-later.",
        )
        .set("OriginalFilename", "redrob-query.exe")
        .set("InternalName", "redrob-query");
    if let Err(e) = res.compile() {
        if std::env::var_os("REDROB_QUERY_REQUIRE_WINRES").is_some() {
            println!("cargo:warning=embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=redrob-query.exe built without icon/version resources: {e}");
    }
}
