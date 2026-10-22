"""Offline tests for the Google Flights worker: python -m unittest workers/test_google_flights.py"""

import base64
import json
import unittest

import google_flights as worker

REQUEST = {
    "legs": [{"date": "2026-12-10", "from": ["FCO", "MXP"], "to": ["NRT", "HND"]}],
    "max_stops": 1,
    "adults": 2,
    "currency": "EUR",
    "checked_bags": 1,
}


class EncodeTfs(unittest.TestCase):
    def test_every_airport_is_encoded_once(self):
        tfs, _ = worker.encode_tfs(REQUEST)
        raw = base64.b64decode(tfs)
        for code in ("FCO", "MXP", "NRT", "HND"):
            self.assertEqual(raw.count(code.encode()), 1, code)
        self.assertIn(b"2026-12-10", raw)

    def test_round_trip_has_two_legs(self):
        request = dict(REQUEST, legs=REQUEST["legs"] + [{"date": "2026-12-20", "from": ["NRT"], "to": ["FCO"]}])
        tfs, query = worker.encode_tfs(request)
        self.assertEqual(query.get_trip_type(), "round-trip")
        self.assertIn(b"2026-12-20", base64.b64decode(tfs))

    def test_varint_handles_long_lengths(self):
        self.assertEqual(worker.varint(300), b"\xac\x02")


class ExtractPayload(unittest.TestCase):
    def page(self, body, title="Google Flights"):
        return f"<html><head><title>{title}</title></head><body>{body}</body></html>"

    def test_reads_results_script(self):
        script = "AF_initDataCallback({key: 'ds:1', hash: '2', data:[null,[1,2]], sideChannel: {}});"
        html = self.page(f'<script class="ds:1">{script}</script>')
        self.assertEqual(worker.extract_payload(html), [None, [1, 2]])

    def test_no_flights_is_an_empty_payload(self):
        script = "AF_initDataCallback({key: 'ds:1', hash: '2', data:null, errorHasStatus: true, sideChannel: {}});"
        html = self.page(f'<script class="ds:1">{script}</script>')
        self.assertIsNone(worker.extract_payload(html))

    def test_consent_page_is_blocked(self):
        with self.assertRaises(worker.WorkerError) as ctx:
            worker.extract_payload(self.page("", title="Before you continue to Google"))
        self.assertEqual(ctx.exception.kind, "blocked")

    def test_changed_layout_is_a_parse_error(self):
        with self.assertRaises(worker.WorkerError) as ctx:
            worker.extract_payload(self.page("<div>nothing here</div>"))
        self.assertEqual(ctx.exception.kind, "parse")


class RpcPayloads(unittest.TestCase):
    def test_decodes_streamed_answer(self):
        inner = [None, None, [[["XX"]]], [[["YY"], ["ZZ"]]]]
        line = '[["wrb.fr",null,' + json.dumps(json.dumps(inner)) + "]]"
        body = ")]}'\n\n123\n" + line + '\n56\n[["di",47]]\n'
        payloads = worker.rpc_payloads(body)
        self.assertEqual(payloads, [inner])
        self.assertEqual(worker.flight_count(payloads[0]), 3)

    def test_ignores_error_envelopes(self):
        self.assertEqual(worker.rpc_payloads('[["wrb.fr",null,null,null,null,[13]]]'), [])


if __name__ == "__main__":
    unittest.main()
