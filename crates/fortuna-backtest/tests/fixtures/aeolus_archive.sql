-- Small, text-diffable fixture mirroring the FIRMED real Aeolus schema
-- (aeolus_kalshi.db). The S6 adapter (AeolusArchiveSource) is tested against
-- this in-memory database, NEVER the 17.8 GB live DB.
--
-- The fixture is deliberately constructed so the three temporal instants of a
-- forecast lifecycle are DISTINCT, so the post-resolution-leak trap can bite:
--   forecast_init_time  (ISSUANCE / knowledge instant)  2026-07-01T00:00:00Z
--   target_date         (EVENT day)                      2026-07-04
--   settled_at          (RESOLUTION instant)             2026-07-05T18:00:00Z
-- A belief's available_at MUST be the issuance instant — never target_date,
-- never settled_at.
--
-- Markets in the fixture:
--   MKT-YES     : YES-resolved   (result='yes', outcome 1.0)
--   MKT-NO      : NO-resolved    (result='no',  outcome 0.0)
--   MKT-VOID    : VOIDED         (result='void' — NOT IN ('yes','no'))
--   MKT-PENDING : PENDING        (engaged belief, NO market_resolutions row)
-- All four are engaged (a belief was logged), so all four appear in the
-- universe manifest. MKT-VOID carries voided=true; MKT-PENDING carries
-- resolved=false AND voided=false (it has no recorded resolution). The pending
-- market mirrors the REAL Aeolus shape that broke the live smoke: 67 engaged
-- markets with no resolution row. It must be EXEMPT from G-DEAD (it cannot be
-- scored — there is no outcome), while MKT-YES/MKT-NO/MKT-VOID must still be
-- covered. MKT-PENDING's forecast_init_time is BEFORE its target_date (a
-- forecast issued ahead of the event day, the same as production).

-- THE BELIEF SOURCE. Carries NO crps/pit/score columns -> clean by construction.
CREATE TABLE bracket_probability_log (
    station_id          TEXT NOT NULL,
    target_date         TEXT NOT NULL,
    forecast_init_time  TEXT NOT NULL,
    market_ticker       TEXT NOT NULL,
    side                TEXT NOT NULL CHECK (side IN ('yes','no')),
    bracket_lo          INTEGER,
    bracket_hi          INTEGER,
    predicted_prob      REAL NOT NULL,
    model_version       TEXT,
    created_at          TEXT,
    PRIMARY KEY (station_id, target_date, forecast_init_time, market_ticker, side)
);

INSERT INTO bracket_probability_log
    (station_id, target_date, forecast_init_time, market_ticker, side,
     bracket_lo, bracket_hi, predicted_prob, model_version, created_at)
VALUES
    ('KNYC', '2026-07-04', '2026-07-01T00:00:00Z', 'MKT-YES',  'yes',
     40, 44, 0.73, 'emos-v3', '2026-07-01T00:05:00Z'),
    ('KNYC', '2026-07-04', '2026-07-01T00:00:00Z', 'MKT-NO',   'yes',
     45, 49, 0.31, 'emos-v3', '2026-07-01T00:05:00Z'),
    ('KDFW', '2026-07-04', '2026-07-01T00:00:00Z', 'MKT-VOID', 'yes',
     50, 54, 0.12, 'emos-v3', '2026-07-01T00:05:00Z'),
    -- PENDING market: a bare YYYY-MM-DD target_date (2026-07-04), with the
    -- forecast issued BEFORE the event day (the same 2026-07-01 issuance as the
    -- other beliefs, so available_at < decided_at holds). It has NO
    -- market_resolutions row → resolved=false, voided=false → EXEMPT from G-DEAD.
    ('KNYC', '2026-07-04', '2026-07-01T00:00:00Z', 'MKT-PENDING', 'yes',
     55, 59, 0.42, 'emos-v3', '2026-07-01T00:05:00Z');

-- OUTCOME. Voided = result NOT IN ('yes','no'). YES->1.0, NO->0.0.
CREATE TABLE market_resolutions (
    market_ticker       TEXT PRIMARY KEY,
    event_ticker        TEXT,
    result              TEXT,
    scalar_value_cents  INTEGER,
    close_time          TEXT,
    settled_at          TEXT,
    queried_at          TEXT
);

INSERT INTO market_resolutions
    (market_ticker, event_ticker, result, scalar_value_cents,
     close_time, settled_at, queried_at)
VALUES
    ('MKT-YES',  'EVT-NYC', 'yes',  NULL,
     '2026-07-05T17:00:00Z', '2026-07-05T18:00:00Z', '2026-07-05T18:05:00Z'),
    ('MKT-NO',   'EVT-NYC', 'no',   NULL,
     '2026-07-05T17:00:00Z', '2026-07-05T18:00:00Z', '2026-07-05T18:05:00Z'),
    ('MKT-VOID', 'EVT-DFW', 'void', NULL,
     '2026-07-05T17:00:00Z', '2026-07-05T18:00:00Z', '2026-07-05T18:05:00Z');

-- SNAPSHOT batches: the snapshot instant = captured_at (snapshot_quotes has
-- no timestamp of its own; join on batch_id).
CREATE TABLE snapshot_batches (
    batch_id            INTEGER PRIMARY KEY,
    event_ticker        TEXT,
    station_id          TEXT,
    target_date         TEXT,
    captured_at         TEXT,
    capture_reason      TEXT,
    forecast_init_time  TEXT,
    created_at          TEXT
);

INSERT INTO snapshot_batches
    (batch_id, event_ticker, station_id, target_date,
     captured_at, capture_reason, forecast_init_time, created_at)
VALUES
    (1, 'EVT-NYC', 'KNYC', '2026-07-04',
     '2026-07-02T12:00:00Z', 'scheduled', '2026-07-01T00:00:00Z', '2026-07-02T12:00:01Z');

CREATE TABLE snapshot_quotes (
    batch_id            INTEGER,
    market_ticker       TEXT,
    yes_bid_cents       INTEGER,
    yes_ask_cents       INTEGER,
    yes_mid_cents       REAL,
    no_mid_cents        REAL,
    last_price_cents    INTEGER,
    PRIMARY KEY (batch_id, market_ticker)
);

INSERT INTO snapshot_quotes
    (batch_id, market_ticker, yes_bid_cents, yes_ask_cents,
     yes_mid_cents, no_mid_cents, last_price_cents)
VALUES
    (1, 'MKT-YES', 68, 72, 70.0, 30.0, 71);

-- PAPER TRADES. shadow_intents: no real order ever placed -> orders = 0.
CREATE TABLE shadow_intents (
    id                     INTEGER PRIMARY KEY,
    station_id             TEXT,
    target_date            TEXT,
    init_time              TEXT,
    event_ticker           TEXT,
    entry_batch_id         INTEGER,
    market_ticker          TEXT,
    side                   TEXT,
    model_probability      REAL,
    market_probability     REAL,
    edge                   REAL,
    contracts              INTEGER,
    reference_price_cents  INTEGER,
    maker_fee_cents        INTEGER,
    notional_cents         INTEGER,
    created_at             TEXT
);

INSERT INTO shadow_intents
    (id, station_id, target_date, init_time, event_ticker, entry_batch_id,
     market_ticker, side, model_probability, market_probability, edge,
     contracts, reference_price_cents, maker_fee_cents, notional_cents, created_at)
VALUES
    (1, 'KNYC', '2026-07-04', '2026-07-01T00:00:00Z', 'EVT-NYC', 1,
     'MKT-YES', 'yes', 0.73, 0.70, 0.03,
     5, 70, 4, 350, '2026-07-02T12:30:00Z');
