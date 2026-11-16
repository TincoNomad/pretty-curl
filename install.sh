#!/usr/bin/env bash
set -e

INSTALL_DIR="${HOME}/.local/bin"
BINARY="pcurl"
REPO="TincoNomad/pretty-curl"

print_banner() {
  echo -e "\033[90m┌─────────────────────────┐\033[0m"
  echo -e "\033[90m│\033[95m\033[1m █▀█ █▀▄ █▀▀ ▀█▀ ▀█▀ █▄█ \033[0m\033[90m│\033[0m"
  echo -e "\033[90m│\033[95m\033[1m █▀▀ █▀▄ █▀▀  █   █   █  \033[0m\033[90m│\033[0m"
  echo -e "\033[90m│\033[95m\033[1m ▀   ▀ ▀ ▀▀▀  ▀   ▀   ▀  \033[0m\033[90m│\033[0m"
  echo -e "\033[90m│\033[96m\033[1m █▀▀ █ █ █▀▄ █           \033[0m\033[90m│\033[0m"
  echo -e "\033[90m│\033[96m\033[1m █   █ █ █▀▄ █           \033[0m\033[90m│\033[0m"
  echo -e "\033[90m│\033[96m\033[1m ▀▀▀ ▀▀▀ ▀ ▀ ▀▀▀         \033[0m\033[90m│\033[0m"
  echo -e "\033[90m└──────────────── v1.6.0 ┘\033[0m"
  echo ""
}

print_banner
echo "  Make your HTTP requests beautiful 💅 "
echo "  ─────────────────────────────────────"
echo ""
echo "  ⚠️  Security Notice:"
echo "     This script downloads and installs pcurl from GitHub."
echo "     If you prefer manual installation, see:"
echo "     https://github.com/TincoNomad/pretty-curl#installation"
echo ""

# Pedir confirmación si hay terminal disponible (lee de /dev/tty para funcionar con pipe)
if [[ -t 0 ]] || [[ -e /dev/tty ]]; then
  read -p "  Continue with installation? (y/N) " -n 1 -r < /dev/tty
  echo ""
  if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "  Installation cancelled."
    exit 0
  fi
  echo ""
else
  echo "  ➡️  Running in non-interactive mode (piped)"
  echo ""
fi

# Detectar sistema operativo y arquitectura
detect_platform() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)
  
  case $OS in
    linux)
      case $ARCH in
        x86_64) PLATFORM="linux-x64" ;;
        aarch64) PLATFORM="linux-arm64" ;;
        arm64) PLATFORM="linux-arm64" ;;
        *) echo "  ❌ Architecture $ARCH not supported for Linux"; exit 1 ;;
      esac
      ;;
    darwin)
      case $ARCH in
        x86_64) PLATFORM="macos-x64" ;;
        arm64) PLATFORM="macos-arm64" ;;
        *) echo "  ❌ Architecture $ARCH not supported for macOS"; exit 1 ;;
      esac
      ;;
    *)
      echo "  ❌ Operating system $OS not supported"
      exit 1
      ;;
  esac
}

# SHA-256 of a file, with whichever tool the system has
sha256_of() {
  if command -v sha256sum &>/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum &>/dev/null; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  fi
}

# Compare the downloaded binary against the SHA256SUMS published in the release.
# Skipped with a warning when the release predates checksums.
verify_checksum() {
  local asset_name="$1"
  local file="$2"
  local sums_url="https://github.com/$REPO/releases/latest/download/SHA256SUMS"
  local expected
  expected=$(curl -fsSL "$sums_url" 2>/dev/null | awk -v a="$asset_name" '$2 == a || $2 == "*"a {print $1}' | head -1)

  if [ -z "$expected" ]; then
    echo "  ⚠️  No SHA256SUMS in this release — skipping checksum verification"
    return 0
  fi

  local actual
  actual=$(sha256_of "$file")
  if [ "$expected" != "$actual" ]; then
    echo "  ❌ Checksum mismatch for $asset_name"
    echo "     expected: $expected"
    echo "     actual:   $actual"
    rm -f "$file"
    exit 1
  fi
  echo "  ✅ Checksum verified"
}

# Descargar binario precompilado
download_binary() {
  local version=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
  local asset_name="pcurl-$PLATFORM"
  
  if [[ "$PLATFORM" == *"windows"* ]]; then
    asset_name="$asset_name.exe"
  fi
  
  echo "  ➡️  Downloading pcurl $version for $PLATFORM..."
  
  local download_url="https://github.com/$REPO/releases/latest/download/$asset_name"
  
  if ! curl -fsSL "$download_url" -o "$INSTALL_DIR/$BINARY"; then
    echo "  ❌ Error downloading pcurl"
    echo "    Check: https://github.com/$REPO/releases"
    exit 1
  fi

  verify_checksum "$asset_name" "$INSTALL_DIR/$BINARY"
  
  chmod +x "$INSTALL_DIR/$BINARY"
}

# Verificar si Rust está disponible (fallback)
try_rust_install() {
  if command -v cargo &>/dev/null; then
    echo "  ➡️ Rust detected, compiling from source..."
    cargo build --release --quiet
    cp "target/release/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"
  else
    echo "  ❌ Could not download nor compile pcurl"
    echo "    Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
  fi
}

# Main
detect_platform
mkdir -p "$INSTALL_DIR"

echo "  ➡️  Platform detected: $PLATFORM"

# Intentar descargar binario, fallback a compilación
if ! download_binary; then
  echo "  ⚠️  Download failed, trying compilation..."
  try_rust_install
fi

echo "  ✅ Installed in $INSTALL_DIR/$BINARY"
echo ""

# Detectar shell y archivo de configuración correcto
get_shell_info() {
  local rc_file
  local source_cmd

  if [[ -n "$ZSH_VERSION" ]] || [[ "$SHELL" == */zsh ]]; then
    rc_file="$HOME/.zshrc"
    source_cmd="source ~/.zshrc"
  elif [[ -n "$FISH_VERSION" ]] || [[ "$SHELL" == */fish ]]; then
    rc_file="$HOME/.config/fish/config.fish"
    source_cmd="source ~/.config/fish/config.fish"
  else
    # Bash u otros
    rc_file="$HOME/.bashrc"
    source_cmd="source ~/.bashrc"
  fi

  echo "$rc_file|$source_cmd"
}

# Agregar al PATH automáticamente si no está
add_to_path() {
  local shell_info
  shell_info=$(get_shell_info)
  local rc_file="${shell_info%|*}"
  local source_cmd="${shell_info#*|}"

  # Agregar solo si no existe ya en el rc file
  if ! grep -q '.local/bin' "$rc_file" 2>/dev/null; then
    # Crear directorio si no existe (para fish)
    mkdir -p "$(dirname "$rc_file")" 2>/dev/null || true

    echo '' >> "$rc_file"
    echo '# Added by pcurl installer' >> "$rc_file"
    if [[ -n "$FISH_VERSION" ]] || [[ "$SHELL" == */fish ]]; then
      echo 'set -gx PATH "$HOME/.local/bin" $PATH' >> "$rc_file"
    else
      echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$rc_file"
    fi
    echo "  ✅ Added ~/.local/bin to $rc_file"
  fi

  # Aplicar para la sesión actual
  export PATH="$HOME/.local/bin:$PATH"
}

# Verificar PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo "  ⚠️  $INSTALL_DIR is not in your PATH."
  if [[ -t 0 ]] || [[ -e /dev/tty ]]; then
    read -p "  Add it automatically? Press Y to add, any other key to skip: " -n 1 -r < /dev/tty
    echo ""
    if [[ $REPLY =~ ^[Yy]$ ]]; then
      add_to_path
      # Obtener comando source correcto
      local shell_info
      shell_info=$(get_shell_info)
      local source_cmd="${shell_info#*|}"
      echo ""
      echo "  ✅ Installed successfully!"
      echo ""
      echo "  To use pcurl in this terminal session, run:"
      echo "    $source_cmd"
      echo ""
      echo "  Or open a new terminal. Then try:"
    else
      echo "     Add this to your shell config manually:"
      echo ""
      echo "     export PATH=\"\$HOME/.local/bin:\$PATH\""
      echo ""
      echo "  ✅ Installed successfully! Quick examples:"
    fi
  else
    # Modo no-interactivo: agregar automáticamente
    add_to_path
    local shell_info
    shell_info=$(get_shell_info)
    local source_cmd="${shell_info#*|}"
    echo ""
    echo "  ✅ Installed successfully!"
    echo ""
    echo "  To use pcurl in this terminal session, run:"
    echo "    $source_cmd"
    echo ""
    echo "  Or open a new terminal. Then try:"
  fi
else
  echo ""
  echo "  ✅ Installed successfully! Quick examples:"
fi
echo ""
echo "    pcurl 'curl https://httpbin.org/get'     # HTTP mode"
echo "    pcurl wss://echo.websocket.org           # WebSocket mode"
echo "    pcurl --help                             # Full help"
echo ""
