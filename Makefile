# Regenerate game datasets from an installed Star Citizen build.
#
#   make                       # build, extract, parse into data/
#   make CHANNEL=PTU           # same, from the PTU install
#   make DOMAINS="mining"      # just one domain
#   make parse                 # re-parse without touching the archive
#   make live | ptu | hotfix   # shorthand for CHANNEL=LIVE / PTU / HOTFIX
#   make changes FROM=4.10     # buffs and nerfs vs a previous build
#   make publish-dry           # then: make publish (tag + GitHub release)
#   make republish             # add datasets to a build already published
#
# Optional overrides: SC_ROOT, CHANNEL, P4K, STARBREAKER, EXTRACT_DIR, OUT,
# DOMAINS, FROM, TO, PUBLISH_CHANNEL. P4K defaults to $SC_DATA_P4K, else
# $(SC_ROOT)/$(CHANNEL)/Data.p4k.

SC_ROOT     ?= /mnt/games/star-citizen/drive_c/Program Files/Roberts Space Industries/StarCitizen
CHANNEL     ?= LIVE
EXTRACT_DIR ?= ./extracted-data
OUT         ?= ./data
DOMAINS     ?=
P4K         ?= $(or $(SC_DATA_P4K),$(SC_ROOT)/$(CHANNEL)/Data.p4k)
# StarBreaker: $STARBREAKER_BIN, else the local source build, else PATH.
STARBREAKER_SRC ?= $(HOME)/git/StarBreaker
STARBREAKER ?= $(or $(STARBREAKER_BIN),$(wildcard $(STARBREAKER_SRC)/target/release/starbreaker))
FROM        ?=
TO          ?= local

CLI   := node dist/cli.js
FLAGS := --channel $(CHANNEL) --extract-dir $(EXTRACT_DIR) --out $(OUT) \
	$(if $(P4K),--p4k "$(P4K)") $(if $(STARBREAKER),--starbreaker "$(STARBREAKER)")

SRC := $(shell find src -name '*.ts' ! -name '*.test.ts') tsconfig.json

.PHONY: gamedata all live ptu hotfix extract parse build install test lint \
	check changes export publish publish-dry republish builds clean help p4k starbreaker

gamedata: all

# The string table is part of a full dataset set; a DOMAINS run leaves it be.
STRINGS = $(if $(DOMAINS),,$(CLI) strings --write --extract-dir $(EXTRACT_DIR) --out $(OUT))

all: build p4k
	$(CLI) all $(DOMAINS) $(FLAGS)
	$(STRINGS)
	$(CLI) builds

live ptu hotfix:
	$(MAKE) all CHANNEL=$(shell echo $@ | tr a-z A-Z)

p4k:
	@test -f "$(P4K)" || { echo "no Data.p4k at: $(P4K)"; echo "install the $(CHANNEL) channel, or pass P4K=<path> / SC_ROOT=<dir>"; exit 1; }

extract: build p4k
	$(CLI) extract $(DOMAINS) $(FLAGS)

parse: build
	$(CLI) parse $(DOMAINS) $(FLAGS)
	$(STRINGS)

build: dist/cli.js

dist/cli.js: node_modules $(SRC)
	npx tsc
	@touch $@

install: node_modules

node_modules: package.json package-lock.json
	npm ci
	@touch $@

test: node_modules
	npx vitest run

lint: node_modules
	npx eslint "src/**/*.ts"

check: lint test build

changes: build
	@test -n "$(FROM)" || { echo "usage: make changes FROM=<build> [TO=local]"; exit 1; }
	$(CLI) changes $(FROM) $(TO) --out $(OUT)

export: build
	npm run export

# Tags the build and creates its GitHub release (PTU = prerelease). The channel
# comes from the extract marker; PUBLISH_CHANNEL overrides it.
PUBLISH := SC_EXTRACT_DIR=$(EXTRACT_DIR) node scripts/publish-dump.mjs \
	$(if $(PUBLISH_CHANNEL),--channel $(PUBLISH_CHANNEL))

publish-dry: build
	$(PUBLISH) --dry-run

publish: build
	$(PUBLISH)

# Replace an already-published build with the current data/ (same build only).
republish: build
	$(PUBLISH) --republish

builds: build
	$(CLI) builds --out $(OUT)

# Rebuild the StarBreaker CLI from the local checkout.
starbreaker:
	cargo build --release --manifest-path $(STARBREAKER_SRC)/Cargo.toml -p starbreaker

clean:
	rm -rf dist

help:
	@sed -n '1,/^$$/p' $(firstword $(MAKEFILE_LIST)) | sed 's/^# \{0,1\}//'
