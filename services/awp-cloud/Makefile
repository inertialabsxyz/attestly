.PHONY: check lint test fix up down logs seed seed-overage seed-old-attestations

check: lint test

lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

fix:
	cargo fmt --all
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

# Bring the local stack up. Postgres + Minio + awp-cloud all behind docker.
up:
	docker compose up -d --build

down:
	docker compose down

# Wipe persistent volumes too.
clean:
	docker compose down -v

logs:
	docker compose logs -f awp-cloud

# Insert 10k synthetic attestations for hand-testing. Prints the API key on
# stdout; capture it into `$$TEST_KEY` for the curl commands in the README.
seed:
	docker compose exec awp-cloud /usr/local/bin/awp-cloud-seed

# Provision a synthetic Team account with 1.05M attestations of usage in
# the current calendar month. Drives the Step 4 metered-billing
# verification path:
#
#   make seed-overage
#   curl -X POST http://localhost:8080/v1/admin/bill-period \
#        -H "x-admin-key: $$AWP_ADMIN_KEY"
seed-overage:
	docker compose exec awp-cloud /usr/local/bin/awp-cloud-seed overage

# Insert 100 attestations dated 400 days ago under a fresh Team account.
# Drives the Step 4 retention-sweeper verification:
#
#   make seed-old-attestations
#   docker compose exec awp-cloud /usr/local/bin/sweeper run-once
#   # → 100 rows deleted
seed-old-attestations:
	docker compose exec awp-cloud /usr/local/bin/awp-cloud-seed old-attestations
