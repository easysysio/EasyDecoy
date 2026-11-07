# Syslog

EasyDecoy can send every event to a syslog collector such as
[EasyLog](https://easylog.easysys.io) as well, over UDP or TCP. Enable it in
the `[syslog]` section of the [configuration](configuration.md):

```toml
[syslog]
enabled  = true
server   = "10.0.0.5:514"   # host:port of the collector
protocol = "udp"            # or "tcp"
format   = "rfc5424"        # or "rfc3164"
facility = "local0"
hostname = ""               # empty uses this machine's name
```

Restart the service, and the startup log confirms where events are going:

```text
syslog: udp to 10.0.0.5:514 (rfc5424, local0)
```

## What is sent

Each event becomes one syslog message. Its text is the event's JSON line,
exactly as it is written to the [events file](events.md).

=== "RFC 5424 (default)"

    ```text
    <132>1 2026-11-07T11:20:40.409Z sensor1 easydecoy - login_attempt - {"time":"2026-11-07T11:20:40.409920Z","decoy":"telnet",…}
    ```

    A full UTC timestamp with milliseconds, and the event's `kind` as the
    message ID, so a collector can filter on it without parsing the JSON.

=== "RFC 3164"

    ```text
    <132>Nov  7 11:20:40 sensor1 easydecoy: {"time":"2026-11-07T11:20:40.409920Z","decoy":"telnet",…}
    ```

    The older BSD format, for collectors that read nothing else. Its timestamp
    has no time zone; EasyDecoy sends UTC. The JSON inside always carries the
    exact time.

| Event | Severity |
|---|---|
| `login_attempt` | warning (4) |
| everything else | notice (5) |

The HOSTNAME is the machine EasyDecoy runs on, not the name a decoy pretends
to have, so the collector knows which sensor saw the event. Set `hostname` to
use another name.

Over TCP, each message ends with a newline, which is the framing EasyLog and
rsyslog read.

## When the collector is down

Syslog is a copy, never the record. Each event is written to the events file
first and handed to the syslog sender without waiting, so a collector that is
slow, unreachable or gone never holds up a decoy or the local record.

What it costs is the messages sent while it was down. EasyDecoy reports that
in its own log (`journalctl -u easydecoy`): once when delivery fails, once when
it works again, and a count of undelivered events every five minutes.

```text
WARN syslog: cannot deliver to 10.0.0.5:514: Connection refused (os error 111) (events are still recorded locally)
INFO syslog: delivering to 10.0.0.5:514 again
```

Over TCP, a lost connection is made again on a later event, at most once
every 10 seconds.

!!! note "Plain syslog has no acknowledgements"
    Over UDP, a message to a collector that isn't listening is simply lost.
    Over TCP, a message written just before the collector went away can be lost
    without an error. Either way, the events file still has every event.

## Sending to EasyLog

EasyLog decides how to read a message from the address it came from. Add the
EasyDecoy host in EasyLog's **Sources** page as a **General** source, and its
events are stored and searchable as they arrive. Then point EasyDecoy at
EasyLog's syslog port:

```toml
[syslog]
enabled = true
server  = "easylog.example.internal:514"
```
