FROM ubuntu:kinetic
ADD ./target/release/examples/perf-client /usr/local/bin/perf-client
ADD ./target/release/examples/perf-server /usr/local/bin/perf-server
ENTRYPOINT [ "perf-server"]