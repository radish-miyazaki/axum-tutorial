build:
	docker-compose build

up:
	docker-compose up -d

down:
	docker-compose down

watch:
	sqlx db create
	sqlx migrate run
	cargo watch -x run

test:
	cargo test

test-s: # stand alone test
	cargo test --no-default-features
