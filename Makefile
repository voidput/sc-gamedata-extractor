# Regenerate game datasets from an installed Star Citizen build, with the Rust
# binary (target/release/sc-gamedata, built by cargo on every target that needs it).
#
#   make                       # build, then read Data.p4k into data/ (every dataset,
#                              # the string table and the search index)
#   make CHANNEL=PTU           # same, from the PTU install
#   make DOMAINS="mining"      # just one domain
#   make parse | extract       # same as make all, without the builds listing
#   make live | ptu | hotfix   # shorthand for CHANNEL=LIVE / PTU / HOTFIX
#   make changes FROM=4.10     # buffs and nerfs vs a previous build
#   make notes FROM=4.10.197 TO=local  # player-facing patch notes (NOTES=file.md writes it)
#   make obtain ITEM="Verdict LMG"  # where to loot, craft, earn or buy an item
#   make obtain ITEM="Cambio SRT Canister" AT=grimhex  # only shops at a place
#   make obtain ITEM="Cambio SRT" AMMO=1 AT=grimhex  # the weapon's ammo instead
#   make where ITEM="FS-9 LMG"  # one table: buy, reward, craft, loot (AT, AMMO too)
#   make shop AT="grim hex"    # every item UEX lists at a terminal or place
#   make find [Q=verdict]      # fuzzy finder (fzf) over every item and ship
#   make publish-dry           # then: make publish (commit, tag, push + gh release)
#   make republish             # add datasets to a build already published
#   make release-live | release-ptu  # extract + parse + publish-dry + publish one channel
#   make release-check         # installed LIVE/PTU builds vs what the dumps have
#   make hook-install [AUTO=1] # launcher post-exit hook: release unpublished builds
#   make hook-uninstall        # remove the hook (sc-postexit.sh + sc-prelaunch.sh)
#   make export                # infographic CSV/XLSX from $(OUT) (needs the TS build)
#   make test | lint | check   # cargo test + vitest, clippy + eslint, both + build
#
# Optional overrides: SC_ROOT, CHANNEL, P4K, EXTRACT_DIR, OUT, DOMAINS, FROM, TO,
# NOTES, PUBLISH_CHANNEL, PATCH, ITEM, AT, AMMO, Q. P4K defaults to $SC_DATA_P4K,
# else $(SC_ROOT)/$(CHANNEL)/Data.p4k. EXTRACT_DIR and OUT default per channel:
# ./extracted-data + ./data for LIVE, ./extracted-data-ptu + ./data-ptu for PTU,
# so a PTU run never overwrites the LIVE datasets. EXTRACT_DIR now only holds the
# _build.json marker that publish reads. STARBREAKER is for make starbreaker only.

SC_ROOT     ?= /mnt/games/star-citizen/drive_c/Program Files/Roberts Space Industries/StarCitizen
CHANNEL     ?= LIVE
CHANNEL_SUFFIX := $(if $(filter LIVE,$(CHANNEL)),,-$(shell echo '$(CHANNEL)' | tr A-Z a-z))
EXTRACT_DIR ?= ./extracted-data$(CHANNEL_SUFFIX)
OUT         ?= ./data$(CHANNEL_SUFFIX)
DOMAINS     ?=
P4K         ?= $(or $(SC_DATA_P4K),$(SC_ROOT)/$(CHANNEL)/Data.p4k)
# StarBreaker: $STARBREAKER_BIN, else the local source build, else PATH.
STARBREAKER_SRC ?= $(HOME)/git/StarBreaker
STARBREAKER ?= $(or $(STARBREAKER_BIN),$(wildcard $(STARBREAKER_SRC)/target/release/starbreaker))
FROM        ?=
TO          ?= local

# The Rust binary does every dataset step; `make build` (cargo) builds it. The
# TypeScript left under src/ is SCMCP's vendored query layer (src/query/) and
# the combat maths `make export` reads (dist/deflection.js, dist/engagement.js).
BIN   := ./target/release/sc-gamedata
CLI   := $(BIN)
FLAGS := --channel $(CHANNEL) --extract-dir $(EXTRACT_DIR) --out $(OUT) \
	$(if $(P4K),--p4k "$(P4K)")

TS_SRC := $(shell find src -name '*.ts' ! -name '*.test.ts') tsconfig.json

.PHONY: gamedata all live ptu hotfix extract parse build build-ts install test lint \
	check changes notes obtain where shop find export publish publish-dry republish builds clean help p4k starbreaker \
	release-live release-ptu release-check hook-install hook-uninstall

gamedata: all

# The binary reads Data.p4k itself: extract, parse and all are one pass that
# writes every dataset, plus the string table and search index when DOMAINS is
# empty (a DOMAINS run leaves those be). EXTRACT_DIR only holds _build.json.
all: build p4k
	$(CLI) all $(DOMAINS) $(FLAGS)
	$(CLI) builds --out $(OUT)

live ptu hotfix:
	$(MAKE) all CHANNEL=$(shell echo $@ | tr a-z A-Z)

p4k:
	@test -f "$(P4K)" || { echo "no Data.p4k at: $(P4K)"; echo "install the $(CHANNEL) channel, or pass P4K=<path> / SC_ROOT=<dir>"; exit 1; }

extract: build p4k
	$(CLI) extract $(DOMAINS) $(FLAGS)

parse: build p4k
	$(CLI) parse $(DOMAINS) $(FLAGS)

# cargo decides what to rebuild, so this runs it every time (a no-op when current).
build:
	cargo build --release -q -p sc-gamedata --bin sc-gamedata

# The TypeScript build, for make export only.
build-ts: dist/engagement.js

dist/engagement.js: node_modules $(TS_SRC)
	npx tsc
	@touch $@

install: node_modules

node_modules: package.json package-lock.json
	npm ci
	@touch $@

test: node_modules
	cargo test -q -p sc-gamedata
	npx vitest run

lint: node_modules
	cargo clippy -q --all-targets
	npx eslint "src/**/*.ts"

check: lint test build

find: build
	$(CLI) find $(if $(Q),'$(Q)') --out $(OUT)

obtain: build
	@if [ -z '$(ITEM)' ]; then echo 'usage: make obtain ITEM="<name or class name>"'; exit 1; fi
	$(CLI) obtain '$(ITEM)' --out $(OUT) $(if $(AT),--at '$(AT)') $(if $(AMMO),--ammo)

where: build
	@if [ -z '$(ITEM)' ]; then echo 'usage: make where ITEM="<name or class name>" [AT=<place>] [AMMO=1]'; exit 1; fi
	$(CLI) where '$(ITEM)' --out $(OUT) $(if $(AT),--at '$(AT)') $(if $(AMMO),--ammo)

shop: build
	@if [ -z '$(AT)' ]; then echo 'usage: make shop AT="<terminal or place>"'; exit 1; fi
	$(CLI) shop '$(AT)' --out $(OUT)

changes: build
	@test -n "$(FROM)" || { echo "usage: make changes FROM=<build> [TO=local]"; exit 1; }
	$(CLI) changes $(FROM) $(TO) --out $(OUT)

notes: build
	@test -n "$(FROM)" || { echo "usage: make notes FROM=<build> [TO=local] [NOTES=notes.md]"; exit 1; }
	$(CLI) notes $(FROM) $(TO) --out $(OUT) $(if $(NOTES),--write '$(NOTES)')

# Reads $(OUT) (the Rust output) with the TypeScript deflection/engagement modules.
export: build-ts
	@test -f '$(OUT)/game-weapons.json' || { echo "no datasets in $(OUT): run make parse first, or set OUT"; exit 1; }
	DATA='$(OUT)' npm run export

# Commits, tags and pushes the build to the dumps repo (sc-gamedata publish),
# then runs the GitHub release it wrote to GH_RELEASE: gh release edit on a
# republish, then gh release create unless the release exists (PTU, EPTU and
# TECH-PREVIEW are prereleases; only LIVE is --latest). The binary never runs gh.
# The channel comes from the extract marker; PUBLISH_CHANNEL overrides it.
# PATCH=4.10.3 names the patch in the release title when the manifest branch
# gets it wrong.
GH_RELEASE := ./.tmp/gh-release-$(CHANNEL).sh
PUBLISH := SC_EXTRACT_DIR=$(EXTRACT_DIR) SC_DATA_DIR=$(OUT) $(CLI) publish \
	$(if $(PUBLISH_CHANNEL),--channel $(PUBLISH_CHANNEL)) $(if $(PATCH),--patch $(PATCH))

publish-dry: build
	$(PUBLISH) --dry-run

publish: build
	@rm -f $(GH_RELEASE)
	$(PUBLISH) --gh-script $(GH_RELEASE)
	sh $(GH_RELEASE)

# Replace an already-published build with the current data/ (same build only).
republish: build
	@rm -f $(GH_RELEASE)
	$(PUBLISH) --republish --gh-script $(GH_RELEASE)
	sh $(GH_RELEASE)

builds: build
	$(CLI) builds --out $(OUT)

# One channel end to end. Each step is its own make so CHANNEL picks the
# channel's EXTRACT_DIR/OUT defaults.
release-live release-ptu:
	$(MAKE) all CHANNEL=$(shell echo $(@:release-%=%) | tr a-z A-Z)
	$(MAKE) publish-dry CHANNEL=$(shell echo $(@:release-%=%) | tr a-z A-Z)
	$(MAKE) publish CHANNEL=$(shell echo $(@:release-%=%) | tr a-z A-Z)

release-check:
	@SC_ROOT="$(SC_ROOT)" scripts/release-on-exit.sh check

# AUTO=1 lets the hook publish; without it the hook stops at publish-dry.
hook-install:
	@SC_AUTO_PUBLISH=$(or $(AUTO),0) scripts/release-on-exit.sh install

hook-uninstall:
	@scripts/release-on-exit.sh uninstall

# Rebuild the StarBreaker CLI from the local checkout. The pipeline no longer
# needs it (the binary links the StarBreaker crates); it is for ad-hoc use.
starbreaker:
	cargo build --release --manifest-path $(STARBREAKER_SRC)/Cargo.toml -p starbreaker

clean:
	rm -rf dist
	cargo clean --release -p sc-gamedata

help:
	@sed -n '1,/^$$/p' $(firstword $(MAKEFILE_LIST)) | sed 's/^# \{0,1\}//'
