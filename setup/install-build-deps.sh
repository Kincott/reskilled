#!/usr/bin/env bash
# System packages Horizon's game build needs on Ubuntu: a C compiler and linker, pkg-config, and the ALSA and libudev
# development files (Horizon docs/native-mods.md, "What the launcher does"). Run as root:
# sudo bash setup/install-build-deps.sh
set -euo pipefail

apt-get update
apt-get install -y build-essential pkg-config libasound2-dev libudev-dev
