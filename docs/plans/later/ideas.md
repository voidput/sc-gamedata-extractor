---
title: Ideas
tags: [docs, plans, later]
updated: 2026-11-25
---

# Ideas

- Unattended launcher on zima: Wine + Xvfb + noVNC service pinned to zima4, a
  `build_manifest.id` watcher, and a `runs-on: zima` release job, so PTU updates without
  playing. Blockers: login that persists through 2FA, scripted update clicks, ToS on
  unattended use, 310 GB on `local-path`. ([spec](../specs/tool-data-sync.md), "Build trigger")
