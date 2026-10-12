# rMath — convenience commands
#
#   make build      build the kernel (debug)
#   make release    build the kernel (release, used by the notebook)
#   make test       run the test suite (unit + golden)
#   make py-test    run the Python front-end unit tests
#   make check      test + py-test + oracle + lean-test (the fast CI set)
#   make repl       interactive kernel REPL
#   make eval E='D[Sin[x]^2, x]'    one-shot evaluation
#   make demo       evaluate demo/demo.mt headlessly
#   make notebook   start the web notebook (foreground)
#   make notebook-bg   start it in the background
#   make stop       stop the notebook server
#   make restart    stop + start in the background
#   make mcp        run the MCP server (stdio) for an MCP client
#   make mcp-http   run the MCP server (Streamable HTTP, loopback)
#   make clean      remove build artifacts

SHELL  := /bin/bash
ROOT   := $(patsubst %/,%,$(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
CARGO  := $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
PORT   ?= 8787
MCP_PORT ?= 8790
BIN    := $(ROOT)/rmath/target/release/rmath
LOGFILE ?= /tmp/rmath-notebook.log

.PHONY: help build release test py-test check repl eval demo notebook notebook-bg stop restart clean \
        mcp mcp-http \
        app app-run app-install app-stop app-uninstall shot svg2png report uitest texcheck lean-test lean-verify demo-gen

help:
	@sed -n '2,18p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

build:
	cd $(ROOT)/rmath && $(CARGO) build

release:
	cd $(ROOT)/rmath && $(CARGO) build --release

test:
	cd $(ROOT)/rmath && $(CARGO) test

# Unit tests for the Python front end (notebook server, SVG, paths, harness and
# the Lean bridge's translation helpers). No Lean, network or running server.
py-test:
	cd $(ROOT) && python3 -m unittest discover -s tests

# The fast, dependency-free set CI runs: kernel tests, front-end tests, the
# independent algebra oracle, and the Lean translation tests.
check: test py-test oracle lean-test
	@echo "rMath check: all fast suites passed"

repl: release
	cd $(ROOT) && $(BIN)

eval: release
	@cd $(ROOT) && $(BIN) -e "$(E)"

demo: release
	@cd $(ROOT) && $(BIN) -e 'Get["demo/demo.mt"]' 2>&1 | tail -5

notebook: release
	cd $(ROOT) && python3 -m rmathpy.notebook --port $(PORT)

# MCP server for agents. `make mcp` speaks the default stdio transport (an MCP
# client launches it); `make mcp-http` serves Streamable HTTP on loopback.
mcp: release
	cd $(ROOT) && python3 -m rmathpy.mcp

mcp-http: release
	cd $(ROOT) && python3 -m rmathpy.mcp --http --port $(MCP_PORT)

notebook-bg: release
	@if pgrep -f 'rmathpy.notebook' >/dev/null 2>&1; then \
	  echo "already running — 'make stop' first to restart"; \
	else \
	  cd $(ROOT) && nohup python3 -m rmathpy.notebook --port $(PORT) > $(LOGFILE) 2>&1 & \
	  sleep 1; \
	  echo "rMath notebook: http://127.0.0.1:$(PORT)  (log: $(LOGFILE))"; \
	fi

stop:
	@pkill -f 'rmathpy.notebook' 2>/dev/null; pkill -f 'rmath --server' 2>/dev/null; \
	  sleep 1; echo "rMath notebook stopped"

restart: stop notebook-bg

clean:
	cd $(ROOT)/rmath && $(CARGO) clean

# ---------------- development helpers ----------------
# Render the running notebook (or any URL) to a PNG with WebKit — no screen
# recording permission needed, because it renders the page rather than
# capturing the screen.
#   make shot                                   # http://127.0.0.1:8787
#   make shot URL=http://127.0.0.1:8787 JS='document.getElementById("inp").value="1+1"; evaluate();'
URL ?= http://127.0.0.1:8787
JS  ?=
OUT ?= /tmp/rmath-shot.png
shot: scripts/dev/screenshot
	@scripts/dev/screenshot "$(URL)" "$(OUT)" 8 '$(JS)'
	@echo "wrote $(OUT)"

scripts/dev/screenshot: scripts/dev/screenshot.swift
	@mkdir -p $(ROOT)/scripts/dev/.build/modulecache
	@CLANG_MODULE_CACHE_PATH="$(ROOT)/scripts/dev/.build/modulecache" \
	  swiftc -O "$<" -o "$@" -framework Cocoa -framework WebKit \
	  -module-cache-path "$(ROOT)/scripts/dev/.build/modulecache"
	@echo "built $@"

# Ask the running app for a bug report (screenshot + transcript + log) and
# print the path; the same text is placed on the clipboard.
report:
	@$(HOME)/Applications/rMath.app/Contents/MacOS/rMath --report

# Click every notebook control in a real WebKit page (fails on JS errors).
#   make uitest          # against http://127.0.0.1:8787
uitest:
	@bash $(ROOT)/scripts/dev/uitest.sh "$(URL)"

# Verify a battery of claims in Lean, including ones that must be rejected.
# Slow (Mathlib loads per claim, ~10s each): run it deliberately, not on every build.
lean-verify:
	@python3 $(ROOT)/scripts/dev/lean_verify_test.py

# Regenerate demo/demo.mt: every candidate example is executed against the
# kernel first, and only examples that really evaluate are written out.
demo-gen:
	@python3 $(ROOT)/scripts/dev/demo_gen.py

# Translation tests for the Lean bridge (no Lean needed).
lean-test:
	@python3 $(ROOT)/scripts/dev/lean_translation_test.py

# Benchmark the Lean bridge over the golden files and the demo: translation by
# default (fast); `make lean-bench BENCH=--verify` also runs Lean per claim.
lean-bench:
	@python3 $(ROOT)/scripts/dev/lean_bench.py $(BENCH)

# Random + invariant correctness oracle over the algebra surface (dev only).
# `make oracle ORACLE="-n 200 --seed 7"` for a longer run.
oracle:
	@python3 $(ROOT)/scripts/dev/oracle_test.py $(ORACLE)

# Check that every TeX string the demo produces parses in KaTeX.
#   make texcheck            # loads demo/demo.mt
#   make texcheck FILE=other.mt
FILE ?= demo/demo.mt
texcheck:
	@bash $(ROOT)/scripts/dev/texcheck.sh "$(URL)" "$(FILE)"

# Convert an SVG (e.g. an exported plot) to PNG with an independent renderer.
#   make svg2png IN=plot.svg OUT=plot.png
RSVG ?= $(shell command -v rsvg-convert 2>/dev/null || echo $(HOME)/miniconda3/bin/rsvg-convert)
svg2png:
	@$(RSVG) -o "$(OUT)" "$(IN)" && echo "wrote $(OUT)"

# ---------------- macOS application bundle ----------------
APPDIR   ?= $(HOME)/Applications
APP      := $(ROOT)/dist/rMath.app

app: release
	bash $(ROOT)/scripts/build_app.sh

app-run: app
	open $(APP)

# Kill any running instance before replacing/installing the bundle.
app-stop:
	@$(APP)/Contents/MacOS/rMath --stop 2>/dev/null || true
	@echo "rMath app stopped"

app-install: app-stop app
	@# Exactly one installation, in the place the Finder, Launchpad and Spotlight
	@# actually look: /Applications when it is writable, otherwise ~/Applications.
	@# (Two copies would go stale against each other and make `open -a rMath`
	@# ambiguous.)
	@INSTALLED=""; \
	if mkdir -p /Applications 2>/dev/null && [ -w /Applications ]; then \
	  rm -rf /Applications/rMath.app && cp -R $(APP) /Applications/rMath.app && \
	  rm -rf $(APPDIR)/rMath.app && INSTALLED=/Applications/rMath.app; \
	elif mkdir -p $(APPDIR) 2>/dev/null && [ -w $(APPDIR) ]; then \
	  rm -rf $(APPDIR)/rMath.app && cp -R $(APP) $(APPDIR)/rMath.app && \
	  INSTALLED=$(APPDIR)/rMath.app; \
	fi; \
	if [ -z "$$INSTALLED" ]; then \
	  echo "app-install: neither /Applications nor $(APPDIR) is writable here" >&2; \
	  echo "             (run it outside a restricted sandbox, or copy $(APP) yourself)" >&2; \
	  exit 1; \
	fi; \
	LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister; \
	$$LSREG -f "$$INSTALLED" >/dev/null 2>&1 || true; \
	mdimport "$$INSTALLED" >/dev/null 2>&1 || true; \
	mkdir -p "$(HOME)/Library/Application Support/rMath" 2>/dev/null || true; \
	printf '%s\n' "$(ROOT)" > "$(HOME)/Library/Application Support/rMath/workspace" 2>/dev/null || true; \
	echo "    installed: $$INSTALLED"; \
	echo "    launch with: open -a rMath   (or Spotlight: Cmd-Space, type rMath)"

app-uninstall: app-stop
	rm -rf $(APPDIR)/rMath.app
	@echo "removed $(APPDIR)/rMath.app"
