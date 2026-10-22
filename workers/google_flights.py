#!/usr/bin/env python3
"""Google Flights worker for flight-monitor.

Reads one JSON query on stdin and writes one JSON reply on stdout:

    {"status": "ok", "result": {"url": "...", "payload": [...] | null}}
    {"status": "error", "kind": "blocked|rate_limited|network|parse", "message": "..."}

The query is encoded with fast-flights' protobuf definitions (adding repeated
airports so one request covers several origins/destinations) and fetched with
a browser-like TLS fingerprint. The raw `ds:1` payload is returned untouched:
parsing and normalisation happen in the Rust core.
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
    return {"url": url, "payload": extract_payload(response.text)}


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
