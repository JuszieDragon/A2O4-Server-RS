fn main() {
    // Tell Cargo to re-run this build script if any file in migrations changes
    println!("cargo:rerun-if-changed=migrations");
}
