FROM ubuntu:kinetic
ADD ./target/release/perf-client /usr/local/bin/perf-client
ADD ./target/release/perf-server /usr/local/bin/perf-server
ENTRYPOINT [ "perf-server"]