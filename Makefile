.PHONY: run build rust frontend tauri-backend deps clean release release-watch release-download

.DEFAULT_GOAL := run

ROOT_DIR := $(CURDIR)
DESKTOP_DIR := $(ROOT_DIR)/desktop
TAURI_DIR := $(DESKTOP_DIR)/src-tauri

run: build
	cd "$(DESKTOP_DIR)" && npm run tauri -- dev

build: deps rust frontend tauri-backend

deps: $(DESKTOP_DIR)/node_modules

$(DESKTOP_DIR)/node_modules:
	cd "$(DESKTOP_DIR)" && npm install

rust:
	cargo build

frontend: deps
	cd "$(DESKTOP_DIR)" && npm run build

tauri-backend:
	cargo check --manifest-path "$(TAURI_DIR)/Cargo.toml"

clean:
	cargo clean
	rm -rf "$(DESKTOP_DIR)/dist" "$(DESKTOP_DIR)/node_modules" "$(TAURI_DIR)/target"

release:
	gh workflow run release-windows.yml

release-watch:
	@RUN_ID=$$(gh run list --workflow release-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId'); \
	if [ -z "$$RUN_ID" ]; then \
		echo "No runs found for release-windows.yml"; \
		exit 1; \
	fi; \
	gh run watch "$$RUN_ID"

release-download:
	@RUN_ID=$$(gh run list --workflow release-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId'); \
	if [ -z "$$RUN_ID" ]; then \
		echo "No runs found for release-windows.yml"; \
		exit 1; \
	fi; \
	mkdir -p "$(ROOT_DIR)/release"; \
	gh run download "$$RUN_ID" --dir "$(ROOT_DIR)/release"
