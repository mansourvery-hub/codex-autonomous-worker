#!/usr/bin/env bash
set -euo pipefail

# Self-contained installer for Codex Autonomous Worker
# Usage: ./install.sh [--with-systemd]

TARGET_USER="${USER:-ubuntu}"
TARGET_HOME="${HOME:-/home/$TARGET_USER}"
INSTALL_DIR="$TARGET_HOME/codex-worker"
SPOOL_DIR="$TARGET_HOME/srv-codex"
BIN_DIR="$TARGET_HOME/.local/bin"

echo "=================================================="
echo "  Installing 24/7 Codex Autonomous Worker"
echo "  Target User: $TARGET_USER"
echo "  Target Home: $TARGET_HOME"
echo "=================================================="

# 1. Create directory structure
echo "[1/6] Creating runtime directories..."
mkdir -p "$INSTALL_DIR"
mkdir -p "$SPOOL_DIR"/{tasks,repos,worktrees,state,logs}
mkdir -p "$BIN_DIR"

# 2. Copy worker code and configs
echo "[2/6] Installing worker daemon and configuration..."
cp worker.py "$INSTALL_DIR/worker.py"
chmod +x "$INSTALL_DIR/worker.py"

if [ ! -f "$INSTALL_DIR/config.json" ]; then
  cp config/config.example.json "$INSTALL_DIR/config.json"
fi

cp prompts/task_prompt.md "$INSTALL_DIR/task_prompt.md"

# 3. Install CLI tool
echo "[3/6] Installing 'task' CLI into $BIN_DIR/task..."
cp bin/task "$BIN_DIR/task"
chmod +x "$BIN_DIR/task"

# 4. Prepare systemd unit files with current user paths
echo "[4/6] Generating systemd unit definitions..."
mkdir -p "$INSTALL_DIR/systemd"
sed -e "s|/home/ubuntu|$TARGET_HOME|g" \
    -e "s|User=ubuntu|User=$TARGET_USER|g" \
    -e "s|Group=ubuntu|Group=$TARGET_USER|g" \
    systemd/codex-worker.service > "$INSTALL_DIR/systemd/codex-worker.service"

sed -e "s|/home/ubuntu|$TARGET_HOME|g" \
    -e "s|User=ubuntu|User=$TARGET_USER|g" \
    -e "s|Group=ubuntu|Group=$TARGET_USER|g" \
    systemd/cliproxyapi.service > "$INSTALL_DIR/systemd/cliproxyapi.service"

echo "[5/6] Checking systemd privileges..."
if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
  echo "Installing systemd units to /etc/systemd/system/..."
  sudo cp "$INSTALL_DIR/systemd/codex-worker.service" /etc/systemd/system/
  sudo cp "$INSTALL_DIR/systemd/cliproxyapi.service" /etc/systemd/system/
  sudo systemctl daemon-reload
  sudo systemctl enable --now cliproxyapi.service
  sudo systemctl enable --now codex-worker.service
  echo "✔ Services enabled and running!"
else
  echo "Note: Run these commands with sudo to register systemd units:"
  echo "  sudo cp $INSTALL_DIR/systemd/*.service /etc/systemd/system/"
  echo "  sudo systemctl daemon-reload"
  echo "  sudo systemctl enable --now cliproxyapi.service"
  echo "  sudo systemctl enable --now codex-worker.service"
fi

echo "[6/6] Installation complete!"
echo ""
echo "Verify status:"
echo "  task list"
echo "  task current"
echo "  task logs -f"

