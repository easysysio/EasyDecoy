# Decoys

A decoy is a fake network service. It answers like the real thing for long
enough to be convincing, records what happens, and refuses. EasyDecoy's decoys
are **low-interaction**: they imitate prompts and replies, and nothing a client
sends is ever run or passed to the host.

Every decoy records a `connect` event the moment a client connects, before
anything else, so a port scan that connects and leaves still shows up. What
each decoy records after that is below; [Events](events.md) has the fields.

## Telnet

A Linux host's login prompt:

```text
Ubuntu 22.04.4 LTS
fileserver01 login: admin
Password:
Login incorrect
fileserver01 login:
```

- The banner comes first, then `<hostname> login:`.
- At the password prompt, EasyDecoy tells the client it will echo. The client
  then stops showing what is typed, as it would for a real password prompt.
- Every login fails after a **2-second** pause. A real check takes a moment,
  and an instant answer gives a decoy away.
- After **three** tries the connection is closed.

Each try is recorded as a `login_attempt` with the `username` and the
`password_length`, never the password itself.

Telnet clients mix option negotiation into what the user types. EasyDecoy
strips it as bytes arrive, so what is recorded is what was typed, even when a
negotiation is split across two packets. A line longer than 1024 bytes is cut
off rather than buffered.

## Web admin panel

A small appliance's web interface:

| Request | Answer | Recorded as |
|---|---|---|
| `GET /` or `GET /login` | The sign-in page | `request` |
| `POST /login` | The sign-in page with *Invalid username or password*, after a 1-second pause | `login_attempt` |
| Any other path | A plain 404 page | `request` |
| `HEAD` | The headers only | `request` |
| Something that isn't HTTP | Nothing; the connection is closed | `non_http` |

- Every request records its `method`, `path`, `host` and `user_agent`.
- A sign-in also records the `username` and the `password_length`. The form
  fields looked for are `username`, `user`, `login` or `email`, and
  `password`, `pass` or `pwd`, so a script written for another product's form
  is still caught.
- Something that isn't HTTP, such as an SSH client or a protocol probe, is
  recorded as `non_http` with a `sample`: the first 64 bytes it sent. A client
  that sends a few bytes and waits is recorded when it gives up, goes quiet
  for the idle timeout, or has sent 8 KiB without a complete request.

The decoy handles one request per connection and closes it (`Connection:
close`). The request head may be at most **8 KiB** and the body at most
**4 KiB**. Anything longer is cut off, not buffered.

## FTP

A file server that never lets anyone in, answering the way vsftpd does with
anonymous logins turned off:

```text
220 (vsFTPd 3.0.5)
USER admin
331 Please specify the password.
PASS ********
530 Login incorrect.
```

- The greeting is `220` followed by the **banner**.
- Every login fails after a **1-second** pause, `anonymous` included. After
  **three** failed logins the connection is closed, as vsftpd does.
- Each try is recorded as a `login_attempt` with the `username` and the
  `password_length`, never the password. An anonymous login's "password" is
  usually an email address, so that isn't kept either.
- Nobody gets past the login, so there is no data connection and no file to
  read. Anything that would need a login gets
  `530 Please login with USER and PASS.`

What a client sends on its way to a login (`FEAT`, `SYST`, `AUTH TLS`, `OPTS`,
`NOOP`, `HELP`, `QUIT` and the like) is answered and not recorded. Any other
command, such as `SITE EXEC`, `RETR` or `PORT` sent without a login, is
recorded as a `command` event with the `command` and its `argument`: an
ordinary client has no reason to send it. A connection is closed after 50
commands, and a line longer than 1024 bytes is cut off rather than buffered.

The packaged service starts it on port **21**. An upgrade from 0.1.0, which
had no FTP decoy, leaves it off; turn it on from the **Decoys** page.

## Making them believable

A decoy only works if an intruder believes it.

- **Name it like your real machines.** If your file servers are called
  `fs-ams-02`, a `fileserver01` stands out. On the GUI's **Decoys** page, set
  the hostname, banners, Server header and page title to fit in. A file
  server whose FTP greeting says ProFTPD where your others run vsftpd is a
  tell.
- **Give it an address nothing uses.** No legitimate client should have any
  reason to reach it, so every event means something.
- **Keep it out of anything that would be used normally.** Don't put the
  decoy in DNS names that people or scripts resolve every day, or in
  monitoring that polls it, or the events fill with your own traffic.
- **Use the real ports.** An intruder scanning for Telnet tries 23, not 2323.
  The package does this for you.

## Limits

These apply to every decoy together:

| Limit | Default | Setting |
|---|---|---|
| Open sessions | 256 | `max_connections` |
| Silence before a client is dropped | 30 s | `idle_timeout_secs` |
| Longest session | 120 s | `session_limit_secs` |

At the session limit, new connections are closed at once. A flood of
connections costs a socket for a moment, never memory or a stuck decoy.
