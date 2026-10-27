fn main() {
    println!("cargo:rerun-if-changed=proto/cubecoin/p2p/v1/p2p.proto");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("locate vendored protoc");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&["proto/cubecoin/p2p/v1/p2p.proto"], &["proto"])
        .expect("compile CubeCoin P2P protobuf schema");
}
