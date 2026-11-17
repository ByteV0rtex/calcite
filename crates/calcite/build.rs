//! Records the newest database migration the bundled backend knows, so the
//! Move-in flow can tell whether another launcher's database is too new.
use std::path::Path;

fn main() {
    let migrations = Path::new("../../vendor/modrinth/packages/app-lib/migrations");
    println!("cargo::rerun-if-changed={}", migrations.display());
    let newest = std::fs::read_dir(migrations)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.split('_').next()?.parse::<i64>().ok()
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    println!("cargo::rustc-env=CALCITE_DB_SCHEMA={newest}");

    let backend = std::fs::read_to_string("../../BACKEND_VERSION").unwrap_or_default();
    println!("cargo::rerun-if-changed=../../BACKEND_VERSION");
    println!(
        "cargo::rustc-env=CALCITE_BACKEND_VERSION={}",
        backend.trim()
    );

    // The icon Explorer, the taskbar and the installer show for calcite.exe.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = "../../packaging/icons/calcite.ico";
        println!("cargo::rerun-if-changed={icon}");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon(icon)
            .set("ProductName", "Calcite")
            .set("FileDescription", "Calcite");
        if let Err(error) = resource.compile() {
            println!("cargo::warning=couldn't embed the Windows icon: {error}");
        }
    }
}
