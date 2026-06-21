"""HEATER command line: backtest / sports / clv."""
from __future__ import annotations

import argparse
import sys

from .config import BacktestConfig, KModelConfig


def _cmd_backtest(args: argparse.Namespace) -> int:
    from .backtest import format_report, run_backtest

    cfg = BacktestConfig(season_from=args.season_from, model=KModelConfig())
    if args.real:
        from .data.statcast import load_starts

        df = load_starts()
        src = f"Statcast ({len(df):,} starts)"
    else:
        from .synth import make_starts

        df = make_starts(n=args.n, seed=args.seed)
        src = f"SYNTHETIC demo ({len(df):,} starts, seed={args.seed})"

    res, report = run_backtest(cfg, df)
    print(f"\nHEATER Phase-A backtest — {src}, scored {len(res):,} starts ({cfg.season_from}+)\n")
    print(format_report(report))
    print(
        "\nRead: ll_delta < 0 with ll_ci fully below 0 => the matchup+leash model beats the "
        "naive trailing-K baseline (beats_base=True). heater_rps < base_rps => sharper full "
        "distribution. heater_ece near 0 => calibrated. This is a no-money sanity gate; real "
        "edge needs CLV vs the closing line (Phase B, `heater clv`)."
    )
    if args.out:
        res.drop(columns=["_h_pmf", "_b_pmf"]).to_csv(args.out, index=False)
        print(f"\nScored rows -> {args.out}")
    return 0


def _cmd_ablate(args: argparse.Namespace) -> int:
    from .backtest import ablate
    from .synth import make_starts

    cfg = BacktestConfig(season_from=args.season_from, model=KModelConfig())
    if args.real:
        from .data.statcast import load_starts

        df = load_starts()
        src = f"Statcast ({len(df):,} starts)"
    else:
        df = make_starts(n=args.n, seed=args.seed)
        src = f"SYNTHETIC demo ({len(df):,} starts)"
    table = ablate(cfg, df)
    import pandas as pd

    with pd.option_context("display.max_rows", None, "display.width", 200):
        print(f"\nHEATER ablation — {src}, scored from {cfg.season_from}\n")
        print(table.to_string(index=False))
    print(
        "\nRead: each row adds one component. ll/rps lower = better; *_vs_base = gain over the "
        "naive baseline. Compare leash_only vs opponent_only to see whether the win is the "
        "(interesting) matchup signal or the (cheaper) variance-widening. csw rows need real data."
    )
    return 0


def _cmd_sports(_args: argparse.Namespace) -> int:
    from .data.odds_api import OddsAPI

    api = OddsAPI()
    mlb = api.mlb_active()
    if mlb:
        flag = "active" if mlb.get("active") else "inactive"
        print(f"MLB sport key: {mlb['key']} — {mlb['title']} [{flag}]")
    else:
        print("baseball_mlb not found in sport list.")
    print(f"Credits remaining this month: {api.requests_remaining}")
    return 0


def _cmd_kalshi_scan(_args: argparse.Namespace) -> int:
    from .data.kalshi import KalshiReadClient, looks_like_mlb, looks_like_strikeout

    api = KalshiReadClient()
    print(f"Kalshi read-only client — auth: {'SIGNED (creds present)' if api.signed else 'unsigned (public)'}")
    try:
        st = api.exchange_status()
        print(f"exchange_status: {st}")
    except Exception as e:  # noqa: BLE001
        print(f"exchange_status failed: {e}")

    # find MLB series, then their open markets
    series = api.series_list(category="Sports")
    mlb_series = [s for s in series if looks_like_mlb(s)]
    print(f"\nSports series: {len(series)} | MLB-looking series: {len(mlb_series)}")
    for s in mlb_series[:12]:
        print(f"  {s.get('ticker',''):<22} {str(s.get('title',''))[:60]}")

    markets: list = []
    for s in mlb_series:
        try:
            markets += api.markets(series_ticker=s.get("ticker"), status="open")
        except Exception as e:  # noqa: BLE001
            print(f"  markets({s.get('ticker')}) failed: {e}")
    if not mlb_series:
        # fallback: scan open events for anything MLB
        evs = [e for e in api.events(status="open") if looks_like_mlb(e)]
        print(f"fallback open-events MLB scan: {len(evs)} events")
        for e in evs[:20]:
            print(f"  EVENT {e.get('event_ticker',''):<24} {str(e.get('title',''))[:55]}")

    strikeout = [m for m in markets if looks_like_strikeout(m)]
    print(f"\nOpen MLB markets found: {len(markets)} | strikeout markets: {len(strikeout)}")
    sample = strikeout[:8] if strikeout else markets[:8]
    label = "STRIKEOUT markets" if strikeout else "sample MLB markets (no strikeout markets found)"
    print(f"\n{label}:")
    for m in sample:
        print(f"  {m.get('ticker',''):<30} yes_bid={m.get('yes_bid')} yes_ask={m.get('yes_ask')} "
              f"| {str(m.get('title') or m.get('yes_sub_title',''))[:55]}")
    print("\n(read-only: only GET market-data endpoints were called; no orders/portfolio touched)")
    return 0


def _cmd_kalshi_slate(args: argparse.Namespace) -> int:
    """Run the live Marcel comparison across EVERY open pitcher and summarize the
    edge distribution — to see whether the small over-tilt is systematic or not."""
    from .config import KModelConfig
    from .data.kalshi import (
        KalshiReadClient,
        market_p_yes,
        pitcher_options,
        strikeout_ladder,
        strikeout_markets,
    )
    from .data.live import build_current_priors, parse_event_teams, resolve_priors
    from .kdist import bf_pmf, expected_k, k_pmf, p_over
    from .log5 import matchup_k

    api = KalshiReadClient()
    mkts = strikeout_markets(api)
    pitchers = pitcher_options(mkts)
    priors = build_current_priors(args.season_year, refresh=args.refresh)
    cfg = KModelConfig()

    rows, skipped = [], 0
    for pkey, name in pitchers.items():
        _, opp = parse_event_teams(pkey)
        rp = resolve_priors(priors, name, opp)
        ladder = strikeout_ladder(mkts, pkey)
        if rp is None or not ladder:
            skipped += 1
            continue
        q = matchup_k(rp["pit_k"], rp["opp_k"], cfg.league_k)
        bfs, w = bf_pmf(rp["proj_bf_mean"], rp["proj_bf_sd"], cfg.bf_floor, cfg.bf_cap)
        pmf = k_pmf(q, bfs, w)
        edges, med_edge, med_d = [], None, 1.0
        for r in ladder:
            mp = market_p_yes(r)
            if mp is None:
                continue
            e = p_over(r["line"], pmf) - mp
            edges.append(e)
            if abs(mp - 0.5) < med_d:  # edge at the rung nearest a 50/50 market
                med_d, med_edge = abs(mp - 0.5), e
        if not edges:
            skipped += 1
            continue
        rows.append({
            "pitcher": name[:18], "yr": rp["n_years"], "GS": rp["n_starts"],
            "pit_k": round(rp["pit_k"], 3), "modelEK": round(expected_k(pmf), 2),
            "meanEdge": round(sum(edges) / len(edges), 3),
            "medEdge": round(med_edge, 3) if med_edge is not None else None,
            "rungs": len(edges),
        })
    if not rows:
        print("No resolvable pitchers (try --refresh).", file=sys.stderr)
        return 1
    import pandas as pd

    df = pd.DataFrame(rows).sort_values("meanEdge", ascending=False).reset_index(drop=True)
    over = int((df["meanEdge"] > 0).sum())
    with pd.option_context("display.max_rows", None, "display.width", 200):
        print(f"\nHEATER slate scan ({args.season_year} Marcel priors) — "
              f"{len(df)} resolved, {skipped} skipped\n")
        print(df.to_string(index=False))
    print(f"\nSummary: {over}/{len(df)} pitchers model-OVER (meanEdge>0); "
          f"slate mean meanEdge = {df['meanEdge'].mean():+.3f}, median = {df['meanEdge'].median():+.3f}")
    print("If nearly all are the same sign => systematic model bias (fixable: leash skew / "
          "baseline). A wide spread => idiosyncratic; the tails are the real edge candidates.")
    return 0


def _cmd_kalshi_compare(args: argparse.Namespace) -> int:
    from .data.kalshi import (
        KalshiReadClient,
        market_p_yes,
        pitcher_options,
        strikeout_ladder,
        strikeout_markets,
    )
    from .kdist import bf_pmf, expected_k, k_pmf, p_over
    from .log5 import matchup_k

    api = KalshiReadClient()
    mkts = strikeout_markets(api)
    pitchers = pitcher_options(mkts)
    if not pitchers:
        print("No open KXMLBKS pitcher-strikeout markets right now.", file=sys.stderr)
        return 1
    if not args.event:
        print(f"Open pitcher ladders ({len(pitchers)}). Pass one with --event <key>:")
        for pk, who in list(pitchers.items())[:30]:
            print(f"  {pk:<42} {who}")
        return 0

    ladder = strikeout_ladder(mkts, args.event)
    if not ladder:
        print(f"No ladder for event {args.event}.", file=sys.stderr)
        return 1

    pit_k, opp_k, bf_mean, bf_sd = args.pit_k, args.opp_k, args.proj_bf_mean, args.proj_bf_sd
    src = "manual priors"
    if args.live:
        from .data.live import build_current_priors, parse_event_teams, resolve_priors

        priors = build_current_priors(args.season_year, refresh=args.refresh)
        _, opp = parse_event_teams(args.event)
        rp = resolve_priors(priors, pitchers.get(args.event, ""), opp)
        if rp is None:
            print(f"Pitcher not found in {args.season_year} data (try --refresh, or pass manual priors).",
                  file=sys.stderr)
            return 1
        pit_k, opp_k, bf_mean, bf_sd = rp["pit_k"], rp["opp_k"], rp["proj_bf_mean"], rp["proj_bf_sd"]
        src = (f"LIVE {args.season_year} Marcel({rp['n_years']}yr): {rp['n_starts']} {args.season_year} starts, "
               f"{rp['throws']}HP, opp={rp['opp_src']}")

    cfg = KModelConfig()  # park off, trailing prior (the validated config)
    q = matchup_k(pit_k, opp_k, cfg.league_k)
    bfs, w = bf_pmf(bf_mean, bf_sd, cfg.bf_floor, cfg.bf_cap)
    pmf = k_pmf(q, bfs, w)
    print(
        f"\nHEATER vs Kalshi — {pitchers.get(args.event, args.event)}  ({args.event})\n"
        f"inputs [{src}]: pit_k={pit_k:.3f} opp_k={opp_k:.3f} "
        f"proj_bf={bf_mean:.0f}±{bf_sd:.1f}  =>  E[K]={expected_k(pmf):.2f}\n"
    )
    print(f"{'line':>5} {'P(K>=N)':>8} {'model':>7} {'mkt':>7} {'edge':>7}  {'yes_bid/ask':>12}  size")
    log_rows = []
    for r in ladder:
        mp = market_p_yes(r)
        model = p_over(r["line"], pmf)
        edge = (model - mp) if mp is not None else float("nan")
        mkt_s = f"{mp:.3f}" if mp is not None else "  -  "
        edge_s = f"{edge:+.3f}" if mp is not None else "  -  "
        ba = f"{r['yes_bid']}/{r['yes_ask']}"
        print(f"{r['line']:>5.1f} {r['n']:>6}+  {model:>7.3f} {mkt_s:>7} {edge_s:>7}  {ba:>12}  {r['yes_bid_size']}")
        log_rows.append({"pitcher_key": args.event, "pitcher": pitchers.get(args.event, ""),
                         "line": r["line"], "n": r["n"], "model_p": round(model, 4),
                         "market_p": round(mp, 4) if mp is not None else None,
                         "edge": round(edge, 4) if mp is not None else None,
                         "yes_bid": r["yes_bid"], "yes_ask": r["yes_ask"]})
    if args.log:
        _append_clv_log(log_rows)
        print(f"\nlogged {len(log_rows)} rungs -> {_clv_log_path()} (re-run near first pitch to capture the close)")
    print(
        "\nedge = model P(K>=N) - Kalshi devigged P(K>=N). Positive => model favours the OVER (YES).\n"
        "Uniform-sign edge across the whole ladder = the model's MEAN disagrees with the market\n"
        "(re-check the prior), not alpha. Real edge = shape disagreement, proven by CLV vs the close.\n"
        "(read-only: only GET market-data endpoints were called.)"
    )
    return 0


def _clv_log_path():
    from .config import data_dir

    return data_dir() / "clv_log.csv"


def _append_clv_log(rows: list[dict]) -> None:
    import datetime as _dt

    import pandas as pd

    path = _clv_log_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    stamp = _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds")
    df = pd.DataFrame([{"ts_utc": stamp, **r} for r in rows])
    df.to_csv(path, mode="a", header=not path.exists(), index=False)


def _cmd_clv(args: argparse.Namespace) -> int:
    from .clv import measure_k_clv
    from .data.odds_api import OddsAPI

    api = OddsAPI()
    res = measure_k_clv(
        api, args.event_id, args.pitcher, args.entry, args.close, args.side, args.devig
    )
    if res is None:
        print("Pitcher prop not found in one of the snapshots.", file=sys.stderr)
        return 1
    print(res)
    print(
        f"\nCLV (price): {res.clv_price:+.3%}   CLV (prob): {res.clv_prob:+.4f}   "
        f"credits left: {api.requests_remaining}"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="heater", description="Pitcher-strikeout prop research harness")
    sub = p.add_subparsers(dest="cmd", required=True)

    b = sub.add_parser("backtest", help="Phase-A: matchup+leash model vs naive trailing-K baseline")
    b.add_argument("--real", action="store_true", help="use Statcast starts.csv instead of the synthetic demo")
    b.add_argument("--season-from", type=int, default=2021, help="first season to score")
    b.add_argument("--n", type=int, default=4000, help="synthetic starts to generate (demo only)")
    b.add_argument("--seed", type=int, default=7, help="synthetic seed (demo only)")
    b.add_argument("--out", default=None, help="write scored rows to CSV")
    b.set_defaults(func=_cmd_backtest)

    a = sub.add_parser("ablate", help="decompose which model components carry the win")
    a.add_argument("--real", action="store_true", help="use Statcast starts.csv instead of synthetic")
    a.add_argument("--season-from", type=int, default=2021)
    a.add_argument("--n", type=int, default=4000, help="synthetic starts (demo only)")
    a.add_argument("--seed", type=int, default=7)
    a.set_defaults(func=_cmd_ablate)

    s = sub.add_parser("sports", help="confirm the MLB sport key + credit balance (needs key)")
    s.set_defaults(func=_cmd_sports)

    k = sub.add_parser("kalshi-scan", help="read-only: discover Kalshi MLB / strikeout markets")
    k.set_defaults(func=_cmd_kalshi_scan)

    ks = sub.add_parser("kalshi-slate", help="read-only: live Marcel comparison across ALL open pitchers")
    ks.add_argument("--season-year", type=int, default=2026)
    ks.add_argument("--refresh", action="store_true")
    ks.set_defaults(func=_cmd_kalshi_slate)

    kc = sub.add_parser("kalshi-compare", help="read-only: HEATER P(K>=N) vs a live Kalshi ladder")
    kc.add_argument("--event", default=None, help="KXMLBKS pitcher key (omit to list open pitchers)")
    kc.add_argument("--live", action="store_true", help="auto-fill priors from current-season Statcast")
    kc.add_argument("--season-year", type=int, default=2026, help="season for --live priors")
    kc.add_argument("--refresh", action="store_true", help="re-pull/rebuild the live prior cache")
    kc.add_argument("--log", action="store_true", help="append the snapshot to data/heater/clv_log.csv")
    kc.add_argument("--pit-k", type=float, default=0.24, help="manual pitcher per-PA K% prior")
    kc.add_argument("--opp-k", type=float, default=0.225, help="manual opponent team per-PA K% prior")
    kc.add_argument("--proj-bf-mean", type=float, default=24.0)
    kc.add_argument("--proj-bf-sd", type=float, default=4.0)
    kc.set_defaults(func=_cmd_kalshi_compare)

    c = sub.add_parser("clv", help="measure CLV for one pitcher prop (entry vs close snapshot)")
    c.add_argument("--event-id", required=True, help="Odds API MLB event id")
    c.add_argument("--pitcher", required=True, help="pitcher name as quoted by the book")
    c.add_argument("--entry", required=True, help="entry snapshot ISO8601 UTC, e.g. 2026-06-01T20:00:00Z")
    c.add_argument("--close", required=True, help="closing snapshot ISO8601 UTC")
    c.add_argument("--side", required=True, choices=["over", "under"])
    c.add_argument("--devig", default="shin", choices=["shin", "proportional"])
    c.set_defaults(func=_cmd_clv)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
