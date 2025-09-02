fn main() -> anyhow::Result<()> {
    let proto_file = "../geyser/proto/event.proto";

    println!("cargo:rerun-if-changed={}", proto_file);

    let mut config = prost_build::Config::new();
    config.boxed(".heimdall.types.MessageWrapper");
    config.protoc_arg("--experimental_allow_proto3_optional");
    config.compile_protos(&[proto_file], &["../geyser/proto/"])?;

    gm gm

    Ok(())
}
