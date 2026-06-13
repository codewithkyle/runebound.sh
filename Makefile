.PHONY: run build rust frontend tauri-backend deps clean

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
