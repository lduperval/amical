#!/bin/sh
# Install (or update) the Amical Integration GNOME Shell extension for the
# current user and enable it. Run this as your normal user, not with sudo.
#
# GNOME Shell on Wayland only loads newly installed extensions on login, so
# log out and back in after the first install. Updates of an already loaded
# extension also take effect at the next login.
set -eu

UUID="amical@amical.ai"
HERE="$(cd "$(dirname "$0")" && pwd)"
ZIP="$HERE/$UUID.shell-extension.zip"

if [ "$(id -u)" -eq 0 ]; then
  echo "Run this script as the user who will use Amical, not as root." >&2
  exit 1
fi
if ! command -v gnome-extensions >/dev/null 2>&1; then
  echo "The gnome-extensions tool is missing; install the gnome-shell package." >&2
  exit 1
fi
if [ ! -f "$ZIP" ]; then
  echo "Extension archive not found: $ZIP" >&2
  exit 1
fi

gnome-extensions install --force "$ZIP"
echo "Installed $UUID to ~/.local/share/gnome-shell/extensions/$UUID"

if gnome-extensions enable "$UUID" 2>/dev/null; then
  echo "Enabled $UUID."
else
  # The shell has not loaded the freshly installed extension yet. Record the
  # enable request so it is active right after the next login.
  if command -v gsettings >/dev/null 2>&1; then
    current="$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null || echo "@as []")"
    case "$current" in
      *"'$UUID'"*) ;;
      "@as []"|"[]") gsettings set org.gnome.shell enabled-extensions "['$UUID']" ;;
      *) gsettings set org.gnome.shell enabled-extensions "$(printf '%s' "$current" | sed "s/]$/, '$UUID']/")" ;;
    esac
  fi
  echo "Marked $UUID as enabled; it loads at your next login."
fi

cat <<MSG

Next steps:
  1. Log out of GNOME and log back in (required once on Wayland).
  2. Check it is running:  gnome-extensions info $UUID
  3. Check Amical can reach it:
     busctl --user call org.gnome.Shell /org/gnome/Shell/Extensions/Amical \\
       org.gnome.Shell.Extensions.Amical GetStatus
  4. Restart Amical.
MSG
