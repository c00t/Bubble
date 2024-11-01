use vergen::{BuildBuilder, CargoBuilder, Emitter, RustcBuilder};

// Example custom build script.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let current_path = std::env::current_dir().unwrap();
    // if it's release build
    let target_dir_path = current_path.join("../target/debug");
    println!(
        "cargo:rustc-link-search=native={}",
        target_dir_path.to_str().unwrap()
    );

    let build = BuildBuilder::all_build()?;
    let cargo = CargoBuilder::all_cargo()?;
    let rustc = RustcBuilder::all_rustc()?;

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?
        .emit()?;

    Ok(())
}
