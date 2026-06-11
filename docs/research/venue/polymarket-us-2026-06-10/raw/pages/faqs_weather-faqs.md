> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Weather FAQs

> Frequently asked questions about trading Weather Contracts on Polymarket US

## General Rules

### What kinds of Weather Contracts are offered?

Polymarket US offers Temperature Contracts -- Event Contracts that resolve based on whether the temperature in a specified location during a specified period satisfies a specified condition relative to a specified value.

### How are Weather Contracts settled?

Settlement is determined by the official NWS Daily Climate Report (CLI) published by the local Weather Forecast Office. The CLI is an official government record that reports observed high, low, and average temperatures for a given location and date.

The settlement source for each currently offered city is:

| City          | Station                                    | CLI Source                                                                                                                              |
| ------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| New York City | KNYC (Central Park)                        | [CLINYC](https://forecast.weather.gov/product.php?site=NWS\&issuedby=NYC\&product=CLI\&format=CI\&version=1\&glossary=1\&highlight=off) |
| San Francisco | KSFO (San Francisco International Airport) | [CLISFO](https://forecast.weather.gov/product.php?site=NWS\&issuedby=SFO\&product=CLI\&format=CI\&version=1\&glossary=1\&highlight=off) |
| Miami         | KMIA (Miami International Airport)         | [CLIMIA](https://forecast.weather.gov/product.php?site=NWS\&issuedby=MIA\&product=CLI\&format=CI\&version=1\&glossary=1\&highlight=off) |
| Chicago       | KMDW (Chicago Midway Airport)              | [CLIMDW](https://forecast.weather.gov/product.php?site=NWS\&issuedby=MDW\&product=CLI\&format=CI\&version=1\&glossary=1\&highlight=off) |
| Los Angeles   | KLAX (Los Angeles International Airport)   | [CLILAX](https://forecast.weather.gov/product.php?site=NWS\&issuedby=LAX\&product=CLI\&format=CI\&version=1\&glossary=1\&highlight=off) |

### When does settlement occur?

Settlement occurs at 8:00 AM ET on the day following the Contract's specified date. If the CLI reading is inconsistent with the 24-hour METAR observation for the same location, settlement may be delayed until 11:00 AM ET for review. If no data is published within one week of the scheduled release, the Contract settles at last fair market prices.
