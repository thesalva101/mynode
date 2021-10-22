extern crate protoc_rust_grpc;

fn main() {
    let protobuf_sources = &["protobuf/raft.proto", "protobuf/store.proto"];

    // Generate Protobuf shims
    protoc_rust_grpc::run(protoc_rust_grpc::Args {
        out_dir: "src/proto",
        includes: &[],
        input: protobuf_sources,
        rust_protobuf: true, // also generate protobuf messages, not just services
        ..Default::default()
    })
    .expect("protoc-rust-grpc");

    for src in protobuf_sources {
        println!("cargo:rerun-if-changed={}", src);
    }
}
