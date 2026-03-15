.PHONY: build install test clean

build:
	cargo build --release

install: build
	cp target/release/claude-statusline ~/.claude/claude-statusline
	@echo "Installed to ~/.claude/claude-statusline"
	@echo ""
	@echo "Add to your settings.json:"
	@echo '  { "statusLine": { "type": "command", "command": "~/.claude/claude-statusline" } }'

test: build
	@bash test.sh

clean:
	cargo clean
