FROM rust:1.94-slim-bookworm AS builder

WORKDIR /app

COPY . .

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates util-linux \
    && groupadd --system onshape-export \
    && useradd --system --gid onshape-export --home-dir /nonexistent --shell /usr/sbin/nologin onshape-export \
    && mkdir -p /data \
    && chown onshape-export:onshape-export /data \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/onshape-export /app/onshape-export
COPY --from=builder /app/catalog /app/catalog
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh

ENV BIND_ADDR=0.0.0.0:8080
ENV DATABASE_URL=sqlite:///data/onshape-export.db?mode=rwc

EXPOSE 8080

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["/app/onshape-export", "serve"]
