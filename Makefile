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
        app app-run app-install app-stop app-uninstall shot svg2png

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
	@/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
	  -f $(APPDIR)/rMath.app >/dev/null 2>&1 || true
	@echo "installed: $(APPDIR)/rMath.app  (open it from Finder or Spotlight)"

app-uninstall: app-stop
	rm -rf $(APPDIR)/rMath.app
	@echo "removed $(APPDIR)/rMath.app"
