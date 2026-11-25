# Regenerate game datasets from an installed Star Citizen build.
#
#   make                       # build, extract, parse into data/
#   make CHANNEL=PTU           # same, from the PTU install
#   make DOMAINS="mining"      # just one domain
#   make parse                 # re-parse without touching the archive
#   make live | ptu | hotfix   # shorthand for CHANNEL=LIVE / PTU / HOTFIX
#   make changes FROM=4.10     # buffs and nerfs vs a previous build
#   make notes FROM=4.10.197 TO=local  # player-facing patch notes (NOTES=file.md writes it)
#   make obtain ITEM="Verdict LMG"  # where to loot, craft, earn or buy an item
#   make obtain ITEM="Cambio SRT Canister" AT=grimhex  # only shops at a place
#   make obtain ITEM="Cambio SRT" AMMO=1 AT=grimhex  # the weapon's ammo instead
#   make where ITEM="FS-9 LMG"  # one table: buy, reward, craft, loot (AT, AMMO too)
#   make shop AT="grim hex"    # every item UEX lists at a terminal or place
#   make find [Q=verdict]      # fuzzy finder (fzf) over every item and ship
#   make publish-dry           # then: make publish (tag + GitHub release)
#   make republish             # add datasets to a build already published
#   make release-live | release-ptu  # extract + parse + publish-dry + publish one channel
#   make release-check         # installed LIVE/PTU builds vs what the dumps have
#   make hook-install [AUTO=1] # launcher post-exit hook: release unpublished builds
#   make hook-uninstall        # remove the hook (sc-postexit.sh + sc-prelaunch.sh)
#
# Optional overrides: SC_ROOT, CHANNEL, P4K, STARBREAKER, EXTRACT_DIR, OUT,
# DOMAINS, FROM, TO, NOTES, PUBLISH_CHANNEL, PATCH, ITEM, AT, AMMO. P4K defaults to $SC_DATA_P4K, else
# $(SC_ROOT)/$(CHANNEL)/Data.p4k. EXTRACT_DIR and OUT default per channel:
# ./extracted-data + ./data for LIVE, ./extracted-data-ptu + ./data-ptu for PTU,
# so a PTU run never overwrites the LIVE datasets.

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

CLI   := node dist/cli.js
FLAGS := --channel $(CHANNEL) --extract-dir $(EXTRACT_DIR) --out $(OUT) \
	$(if $(P4K),--p4k "$(P4K)") $(if $(STARBREAKER),--starbreaker "$(STARBREAKER)")

SRC := $(shell find src -name '*.ts' ! -name '*.test.ts') tsconfig.json

.PHONY: gamedata all live ptu hotfix extract parse build install test lint \
	check changes notes obtain where shop find export publish publish-dry republish builds clean help p4k starbreaker \
	release-live release-ptu release-check hook-install hook-uninstall

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

export: build
	npm run export

# Tags the build and creates its GitHub release (PTU = prerelease). The channel
# comes from the extract marker; PUBLISH_CHANNEL overrides it. PATCH=4.10.3
# names the patch in the release title when the manifest branch gets it wrong.
PUBLISH := SC_EXTRACT_DIR=$(EXTRACT_DIR) SC_DATA_DIR=$(OUT) node scripts/publish-dump.mjs \
	$(if $(PUBLISH_CHANNEL),--channel $(PUBLISH_CHANNEL)) $(if $(PATCH),--patch $(PATCH))

publish-dry: build
	$(PUBLISH) --dry-run

publish: build
	$(PUBLISH)

# Replace an already-published build with the current data/ (same build only).
republish: build
	$(PUBLISH) --republish

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

# Rebuild the StarBreaker CLI from the local checkout.
starbreaker:
	cargo build --release --manifest-path $(STARBREAKER_SRC)/Cargo.toml -p starbreaker

clean:
	rm -rf dist

help:
	@sed -n '1,/^$$/p' $(firstword $(MAKEFILE_LIST)) | sed 's/^# \{0,1\}//'
