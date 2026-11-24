fn main() {
    // tauri-build doesn't watch the icons, so a new icon would otherwise keep the old one embedded.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
