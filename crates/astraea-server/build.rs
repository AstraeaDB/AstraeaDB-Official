fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Must live inside the crate: `cargo package` only includes files under
    // the crate directory, so a workspace-root path builds locally and then
    // fails to publish with "Could not make proto path relative".
    let proto_file = "proto/astraea.proto";

    // Recompile if the proto file changes.
    println!("cargo:rerun-if-changed={proto_file}");

    tonic_build::compile_protos(proto_file)?;

    Ok(())
}
