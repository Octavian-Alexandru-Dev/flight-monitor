# Fixtures

Responses used by the parsing tests. No test touches the network.

| Directory | Origin |
|---|---|
| `google_flights/` | Real Google Flights data recorded on 2026-10-22: `*.json` are the URL and raw payload of a search, `page_fco_nrt.html` a results page trimmed to its title and `ds:1` script, `shopping_results_rpc.txt` the streamed answer of the `GetShoppingResults` RPC. |
| `travelpayouts/` | Built by hand from the documented `/aviasales/v3/prices_for_dates` response shape: no API token was available when they were written. Replace them with real answers (`curl -H "X-Access-Token: $TRAVELPAYOUTS_TOKEN" ...`) once you have a token. |
| `llm/` | Hand-written OpenAI-compatible chat completion, as returned by Ollama's `/v1/chat/completions`. |
