fn main() {
    prost_build::compile_protos(
        &["src/browser/protocol/proto/message.proto"],
        &["src/browser/protocol/proto/"],
    )
    .unwrap();
}
