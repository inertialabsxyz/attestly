.PHONY: check lint test test-unit test-int fix \
        cloud-check cloud-lint cloud-test cloud-fix

# Top-level gate: lint + test both workspaces. `services/awp-cloud/` is its
# own workspace by design (independent release cadence, isolation from the
# core crate's clippy regime); we run its `make check` recursively here so
# every contributor's `make check` is a complete CI proxy.
check: lint test cloud-check

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
	$(MAKE) -C services/awp-cloud fix

# Recursive targets into the awp-cloud sub-workspace.
cloud-check:
	$(MAKE) -C services/awp-cloud check

cloud-lint:
	$(MAKE) -C services/awp-cloud lint

cloud-test:
	$(MAKE) -C services/awp-cloud test

cloud-fix:
	$(MAKE) -C services/awp-cloud fix
