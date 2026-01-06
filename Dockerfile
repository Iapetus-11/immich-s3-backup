FROM rust:1.88-alpine3.22 AS build

COPY Cargo.lock Cargo.toml ./

# Separate stage for building dependencies, to better utilize Docker's caching
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo fetch
RUN cargo build --locked --profile release
RUN rm target/release/immich-s3-backup target/release/immich-s3-backup.d
RUN rm -rf src/ target/release/deps/immich-s3-backup*

COPY src/ ./src/

RUN cargo build --frozen --profile release

FROM alpine:3.22 AS runner

COPY --from=build /target/release/immich-s3-backup .

ENTRYPOINT ./immich-s3-backup