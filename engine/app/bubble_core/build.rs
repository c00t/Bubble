// Example custom build script.
fn main() {
    // println!("cargo::rerun-if-changed=../dyallocator/src/lib.rs");
    // cargo build the dyallocator crate, then add it's output dll file to lib search path
    // let output = Command::new("cargo")
    //     .arg("build")
    //     .arg("--release")
    //     .current_dir("../dyallocator")
    //     .output()
    //     .expect("Failed to build dyallocator");

    let target_dir_path =
        "C:\\Users\\sh33p\\Documents\\Repos\\Bubble_engine\\app\\target\\release\\".to_string();
    println!("cargo:rustc-link-search=native={}", target_dir_path);
    // #[cfg(target_os = "windows")]
    // println!("cargo::rustc-link-lib=dylib=dyallocator.{}", DLL_EXTENSION);
    // #[cfg(not(target_os = "windows"))]
    // println!("cargo::rustc-link-lib=dylib=dyallocator");
}
