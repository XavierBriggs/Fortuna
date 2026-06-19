#!/usr/bin/env python3
"""Build data/heater/starts.csv from real Statcast pitch-by-pitch data (pybaseball).

Pull -> aggregate to one row per starting pitcher per game -> compute AS-OF-DATE
trailing priors (strictly prior starts/games only, so nothing peeks at the start
it predicts) -> write the canonical leakage-safe schema the backtest reads.

What this version adds over the v0 scaffold:
  - trailing CSW% / SwStr% (swing-and-miss rates that stabilize fast) and a
    leakage-safe EXPANDING OLS map CSW% -> K%, giving an alternate `pit_k_csw`
    prior plus a `pit_k_blend` (memo finding: whiff rates out-predict raw K%).
  - real park strikeout factors fit on the EARLIEST (training) season and applied
    forward (so park is no longer inert at 1.0).
  - multi-season pulls; only seasons AFTER the training season are emitted, so the
    output csv is leakage-safe to score end-to-end. The 2023->2024 split also
    straddles the 2023 rule changes (pitch clock / shift ban) on purpose.

pybaseball is semi-dormant and its FanGraphs path is 403-blocked (memo #14) — we
use the Statcast path only. Install: pip install -e ".[data]".

Usage:
    python scripts/get_statcast.py --seasons 2023,2024        # train on 2023, score 2024
    python scripts/get_statcast.py --start 2024-04-01 --end 2024-08-01   # single window
    heater backtest --real --season-from 2024
"""
from __future__ import annotations

import argparse
import sys

import numpy as np
import pandas as pd

from heater.config import data_dir

_K_EVENTS = {"strikeout", "strikeout_double_play"}
_WHIFF = {"swinging_strike", "swinging_strike_blocked"}
_CALLED = {"called_strike"}
_COLS = [
    "game_date", "game_pk", "pitcher", "player_name", "p_throws", "events", "description",
    "inning", "inning_topbot", "home_team", "away_team", "at_bat_number",
]
_PRIOR_PA_PIT = 100.0          # shrink pitcher trailing K% toward league over this many PA
_PRIOR_PA_TEAM = 300.0         # shrink opponent team K% toward league
_PARK_REG_PA = 2000.0          # heavy regression for park factors (park effects are small)
_CSW_MIN_FIT = 200             # min prior (csw, K) pairs before trusting the OLS map
_CSW_BLEND = 0.5               # weight on the CSW-implied prior in pit_k_blend


def pull_window(start: str, end: str, retries: int = 4) -> pd.DataFrame:
    import pybaseball
    from pybaseball import statcast

    pybaseball.cache.enable()  # persist day-chunks locally so retries resume, not restart
    last = None
    for attempt in range(retries):
        try:
            df = statcast(start_dt=start, end_dt=end)
            keep = [c for c in _COLS if c in df.columns]
            return df[keep]
        except Exception as e:  # noqa: BLE001 — network flakiness; cached chunks make retry cheap
            last = e
            print(f"    pull retry {attempt + 1}/{retries} after: {type(e).__name__}")
    raise RuntimeError(f"Statcast pull failed after {retries} attempts: {last}")


def pull(seasons: list[int] | None, start: str | None, end: str | None) -> pd.DataFrame:
    frames = []
    if seasons:
        for yr in seasons:
            print(f"  pulling {yr} regular season ...")
            frames.append(pull_window(f"{yr}-03-28", f"{yr}-10-02"))
    else:
        frames.append(pull_window(start, end))
    df = pd.concat(frames, ignore_index=True)
    return df.dropna(subset=["game_pk", "pitcher", "at_bat_number", "inning", "inning_topbot"])


def _expanding_ols(x: np.ndarray, y: np.ndarray, min_fit: int) -> np.ndarray:
    """Leakage-safe pooled CSW->K map: at each row, fit OLS y~x on STRICTLY PRIOR
    rows (only where x is valid) and predict this row's y. NaN where x is missing
    or fewer than `min_fit` prior pairs exist (caller falls back to trailing K)."""
    valid = ~np.isnan(x)
    xc = np.where(valid, x, 0.0)
    yc = np.where(valid, y, 0.0)
    n_prior = np.cumsum(valid.astype(float)) - valid.astype(float)
    sx = np.cumsum(xc) - xc
    sy = np.cumsum(yc) - yc
    sxx = np.cumsum(xc * xc) - xc * xc
    sxy = np.cumsum(xc * yc) - xc * yc
    denom = n_prior * sxx - sx * sx
    ok = (n_prior >= min_fit) & (denom > 0) & valid
    slope = np.where(ok, (n_prior * sxy - sx * sy) / np.where(denom == 0, np.nan, denom), np.nan)
    intercept = np.where(ok, (sy - slope * sx) / np.where(n_prior == 0, np.nan, n_prior), np.nan)
    return slope * x + intercept


def build_starts(pitches: pd.DataFrame, min_prior_starts: int) -> tuple[pd.DataFrame, float]:
    p = pitches.copy()
    p["game_pk"] = p["game_pk"].astype(int)
    p["pitcher"] = p["pitcher"].astype(int)
    p["at_bat_number"] = p["at_bat_number"].astype(int)
    p["batting_team"] = p["away_team"].where(p["inning_topbot"].eq("Top"), p["home_team"])
    is_k = p["events"].isin(_K_EVENTS)
    is_whiff = p["description"].isin(_WHIFF)
    is_called = p["description"].isin(_CALLED)
    total_pa = int(p.groupby("game_pk")["at_bat_number"].nunique().sum())
    league_k = float(is_k.sum()) / float(total_pa)

    # --- per (game, pitcher) start aggregation ---
    agg = p.groupby(["game_pk", "pitcher"]).agg(
        game_date=("game_date", "first"),
        player_name=("player_name", "first"),
        throws=("p_throws", "first"),
        opp_team=("batting_team", "first"),
        home_team=("home_team", "first"),
        realized_bf=("at_bat_number", "nunique"),
        n_pitches=("description", "size"),
    ).reset_index()
    for col, mask in (("realized_k", is_k), ("n_whiff", is_whiff), ("n_called", is_called)):
        c = p.loc[mask].groupby(["game_pk", "pitcher"]).size().rename(col)
        agg = agg.merge(c, on=["game_pk", "pitcher"], how="left")
        agg[col] = agg[col].fillna(0).astype(int)

    starters = p.loc[p["inning"].eq(1), ["game_pk", "pitcher"]].drop_duplicates()
    agg = agg.merge(starters, on=["game_pk", "pitcher"], how="inner")
    agg["game_date"] = pd.to_datetime(agg["game_date"])
    agg["season"] = agg["game_date"].dt.year
    agg = agg.sort_values(["game_date", "game_pk"]).reset_index(drop=True)

    # --- pitcher trailing K%, CSW%, SwStr%, leash (strictly prior starts) ---
    g = agg.groupby("pitcher")
    cum_k = g["realized_k"].cumsum() - agg["realized_k"]
    cum_bf = g["realized_bf"].cumsum() - agg["realized_bf"]
    cum_p = g["n_pitches"].cumsum() - agg["n_pitches"]
    cum_w = g["n_whiff"].cumsum() - agg["n_whiff"]
    cum_c = g["n_called"].cumsum() - agg["n_called"]
    agg["prior_starts"] = g.cumcount()
    agg["pit_k_prior"] = (cum_k + league_k * _PRIOR_PA_PIT) / (cum_bf + _PRIOR_PA_PIT)
    agg["trailing_csw"] = np.where(cum_p > 0, (cum_w + cum_c) / cum_p.replace(0, np.nan), np.nan)
    agg["trailing_swstr"] = np.where(cum_p > 0, cum_w / cum_p.replace(0, np.nan), np.nan)
    agg["proj_bf_mean"] = g["realized_bf"].transform(lambda s: s.expanding().mean().shift(1)).fillna(22.0).clip(8, 34)
    agg["proj_bf_sd"] = g["realized_bf"].transform(lambda s: s.expanding().std().shift(1)).fillna(4.0).clip(1.5, 8.0)

    # --- CSW% -> K% expanding OLS (leakage-safe), blend ---
    k_rate = (agg["realized_k"] / agg["realized_bf"]).to_numpy()
    csw_implied = _expanding_ols(agg["trailing_csw"].to_numpy(), k_rate, _CSW_MIN_FIT)
    agg["pit_k_csw"] = np.where(np.isnan(csw_implied), agg["pit_k_prior"], np.clip(csw_implied, 0.10, 0.40))
    agg["pit_k_blend"] = (1 - _CSW_BLEND) * agg["pit_k_prior"] + _CSW_BLEND * agg["pit_k_csw"]

    # --- opponent team trailing batting K% (strictly prior games) ---
    tb = p.groupby(["game_pk", "batting_team"]).agg(
        game_date=("game_date", "first"), team_pa=("at_bat_number", "nunique")
    ).reset_index()
    tbk = p.loc[is_k].groupby(["game_pk", "batting_team"]).size().rename("team_k")
    tb = tb.merge(tbk, on=["game_pk", "batting_team"], how="left")
    tb["team_k"] = tb["team_k"].fillna(0).astype(int)
    tb["game_date"] = pd.to_datetime(tb["game_date"])
    tb = tb.sort_values(["game_date", "game_pk"])
    tg = tb.groupby("batting_team")
    tb["opp_cum_k"] = tg["team_k"].cumsum() - tb["team_k"]
    tb["opp_cum_pa"] = tg["team_pa"].cumsum() - tb["team_pa"]
    agg = agg.merge(
        tb[["game_pk", "batting_team", "opp_cum_k", "opp_cum_pa"]].rename(columns={"batting_team": "opp_team"}),
        on=["game_pk", "opp_team"], how="left",
    )
    agg["opp_k_prior"] = (agg["opp_cum_k"].fillna(0) + league_k * _PRIOR_PA_TEAM) / (
        agg["opp_cum_pa"].fillna(0) + _PRIOR_PA_TEAM
    )

    # --- park K factors from the TRAINING (earliest) season, applied forward ---
    train_season = int(agg["season"].min())
    seasons = sorted(agg["season"].unique())
    if len(seasons) > 1:
        tr = p[pd.to_datetime(p["game_date"]).dt.year.eq(train_season)]
        # PAs per park = distinct (game, at-bat) pairs (at_bat_number resets each game)
        park_pa = tr.drop_duplicates(["game_pk", "at_bat_number"]).groupby("home_team").size()
        park_k = tr.loc[is_k.reindex(tr.index, fill_value=False)].groupby("home_team").size()
        mult = ((park_k.reindex(park_pa.index).fillna(0) + league_k * _PARK_REG_PA)
                / (park_pa + _PARK_REG_PA)) / league_k
        agg["park_k_mult"] = agg["home_team"].map(mult).fillna(1.0)
    else:
        agg["park_k_mult"] = 1.0  # single season: no leakage-safe park training set

    agg["pitcher"] = agg["player_name"]
    agg["date"] = agg["game_date"].dt.date
    # emit only seasons AFTER the training season (leakage-safe), with enough pitcher history
    scored_seasons = seasons[1:] if len(seasons) > 1 else seasons
    out = agg[(agg["prior_starts"] >= min_prior_starts) & (agg["season"].isin(scored_seasons))].copy()
    cols = [
        "date", "season", "pitcher", "throws", "opp_team",
        "pit_k_prior", "pit_k_csw", "pit_k_blend", "opp_k_prior",
        "proj_bf_mean", "proj_bf_sd", "park_k_mult", "trailing_csw", "trailing_swstr",
        "realized_bf", "realized_k",
    ]
    return out[cols].reset_index(drop=True), league_k


def main() -> int:
    ap = argparse.ArgumentParser(description="Build starts.csv from Statcast")
    ap.add_argument("--seasons", default=None, help="comma list, e.g. 2023,2024 (earliest = train, rest scored)")
    ap.add_argument("--start", default="2024-04-01", help="single-window ISO date (ignored if --seasons)")
    ap.add_argument("--end", default="2024-08-01", help="single-window ISO date (ignored if --seasons)")
    ap.add_argument("--min-prior-starts", type=int, default=3)
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    try:
        import pybaseball  # noqa: F401
    except ImportError:
        print('pybaseball not installed. Run: pip install -e ".[data]"', file=sys.stderr)
        return 1

    seasons = [int(s) for s in args.seasons.split(",")] if args.seasons else None
    scope = f"seasons {seasons}" if seasons else f"{args.start}..{args.end}"
    print(f"Pulling Statcast ({scope}); first pull is slow, cached after ...")
    pitches = pull(seasons, args.start, args.end)
    print(f"  {len(pitches):,} pitches across {pitches['game_pk'].nunique():,} games")
    starts, league_k = build_starts(pitches, args.min_prior_starts)
    out_path = args.out
    if out_path is None:
        d = data_dir()
        d.mkdir(parents=True, exist_ok=True)
        out_path = d / "starts.csv"
    starts.to_csv(out_path, index=False)
    park = "real (train-season)" if seasons and len(seasons) > 1 else "inert (1.0; single season)"
    print(
        f"  league K/PA={league_k:.4f} | park factors: {park} | wrote {len(starts):,} scored starts "
        f"-> {out_path}\n  Now: heater backtest --real" + (" --season-from " + str(sorted(set(starts.season))[0]) if len(starts) else "")
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
