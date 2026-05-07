.PHONY: check lint test test-unit test-int fix

check: lint test

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

test-unit:
	cargo test --workspace --lib

test-int:
	cargo test --workspace --tests

fix:
	cargo fmt --all
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged
