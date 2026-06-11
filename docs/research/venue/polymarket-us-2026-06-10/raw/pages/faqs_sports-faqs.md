> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Sports FAQs

> Frequently asked questions about trading sports Contracts on Polymarket US

## General Rules

### What kinds of markets are offered?

Polymarket US offers several types of sports Contracts:

| Contract              | What it covers                                                                             | Example                                                    |
| --------------------- | ------------------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| **Winner (w/o ties)** | Which team or player wins a game                                                           | “Which team will win, Lakers vs Celtics?”                  |
| **Winner (w/ ties)**  | Which team or player wins a game or match where a draw is a standard outcome (e.g. soccer) | “Will Man City vs Arsenal end in a draw?”                  |
| **Spread**            | Whether a team wins by a certain margin                                                    | “Will the Chiefs win by more than 7?”                      |
| **Total**             | Whether the combined score is over or under a threshold                                    | “Will the total score in Patriots vs Chiefs be over 47.5?” |
| **Future**            | Who will win a championship, award, or tournament                                          | “Will the Lakers win the 2026 NBA Championship?”           |
| **Qualifier**         | Whether a team or player qualifies for an event                                            | “Will Duke make the NCAA Tournament Final Four?”           |
| **Player Prop**       | Whether an individual player’s stat reaches a threshold                                    | “Will LeBron score over 25.5 points vs the Celtics?”       |

### How are markets settled?

All markets are settled using the official result obtained through a hierarchy of sources:

* **Primary source.** The official governing body or sanctioning organization responsible for the event.
* **Secondary sources.** If the primary source is unavailable, Polymarket US may reference official competition scorecards, referee or umpire reports, press releases, and results databases maintained by the governing body.
* **Tertiary sources.** If secondary sources are unavailable, Polymarket US may reference the Associated Press, Reuters, ESPN, BBC Sport, official team and league websites, and major sports wire services or data providers.

Settlement is delayed if the official result is under review. If no official result is declared by the Contract’s expiration date, the Contract settles at last fair market prices.

### What is “last fair market price” and how does it differ from the last traded price?

Last fair market price (LFMP) is the prevailing fair market price on the Exchange at a specified moment in time, typically the moment an official announcement is made (e.g. a cancellation, walkover, or no-contest). It is **not** the last traded price at market close. Markets may remain open for a period after an announcement, and trades that occur during this window are executed at the trader’s own risk. The settlement price reflects LFMP at the time of the announcement, not any prices that print afterward. Traders are responsible for monitoring official announcements prior to and during trading.

### How do ties and draws work?

It depends on the Contract type:

* **Winner (w/o tie):** If the game ends in a tie with no winner declared, the Contract settles at \$0.50.

  Example: an NFL game enters overtime and no team scores. The game ends in a tie and each Contract settles at \$0.50.

* **Winner (w/ tie):** Three Contracts per game (Team A Win, Team B Win, Draw). Exactly one settles at \$1.00, the others at \$0.00.

  Example: an EPL match is tied after 90 minutes plus stoppage time. The Draw Contract settles at \$1.00 and the two team Contracts settle at \$0.00.

* **Spread:** Half-point spreads (e.g. -3.5) are used to avoid ties.

  Example: the Chiefs are -3.5 vs the Eagles. The Chiefs Contract settles at \$1.00 if they win by 4 or more. The Eagles Contract settles at \$1.00 if they lose by 3 or fewer or win outright.

* **Total:** Half-point totals (e.g. 47.5) are used to avoid ties.

  Example: total of 47.5 in Chiefs vs Eagles. If the combined score is 47 or fewer, Under settles at \$1.00. If 48 or more, Over settles at \$1.00.

* **Co-winners:** If multiple participants are declared co-winners by the governing body without a playoff or tiebreaker, each winner’s Contract settles at \$1.00 divided by the number of winners, rounded down to the nearest tick. All non-winning Contracts settle at \$0.00.

### How does overtime work?

For game-level Contracts (Athletic Event, Athletic Spread, Total Score), overtime, extra time, extra innings, penalty shootouts, and tiebreakers are **included** by default unless otherwise specified in the Contract Terms. The official final result governs settlement.

The exception is **soccer**: for league matches (e.g. EPL, MLS regular season) the result at the end of regulation time (90 minutes plus stoppage) governs. Extra time and penalties are excluded because draws are a valid outcome.

Sport-specific overtime mechanics (e.g. NHL regular-season vs playoff overtime, NBA continuous overtime) are described in each sport’s section.

### What if a game is postponed or rescheduled?

If a game is postponed before the start of play and rescheduled before the Contract’s expiration date (typically two weeks from the original event date), the official result of the rescheduled game governs settlement. If the rescheduled game falls outside the expiration date, all Contracts on the original game settle at last fair market prices.

Postponed games do not automatically carry over to the rescheduled date.

For soccer specifically, if a postponed match is rescheduled with the home and away team designations reversed, the original Contract settles at LFMP because the nature of the match has fundamentally changed.

### What if a game is suspended, abandoned, or shortened mid-play?

Settlement depends on whether the game has reached the **official game threshold** required by the governing body to declare a result (each sport defines its own threshold):

* **Threshold reached at the time of stoppage.** The official result at the time of stoppage governs settlement for all markets.
* **Threshold not reached and game completed before expiration.** The official final result governs settlement.
* **Threshold not reached and game not completed before expiration.** All Contracts settle at last fair market prices.

### What if a game is canceled and never replayed?

If a game is canceled and not rescheduled before the Contract’s expiration date, the Contract settles at last fair market prices as of the time the cancellation was officially announced. This includes games abandoned or suspended mid-play without the governing body declaring an official result.

### What if a participant withdraws?

It depends on the Contract type and the timing of the withdrawal:

* **Game-level Contracts (Winner, Spread, Total).** Pre-event participant withdrawal: Contract settles at last fair market prices.
* **Futures and Qualifiers.** The withdrawn participant’s Contract settles at \$0.00. Sport-specific definitions of when a participant has “entered” the event (e.g. tee-off in golf, first serve in tennis, opening bell in UFC) are described in each sport’s section.

If a participant retires, defaults, or is disqualified mid-event and the governing body declares a winner or official result, that result governs settlement.

### What if there’s a forfeit?

The participant awarded the forfeit is the winner. For Spread and Total Contracts, the official forfeit score (e.g. 9-0 in baseball) governs. For Winner (w/ tie) Contracts, the participant awarded the forfeit settles at \$1.00 and the “Tie” Contract settles at \$0.00.

### What if there’s a no contest?

If the governing body declares the event a no contest, the Contract settles at last fair market prices as of the announcement.

Example: a UFC fight or boxing match ends in a no contest. The Contract settles at LFMP at the time the no contest was announced.

### What about replayed or protested games?

If a game is replayed in its entirety pursuant to a protest or governing body decision before the Contract’s expiration date, the replay result governs. If the replay happens after expiration, the original result stands and a new market is created for the replayed game.

### Does a venue change affect Contracts?

A venue change has no impact on any active markets. All Contracts remain valid and orders remain open regardless of any change to the location, including changes to a neutral site or different stadium.

***

## Futures & Qualifiers

### How does settlement work?

A Futures market is settled using results from multiple games or rounds, such as a championship or season-long award. Futures settle on the participant officially declared winner by the governing body, typically at the moment of the trophy presentation or final awards announcement. A team or player Contract may settle at \$0.00 as soon as it is mathematically impossible for them to win or qualify and that elimination has been officially recognized by the governing body.

A Qualifier Contract settles at \$1.00 when the governing body officially declares that the participant has met the qualification criteria for the event. Qualification may be determined through any officially recognized path: league standings or points accumulation, tournament progression, direct selection or invitation by the governing body, or performance-based criteria.

### What if a team is rebranded, relocated, or renamed?

Futures placed before the change remain valid and follow the team under its new name or location, as long as the governing body continues to recognize the team as the same competitive entity.

### How do elimination and qualification work?

* **Futures.** If a team is eliminated from a championship (e.g. knocked out of the playoffs), the Contract settles at \$0.00.
* **Qualifiers.** If a team cannot qualify for the specified event or round (e.g. eliminated during the tournament stage and cannot make the playoffs), the Contract settles at \$0.00.

### What if a qualification is reversed before expiration?

If a participant initially qualifies or is eliminated and that determination is later reversed, vacated, or reassigned before the Contract’s expiration date, the Contract settles based on the final official determination as of the expiration date. Reversals announced after expiration do not affect settlement.

***

## Sport-Specific FAQs

All game-level rules in *General Rules* apply to both pregame and in-game trading unless explicitly stated otherwise in a sport-specific section.

***

## Tennis (WTA, ATP, & ITF)

### When does a match officially begin?

A tennis match officially begins when the first serve is struck. Anything happening before the first serve (cancellation, walkover, withdrawal) is treated as a pre-event scenario and Contracts settle at last fair market prices as of the official announcement. Except in the case of ITF Men's and Women's matches, cancellation, walkover, or withdrawal will resolve at \$0.50 per Contract.

### Mid-match retirement, default, or disqualification

If a player retires after the first serve, defaults for a code violation, or is disqualified, the market resolves on the official result declared by the governing body. Whoever is awarded the win settles at \$1.00, regardless of how many games or sets were completed.

### Untraditional format

If a match format differs from traditional WTA, ATP, or ITF format, or if formats are updated mid-tournament, all markets stand and resolve on the official governing-body result.

### Venue or surface change

If a match is moved to a different venue, court type, or surface but continues to be played before the expiration date between the same players, the market remains open and trading continues.

***

## Baseball (MLB)

### Game threshold and universal settlement rule

If MLB declares an official result, that result governs settlement for all markets (Winner, Spread, Total) regardless of how many innings were played. The official score at the time of the MLB ruling governs all market settlement.

Example: a game is called due to weather and MLB awards an official result with a score of 1-0. The team leading 1-0 wins the Winner market, +1.5 on the spread settles at \$1.00, and the Totals market settles based on a score of 1-0.

### Doubleheaders

Each game in a doubleheader is an independent Contract. If one game of a scheduled doubleheader is not played on the original date and is not rescheduled before the Contract’s expiration date, that game’s Contracts settle at LFMP. The completed game settles per the official MLB result.

For non-standard doubleheader formats (e.g. seven-inning doubleheaders), the official game threshold is per MLB’s rules for that format.

### Player doesn’t play

If the player doesn’t participate at all, the Contract settles at LFMP.

### Player leaves the game early

Stats accumulated up to that point count per the official box score. If the threshold was already crossed, the Contract settles at \$1.00. If it can no longer be met, it settles at \$0.00. If the outcome is unclear, the final official stats determine settlement.

### Settlement source for awards

Awards settle on the official announcement by the relevant governing body. For BBWAA awards (MVP, Cy Young, Rookie of the Year), the official BBWAA vote result governs settlement.

### Player traded between leagues

If a player is traded between the AL and NL, they are no longer eligible for the league-specific award tied to the league they left. Their Contract for that award (e.g. AL MVP) settles at \$0.00. Applies to MVP, Cy Young, and Rookie of the Year.

***

## Soccer

### Result is regulation time

All markets are based on the result at the end of 90 minutes plus stoppage, unless explicitly stated otherwise. Extra time, golden goals, and penalty shootouts are not included unless the Contract Terms specify otherwise. For any market in which a draw is a listed outcome (league or group-stage matches), the market refers to regulation time only.

### Postponement with home/away flip

If a postponed match is rescheduled with the home and away team designations reversed, the original Contract settles at LFMP because the nature of the match has fundamentally changed.

### Soccer Futures

Unless explicitly designated as a regular-season winner market, Soccer Futures settle on the final competition winner including playoff rounds.

***

## Golf

### How tournament winners are settled

Playoffs are included: if two or more players are tied at the end of regulation and a playoff is conducted, the playoff winner is the tournament winner for settlement purposes.

### Dead heat rules

If two or more players are declared co-winners without a playoff, each winner’s Contract settles at \$1.00 divided by the number of winners, rounded down to the nearest tick. All other player Contracts settle at \$0.00.

### Tournament shortened or canceled

If the governing body declares an official winner, that result governs regardless of how many holes or rounds were completed. If the tournament does not reach the minimum threshold required by the governing body to declare an official winner, all Contracts settle at LFMP.

***

## UFC

### Draws and technical draws

A draw occurs when the judges’ scorecards are equal at the end of the bout. A technical draw occurs when a fight is stopped before the scheduled distance due to an accidental foul and the scorecards are used to determine the result. Both are treated identically for settlement: all Contracts settle at \$0.50.

### Disqualification or retirement between rounds

If a fighter is disqualified during a bout, the opponent is declared the winner and the opponent’s Contract settles at \$1.00. If a fighter retires between rounds and does not answer the bell for the next round, the bout is deemed to have ended at the conclusion of the previous round and the opponent wins.

### Mid-fight no contest

If a bout is declared a no contest during the fight (e.g. due to an accidental foul or headbutt), all Contracts settle at LFMP regardless of how many rounds were completed.

### Substitutions and bout changes

* **Fighter substitution.** Orders on the original contest settle at LFMP and a new market is created for the new fight.
* **Round count change** (3 ↔ 5). All existing Winner Contracts remain active and settle on the official UFC result.
* **Weight miss or catchweight.** All existing Contracts remain active and settle on the official UFC result.

***

## NHL

### Overtime and shootouts

* **Regular season.** If tied after 60 minutes of regulation, a 5-minute 3-on-3 sudden-death overtime is played. If still tied, a shootout determines the winner. All Winner, Spread, and Total markets include overtime and shootout results unless explicitly stated otherwise. The shootout winner is credited with one goal in the official NHL recorded score, and that score governs settlement for all markets.
* **Playoffs.** Full 5-on-5 sudden-death overtime periods continue until a goal is scored. There is no shootout. All overtime periods count toward Winner, Spread, and Total markets.

***

## Basketball (NBA / NCAA)

### Overtime

All Winner, Spread, and Total markets include overtime unless explicitly stated. Overtime periods continue until a winner is determined; there are no ties. The official final score after all overtime periods governs settlement.

***

## Cricket

### Tie, draw, or no-result

If a match ends in a tie, draw, no result (NR), abandonment, or cancellation and no official winner is declared, all markets settle at \$0.50.

### Forfeit, disqualification, or concession

* **Before the match begins.** All markets settle at \$0.50.
* **After the match has begun.** If the governing body declares an official winner, the market settles based on that result.

### Insufficient play

If play begins but insufficient play occurs to determine an official result, all markets settle at \$0.50.
