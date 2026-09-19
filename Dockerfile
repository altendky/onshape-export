FROM rust:1.98-slim-bookworm AS builder

WORKDIR /app

COPY . .

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && groupadd --system onshape-export \
    && useradd --system --gid onshape-export --home-dir /nonexistent --shell /usr/sbin/nologin onshape-export \
    && mkdir -p /data \
    && chown onshape-export:onshape-export /data \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/onshape-export /app/onshape-export
COPY --from=builder /app/catalog /app/catalog

ENV BIND_ADDR=0.0.0.0:8080
ENV DATABASE_URL=sqlite:///data/onshape-export.db?mode=rwc

EXPOSE 8080

USER onshape-export

CMD ["/app/onshape-export", "serve"]
