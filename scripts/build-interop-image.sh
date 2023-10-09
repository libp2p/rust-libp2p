#!/bin/bash

CACHE_TO=""

# If we have credentials, write to cache
if [[ -n "${AWS_SECRET_ACCESS_KEY}" ]]; then
  CACHE_TO="--cache-to   type=s3,mode=max,bucket=libp2p-by-tf-aws-bootstrap,region=us-east-1,prefix=buildCache,name=${FLAVOUR}-rust-libp2p-head"
fi

docker buildx build \
  --load \
  $CACHE_TO \
  --cache-from type=s3,mode=max,bucket=libp2p-by-tf-aws-bootstrap,region=us-east-1,prefix=buildCache,name=${FLAVOUR}-rust-libp2p-head \
  -t ${FLAVOUR}-rust-libp2p-head \
  . \
  -f interop-tests/Dockerfile.${FLAVOUR}
