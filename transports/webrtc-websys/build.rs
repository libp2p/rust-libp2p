fn main() {
    prost_build::compile_protos(&["src/browser/proto/message.proto"], &["src/browser/proto"])
        .unwrap();
}