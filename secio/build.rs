extern crate protobuf_codegen_pure;

use std::fs;

fn main() {
    fs::create_dir_all("src/protobuf_structs").unwrap();

    protobuf_codegen_pure::run(protobuf_codegen_pure::Args {
        out_dir: "src/protobuf_structs",
        input: &["src/keys.proto", "src/structs.proto"],
        includes: &["src"],
        customize: Default::default(),
    }).expect("protoc failed to run");
}
