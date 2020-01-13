fn main() {
	prost_build::compile_protos(&["src/keys.proto"], &["src"]).unwrap();
}
