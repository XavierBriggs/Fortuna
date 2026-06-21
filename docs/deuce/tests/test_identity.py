from deuce.identity import apply_alias, build_aliases_from_sackmann
from deuce.names import canonical_player, _heuristic_key


def test_seed_unifies_particle_surname_across_formats():
    # First-Last (Kalshi/Odds API) and Last-I. (tennis-data) -> the SAME id
    assert canonical_player("Alex de Minaur") == canonical_player("De Minaur A.")
    assert canonical_player("Alex de Minaur") == "de-minaur_a"


def test_seed_unifies_compound_surname():
    assert canonical_player("Alejandro Davidovich Fokina") == canonical_player("Davidovich Fokina A.")
    assert canonical_player("Roberto Bautista Agut") == canonical_player("Bautista Agut R.")


def test_plain_names_are_untouched():
    assert canonical_player("Alexander Zverev") == "zverev_a"
    assert canonical_player("Zverev A.") == "zverev_a"
    assert canonical_player("Roger Federer") == "federer_r"


def test_apply_alias_is_identity_for_unknown():
    assert apply_alias("federer_r") == "federer_r"


def test_build_aliases_unifies_divergent_forms():
    players = [
        ("1", "Alex", "De Minaur"),                 # diverges: minaur_a vs de-minaur_a
        ("2", "Roger", "Federer"),                  # no divergence
        ("3", "Alejandro", "Davidovich Fokina"),    # diverges: fokina_a vs davidovich-fokina_a
    ]
    aliases = build_aliases_from_sackmann(players, _heuristic_key)
    assert "de-minaur_a" in aliases
    assert "minaur_a" in aliases["de-minaur_a"]["keys"]
    assert aliases["de-minaur_a"]["sackmann_id"] == "1"
    assert "federer_r" not in aliases               # no divergence -> not emitted


def test_build_aliases_drops_ambiguous_truncation():
    # two players whose First-Last truncations collide -> drop, don't mis-match
    players = [
        ("1", "Alex", "De Minaur"),    # truncated -> minaur_a
        ("2", "Anna", "Van Minaur"),   # truncated -> minaur_a (same last token + initial)
    ]
    aliases = build_aliases_from_sackmann(players, _heuristic_key)
    assert aliases == {}               # minaur_a is ambiguous -> neither claims it
