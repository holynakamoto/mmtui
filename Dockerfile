FROM rust:1.88 AS builder
LABEL org.opencontainers.image.source=https://github.com/holynakamoto/mmtui
LABEL org.opencontainers.image.description="A terminal user interface for the NCAA tournament data API, written in Rust."
LABEL org.opencontainers.image.licenses=MIT

WORKDIR /usr/src/mmtui
COPY . .
RUN cargo build --release

FROM gcr.io/distroless/cc
COPY --from=builder /usr/src/mmtui/target/release/mmtui /usr/local/bin/mmtui
CMD ["/usr/local/bin/mmtui"]
