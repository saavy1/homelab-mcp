FROM rust:1.87-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p media-mcp

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/media-mcp /usr/local/bin/media-mcp
EXPOSE 8080
ENTRYPOINT ["media-mcp"]
