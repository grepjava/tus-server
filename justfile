default: build

build:
    cargo build

run:
    cargo run

release:
    cargo build --release

test:
    cargo test

fmt:
    cargo fmt

clippy:
    cargo clippy -- -D warnings

prepare:
    cargo sqlx prepare

dev-server:
    RUST_LOG=debug cargo run

dev-ui:
    cd dashboard-ui && npm run dev

all: build
