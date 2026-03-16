.PHONY: build install test clean

BINARY := claude-statusline

build:
	cargo build --release

install: build
	cp target/release/$(BINARY) /usr/local/bin/$(BINARY)
	@echo "Installed to /usr/local/bin/$(BINARY)"

test: build
	@bash test.sh

clean:
	cargo clean
