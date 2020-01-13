fn main() {
	prost_build::compile_protos(&["src/dht.proto"], &["src"]).unwrap();
}

