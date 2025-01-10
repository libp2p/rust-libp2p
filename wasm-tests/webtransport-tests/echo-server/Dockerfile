# syntax=docker/dockerfile:1.5-labs
FROM docker.io/library/golang:1.22 AS builder
WORKDIR /workspace
ADD . .
RUN CGO_ENABLED=0 go build .

FROM scratch
COPY --from=builder /workspace/echo-server /
ENTRYPOINT ["/echo-server"]
