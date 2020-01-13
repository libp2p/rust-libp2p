fn main() {
	prost_build::compile_protos(&["src/structs.proto"], &["src"]).unwrap();
}

