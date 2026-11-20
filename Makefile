.PHONY: build install test clean

BINARY := claude-statusline

# Same destination (and env var) as install.sh, so a dev build replaces the
# binary `statusLine` already points at instead of landing beside it.
# Override: make install INSTALL_DIR=/usr/local/bin
INSTALL_DIR ?= $(or $(CLAUDE_STATUSLINE_INSTALL_DIR),$(HOME)/.local/bin)

build:
	cargo build --release

# Copy next to the target, then rename: Claude Code runs this binary on every
# prompt, and a rename never exposes a half-written file to a live session.
install: build
	mkdir -p "$(INSTALL_DIR)"
	cp target/release/$(BINARY) "$(INSTALL_DIR)/.$(BINARY).new"
	mv -f "$(INSTALL_DIR)/.$(BINARY).new" "$(INSTALL_DIR)/$(BINARY)"
	@echo "Installed to $(INSTALL_DIR)/$(BINARY)"

test: build
	cargo test
	sh tests/install_test.sh

clean:
	cargo clean
