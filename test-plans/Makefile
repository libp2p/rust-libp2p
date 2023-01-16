all: ping-image.tar

ping-image.tar: Dockerfile Cargo.toml src
	docker build -t rust-libp2p-head -f Dockerfile --build-arg BINARY_NAME=ping \
		--build-arg GIT_REF=$GIT_REF --build-arg GIT_TARGET=$GIT_TARGET .
	docker image save -o $@ rust-libp2p-head

.PHONY: clean

clean:
	rm ping-image.tar
