FROM ubuntu:latest
COPY target/debug/examples/tokio_listen /usr/bin/tokio_listen
CMD ["bash", "-c", "tokio_listen"]
