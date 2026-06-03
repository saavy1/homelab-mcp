FROM rust:1.87-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p model-catalog-mcp

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/model-catalog-mcp /usr/local/bin/model-catalog-mcp
EXPOSE 8080
ENTRYPOINT ["model-catalog-mcp"]
