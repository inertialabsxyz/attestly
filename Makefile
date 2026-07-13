.PHONY: check lint test test-unit test-int fix \
        check-python python-build python-test python-test-langgraph \
        cloud-check cloud-lint cloud-test cloud-fix \
        seed-overage seed-old-attestations

# Python venv used for the attestly-python tests. Override with PY_VENV=... at
# the call site if you want to point at an existing interpreter. The
# `python-build` target creates it on first use.
PY_VENV ?= .venv
PY_VENV_BIN := $(PY_VENV)/bin

# Top-level gate: lint + test the core workspace, the Python bindings, and
# the `services/attestly-cloud/` sub-workspace (which is its own Cargo workspace
# by design — independent release cadence, isolated from the core's clippy
# regime). Every contributor's `make check` is a complete CI proxy.
check: lint test check-python cloud-check

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

# `attestly-python` is excluded from `cargo test` because its `cdylib` target
# triggers a libpython link error on platforms where the Python
# framework's rpath is not on the linker's default search path. The
# Python-side tests under `crates/attestly-python/tests/` cover that crate's
# surface via pytest (run by `check-python`).
test:
	cargo test --workspace --exclude attestly-python

test-unit:
	cargo test --workspace --exclude attestly-python --lib

test-int:
	cargo test --workspace --exclude attestly-python --tests

# Build the Python wheel + run pytest. Builds the `attestly-verify` binary
# first so `tests/cross_language.py` can pipe attestations through it.
check-python: python-build python-test python-test-langgraph

python-build:
	@test -d $(PY_VENV) || python3 -m venv $(PY_VENV)
	$(PY_VENV_BIN)/pip install --quiet --upgrade pip
	$(PY_VENV_BIN)/pip install --quiet maturin pytest langgraph
	cargo build -p attestly-core --bin attestly-verify --release
	cd crates/attestly-python && ../../$(PY_VENV_BIN)/maturin develop --release --quiet

python-test:
	$(PY_VENV_BIN)/python -m pytest crates/attestly-python/tests -v

# `attestly-langgraph` is a pure-Python package that shares the `attestly` namespace
# package with `attestly-core-py`. It must be installed (editable) rather than
# put on PYTHONPATH: it ships no `attestly/__init__.py`, so a raw PYTHONPATH
# entry would not merge into the `attestly` package that `maturin develop`
# installed. `--no-deps` skips attestly-core-py (already built above) and
# langgraph (installed by `python-build`).
python-test-langgraph:
	$(PY_VENV_BIN)/pip install --quiet --no-deps -e python/attestly-langgraph
	$(PY_VENV_BIN)/python -m pytest python/attestly-langgraph/tests -v

fix:
	cargo fmt --all
	cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged
	$(MAKE) -C services/attestly-cloud fix

# Recursive targets into the attestly-cloud sub-workspace.
cloud-check:
	$(MAKE) -C services/attestly-cloud check

cloud-lint:
	$(MAKE) -C services/attestly-cloud lint

cloud-test:
	$(MAKE) -C services/attestly-cloud test

cloud-fix:
	$(MAKE) -C services/attestly-cloud fix

# Step 4 verification helpers — forwarded into the attestly-cloud sub-workspace
# so they're discoverable from the repo root alongside `make check`.
seed-overage:
	$(MAKE) -C services/attestly-cloud seed-overage

seed-old-attestations:
	$(MAKE) -C services/attestly-cloud seed-old-attestations
