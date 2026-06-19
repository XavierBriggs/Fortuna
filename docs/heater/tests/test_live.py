"""Pure (no-network) tests for live-prior name/team matching and resolution."""
from heater.data.live import marcel_k, norm_name, parse_event_teams, resolve_priors

_LG = 0.215  # starter-league K% baseline


def test_marcel_keeps_established_ace_elite():
    # 3 weighted years of ~.29 K% over ~2700 weighted BF: the 100-BF ballast barely moves it
    ace = marcel_k([(52, 200), (210, 700), (200, 680)], _LG)
    assert 0.28 < ace < 0.30  # stays elite, NOT dragged toward starter-league .215


def test_marcel_regresses_small_sample_toward_league():
    # a rookie with one short sample is pulled hard toward starter-league
    rook = marcel_k([(11, 50), (0, 0), (0, 0)], _LG)
    assert abs(rook - _LG) < abs(0.22 - _LG)  # closer to league than to his raw .22


def test_marcel_recency_weights_favor_current_year():
    # same totals but loaded into the current (weight-3) vs oldest (weight-1) slot
    recent_hot = marcel_k([(70, 200), (0, 0), (0, 0)], _LG)   # .35 this year
    old_hot = marcel_k([(0, 0), (0, 0), (70, 200)], _LG)      # .35 three years ago
    assert recent_hot > old_hot  # current year weighted more, so less regressed


def test_norm_name_collides_last_first_and_first_last():
    assert norm_name("Skubal, Tarik") == norm_name("Tarik Skubal")


def test_norm_name_strips_accents_and_suffix():
    assert norm_name("Randy Vásquez") == norm_name("Vasquez, Randy")
    assert norm_name("Luis Ortiz Jr.") == norm_name("Ortiz, Luis")


def test_parse_event_teams_pitcher_and_opponent():
    assert parse_event_teams("KXMLBKS-26JUN191840CWSDET-DETTSKUBAL29") == ("DET", "CWS")
    assert parse_event_teams("KXMLBKS-26JUN191420TORCHC-TORKGAUSMAN34") == ("TOR", "CHC")
    assert parse_event_teams("KXMLBKS-26JUN191420TORCHC-CHCBBROWN32") == ("CHC", "TOR")


def test_resolve_priors_maps_team_and_hand():
    priors = {
        "league_k": 0.225,
        "pitchers": {
            norm_name("Tarik Skubal"): {
                "name": "Skubal, Tarik", "throws": "L", "pit_k_prior": 0.30,
                "proj_bf_mean": 25.0, "proj_bf_sd": 4.0, "n_starts": 15,
            }
        },
        "teams": {"CWS|L": 0.245},  # 2026 Statcast uses CWS for the White Sox, vs LHP
    }
    rp = resolve_priors(priors, "Tarik Skubal", "CWS")
    assert rp is not None
    assert rp["pit_k"] == 0.30 and rp["opp_k"] == 0.245
    assert rp["opp_src"] == "CWS vs LHP"


def test_resolve_priors_unknown_pitcher_is_none():
    priors = {"league_k": 0.225, "pitchers": {}, "teams": {}}
    assert resolve_priors(priors, "Nobody Here", "CWS") is None
