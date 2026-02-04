fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_file = "../../proto/astraea.proto";

    // Recompile if the proto file changes.
    println!("cargo:rerun-if-changed={proto_file}");

    tonic_build::compile_protos(proto_file)?;

    Ok(())
}
