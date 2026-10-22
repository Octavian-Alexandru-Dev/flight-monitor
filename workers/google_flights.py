#!/usr/bin/env python3
"""Google Flights worker for flight-monitor.

Reads one JSON query on stdin and writes one JSON reply on stdout:

    {"status": "ok", "result": {"url": "...", "payload": [...] | null}}
    {"status": "error", "kind": "blocked|rate_limited|network|parse", "message": "..."}

The query is encoded with fast-flights' protobuf definitions (adding repeated
airports so one request covers several origins/destinations) and fetched with
a browser-like TLS fingerprint. Google embeds the results in the page only for
searches it has cached; otherwise the page loads them afterwards through an RPC
guarded by an anti-bot token that only its JavaScript can produce. In that case
the page is opened in headless Chromium (Playwright) and the RPC answer is
captured. Either way the raw payload is returned untouched: parsing and
normalisation happen in the Rust core.
"""

import json
import sys
from base64 import b64encode
from urllib.parse import urlencode

from fast_flights import FlightQuery, Passengers, create_query
from primp import Client
from selectolax.lexbor import LexborHTMLParser

URL = "https://www.google.com/travel/flights/search"
# Pre-accepted consent so EU requests are not redirected to consent.google.com.
CONSENT_COOKIES = {"SOCS": "CAESHAgBEhJnd3NfMjAyMzA4MTAtMF9SQzIaAmRlIAEaBgiAo_CmBg"}
RESULTS_RPC = "GetShoppingResults"
BROWSER_TIMEOUT_MS = 30_000
AIRPORT_FROM_FIELD = 13
AIRPORT_TO_FIELD = 14
FLIGHT_DATA_FIELD = 3


class WorkerError(Exception):
    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def length_delimited(field, payload):
    return varint((field << 3) | 2) + varint(len(payload)) + payload


def airport(code):
    # Airport { string airport = 2; }
    return length_delimited(2, code.encode())


def encode_tfs(request):
    legs = request["legs"]
    query = create_query(
        flights=[
            FlightQuery(date=leg["date"], from_airport=leg["from"][0], to_airport=leg["to"][0])
            for leg in legs
        ],
        trip="round-trip" if len(legs) == 2 else "one-way",
        passengers=Passengers(adults=request["adults"]),
        max_stops=request["max_stops"],
        checked_bags=request.get("checked_bags", 0),
        currency=request["currency"],
        language=request.get("language", "en"),
    )
    info = query.pb()
    flight_data = list(info.data)
    del info.data[:]
    encoded = info.SerializeToString()
    for leg, data in zip(legs, flight_data):
        leg_bytes = data.SerializeToString()
        leg_bytes += b"".join(length_delimited(AIRPORT_FROM_FIELD, airport(c)) for c in leg["from"][1:])
        leg_bytes += b"".join(length_delimited(AIRPORT_TO_FIELD, airport(c)) for c in leg["to"][1:])
        encoded += length_delimited(FLIGHT_DATA_FIELD, leg_bytes)
    return b64encode(encoded).decode(), query


def fetch(request):
    tfs, query = encode_tfs(request)
    params = {"tfs": tfs, "hl": query.language, "curr": query.currency}
    url = f"{URL}?{urlencode(params)}"
    client = Client(impersonate="chrome_145", impersonate_os="macos", referer=True, cookie_store=True)
    try:
        response = client.get(URL, params=params, cookies=CONSENT_COOKIES, timeout=40)
    except Exception as exc:  # primp raises its own exception types
        raise WorkerError("network", str(exc)) from exc

    if response.status_code == 429:
        raise WorkerError("rate_limited", "HTTP 429")
    if response.status_code in (403, 503) or "/sorry/" in str(response.url):
        raise WorkerError("blocked", f"HTTP {response.status_code} at {response.url}")
    if response.status_code != 200:
        raise WorkerError("network", f"HTTP {response.status_code}")
    payload = extract_payload(response.text)
    if payload is not None and flight_count(payload) == 0:
        payload = fetch_with_browser(url)
    return {"url": url, "payload": payload}


def flight_count(payload):
    """Itineraries in a payload: best flights at [2][0], the others at [3][0]."""
    count = 0
    for index in (2, 3):
        block = payload[index] if len(payload) > index else None
        if block and block[0]:
            count += len(block[0])
    return count


def rpc_payloads(body):
    """Decodes a streamed batchexecute answer into its inner payloads."""
    payloads = []
    for line in body.splitlines():
        if '"wrb.fr"' not in line:
            continue
        try:
            envelope = json.loads(line)[0]
        except (json.JSONDecodeError, IndexError):
            continue
        if len(envelope) > 2 and isinstance(envelope[2], str):
            payloads.append(json.loads(envelope[2]))
    return payloads


def fetch_with_browser(url):
    try:
        from playwright.sync_api import Error as PlaywrightError
        from playwright.sync_api import sync_playwright
    except ImportError as exc:
        raise WorkerError(
            "parse",
            "results are loaded asynchronously and Playwright is missing: run scripts/setup-worker.sh",
        ) from exc

    answers = []
    try:
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch()
            context = browser.new_context(locale="en-US")
            context.add_cookies(
                [{"name": name, "value": value, "domain": ".google.com", "path": "/"}
                 for name, value in CONSENT_COOKIES.items()]
            )
            page = context.new_page()

            def capture(response):
                if RESULTS_RPC in response.url and response.ok:
                    answers.extend(rpc_payloads(response.text()))

            page.on("response", capture)
            page.goto(url, wait_until="domcontentloaded", timeout=BROWSER_TIMEOUT_MS)
            # The first request often warms Google's cache, so the page may now embed the results.
            try:
                embedded = extract_payload(page.content())
            except WorkerError:
                embedded = None
            if embedded is not None:
                answers.append(embedded)
            waited = 0
            while waited < BROWSER_TIMEOUT_MS and not any(flight_count(a) for a in answers):
                page.wait_for_timeout(500)
                waited += 500
            final_html = page.content()
            browser.close()
    except PlaywrightError as exc:
        raise WorkerError("network", f"browser fallback failed: {exc}") from exc
    if not answers:
        extract_payload(final_html)  # raises "blocked" for consent or captcha pages
        raise WorkerError("parse", "results RPC not observed in the browser")
    return max(answers, key=flight_count)


def extract_payload(html):
    parser = LexborHTMLParser(html)
    title = parser.css_first("title")
    if title is not None and "before you continue" in title.text().lower():
        raise WorkerError("blocked", "consent page")
    script = parser.css_first(r"script.ds\:1")
    if script is None:
        if "unusual traffic" in html.lower():
            raise WorkerError("blocked", "unusual traffic page")
        raise WorkerError("parse", "results script ds:1 not found")
    text = script.text()
    try:
        data = text.split("data:", 1)[1].rsplit(",", 1)[0]
    except IndexError as exc:
        raise WorkerError("parse", "unexpected ds:1 layout") from exc
    if data.strip().endswith("errorHasStatus: true"):
        return None
    try:
        return json.loads(data)
    except json.JSONDecodeError as exc:
        raise WorkerError("parse", f"invalid ds:1 JSON: {exc}") from exc


def main():
    try:
        request = json.load(sys.stdin)
        reply = {"status": "ok", "result": fetch(request)}
    except WorkerError as exc:
        reply = {"status": "error", "kind": exc.kind, "message": str(exc)}
    except (KeyError, IndexError, TypeError, ValueError) as exc:
        reply = {"status": "error", "kind": "parse", "message": f"bad request: {exc!r}"}
    json.dump(reply, sys.stdout)


if __name__ == "__main__":
    main()
