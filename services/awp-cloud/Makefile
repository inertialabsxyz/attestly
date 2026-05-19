.PHONY: check lint test fix up down logs seed

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
