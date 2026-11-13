#!/usr/bin/env bash
# Installs rustup from Ubuntu's own archive (no snap, no curl | sh).
# Run as root: sudo bash setup/install-rustup.sh
set -euo pipefail

apt-get update
apt-get install -y rustup
