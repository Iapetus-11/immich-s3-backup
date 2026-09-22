FROM rust:1.95.0-alpine3.23 AS build

WORKDIR /immich_s3_backup

COPY Cargo.lock Cargo.toml ./

# Separate stage for building dependencies, to better utilize Docker's caching
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo fetch
RUN cargo build --locked --profile release
RUN rm target/release/immich_s3_backup target/release/immich_s3_backup.d
RUN rm -rf src/ target/release/deps/immich_s3_backup*

COPY src/ ./src/

RUN cargo build --frozen --profile release


FROM alpine:3.23 AS runner

WORKDIR /immich_s3_backup

RUN apk add --no-cache postgresql-client

COPY --from=build /immich_s3_backup/target/release/immich_s3_backup .

ENTRYPOINT ["./immich_s3_backup"]
