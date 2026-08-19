FROM rust:1.88-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --locked --release --package homelab-api

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /workspace/target/release/homelab-api /usr/local/bin/homelab-api
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/homelab-api"]
