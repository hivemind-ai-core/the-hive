# The Hive - Common development commands

# Default recipe
default:
    @just --list

# Build all crates
build:
    cargo build --workspace

# Build release binaries
build-release:
    cargo build --workspace --release

# Run tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Check formatting
fmt:
    cargo fmt --check

# Format code
fmt-fix:
    cargo fmt

# Run all checks (fmt + lint + test)
check: fmt lint test
    @echo "All checks passed!"

# Build Docker images
docker-build:
    docker build -t hive-server:latest -f docker/Dockerfile.hive-server .
    docker build -t hive-agent:latest -f docker/Dockerfile.hive-agent .
    docker build -t app-container:latest -f docker/Dockerfile.app-container .

# Build Docker images with docker-compose
docker-build-compose:
    docker-compose -f docker/docker-compose.yml build

# Start containers
docker-up:
    docker-compose -f docker/docker-compose.yml up -d

# Stop containers
docker-down:
    docker-compose -f docker/docker-compose.yml down

# View logs
docker-logs service="":
    docker-compose -f docker/docker-compose.yml logs {{service}}

# Clean build artifacts
clean:
    cargo clean

# Remove target directory
dist-clean:
    rm -rf target

# Build release binaries and install to ~/.hive/
install:
    #!/usr/bin/env sh
    set -eu
    cargo build --release --workspace
    DEST="${HIVE_HOME:-$HOME/.hive}"
    mkdir -p "$DEST/bin" "$DEST/docker"
    cp target/release/hive        "$DEST/bin/"
    cp target/release/hive-server "$DEST/bin/"
    cp target/release/hive-agent  "$DEST/bin/"
    cp target/release/app-daemon  "$DEST/bin/"
    chmod +x "$DEST/bin/"*
    cp crates/hive-cli/src/templates/Dockerfile.server "$DEST/docker/"
    cp crates/hive-cli/src/templates/Dockerfile.agent  "$DEST/docker/"
    cp crates/hive-cli/src/templates/Dockerfile.app    "$DEST/docker/"
    ./target/release/hive --version | awk '{print $2}' > "$DEST/version"
    echo "Installed to $DEST"

# Rebuild and reinstall (alias for install)
reinstall: install

# Generate shell completions
completions shell="bash":
    cargo run --package hive-cli --bin hive -- completion {{shell}}

# Watch mode for development
watch:
    cargo watch -x check -x test

# Run a specific crate
run crate="hive-cli":
    cargo run -p {{crate}}

# Bump version
bump part="patch":
    cargo bump {{part}} --workspace
