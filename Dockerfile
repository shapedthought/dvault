# Build dvault as a container — lets you run it without installing a binary,
# and against a bind-mounted (e.g. OneDrive/SharePoint-synced) vault folder.
#
# Build:  docker build -t dvault .
# Run:    see the "Running with Docker" section of README.md for the wrapper alias.

# --- build stage (edition 2024 needs Rust >= 1.85) ---
FROM rust:1-bookworm AS build
WORKDIR /src
# Copy manifests + sources and build. (A dependency-cache layer could be added
# for faster rebuilds; kept simple here for a build-once tool.)
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# --- runtime stage ---
# The binary statically bundles SQLite, so a slim glibc base is enough (no
# libsqlite3 needed at runtime). distroless/cc would be smaller; debian-slim is
# kept for a friendlier image (has a shell for poking around a mounted vault).
FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/shapedthought/dvault"
COPY --from=build /src/target/release/dvault /usr/local/bin/dvault
WORKDIR /work
ENTRYPOINT ["dvault"]
