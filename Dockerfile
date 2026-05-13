FROM rust:1.88-slim-bookworm AS builder


WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app
COPY --from=builder /app/target/release/aegis-link .


ENV AEGIS_HEADLESS=1

EXPOSE 8080/udp

CMD ["./aegis-link"]