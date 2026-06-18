#!/usr/bin/env bash
# Refresh the operator-local demo ticker seeds from live Kalshi market listings.
#
# Discovery mints events/edges at runtime, but the mechanical and perp strategies
# still need a static book universe at boot. This script updates those static
# seeds from recorded venue listings before the daemon starts.
set -euo pipefail

usage() {
  cat <<'EOF'
Refresh config/fortuna.toml demo market seeds from live Kalshi listings.

Usage:
  scripts/refresh-demo-markets.sh [config-path]

Environment:
  KALSHI_API_BASE  Override the public Kalshi API base.
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_PATH="${1:-${FORTUNA_DEMO_CONFIG:-$ROOT/config/fortuna.toml}}"
API_BASE="${KALSHI_API_BASE:-https://external-api.kalshi.com/trade-api/v2}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd curl
need_cmd jq
need_cmd perl

if [ ! -f "$CONFIG_PATH" ]; then
  echo "missing config file: $CONFIG_PATH" >&2
  exit 1
fi

fetch_markets() {
  local series="$1"
  curl -fsS --get "$API_BASE/markets" \
    --data-urlencode "series_ticker=$series" \
    --data-urlencode "status=open" \
    --data-urlencode "limit=1000"
}

today_utc="$(date -u +%F)"
now_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
if end_utc="$(date -u -v+7d +%F 2>/dev/null)"; then
  :
else
  end_utc="$(date -u -d '+7 days' +%F)"
fi

weather_json="$(fetch_markets KXHIGHNY)"
weather_event_json="$(
  jq -c --arg now "$now_utc" '
    [.markets[]
      | select((.ticker | startswith("KXHIGHNY-"))
          and (.market_type == "binary")
          and (.status == "active")
          and ((.close_time // "") > $now))]
    | group_by(.event_ticker)
    | map({
        event_ticker: .[0].event_ticker,
        close_time: (map(.close_time) | min),
        markets: .
      })
    | map(select(.markets | length >= 3))
    | sort_by(.close_time)
    | .[0] // empty
  ' <<<"$weather_json"
)"

if [ -z "$weather_event_json" ]; then
  echo "could not find an open KXHIGHNY binary day-set in Kalshi listings" >&2
  exit 1
fi

weather_tickers=()
while IFS= read -r ticker; do
  weather_tickers+=("$ticker")
done < <(
  jq -r '
    .markets
    | sort_by([
        (if .strike_type == "less" then 0
         elif .strike_type == "between" then 1
         elif .strike_type == "greater" then 2
         else 3 end),
        (.floor_strike // -1000000000),
        (.cap_strike // -1000000000),
        .ticker
      ])
    | .[].ticker
  ' <<<"$weather_event_json"
)
weather_event="$(jq -r '.event_ticker' <<<"$weather_event_json")"

if [ "${#weather_tickers[@]}" -lt 3 ]; then
  echo "KXHIGHNY event $weather_event did not contain enough bracket markets" >&2
  exit 1
fi

replacement="bracket_sets = [["
for ticker in "${weather_tickers[@]}"; do
  replacement="${replacement}
    \"${ticker}\","
done
replacement="${replacement}
]]"

REPLACEMENT="$replacement" perl -0pi -e '
  my $r = $ENV{REPLACEMENT};
  s/bracket_sets = \[\[\n.*?\]\]/$r/s
    or die "could not replace [kalshi].bracket_sets\n";
' "$CONFIG_PATH"

ladder_tickers=()
while IFS= read -r ticker; do
  ladder_tickers+=("$ticker")
done < <(
  grep -Eo '\[perp_event_basis_v2\.ladder\."KXBTC-[^"]+"\]' "$CONFIG_PATH" \
    | sed -E 's/^\[perp_event_basis_v2\.ladder\."([^"]+)"\]$/\1/' \
    | sort -u
)

if [ "${#ladder_tickers[@]}" -gt 0 ]; then
  ladder_suffixes=()
  while IFS= read -r suffix; do
    ladder_suffixes+=("$suffix")
  done < <(
    printf '%s\n' "${ladder_tickers[@]}" \
      | awk -F- '{print $NF}' \
      | sort -u
  )
  suffixes_json="$(printf '%s\n' "${ladder_suffixes[@]}" | jq -R . | jq -s .)"
  btc_json="$(fetch_markets KXBTC)"
  btc_event="$(
    jq -r --arg now "$now_utc" --argjson suffixes "$suffixes_json" '
      [.markets[]
        | select((.ticker | startswith("KXBTC-"))
            and (.market_type == "binary")
            and (.status == "active")
            and ((.close_time // "") > $now))]
      | group_by(.event_ticker)
      | map({
          event_ticker: .[0].event_ticker,
          close_time: (map(.close_time) | min),
          tickers: [.[].ticker]
        })
      | map(select(. as $g
          | ($suffixes
             | all(. as $s | ($g.tickers | any(endswith("-" + $s)))))))
      | sort_by(.close_time)
      | .[0].event_ticker // empty
    ' <<<"$btc_json"
  )"

  if [ -z "$btc_event" ]; then
    echo "no open KXBTC event contains every configured ladder rung suffix: ${ladder_suffixes[*]}" >&2
    echo "leaving [perp_event_basis_v2] unchanged; update the ladder manually or choose a live rung set" >&2
    exit 1
  fi

  for old in "${ladder_tickers[@]}"; do
    suffix="${old##*-}"
    new="${btc_event}-${suffix}"
    OLD="$old" NEW="$new" perl -0pi -e '
      my $old = $ENV{OLD};
      my $new = $ENV{NEW};
      s/\Q$old\E/$new/g;
    ' "$CONFIG_PATH"
  done
else
  btc_event="none"
fi

RANGE="from=${today_utc}&to=${end_utc}" perl -0pi -e '
  my $range = $ENV{RANGE};
  s/from=\d{4}-\d{2}-\d{2}&to=\d{4}-\d{2}-\d{2}/$range/g;
' "$CONFIG_PATH"

echo "[refresh] KXHIGHNY bracket_set: $weather_event (${#weather_tickers[@]} markets)"
echo "[refresh] KXBTC ladder event: $btc_event"
echo "[refresh] Aeolus query range: $today_utc to $end_utc"
