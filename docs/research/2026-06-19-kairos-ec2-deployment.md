# KAIROS — optimal EC2 deployment (decision-grade)

**2026-06-19.** Where/how to run KAIROS (24/7 read-only perp funding/basis collector: `collect`
60s REST loop + `stream` Kalshi WS → one SQLite DB) so it runs for weeks/months with high uptime.
Grounded in web research (sources at end); the single load-bearing risk (datacenter-IP blocking)
is honestly flagged as **must-verify-on-the-box**.

## TL;DR recommendation
- **Instance:** `t4g.small` (ARM Graviton, 2 vCPU / 2 GiB), **On-Demand**, **us-east-1**, Amazon
  Linux 2023. 2 GiB (not the 1 GiB micro) because KAIROS imports pandas/numpy — OOM-safe for a
  months-long process. ARM is fine (pandas/numpy/cryptography ship aarch64 wheels; websocket-client
  is pure Python). **Cost ≈ $2.40/mo through 2026** (t4g.small free-trial covers the instance → you
  pay only EBS), **≈ $15/mo on-demand after**, **≈ $10/mo** with a 1-yr Compute Savings Plan.
- **Storage:** small gp3 root (20 GB) + a **separate 200 GB gp3 data volume** at `/data` (default
  3000 IOPS / 125 MB/s is ~100× our write rate — do NOT pay for extra). Separate volume so you can
  snapshot/detach the irreplaceable data independently.
- **Two systemd units** (`kairos-collect`, `kairos-stream`), `Restart=always`, non-root user,
  hardened. **Zero inbound** — access via SSM Session Manager, not SSH. Secrets in **SSM Parameter
  Store SecureString** (free; Kalshi RSA PEM + key id), not the repo. **IMDSv2 required.**
- **Dead-man's-switch:** a 5-min cron that alerts (Slack/Telegram) if `ws_book`/`poll_run` rows go
  stale or `/data` > 80% — plus an EC2 auto-recovery alarm. This is the load-bearing reliability piece.
- **⚠️ THE risk — verify first:** OKX/Bitget/Gate may block AWS IPs at the CDN. **Run the smoke test
  (below) on the instance before committing.** Kalshi + Hyperliquid are low-risk from US cloud, so the
  **A-grade core survives even if the offshore CEXs are blocked.**

## 1. The #1 risk: datacenter-IP blocking (decides the architecture)
From a US *residential* line I confirmed Kalshi/OKX/Bitget/Gate/Hyperliquid all return 200 (only
Binance 451 / Bybit 403 geo-block US — already dropped). But **cloud/datacenter IPs are a different
failure mode**: OKX, Bitget, Gate (Cloudflare-fronted CEXs) are documented to **403 entire
AWS/GCP/Azure IP ranges at the CDN edge, independent of country** (QuotaGuard 2026-04-15). A
US-residential 200 does **not** predict an AWS 200, and the per-IP outcome is **not knowable without
testing from the instance**.

Per-venue reachability from US AWS EC2:

| Venue | Host | Reachable from AWS? | Confidence | Why |
|---|---|---|---|---|
| **Kalshi** | external-api.kalshi.com | **Yes** | High | US-regulated; US region satisfies geo; RSA-signed, no IP allowlist; bots run on EC2 |
| **Hyperliquid** | api.hyperliquid.xyz | **Likely yes** | Med-High | Decentralized; IP rate-limits only; read-only widely used from cloud |
| **OKX** | www.okx.com | **Uncertain — moderate block risk** | Low-Med | Named in CDN cloud-block reports; public data needs no KYC |
| **Gate** | api.gateio.ws | **Uncertain — moderate block risk** | Low-Med | Named in CDN cloud-block reports |
| **Bitget** | api.bitget.com | **Uncertain** | Low | Same CEX/Cloudflare pattern; thinnest direct evidence |

**Consequence for design:** KAIROS already degrades gracefully (a dead venue → None, round survives),
so the **A-grade core (Kalshi nowcast + Kalshi-vs-Hyperliquid cross-venue) keeps working** even if
OKX/Bitget/Gate are AWS-blocked. They only add breadth. So: deploy on US EC2, smoke-test, and treat
the offshore CEXs as best-effort.

**Mitigation ladder (only if the smoke test shows OKX/Bitget/Gate blocked):**
1. **Re-roll the Elastic IP** (free, 5 min) — CDN blocks are per-IP-reputation; allocate a fresh EIP,
   re-test; a few rolls often lands a clean one.
2. **Small clean-ASN VPS** (Hetzner/Vultr/OVH, ~$5/mo) for the offshore-CEX leg only — non-AWS ASNs
   frequently pass where EC2 fails. Run Kalshi+HL on EC2, OKX/Bitget/Gate on the VPS, same DB schema.
3. **Residential/ISP egress proxy** (Bright Data / QuotaGuard Shield) for the offshore leg — most
   robust, adds cost/latency; fine for read-only polling. (Header/User-Agent spoofing alone does NOT
   work — CDNs fingerprint TLS JA3/JA4.)

**Region: us-east-1 (US).** Non-US regions buy nothing — they'd *break* Kalshi (US-only) without
reliably fixing the cloud-IP issue (an eu/ap AWS IP is still a flagged cloud IP), and there's no
Binance/Bybit left to justify it. us-east-1 also co-locates with the existing Aeolus box.

## 2. Architecture
```
EC2 t4g.small (AL2023, us-east-1, IMDSv2, no inbound; SSM Session Manager)
├─ /opt/kairos        venv + the kairos package (read-only deploy)
├─ /etc/kairos/kairos.env   0600, owned by kairos — secrets materialized from SSM at boot
├─ /data  (separate 200GB gp3, ext4, fstab nofail, EBS-encrypted)
│    └─ kairos.db (+ -wal,-shm)        ← both units write this (WAL, busy_timeout)
├─ systemd: kairos-collect.service  (kairos collect)   Restart=always, non-root, hardened
│           kairos-stream.service   (kairos stream)    Restart=always, non-root, hardened
├─ cron: deadman.sh */5min (row-age + disk% → Slack/Telegram);  wal_checkpoint(TRUNCATE) timer;
│        nightly ws_book prune + incremental_vacuum;  DLM daily EBS snapshot of /data
└─ IAM role: ssm:GetParameter /kairos/* + kms:Decrypt + AmazonSSMManagedInstanceCore  (no wildcards)
```

## 3. Storage / SQLite (write-heavy, single host, two writers)
- **gp3 default (3000 IOPS / 125 MB/s) is plenty** — our ~1–2 GB/day (~12–24 KB/s) is ~100× under
  baseline; WAL + `synchronous=NORMAL` fsyncs only at checkpoint, not per row. Don't buy extra IOPS.
- **One shared DB is safe** with WAL + `PRAGMA busy_timeout=5000` on both connections (SQLite supports
  multi-process same-host WAL via the `-shm` index; writes serialize, the high-volume `stream` writer
  must batch commits so the 60s `collect` writer always finds a gap). Split into two DBs **only** if
  you observe sustained `SQLITE_BUSY` after timeout.
- **PRAGMAs (both processes):** `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`,
  `auto_vacuum=INCREMENTAL` (must be set at DB creation), `wal_autocheckpoint=1000`. Run a periodic
  `PRAGMA wal_checkpoint(TRUNCATE)` (60–300 s) so the WAL doesn't grow unbounded under the live stream.
- **Retention (ws_book is the disk driver):** keep raw `ws_book` N=7 days; nightly batched
  `DELETE ... WHERE ts < now-7d` (LIMIT-loop so the lock isn't held long) + `incremental_vacuum`.
  Optionally roll up to a tiny `ws_book_1m` before deleting. Add `/data` >80% to the dead-man's-switch.
- **Backups:** AWS Data Lifecycle Manager → daily EBS snapshot of `/data` (tag-targeted, 14–30d
  retention; DLM is free, you pay only incremental snapshot storage). Crash-consistent snapshot of a
  WAL DB recovers cleanly; optionally `VACUUM INTO '/data/backup/kairos-YYYYMMDD.db'` nightly for a
  clean file too. **Practice the restore once** (snapshot → volume → attach → mount → start units).

## 4. Reliability / secrets / security / observability
- **systemd:** two units, `Restart=always`, `RestartSec=5`, `StartLimitIntervalSec`/`Burst` in
  `[Unit]` (or `=0` to never give up — the dead-man's-switch is the safety net either way). Non-root
  `kairos` user; sandbox `NoNewPrivileges`, `ProtectSystem=strict`, `ReadWritePaths=/data`,
  `ProtectHome`, `PrivateTmp`. `enable --now` + fstab `nofail` = reboot-safe self-healing. Consider
  `Type=notify` + `WATCHDOG=1` on the stream to catch half-open-socket hangs (or rely on the external switch).
- **Secrets:** SSM Parameter Store **SecureString** (`/kairos/kalshi/api_key_id`, `/kairos/kalshi/private_key_pem`)
  — free (Standard tier, 4 KB; an RSA PEM fits), KMS-encrypted, CloudTrail-audited, kept out of
  AMIs/snapshots. A boot oneshot writes them to the 0600 `EnvironmentFile`. NOT Secrets Manager (no
  rotation needed → $0.40/mo wasted). Never in the repo/logs.
- **Hardening:** IMDSv2 required (`HttpTokens=required`, hop limit 1); **zero inbound SG** + SSM
  Session Manager (no SSH, no keypair, IAM+CloudTrail); least-priv IAM role; EBS encryption on; AL2023
  `dnf upgrade --security` timer + periodic `--releasever=latest` (note: AL2023 `dnf-automatic` won't
  auto-pull advisories — or use Ubuntu + `unattended-upgrades` if you want hands-off patching).
- **Dead-man's-switch (the important one):** 5-min cron, independent of the app, querying newest
  `ws_book` (>120 s stale) and `poll_run` (>180 s stale) row age + `/data` disk% → Slack/Telegram.
  Catches every real failure (crashed, alive-but-stuck, disk full) that a "process running?" check
  misses. Plus an EC2 `StatusCheckFailed_System` auto-recovery alarm (free self-heal).

## 5. First-boot smoke test (run ON the instance, before committing the deploy)
Distinguishes `200`=ok, `451`=geo-block, `403/503/CAPTCHA-HTML`=datacenter-IP block (the one an
EIP-reroll or VPS fixes). Public endpoints only.
```bash
curl -sS -m10 https://checkip.amazonaws.com                                   # the IP you're testing
for n_u in \
 "KALSHI|https://api.elections.kalshi.com/trade-api/v2/exchange/status" \
 "OKX|https://www.okx.com/api/v5/public/time" \
 "BITGET|https://api.bitget.com/api/v2/public/time" \
 "GATE|https://api.gateio.ws/api/v4/spot/currencies/BTC"; do
  n=${n_u%%|*}; u=${n_u##*|}; printf '%-8s %s\n' "$n" "$(curl -sS -o /dev/null -w '%{http_code}' -m15 "$u")"
done
printf 'HYPERLQD %s\n' "$(curl -sS -o /dev/null -w '%{http_code}' -m15 -X POST \
  https://api.hyperliquid.xyz/info -H 'Content-Type: application/json' -d '{"type":"meta"}')"
# all 200 -> deploy as-is. OKX/BITGET/GATE 403/503 -> reroll EIP & retry; else VPS for those three.
```
Then validate the app reads the same data: `kairos markets`, `kairos dispersion --asset BTC --refresh`.

## 6. Cost (us-east-1)
| Item | Now → Dec-2026 | 2027 On-Demand | 2027 + 1-yr Savings Plan |
|---|---|---|---|
| t4g.small | $0 (free trial) | ~$12.3/mo | ~$8/mo |
| 200 GB gp3 (or 30 GB to start) | $16 (or $2.40) | $16 | $16 |
| data transfer (read-only, tiny) | ~$0 | ~$0 | ~$0 |
| **All-in** | **~$2.40–16/mo** | **~$28/mo** | **~$24/mo** |
(Start with 30 GB gp3 = $2.40/mo and grow online; 200 GB only if you keep full L2 book long.)

## 7. Open decisions for the operator
1. **Dedicated box vs reuse the Aeolus EC2 (t3.large, us-east-1).** Reuse = $0 + fast, but co-tenancy
   risk (KAIROS's 1–2 GB/day WS growth on a box whose Kalshi DB already hit ~17 GB) and shared blast
   radius. **Recommend dedicated t4g.small + separate data volume** for a no-backfill collector; reuse
   only for a quick start.
2. **Do you need full L2 book 24/7?** `ws_book` is the entire disk/retention burden (~1–2 GB/day) and
   FORTUNA can't trade tick signals. If not: stream **ticker only** (or 2–4 liquid symbols, or skip WS
   and rely on 60s REST) → disk drops to MB/day, a **t4g.micro + 20 GB** suffices, retention becomes
   trivial. **This is the biggest cost/complexity lever.**
3. **AL2023 vs Ubuntu** — Ubuntu `unattended-upgrades` is simpler for hands-off patching; AL2023 has
   SSM preinstalled + AWS integration. Both fine.
4. **`synchronous=NORMAL` vs `FULL`+batched** — how many ticks can you lose on a hard power event?
   NORMAL (lose ~1 s, never corrupt) is the right default for market data; snapshots are the backstop.

## Honest caveats
- **The OKX/Bitget/Gate AWS-reachability is the one unknown that can change the plan** — the *pattern*
  is well-sourced, the *current per-IP outcome* is not knowable without the smoke test. Bitget has the
  thinnest direct evidence. Verify on the box.
- Pricing is from AWS docs + Vantage's price mirror (AWS pricing page is JS-rendered); treat hourly
  rates as ±$0.001 and confirm in-console. t4g.small free-trial-through-2026 confirmed via AWS re:Post.

## Sources (accessed 2026-06-19)
QuotaGuard "Bybit/OKX/Gate cloud-CDN 403" (2026-04-15); Scrapfly 403 guide; CCXT manual (datacenter-IP
note); Kalshi API help + base URLs; Hyperliquid rate-limit/jurisdiction docs; AWS Graviton python
support; AWS burstable-credits + unlimited-mode docs; Vantage t4g/t3 price mirror; AWS EBS volume-types
+ pricing; AWS re:Post T4g free-trial extension; SQLite WAL docs; systemd.service(5) + Stapelberg
indefinite-restarts + Red Hat hardening; AWS DLM snapshot blog; Parameter Store vs Secrets Manager;
AWS Session Manager + IMDSv2 security blog; AL2023 security-updates docs.
```
