fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/proto/cdk_ldk_managment.proto");
    tonic_build::compile_protos("src/proto/cdk_ldk_managment.proto")?;
    Ok(())
}
