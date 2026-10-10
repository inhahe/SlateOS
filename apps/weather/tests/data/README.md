# Open-Meteo replies, for the weather app's tests

Real replies from Open-Meteo's free API, fetched once on 2026-10-10 over plain
HTTP for Berlin (a city's coordinates, not anyone's location), so that the
parsers are tested against what the service sends rather than what a test's
author guessed it sends.

| File | Request |
|---|---|
| `openmeteo-search-berlin.json` | `http://geocoding-api.open-meteo.com/v1/search?name=Berlin&count=3&language=en&format=json` |
| `openmeteo-forecast-berlin.json` | `http://api.open-meteo.com/v1/forecast?latitude=52.52437&longitude=13.41053` with the `current`, `hourly` and `daily` fields `src/openmeteo.rs` asks for, `timezone=auto`, `forecast_days=7` |
| `openmeteo-air-berlin.json` | `http://air-quality-api.open-meteo.com/v1/air-quality?latitude=52.52437&longitude=13.41053&current=us_aqi,european_aqi` |
| `openmeteo-raw-chunked.http` | a forecast's whole reply, headers and chunked body, as it arrived on the socket |

Weather data by Open-Meteo.com, licensed under Attribution 4.0 International
(CC BY 4.0): <https://open-meteo.com/en/license>.
