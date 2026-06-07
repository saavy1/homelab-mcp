FROM rust:1.87-bookworm AS builder
WORKDIR /app
ARG PACKAGE=model-catalog-mcp
ARG BINARY=model-catalog-mcp
COPY . .
RUN cargo build --release -p ${PACKAGE}

FROM gcr.io/distroless/cc-debian12:nonroot
ARG BINARY=model-catalog-mcp
COPY --from=builder /app/target/release/${BINARY} /usr/local/bin/server
EXPOSE 8080
ENTRYPOINT ["server"]
