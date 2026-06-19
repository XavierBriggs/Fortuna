"""DEUCE command line: backtest / fetch-sports / clv."""
from __future__ import annotations

import argparse
import sys

from .config import BacktestConfig, EloConfig


def _cmd_backtest(args: argparse.Namespace) -> int:
    from .backtest import format_report, run_backtest

    # defer to EloConfig's tuned default (0.35) unless explicitly overridden
    elo = EloConfig() if args.surface_weight is None else EloConfig(surface_weight=args.surface_weight)
    cfg = BacktestConfig(
        tour=args.tour,
        since_year=args.since,
        burnin_years=args.burnin_years,
        devig_method=args.devig,
        calibrate=args.calibrate,
        calib_method=args.calib_method,
        elo=elo,
    )
    res, report = run_backtest(cfg)
    calib = f"calib={cfg.calib_method}" if cfg.calibrate else "calib=off"
    print(f"\nDEUCE Phase-A backtest — {cfg.tour.upper()}, scored {len(res):,} matches "
          f"({cfg.since_year}+, {cfg.burnin_years}y burn-in), devig={cfg.devig_method}, {calib}\n")
    print(format_report(report))
    print("\nRead: cal_ll < raw_ll => recalibration helped (check cal_ece vs raw_ece too). "
          "cal_vs_mkt < 0 with cal_ci fully below 0 => DEUCE beats the close. Expect ~0 on liquid ATP.")
    if args.out:
        res.to_csv(args.out, index=False)
        print(f"\nScored rows -> {args.out}")
    return 0


def _cmd_tune(args: argparse.Namespace) -> int:
    from .backtest import tune_params

    base = BacktestConfig(tour=args.tour, since_year=args.since, burnin_years=args.burnin_years)
    grid = tune_params(
        base,
        split_year=args.split,
        surface_weights=[0.2, 0.3, 0.4, 0.5],
        k_bases=[75.0, 100.0, 150.0, 250.0],
        margin_scales=[0.0, 0.6],
    )
    import pandas as pd

    with pd.option_context("display.max_rows", None, "display.width", 200):
        print(f"\nDEUCE param tune — {args.tour.upper()}, train {args.since}-{args.split-1}, "
              f"holdout {args.split}+ (selected by train_ll)\n")
        print(grid.to_string(index=False))
    best = grid.iloc[0]
    print(f"\nBest (by train_ll): surface_w={best.surface_w}, k_base={best.k_base}, margin={best.margin} "
          f"-> holdout_ll={best.hold_ll} vs market {best.hold_mkt} (gap {best.hold_ll - best.hold_mkt:+.4f})")
    return 0


def _cmd_fetch_sports(_args: argparse.Namespace) -> int:
    from .data.odds_api import OddsAPI

    api = OddsAPI()
    tennis = api.tennis_sports()
    print(f"Tennis sport keys ({len(tennis)}):")
    for s in tennis:
        flag = "active" if s.get("active") else "inactive"
        print(f"  {s['key']:<34} {s['title']:<26} [{flag}]")
    print(f"\nCredits remaining this month: {api.requests_remaining}")
    return 0


def _cmd_clv(args: argparse.Namespace) -> int:
    from .data.odds_api import OddsAPI
    from .clv import measure_clv

    api = OddsAPI()
    res = measure_clv(
        api, args.sport, args.entry, args.close, args.p1, args.p2, args.bet, args.devig
    )
    if res is None:
        print("Event/prices not found in one of the snapshots.", file=sys.stderr)
        return 1
    print(res)
    print(f"\nCLV (price): {res.clv_price:+.3%}   CLV (prob): {res.clv_prob:+.4f}   "
          f"credits left: {api.requests_remaining}")
    return 0


def _cmd_clv_scan(args: argparse.Namespace) -> int:
    from .clv import clv_bets_from_snapshots, summarize_clv
    from .config import BacktestConfig
    from .data.odds_api import OddsAPI
    from .ratings import RatingBook

    api = OddsAPI()
    entry = api.get_historical_odds(args.sport, args.entry)
    close = api.get_historical_odds(args.sport, args.close)
    print(f"Building RatingBook (asof {args.entry}) — walking tennis-data history...")
    book = RatingBook.build(BacktestConfig(tour=args.tour), asof=args.entry)
    bets = clv_bets_from_snapshots(
        entry.get("data", []), close.get("data", []), book.predict,
        surface=args.surface, best_of=args.best_of,
        threshold=args.threshold, devig_method=args.devig,
    )
    s = summarize_clv(bets)
    n_events = len(entry.get("data", []))
    print(f"\nDEUCE CLV scan — {args.sport}, surface={args.surface}, Bo{args.best_of}, "
          f"edge>={args.threshold:.1%}\n")
    print(f"events in entry snapshot: {n_events}  |  flagged bets: {s['n']}")
    if s["n"]:
        lo, hi = s["ci"]
        print(f"mean CLV (price): {s['mean_clv_price']:+.3%}   CI [{lo:+.3%}, {hi:+.3%}]")
        print(f"mean CLV (prob):  {s['mean_clv_prob']:+.4f}")
        print(f"positive-CLV bets: {s['pct_positive']:.0%}")
        print(f"beats close (CI>0): {s['beats_close']}  — needs >=50-100 bets/segment to trust")
    print(f"\ncredits remaining: {api.requests_remaining}")
    return 0


def _cmd_live_capture(args: argparse.Namespace) -> int:
    from datetime import datetime, timezone

    from .config import BacktestConfig
    from .data.odds_api import OddsAPI
    from .live import (
        append_capture,
        build_sharp_index,
        capture_three_way,
        fetch_kalshi_atp,
        format_capture,
    )
    from .ratings import RatingBook

    print("fetching live Kalshi ATP markets (public)...")
    kmatches = fetch_kalshi_atp()
    print(f"  {len(kmatches)} live Kalshi matches")
    if not kmatches:
        print("No open Kalshi ATP markets right now (between sessions?).")
        return 0

    api = OddsAPI()
    sharp_books = tuple(b.strip() for b in args.books.split(",") if b.strip())
    print(f"building sharp index from Odds API (regions={args.regions}, devig={args.devig})...")
    sharp = build_sharp_index(api, regions=args.regions, devig_method=args.devig)
    print(f"benchmark = unweighted mean of sharp books present: {', '.join(sharp_books)}")
    print("building RatingBook (walking tennis-data history)...")
    book = RatingBook.build(BacktestConfig(tour="atp"))

    asof = datetime.now(timezone.utc).isoformat(timespec="seconds")
    rows = capture_three_way(kmatches, sharp, book.predict, asof, sharp_books=sharp_books)
    print(f"\nDEUCE three-way capture @ {asof}  (sorted by |Kalshi - sharp|)\n")
    print(format_capture(rows))
    path = append_capture(rows)
    matched = sum(1 for r in rows if r["k_minus_sharp"] is not None)
    print(f"\n{matched}/{len(rows)} matched to a sharp line. Appended -> {path}")
    print(f"credits remaining: {api.requests_remaining}")
    print("Run repeatedly over a tournament to build the entry->close time series.")
    return 0


def _cmd_build_aliases(args: argparse.Namespace) -> int:
    from .data.sackmann import load_players
    from .identity import build_aliases_from_sackmann, write_generated
    from .names import _heuristic_key

    players = load_players(tour=args.tour)
    aliases = build_aliases_from_sackmann(players, _heuristic_key)
    path = write_generated(aliases)
    print(f"{len(players):,} {args.tour.upper()} players -> {len(aliases)} alias entries")
    print(f"wrote {path}")
    sample = sorted(aliases.items())[:8]
    for cid, e in sample:
        print(f"  {cid:24} <- {e['keys']}  ({e['display']})")
    return 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="deuce", description="Tennis win-probability research harness")
    sub = p.add_subparsers(dest="cmd", required=True)

    b = sub.add_parser("backtest", help="Phase-A walk-forward log-loss vs the close")
    b.add_argument("--tour", default="atp", choices=["atp", "wta"])
    b.add_argument("--since", type=int, default=2010, help="first season to score")
    b.add_argument("--burnin-years", type=int, default=3)
    b.add_argument("--devig", default="shin", choices=["shin", "proportional"])
    b.add_argument("--surface-weight", type=float, default=None,
                   help="override EloConfig tuned default (0.35)")
    b.add_argument("--no-calibrate", dest="calibrate", action="store_false",
                   help="disable prequential recalibration (show raw Elo only)")
    b.add_argument("--calib-method", default="platt",
                   choices=["platt", "temperature", "isotonic"])
    b.add_argument("--out", default=None, help="write scored rows to CSV")
    b.set_defaults(func=_cmd_backtest, calibrate=True)

    t = sub.add_parser("tune", help="grid-search Elo params (select on train, report on holdout)")
    t.add_argument("--tour", default="atp", choices=["atp", "wta"])
    t.add_argument("--since", type=int, default=2010, help="first season to score")
    t.add_argument("--split", type=int, default=2018, help="train < split <= holdout")
    t.add_argument("--burnin-years", type=int, default=3)
    t.set_defaults(func=_cmd_tune)

    s = sub.add_parser("fetch-sports", help="list tennis sport keys + credit balance (needs key)")
    s.set_defaults(func=_cmd_fetch_sports)

    c = sub.add_parser("clv", help="measure CLV for one match (entry vs close snapshot)")
    c.add_argument("--sport", required=True, help="Odds API sport key, e.g. tennis_atp_french_open")
    c.add_argument("--entry", required=True, help="entry snapshot ISO8601 UTC, e.g. 2023-06-01T00:00:00Z")
    c.add_argument("--close", required=True, help="closing snapshot ISO8601 UTC")
    c.add_argument("--p1", required=True)
    c.add_argument("--p2", required=True)
    c.add_argument("--bet", required=True, help="player you backed")
    c.add_argument("--devig", default="shin", choices=["shin", "proportional"])
    c.set_defaults(func=_cmd_clv)

    cs = sub.add_parser("clv-scan", help="flag DEUCE edges across a snapshot pair, measure CLV")
    cs.add_argument("--sport", required=True, help="Odds API sport key, e.g. tennis_atp_french_open")
    cs.add_argument("--entry", required=True, help="entry snapshot ISO8601 UTC")
    cs.add_argument("--close", required=True, help="closing snapshot ISO8601 UTC")
    cs.add_argument("--surface", default="hard", choices=["hard", "clay", "grass", "carpet"])
    cs.add_argument("--best-of", type=int, default=3, dest="best_of")
    cs.add_argument("--threshold", type=float, default=0.05, help="min |model-market| edge to bet")
    cs.add_argument("--devig", default="shin", choices=["shin", "proportional"])
    cs.add_argument("--tour", default="atp", choices=["atp", "wta"])
    cs.set_defaults(func=_cmd_clv_scan)

    lc = sub.add_parser("live-capture", help="log Kalshi vs sharp benchmark vs DEUCE for live ATP matches")
    lc.add_argument("--regions", default="eu,uk", help="Odds API regions to pull books from")
    lc.add_argument("--books", default="betfair_ex_uk,betfair_ex_eu,pinnacle,matchbook",
                    help="sharp books to average for the benchmark (comma-separated)")
    lc.add_argument("--devig", default="shin", choices=["shin", "proportional"])
    lc.set_defaults(func=_cmd_live_capture)

    ba = sub.add_parser("build-aliases", help="regenerate the player alias map from Sackmann names")
    ba.add_argument("--tour", default="atp", choices=["atp", "wta"])
    ba.set_defaults(func=_cmd_build_aliases)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
