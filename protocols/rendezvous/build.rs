fn main() {
    protobuf_codegen::Codegen::new()
        .pure()
        .includes(&["src"])
        .input("src/rpc.proto")
        .customize(protobuf_codegen::Customize::default().lite_runtime(true))
        .cargo_out_dir("protos")
        .run()
        .unwrap()
}
