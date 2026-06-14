.PHONY: help install test check check-all run tui build clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

# ======== Install ========

install: ## Install all dependencies (Rust + Node.js)
	@echo "==> Installing Rust dependencies..."
	cd baoclaw-core && cargo fetch
	@echo "==> Installing Node.js dependencies..."
	cd ts-ipc && npm install
	@echo "==> Done. Run 'make run' to start."

build: ## Build release binary
	cd baoclaw-core && cargo build --release
	@echo "Binary: baoclaw-core/target/release/baoclaw-core"

# ======== Quality ========

test: ## Run all Rust tests
	cd baoclaw-core && cargo test -- --test-threads=2

test-fast: ## Run unit tests only (skip slow integration tests)
	cd baoclaw-core && cargo test --lib -- --test-threads=4

check: ## Cargo check (fast compile verification)
	cd baoclaw-core && cargo check

clippy: ## Run clippy lints
	cd baoclaw-core && cargo clippy -- -D warnings

fmt: ## Run rustfmt check
	cd baoclaw-core && cargo fmt -- --check

fmt-fix: ## Auto-fix formatting
	cd baoclaw-core && cargo fmt

check-all: check clippy fmt ## All quality checks (no tests)

# ======== Run ========

run: ## Start the BaoClaw daemon
	cd baoclaw-core && cargo run -- --daemon --cwd $(shell pwd)

tui: ## Start the TUI (requires daemon running)
	@SOCK=$$(find /tmp -name "baoclaw*.sock" -user $$USER 2>/dev/null | head -1); \
	if [ -z "$$SOCK" ]; then \
		echo "ERROR: No daemon socket found. Start daemon first: make run"; \
		exit 1; \
	fi; \
	echo "Connecting to $$SOCK"; \
	cd ts-ipc && npm run tui -- $$SOCK

cli: ## Run CLI (one-shot command, requires daemon)
	@SOCK=$$(find /tmp -name "baoclaw*.sock" -user $$USER 2>/dev/null | head -1); \
	if [ -z "$$SOCK" ]; then \
		echo "ERROR: No daemon socket found. Start daemon first: make run"; \
		exit 1; \
	fi; \
	cd ts-ipc && npx tsx cli.ts --socket $$SOCK

# ======== Cleanup ========

clean: ## Clean build artifacts
	cd baoclaw-core && cargo clean
	cd ts-ipc && rm -rf node_modules
	@echo "Cleaned."
