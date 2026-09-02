fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Vendored; README.md says why a submodule would not work here.
    println!("cargo:rerun-if-changed=proto/vedavid.proto");
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/vedavid.proto"], &["proto"])?;
    Ok(())
}
