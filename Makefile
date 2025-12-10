# VibeMQ Makefile
# Build, test, and run conformance tests

.PHONY: all build build-release test test-unit test-integration test-conformance \
        conformance conformance-v3 conformance-v5 run clean help \
        install-testmqtt

# Configuration
BROKER_ADDR ?= localhost:1883
TESTMQTT_VERSION := 0.1.1
TESTMQTT_URL := https://github.com/bromq-dev/testmqtt/releases/download/v$(TESTMQTT_VERSION)/testmqtt_$(TESTMQTT_VERSION)_linux_amd64.tar.gz
TESTMQTT_BIN := ./bin/testmqtt

# Default target
all: build test

# Build targets
build:
	cargo build

build-release:
	cargo build --release

# Test targets (cargo tests)
test: test-unit test-integration test-conformance

test-unit:
	cargo test --lib

test-integration:
	cargo test --test integration

test-conformance:
	cargo test --test conformance

# Install testmqtt binary
install-testmqtt: $(TESTMQTT_BIN)

$(TESTMQTT_BIN):
	@mkdir -p ./bin
	@echo "Downloading testmqtt $(TESTMQTT_VERSION)..."
	@curl -sL $(TESTMQTT_URL) | tar -xz -C ./bin
	@chmod +x $(TESTMQTT_BIN)
	@echo "testmqtt installed to $(TESTMQTT_BIN)"

# External conformance tests using testmqtt
# These require a running broker at BROKER_ADDR
conformance: conformance-v3 conformance-v5

conformance-v3: $(TESTMQTT_BIN)
	@echo "Running MQTT v3.1.1 conformance tests..."
	$(TESTMQTT_BIN) conformance --version 3 --broker tcp://$(BROKER_ADDR) --verbose

conformance-v5: $(TESTMQTT_BIN)
	@echo "Running MQTT v5.0 conformance tests..."
	$(TESTMQTT_BIN) conformance --version 5 --broker tcp://$(BROKER_ADDR) --verbose

# Run specific test groups (usage: make conformance-group GROUPS="Connection,QoS" VERSION=3)
conformance-group: $(TESTMQTT_BIN)
	$(TESTMQTT_BIN) conformance --version $(VERSION) --broker tcp://$(BROKER_ADDR) --tests $(GROUPS) --verbose

# Run broker in background and execute conformance tests
conformance-ci: build-release $(TESTMQTT_BIN)
	@echo "Starting broker..."
	@./target/release/vibemq -b 127.0.0.1:1883 &
	@sleep 2
	@echo "Running conformance tests..."
	@$(MAKE) conformance BROKER_ADDR=127.0.0.1:1883; \
	status=$$?; \
	pkill -f "vibemq -b 127.0.0.1:1883" 2>/dev/null || true; \
	exit $$status

# Run the broker
run:
	cargo run -- -b 0.0.0.0:1883

run-release:
	cargo run --release -- -b 0.0.0.0:1883

# Clean
clean:
	cargo clean
	rm -rf ./bin

# Help
help:
	@echo "VibeMQ Makefile"
	@echo ""
	@echo "Build targets:"
	@echo "  build          - Debug build"
	@echo "  build-release  - Release build"
	@echo ""
	@echo "Test targets (cargo tests):"
	@echo "  test           - Run all cargo tests"
	@echo "  test-unit      - Run unit tests"
	@echo "  test-integration - Run integration tests"
	@echo "  test-conformance - Run conformance tests"
	@echo ""
	@echo "External conformance tests (requires running broker):"
	@echo "  conformance    - Run all conformance tests (v3 + v5)"
	@echo "  conformance-v3 - Run MQTT v3.1.1 tests (77 tests)"
	@echo "  conformance-v5 - Run MQTT v5.0 tests (139 tests)"
	@echo "  conformance-ci - Build, start broker, run tests, stop broker"
	@echo ""
	@echo "  conformance-group VERSION=3 GROUPS='Connection,QoS'"
	@echo "                 - Run specific test groups"
	@echo ""
	@echo "Other targets:"
	@echo "  install-testmqtt - Download testmqtt binary"
	@echo "  run            - Run broker (debug)"
	@echo "  run-release    - Run broker (release)"
	@echo "  clean          - Clean build artifacts"
	@echo ""
	@echo "Configuration:"
	@echo "  BROKER_ADDR    - Broker address (default: localhost:1883)"
