FROM lukemathwalker/cargo-chef:latest-rust-alpine3.24 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .

RUN cargo build --release --features server


FROM dhi.io/static:20251003-alpine3.23 AS runtime
COPY --from=builder /app/target/release/ferris-workflow /ferris-workflow

EXPOSE 8080

ENTRYPOINT ["/ferris-workflow"]
