#!/bin/sh

# This script regenerates the `src/rpc_proto.rs` file from `rpc.proto`.

echo "Checking if docker is installed"
if docker --version | grep -q "Docker version" ; then
    echo "Check that docker is installed: confirmed."
else
    if lsb_release -si | grep -q "Linux" ; then
        if lsb_release -si | grep -q "Ubuntu" ; then
            apt-get update;
            apt-get install -y docker;
        elif [ lsb_release -si | grep -q "Manjaro" ]; then
            sudo pacman -S --needed docker;
        else
            echo "installation for Docker is not configured for your OS, please install manually and make a pull request to install in this script for your OS at https://github.com/libp2p/rust-libp2p/floodsub";
        fi
    fi
fi

echo "Checking that docker is running"
if systemctl status docker | grep -q 'Active: inactive (dead)' ; then
    echo "docker not running, starting docker"
    systemctl start docker
else
    echo "Confirmed that docker is running"
fi

docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    echo 'Checking if protobuf is installed' \
    if lsb_release -si | grep -q 'Linux' ; then \
        if lsb_release -si | grep -q 'Ubuntu' ; then \
            apt-get update; \
            apt-get install -y protobuf-compiler; \
        elif lsb_release -si | grep -q 'Manjaro' ; then \
            sudo pacman -S --needed protobuf; \
        else \
            echo 'installation for protobuf is not configured for your OS, please make a pull request for https://github.com/libp2p/rust-libp2p/floodsub' \
        fi \
    fi \
    cargo install --version 1 protobuf; \
    echo 'Producing rpc.rs via protoc and rpc.proto' \
    protoc --rust_out . rpc.proto"

mv -f rpc.rs ./src/rpc_proto.rs
