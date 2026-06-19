"""Player/tournament name normalization.

tennis-data.co.uk uses "Federer R." while Sackmann uses "Roger Federer". This
module produces a canonical key so the two can be joined (NEXT PHASE, when
serve/return features come in). The Phase-A backtest runs on tennis-data alone
and does not need the join — but it DOES use `canonical_player` to build the
leakage-safe alphabetical player ordering.
"""
from __future__ import annotations

import re
import unicodedata

from .identity import apply_alias

_WS = re.compile(r"\s+")


def _strip_accents(s: str) -> str:
    return "".join(c for c in unicodedata.normalize("NFKD", s) if not unicodedata.combining(c))


def _heuristic_key(name: str) -> str:
    """Best-effort 'lastname_initial' key, accent- and case-folded.

    Handles both "Federer R." (tennis-data) and "Roger Federer" (Sackmann/Kalshi):
      "Federer R."     -> "federer_r"
      "Roger Federer"  -> "federer_r"
      "Auger-Aliassime F." -> "auger-aliassime_f"
    Multi-word/particle surnames diverge between the two forms ("Alex de Minaur" ->
    "minaur_a" but "De Minaur A." -> "de-minaur_a"); the alias map (identity.py)
    reconciles those — this function stays a pure, deterministic heuristic.
    """
    s = _strip_accents(name).strip().lower()
    s = _WS.sub(" ", s)
    if "." in s:  # "lastname x." form (tennis-data)
        parts = s.split()
        initial = parts[-1].rstrip(".")[:1]
        surname = " ".join(parts[:-1])
        return f"{surname.replace(' ', '-')}_{initial}"
    # "First Last" form: take last token as surname, first initial
    parts = s.split()
    if len(parts) == 1:
        return parts[0]
    first_initial = parts[0][:1]
    surname = parts[-1]
    return f"{surname}_{first_initial}"


def canonical_player(name: str) -> str:
    """Canonical player id: the heuristic key, then alias-resolved to one id so the
    same player joins across Kalshi / sportsbooks / tennis-data regardless of format."""
    return apply_alias(_heuristic_key(name))


def canonical_tournament(name: str) -> str:
    s = _strip_accents(name).strip().lower()
    s = re.sub(r"[^a-z0-9]+", "_", s).strip("_")
    return s
