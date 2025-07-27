# gossipsub-tests

A test suite for verifying the behavior of gossipsub protocol.

## Build and Run

**Note: All commands below should be run from the rust-libp2p repository root directory.**

Build the Docker image:
```bash
docker build -f Dockerfile.gossipsub-tests -t gossipsub-tests .
```

Run `subscribe` test case using Kurtosis:
```bash
# Run `subscribe` test
kurtosis run -v brief --enclave gossipsub github.com/ackintosh/gossipsub-package --args-file gossipsub-tests/subscribe.yaml

# Inspect the running enclave and node status
kurtosis enclave inspect gossipsub

# Export logs for analysis
kurtosis dump gossipsub-test-logs
```
