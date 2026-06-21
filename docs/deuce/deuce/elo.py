"""Surface-weighted WElo engine.

Pre-match rating blends an overall Elo with a per-surface Elo; updates use a
538-style decaying K and an optional WElo margin multiplier driven by the
scoreline. The engine is deterministic and stateful: feed matches in
chronological order, calling `predict` BEFORE `update` for each match so the
prediction only ever sees the past.
"""
from __future__ import annotations

from dataclasses import dataclass, field

from .config import EloConfig


def expected_score(rating_a: float, rating_b: float) -> float:
    """P(a beats b) under the logistic Elo curve. Symmetric: e(a,b)=1-e(b,a)."""
    return 1.0 / (1.0 + 10.0 ** ((rating_b - rating_a) / 400.0))


def _k_factor(matches_played: int, cfg: EloConfig) -> float:
    return cfg.k_base / (matches_played + cfg.k_offset) ** cfg.k_shape


def margin_multiplier(winner_games: int, loser_games: int, cfg: EloConfig) -> float:
    """WElo dominance weight, >=1, bounded. 1.0 when the score is unknown/even.

    Dominance = (wg - lg)/(wg + lg) in [0, 1]; a bagel updates harder than a
    third-set tiebreak. This is a deliberately simple v0 heuristic to be tuned
    against held-out log-loss, not a claim about the optimal weighting.
    """
    if not cfg.margin_weighting:
        return 1.0
    total = winner_games + loser_games
    if total <= 0:
        return 1.0
    dominance = (winner_games - loser_games) / total
    return 1.0 + cfg.margin_scale * max(0.0, dominance)


@dataclass
class EloEngine:
    cfg: EloConfig = field(default_factory=EloConfig)
    _overall: dict[str, float] = field(default_factory=dict)
    _surface: dict[tuple[str, str], float] = field(default_factory=dict)
    _n_overall: dict[str, int] = field(default_factory=dict)
    _n_surface: dict[tuple[str, str], int] = field(default_factory=dict)

    def _rating(self, player: str, surface: str) -> tuple[float, float]:
        overall = self._overall.get(player, self.cfg.init_rating)
        # surface rating falls back to overall until the player has surface history
        surf = self._surface.get((player, surface), overall)
        return overall, surf

    def blended(self, player: str, surface: str) -> float:
        overall, surf = self._rating(player, surface)
        w = self.cfg.surface_weight
        return w * surf + (1.0 - w) * overall

    def predict(self, p1: str, p2: str, surface: str) -> float:
        """P(p1 beats p2) on `surface`, using only history seen so far."""
        return expected_score(self.blended(p1, surface), self.blended(p2, surface))

    def has_player(self, player: str) -> bool:
        """Whether the player has any rating history (else predict is a coinflip)."""
        return player in self._overall

    def update(
        self,
        winner: str,
        loser: str,
        surface: str,
        winner_games: int = 0,
        loser_games: int = 0,
    ) -> None:
        mult = margin_multiplier(winner_games, loser_games, self.cfg)

        # --- overall ---
        rw, rl = (
            self._overall.get(winner, self.cfg.init_rating),
            self._overall.get(loser, self.cfg.init_rating),
        )
        ew = expected_score(rw, rl)
        kw = _k_factor(self._n_overall.get(winner, 0), self.cfg) * mult
        kl = _k_factor(self._n_overall.get(loser, 0), self.cfg) * mult
        self._overall[winner] = rw + kw * (1.0 - ew)
        self._overall[loser] = rl + kl * (0.0 - (1.0 - ew))
        self._n_overall[winner] = self._n_overall.get(winner, 0) + 1
        self._n_overall[loser] = self._n_overall.get(loser, 0) + 1

        # --- per-surface (seeded from overall on first appearance via _rating) ---
        wk, lk = (winner, surface), (loser, surface)
        srw = self._surface.get(wk, self._overall.get(winner, self.cfg.init_rating))
        srl = self._surface.get(lk, self._overall.get(loser, self.cfg.init_rating))
        # NOTE: recompute expectation from PRE-update surface ratings
        sew = expected_score(srw, srl)
        skw = _k_factor(self._n_surface.get(wk, 0), self.cfg) * mult
        skl = _k_factor(self._n_surface.get(lk, 0), self.cfg) * mult
        self._surface[wk] = srw + skw * (1.0 - sew)
        self._surface[lk] = srl + skl * (0.0 - (1.0 - sew))
        self._n_surface[wk] = self._n_surface.get(wk, 0) + 1
        self._n_surface[lk] = self._n_surface.get(lk, 0) + 1
