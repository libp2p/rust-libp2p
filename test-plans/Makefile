all: ping-image.tar

ping-image.tar: Dockerfile Cargo.toml src/bin/ping.rs
	cargo build --release -p interop-tests
	mkdir -p target/release/
	cp ../target/release/ping target/release
	docker build -t rust-libp2p-head .
	docker image save -o $@ rust-libp2p-head

.PHONY: clean

clean:
	rm -rf target
	rm ping-image.tar
