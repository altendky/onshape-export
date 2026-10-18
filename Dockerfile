FROM rust:1.99.1-slim-bookworm AS builder

WORKDIR /app

COPY . .

RUN cargo build --release --locked

FROM builder AS package-test-builder

RUN cargo test --release --locked --all-features --no-run \
    && find /app/target/release/deps -maxdepth 1 -type f -executable -name 'onshape_export-*' \
        -exec cp {} /app/package-tests \;

FROM debian:bookworm-slim AS generator

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl jq \
    && rm -rf /var/lib/apt/lists/*

COPY scripts/install-bambu-generator.sh /tmp/install-bambu-generator.sh
RUN sh /tmp/install-bambu-generator.sh

FROM generator AS generator-verification

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 \
    && groupadd --system onshape-export \
    && useradd --system --gid onshape-export --home-dir /nonexistent --shell /usr/sbin/nologin onshape-export \
    && mkdir -p /tmp/package-verification/agents \
    && chown -R onshape-export:onshape-export /tmp/package-verification \
    && rm -rf /var/lib/apt/lists/*

COPY --from=package-test-builder /app/package-tests /usr/local/bin/onshape-export-package-tests
ENV TMPDIR=/tmp/package-verification
ENV TRUSTED_GENERATOR_CONFIG_PATH=/opt/onshape-export/generators/bambu-studio-v0.1.0/deployed-generator.json
ENV BAMBU_REAL_GENERATOR_CONFIG=/opt/onshape-export/generators/bambu-studio-v0.1.0/deployed-generator.json
USER onshape-export
RUN /usr/local/bin/onshape-export-package-tests bambu_real_package_tests --ignored --test-threads=1

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
COPY --from=generator /opt/onshape-export/generators /opt/onshape-export/generators

ENV BIND_ADDR=0.0.0.0:8080
ENV DATABASE_URL=sqlite:///data/onshape-export.db?mode=rwc
ENV TRUSTED_GENERATOR_CONFIG_PATH=/opt/onshape-export/generators/bambu-studio-v0.1.0/deployed-generator.json

EXPOSE 8080

USER onshape-export

CMD ["/app/onshape-export", "serve"]
