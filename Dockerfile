# syntax=docker/dockerfile:1.7

ARG CARGO_CHEF_IMAGE=docker.io/lukemathwalker/cargo-chef:latest-rust-alpine3.24
ARG STATIC_RUNTIME_IMAGE=dhi.io/static:20261020-alpine3.23

FROM ${CARGO_CHEF_IMAGE} AS chef
WORKDIR /app
RUN apk add --no-cache build-base cmake perl pkgconf binutils

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server-builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --no-default-features --features api-server --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --locked --release --no-default-features --features api-server \
    && mkdir -p /data

FROM ${STATIC_RUNTIME_IMAGE} AS server
COPY --from=server-builder --chown=65532:65532 /app/target/release/ferris-workflow /ferris-workflow
COPY --from=server-builder --chown=65532:65532 /data /data
ENV IP=0.0.0.0 \
    PORT=8080 \
    DATABASE_URL=sqlite:///data/ferris-workflow.db?mode=rwc
VOLUME ["/data"]
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/ferris-workflow"]

FROM chef AS dioxus-builder
RUN apk add --no-cache openssl-dev openssl-libs-static \
    && rustup target add wasm32-unknown-unknown
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/tmp/dioxus-cli-target \
    CARGO_TARGET_DIR=/tmp/dioxus-cli-target cargo install dioxus-cli --version 0.7.10 --locked
COPY . .

FROM dioxus-builder AS frontend-builder
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    dx build --platform web --fullstack false --profile web-release --no-default-features --features web

FROM docker.io/nginxinc/nginx-unprivileged:stable-alpine-slim AS frontend
COPY docker/frontend.conf.template /etc/nginx/templates/default.conf.template
COPY --from=frontend-builder /app/target/dx/ferris-workflow/release/web/public/ /usr/share/nginx/html/
ENV FERRIS_WORKFLOW_API_URL=http://server:8080 \
    NGINX_ENVSUBST_FILTER=^FERRIS_WORKFLOW_API_URL$
EXPOSE 8080

FROM dioxus-builder AS fullstack-builder
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    dx build --platform web --fullstack true --profile web-release --no-default-features --features web,fullstack \
    && strip --strip-all target/dx/ferris-workflow/release/web/server \
    && mkdir -p /data

FROM ${STATIC_RUNTIME_IMAGE} AS fullstack
COPY --from=fullstack-builder --chown=65532:65532 /app/target/dx/ferris-workflow/release/web/server /app/ferris-workflow
COPY --from=fullstack-builder --chown=65532:65532 /app/target/dx/ferris-workflow/release/web/public /app/public
COPY --from=fullstack-builder --chown=65532:65532 /data /data
ENV IP=0.0.0.0 \
    PORT=8080 \
    DIOXUS_PUBLIC_PATH=/app/public \
    DATABASE_URL=sqlite:///data/ferris-workflow.db?mode=rwc
VOLUME ["/data"]
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/app/ferris-workflow"]
