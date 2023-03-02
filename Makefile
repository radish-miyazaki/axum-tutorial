build:
	docker-compose build

up:
	docker-compose up

down:
	docker-compose down

watch:
	cargo watch -x run

test:
	cargo test
