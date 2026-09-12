# Alpine/musl builder, scratch runtime. The k3s node is x86_64 while development is on an ARM Mac, so always
# build with an explicit platform:
#   docker buildx build --platform linux/amd64 -t mcp-rust-demo-001:dev --load .

FROM --platform=$BUILDPLATFORM rust:1.97-alpine AS builder

# musl-dev supplies the C runtime rustc links against; the rest are build-only.
RUN apk add --no-cache musl-dev

WORKDIR /build

# Compile dependencies first, from a stub main, so edits to src/ do not invalidate the
# dependency layer.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
# Cargo skips the rebuild unless the real sources are newer than the stub.
RUN touch src/main.rs \
    && cargo build --release \
    && strip target/release/mcp-rust-demo-001

# A passwd entry has to be built here: scratch has no adduser.
RUN echo 'mcp:x:10001:10001::/nonexistent:/sbin/nologin' > /out-passwd


# musl links statically, so the runtime image needs no libc, no shell and no package
# manager. Keep it at scratch: nothing to inventory, nothing to patch.
FROM scratch

COPY --from=builder /out-passwd /etc/passwd
COPY --from=builder /build/target/release/mcp-rust-demo-001 /mcp-rust-demo-001

USER 10001

ENV BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/mcp-rust-demo-001"]
