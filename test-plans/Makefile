all: ping-image.tar

ping-image.tar: Dockerfile Cargo.toml $(wildcard src/*)
	cd .. && docker build -t rust-libp2p-head -f test-plans/Dockerfile .
	docker image save -o $@ rust-libp2p-head

.PHONY: clean

clean:
	rm ping-image.tar
