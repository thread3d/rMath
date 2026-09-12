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

.PHONY: help build release test repl eval demo notebook notebook-bg stop restart clean

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
