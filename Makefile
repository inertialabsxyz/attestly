.PHONY: check lint test test-unit test-int fix \
        check-python python-build python-test python-test-langgraph \
        cloud-check cloud-lint cloud-test cloud-fix \
        seed-overage seed-old-attestations

# Python venv used for the awp-python tests. Override with PY_VENV=... at
# the call site if you want to point at an existing interpreter. The
# `python-build` target creates it on first use.
PY_VENV ?= .venv
PY_VENV_BIN := $(PY_VENV)/bin

# Top-level gate: lint + test the core workspace, the Python bindings, and
# the `services/awp-cloud/` sub-workspace (which is its own Cargo workspace
# by design — independent release cadence, isolated from the core's clippy
# regime). Every contributor's `make check` is a complete CI proxy.
check: lint test check-python cloud-check

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
check-python: python-build python-test python-test-langgraph

python-build:
	@test -d $(PY_VENV) || python3 -m venv $(PY_VENV)
	$(PY_VENV_BIN)/pip install --quiet --upgrade pip
	$(PY_VENV_BIN)/pip install --quiet maturin pytest langgraph
	cargo build -p awp-core --bin awp-verify --release
	cd crates/awp-python && ../../$(PY_VENV_BIN)/maturin develop --release --quiet

python-test:
	$(PY_VENV_BIN)/python -m pytest crates/awp-python/tests -v

# `awp-langgraph` is a pure-Python package that lives outside the maturin
# build. We run its pytest with the package directory on PYTHONPATH so the
# check works against the in-tree source without a separate install step.
# It imports `awp` (provided by the `maturin develop` step above) and
# `langgraph` (installed by `python-build`).
python-test-langgraph:
	PYTHONPATH=python/awp-langgraph $(PY_VENV_BIN)/python -m pytest python/awp-langgraph/tests -v

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

# Step 4 verification helpers — forwarded into the awp-cloud sub-workspace
# so they're discoverable from the repo root alongside `make check`.
seed-overage:
	$(MAKE) -C services/awp-cloud seed-overage

seed-old-attestations:
	$(MAKE) -C services/awp-cloud seed-old-attestations
