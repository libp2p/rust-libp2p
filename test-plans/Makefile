all: ping-image.tar

ping-image.tar: Dockerfile Cargo.toml $(wildcard src/*)
	CARGO_TARGET_DIR=./target cargo build --release -p interop-tests
	docker build -t rust-libp2p-head .
	docker image save -o $@ rust-libp2p-head

.PHONY: clean

clean:
	rm -rf target
	rm ping-image.tar
