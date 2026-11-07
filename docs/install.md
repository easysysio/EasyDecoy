# Installation

EasyDecoy ships as a single binary and installs a **service** that runs the
decoys. Every [GitHub release](https://github.com/easysysio/EasyDecoy/releases)
has `.deb` and `.rpm` packages for **x86_64** and **arm64**.

Installing starts nothing. The decoys go live when you enable the service, so
there is time to make them look like the rest of your network first.

=== "Debian / Ubuntu"

    ```bash
    sudo apt install ./easydecoy_<version>-1_amd64.deb     # or _arm64.deb
    ```

=== "RHEL / Fedora"

    ```bash
    sudo dnf install ./easydecoy-<version>-1.x86_64.rpm    # or .aarch64.rpm
    ```

=== "openSUSE / SLES"

    ```bash
    sudo zypper install ./easydecoy-<version>-1.x86_64.rpm # or .aarch64.rpm
    ```

Then look over the config, and enable the service:

```bash
sudoedit /etc/easydecoy/easydecoy.toml
sudo systemctl enable --now easydecoy
sudo tail -f /var/log/easydecoy/events.jsonl
```

!!! warning "The packaged config uses the real ports"
    The Telnet decoy listens on **23** and the web decoy on **80**. If another
    service on the host already uses one of them, EasyDecoy stops at startup and
    `journalctl -u easydecoy` says which port. Move that decoy to another port,
    or disable it, in the [configuration](configuration.md).

## What the package installs

| Path | What it is |
|---|---|
| `/usr/bin/easydecoy` | The binary |
| `/etc/easydecoy/easydecoy.toml` | The config, written only if there is none |
| `/usr/lib/systemd/system/easydecoy.service` | The systemd unit |
| `/var/log/easydecoy/events.jsonl` | Events, created when the service first starts |
| `/etc/logrotate.d/easydecoy` | Weekly rotation of the events file, twelve weeks kept |

The package also creates a system account, `easydecoy`, with no login shell.
The service runs as that account, never as root. The only extra permission it
gets is binding ports below 1024. The [security model](security.md) explains
the rest of its sandbox.

## Upgrades and removal

- **An upgrade** keeps your config as it is, and restarts the service if it was
  running. A service you stopped stays stopped.
- **Removing** the package stops and disables the service, so no decoy is left
  listening. The config in `/etc/easydecoy` and the events in
  `/var/log/easydecoy` stay behind.

## From source

With a Rust toolchain, EasyDecoy runs straight from a checkout. It reads
`config/easydecoy.toml`, which uses unprivileged ports (Telnet on 2323, web on
8080) and writes events to stdout:

```bash
git clone https://github.com/easysysio/EasyDecoy
cd EasyDecoy
cargo run | jq .
```

From another terminal:

```bash
telnet localhost 2323
curl -d 'username=admin&password=x' http://localhost:8080/login
```

EasyDecoy's own log goes to stderr, so only events reach `jq`.

Next: [make the decoys look like yours](configuration.md).
