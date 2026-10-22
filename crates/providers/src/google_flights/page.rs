//! Where the results live: embedded in the page (`ds:1` script) for searches Google has
//! cached, otherwise in the streamed answer of the `GetShoppingResults` RPC.

use serde_json::Value;

use crate::ProviderError;

/// Itineraries in a payload: best flights at `[2][0]`, the others at `[3][0]`.
pub fn flight_count(payload: &Value) -> usize {
    [&payload[2][0], &payload[3][0]]
        .into_iter()
        .filter_map(Value::as_array)
        .map(Vec::len)
        .sum()
}

/// The `ds:1` payload of a results page; `None` when Google states there are no flights.
pub fn embedded_payload(html: &str) -> Result<Option<Value>, ProviderError> {
    let lower_head = html
        .get(..html.len().min(4096))
        .unwrap_or_default()
        .to_lowercase();
    if lower_head.contains("<title>before you continue") {
        return Err(ProviderError::Blocked);
    }
    let Some(start) = html.find(r#"class="ds:1""#) else {
        if html.to_lowercase().contains("unusual traffic") {
            return Err(ProviderError::Blocked);
        }
        return Err(ProviderError::Parse("results script ds:1 not found".into()));
    };
    let script = &html[start..];
    let script = &script[..script.find("</script>").unwrap_or(script.len())];
    let data = script
        .split_once("data:")
        .and_then(|(_, rest)| rest.rsplit_once(','))
        .map(|(data, _)| data.trim())
        .ok_or_else(|| ProviderError::Parse("unexpected ds:1 layout".into()))?;
    if data.ends_with("errorHasStatus: true") {
        return Ok(None);
    }
    serde_json::from_str(data)
        .map(Some)
        .map_err(|e| ProviderError::Parse(format!("invalid ds:1 JSON: {e}")))
}

/// Inner payloads of a streamed batchexecute answer (`)]}'` then length-prefixed chunks).
pub fn rpc_payloads(body: &str) -> Vec<Value> {
    body.lines()
        .filter(|line| line.contains(r#""wrb.fr""#))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|envelope| envelope[0][2].as_str().map(str::to_string))
        .filter_map(|inner| serde_json::from_str(&inner).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/google_flights/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn reads_results_embedded_in_a_recorded_page() {
        let payload = embedded_payload(&fixture("page_fco_nrt.html"))
            .unwrap()
            .unwrap();
        assert!(flight_count(&payload) >= 5);
    }

    #[test]
    fn reads_results_from_a_recorded_rpc_answer() {
        let payloads = rpc_payloads(&fixture("shopping_results_rpc.txt"));
        assert!(!payloads.is_empty());
        assert!(payloads.iter().any(|p| flight_count(p) > 0));
    }

    #[test]
    fn classifies_pages() {
        let page = |title: &str, body: &str| {
            format!("<html><head><title>{title}</title></head><body>{body}</body></html>")
        };
        assert_eq!(
            embedded_payload(&page("Before you continue to Google", "")),
            Err(ProviderError::Blocked)
        );
        assert_eq!(
            embedded_payload(&page("Sorry", "Our systems have detected unusual traffic")),
            Err(ProviderError::Blocked)
        );
        assert!(matches!(
            embedded_payload(&page("Google Flights", "")),
            Err(ProviderError::Parse(_))
        ));
        let none = r#"<script class="ds:1">AF_initDataCallback({key: 'ds:1', data:null, errorHasStatus: true, sideChannel: {}});</script>"#;
        assert_eq!(embedded_payload(none), Ok(None));
    }

    #[test]
    fn ignores_rpc_error_envelopes() {
        assert!(rpc_payloads(r#"[["wrb.fr",null,null,null,null,[13]]]"#).is_empty());
    }
}
