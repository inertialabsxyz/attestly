.PHONY: check lint test test-unit test-int fix check-python python-build python-test

# Python venv used for the awp-python tests. Override with PY_VENV=... at
# the call site if you want to point at an existing interpreter. The
# `python-build` target creates it on first use.
PY_VENV ?= .venv
PY_VENV_BIN := $(PY_VENV)/bin

check: lint test check-python

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

# `awp-python` is excluded from `cargo test` because its `cdylib` target
# triggers a libpython link error on platforms where the Python
# framework's rpath is not on the linker's default search path. The
# Python-side tests under `crates/awp-python/tests/` cover that crate's
# surface via pytest (run by `check-python`).
test:
	cargo test --workspace --exclude awp-python

test-unit:
	cargo test --workspace --exclude awp-python --lib

test-int:
	cargo test --workspace --exclude awp-python --tests

# Build the Python wheel + run pytest. Builds the `awp-verify` binary
# first so `tests/cross_language.py` can pipe attestations through it.
check-python: python-build python-test

python-build:
	@test -d $(PY_VENV) || python3 -m venv $(PY_VENV)
	$(PY_VENV_BIN)/pip install --quiet --upgrade pip
	$(PY_VENV_BIN)/pip install --quiet maturin pytest
	cargo build -p awp-core --bin awp-verify --release
	cd crates/awp-python && ../../$(PY_VENV_BIN)/maturin develop --release --quiet

python-test:
	$(PY_VENV_BIN)/python -m pytest crates/awp-python/tests -v

fix:
	cargo fmt --all
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged
