// Example custom build script.
fn main() {
    let current_path = std::env::current_dir().unwrap();
    // if it's release build
    let target_dir_path = current_path.join("../target/release");
    println!(
        "cargo:rustc-link-search=native={}",
        target_dir_path.to_str().unwrap()
    );
}
