fn main() {
    // für das Neuladen der Schriften nach dem Installieren (siehe src/fonts.rs)
    println!("cargo:rustc-link-lib=pangoft2-1.0");
    println!("cargo:rustc-link-lib=fontconfig");
}
