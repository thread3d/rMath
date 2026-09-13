# rMath — convenience commands
#
#   make build      build the kernel (debug)
#   make release    build the kernel (release, used by the notebook)
#   make test       run the test suite (unit + golden)
#   make repl       interactive kernel REPL
#   make eval E='D[Sin[x]^2, x]'    one-shot evaluation
#   make demo       evaluate demo/demo.mt headlessly
#   make notebook   start the web notebook (foreground)
#   make notebook-bg   start it in the background
#   make stop       stop the notebook server
#   make restart    stop + start in the background
#   make clean      remove build artifacts

SHELL  := /bin/bash
ROOT   := $(patsubst %/,%,$(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
CARGO  := $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
PORT   ?= 8787
BIN    := $(ROOT)/rmath/target/release/rmath
LOGFILE ?= /tmp/rmath-notebook.log

.PHONY: help build release test repl eval demo notebook notebook-bg stop restart clean \
        app app-run app-install app-stop app-uninstall shot svg2png report uitest texcheck lean-test lean-verify demo-gen

help:
	@sed -n '2,16p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

build:
	cd $(ROOT)/rmath && $(CARGO) build

release:
	cd $(ROOT)/rmath && $(CARGO) build --release

test:
	cd $(ROOT)/rmath && $(CARGO) test

repl: release
	cd $(ROOT) && $(BIN)

eval: release
	@cd $(ROOT) && $(BIN) -e "$(E)"

demo: release
	@cd $(ROOT) && $(BIN) -e 'Get["demo/demo.mt"]' 2>&1 | tail -5

notebook: release
	cd $(ROOT) && python3 -m rmathpy.notebook --port $(PORT)

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
	mkdir -p $(APPDIR)
	rm -rf $(APPDIR)/rMath.app
	cp -R $(APP) $(APPDIR)/rMath.app
	@mkdir -p "$(HOME)/Library/Application Support/rMath"
	@printf '%s\n' "$(ROOT)" > "$(HOME)/Library/Application Support/rMath/workspace"
	@echo "    reports will be written to $(ROOT)/.shots/reports"
	@/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
	  -f $(APPDIR)/rMath.app >/dev/null 2>&1 || true
	@echo "installed: $(APPDIR)/rMath.app  (open it from Finder or Spotlight)"

app-uninstall: app-stop
	rm -rf $(APPDIR)/rMath.app
	@echo "removed $(APPDIR)/rMath.app"
