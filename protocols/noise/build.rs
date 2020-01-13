fn main() {
	prost_build::compile_protos(&["src/io/handshake/payload.proto"], &["src"]).unwrap();
}
