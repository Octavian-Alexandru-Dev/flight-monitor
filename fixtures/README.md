# Fixtures

Responses used by the parsing tests. No test touches the network.

| Directory | Origin |
|---|---|
| `google_flights/` | Real replies of `workers/google_flights.py`, recorded on 2026-10-22 against Google Flights. |
| `travelpayouts/` | Built by hand from the documented `/aviasales/v3/prices_for_dates` response shape: no API token was available when they were written. Replace them with real answers (`curl -H "X-Access-Token: $TRAVELPAYOUTS_TOKEN" ...`) once you have a token. |
| `llm/` | Hand-written OpenAI-compatible chat completion, as returned by Ollama's `/v1/chat/completions`. |

To refresh a Google Flights fixture:

```sh
echo '{"legs":[{"date":"2026-12-10","from":["FCO","MXP"],"to":["NRT","HND"]}],"max_stops":1,"adults":1,"currency":"EUR","checked_bags":0}' \
  | workers/.venv/bin/python workers/google_flights.py > fixtures/google_flights/one_way_multi_airport.json
```
