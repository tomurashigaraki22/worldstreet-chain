IMAGE ?= worldstreet-chain:dev

.PHONY: fmt lint test build audit deny sdk-typecheck devnet-up devnet-down docker-build docker-test

fmt:
	docker compose run --rm wsc cargo fmt --all -- --check

lint:
	docker compose run --rm wsc cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	docker compose run --rm wsc cargo test --workspace

build:
	docker compose run --rm wsc cargo build --workspace

audit:
	docker compose run --rm wsc cargo audit

deny:
	docker compose run --rm wsc cargo deny check

sdk-typecheck:
	cd sdk/typescript && npm install && npm run typecheck

devnet-up:
	docker compose -f devnet/docker-compose.yml up --build

devnet-down:
	docker compose -f devnet/docker-compose.yml down -v

docker-build:
	docker build -t $(IMAGE) .

docker-test:
	docker run --rm $(IMAGE) cargo test --workspace
