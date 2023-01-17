all: ping-image.tar

ping-image.tar: Dockerfile Cargo.toml src
	cd .. && docker build -t rust-libp2p-head -f test-plans/Dockerfile --build-arg BINARY_NAME=ping .
	docker image save -o $@ rust-libp2p-head

.PHONY: clean

clean:
	rm ping-image.tar
