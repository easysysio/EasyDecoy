-- EasyDecoy's database, first version.
--
-- Applied by sqlx at startup (src/db.rs). A later change to the schema is a new
-- numbered file beside this one; this file is never edited once released.

-- Settings: one row per section (limits, telnet, http, syslog, recording), each
-- a JSON object. Read and written whole by src/settings.rs.
CREATE TABLE settings (
    section TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);

-- Values EasyDecoy keeps for itself, such as the GUI's TLS certificate.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Every event, as the exact JSON line written to the events file and to
-- syslog, with the fields the GUI filters and groups on beside it. `time` is
-- RFC 3339 UTC at a fixed width, so ordering the text orders the times.
CREATE TABLE events (
    id       INTEGER PRIMARY KEY,
    time     TEXT    NOT NULL,
    decoy    TEXT    NOT NULL,
    kind     TEXT    NOT NULL,
    src_ip   TEXT    NOT NULL,
    src_port INTEGER NOT NULL,
    dst_port INTEGER NOT NULL,
    line     TEXT    NOT NULL
);
CREATE INDEX events_time   ON events (time);
CREATE INDEX events_src_ip ON events (src_ip, time);
CREATE INDEX events_kind   ON events (kind, time);

-- Accounts that may sign in to the GUI. Passwords are bcrypt hashes.
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- Signed-in browsers. The cookie carries a random token and only its SHA-256
-- is kept here, so a copy of the database cannot be used to sign in.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf       TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

-- Who did what in the GUI: sign-ins, failed sign-ins, and every change.
CREATE TABLE audit (
    id        INTEGER PRIMARY KEY,
    time      TEXT NOT NULL,
    username  TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    action    TEXT NOT NULL,
    detail    TEXT NOT NULL
);
CREATE INDEX audit_time ON audit (time);
