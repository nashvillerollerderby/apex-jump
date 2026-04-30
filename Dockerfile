FROM rust:1.95-slim AS build

WORKDIR /apex-jump
COPY . .

EXPOSE 8001

RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --bin apex-jump --release
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --features static-files --bin apex-jump-fileserver --release

FROM rust:1.95-slim

WORKDIR /files
WORKDIR /
COPY --from=build /apex-jump/target/release/apex-jump .
COPY --from=build /apex-jump/target/release/apex-jump-fileserver .

CMD ["/apex-jump"]
