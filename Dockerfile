FROM rust:1.87-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p model-catalog-mcp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/model-catalog-mcp /usr/local/bin/model-catalog-mcp
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["model-catalog-mcp"]
