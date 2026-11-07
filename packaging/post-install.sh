#!/bin/sh
# =========================================================
# post-install — EasyDecoy
# Installed as /usr/lib/easydecoy/post-install and run by
# the package after a first install and after every upgrade
# (the Debian postinst, and the RPM's post-install script).
#
#   1. the account the service runs as, `easydecoy`: the
#      group and the user, each created only when it is not
#      already there — an upgrade finds them, and an
#      administrator may have made either one beforehand
#   2. the default config, copied to /etc/easydecoy only when
#      there is none, so an upgrade never overwrites the
#      administrator's settings
#   3. systemd told the unit file may have changed, and a
#      running service restarted onto the new binary
#
# A first install starts nothing: the decoys go live when an
# administrator enables them (systemctl enable --now
# easydecoy), after looking at the config.
#
# Safe to run by hand, as root.
# =========================================================
set -e

ACCOUNT=easydecoy
UNIT=easydecoy.service
CONFIG=/etc/easydecoy/easydecoy.toml
DEFAULT=/usr/share/easydecoy/easydecoy.toml

have() { command -v "$1" >/dev/null 2>&1; }

fail() {
    echo "easydecoy: $1" >&2
    exit 1
}

# ── 1. The account ───────────────────────────────────────

if ! getent group "$ACCOUNT" >/dev/null 2>&1; then
    have groupadd || fail "cannot create the group '$ACCOUNT': groupadd is not installed"
    groupadd --system "$ACCOUNT" || fail "could not create the group '$ACCOUNT'"
    echo "easydecoy: created the group '$ACCOUNT'"
fi

if ! getent passwd "$ACCOUNT" >/dev/null 2>&1; then
    have useradd || fail "cannot create the account '$ACCOUNT': useradd is not installed"
    # No login. The path differs between distributions.
    shell=/usr/sbin/nologin
    [ -x "$shell" ] || shell=/sbin/nologin
    [ -x "$shell" ] || shell=/bin/false
    # Nothing to keep at home: everything it writes goes to /var/log/easydecoy.
    useradd --system --gid "$ACCOUNT" --home-dir /nonexistent --no-create-home \
            --shell "$shell" --comment "EasyDecoy" "$ACCOUNT" \
        || fail "could not create the account '$ACCOUNT'"
    echo "easydecoy: created the account '$ACCOUNT'"
fi

# ── 2. The config ────────────────────────────────────────

if [ ! -e "$CONFIG" ] && [ -e "$DEFAULT" ]; then
    mkdir -p "$(dirname "$CONFIG")"
    cp "$DEFAULT" "$CONFIG"
    chmod 644 "$CONFIG"
    echo "easydecoy: installed the default config at $CONFIG"
fi

# ── 3. Bring a running service onto the new binary ───────

if have systemctl && [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    # try-restart touches only a service that is running: one an administrator
    # stopped stays stopped. Said, not failed on: the package is installed, and
    # failing here would leave the package manager half-way for a reason it
    # cannot fix.
    if ! systemctl try-restart "$UNIT" >/dev/null 2>&1; then
        echo "easydecoy: the service did not start again. See: journalctl -u $UNIT" >&2
    fi
fi

exit 0
