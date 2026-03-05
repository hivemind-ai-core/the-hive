fn main() {
    // Make the target triple available at runtime for the update command.
    println!("cargo:rustc-env=HIVE_BUILD_TARGET={}", std::env::var("TARGET").unwrap());
    // Rerun only when Cargo.toml changes (version bump).
    println!("cargo:rerun-if-changed=Cargo.toml");
}
