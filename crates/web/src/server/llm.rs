use std::time::Duration;

use chrono::NaiveDate;
use serde::Deserialize;
use thiserror::Error;

use super::form::{FormDefaults, SearchForm};

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("il modello locale non risponde: {0}")]
    Http(#[from] reqwest::Error),
    #[error("risposta del modello non interpretabile: {0}")]
    Invalid(String),
}

/// OpenAI-compatible chat endpoint (Ollama, llama.cpp, LM Studio...) running locally.
pub struct LlmClient {
    client: reqwest::Client,
    url: String,
    model: String,
}

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

const INSTRUCTIONS: &str = r#"Convert the user's flight search request (often in Italian) into JSON for a flight price monitor.
Reply with ONE JSON object and nothing else. Use only these keys, all string values; omit what the user did not say:
name: short title in Italian.
origin_kind / dest_kind: "airports" (IATA codes), "city", "country" (ISO alpha-2 code), "radius" ("lat, lon, km" or "IATA km"); dest_kind may be "anywhere".
origin_value / dest_value: codes comma-separated, city name, country code or radius.
mode: "one_way", "round_trip_coupled" or "round_trip_decoupled" (default for round trips).
date_from, date_to: YYYY-MM-DD outbound window (date_to empty for a fixed date). Resolve relative dates from TODAY.
stay_min, stay_max: nights at destination for round trips.
return_origin_kind/value, return_dest_kind/value: open-jaw only.
max_stops, passengers, checked_bags: integers. min_layover_h, max_layover_h, max_total_h: hours.
excluded_airlines: 2-letter codes comma-separated. excluded_hours: ranges like "00:00-06:00".
max_price, alert_below: amounts. alert_drop: percent. currency: ISO code.
via_kind/via_value, via_min_h/via_max_h: hub for separate tickets or stopovers.
companions: lines "Name: city" for people departing from elsewhere. group_goal: "min_cost" or "align"."#;

impl LlmClient {
    pub fn new(url: &str, model: &str) -> Result<Self, LlmError> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            url: url.to_string(),
            model: model.to_string(),
        })
    }

    pub async fn parse(
        &self,
        text: &str,
        today: NaiveDate,
        defaults: FormDefaults,
    ) -> Result<SearchForm, LlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "temperature": 0,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": format!("{INSTRUCTIONS}\nTODAY: {today}") },
                { "role": "user", "content": text },
            ],
        });
        let completion: Completion = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let content = completion
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| LlmError::Invalid("nessuna scelta".into()))?;
        merge_reply(&content, defaults)
    }
}

/// Lays the model's fields over a blank form, so anything it omits keeps the defaults.
pub fn merge_reply(content: &str, defaults: FormDefaults) -> Result<SearchForm, LlmError> {
    let json = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| LlmError::Invalid(e.to_string()))?;
    let mut base = serde_json::to_value(SearchForm::blank(defaults))
        .map_err(|e| LlmError::Invalid(e.to_string()))?;
    if let Some(fields) = base.as_object_mut() {
        for (key, value) in parsed {
            if !fields.contains_key(&key) {
                continue;
            }
            let text = match value {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => {
                    if b {
                        "on".into()
                    } else {
                        String::new()
                    }
                }
                _ => continue,
            };
            fields.insert(key, serde_json::Value::String(text));
        }
    }
    serde_json::from_value(base).map_err(|e| LlmError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_recorded_model_reply_over_defaults() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/llm/chat_completion.json"
        ))
        .unwrap();
        let completion: Completion = serde_json::from_str(&raw).unwrap();
        let defaults = FormDefaults {
            budget: 40,
            poll_hours: 6,
        };
        let form = merge_reply(&completion.choices[0].message.content, defaults).unwrap();
        assert_eq!(form.dest_kind, "country");
        assert_eq!(form.dest_value, "JP");
        assert_eq!(form.max_stops, "1");
        assert_eq!(form.currency, "EUR", "default kept");
        assert_eq!(form.budget, "40", "default kept");
    }

    #[test]
    fn rejects_non_json_replies() {
        let defaults = FormDefaults {
            budget: 40,
            poll_hours: 6,
        };
        assert!(merge_reply("Certo! Ecco la ricerca.", defaults).is_err());
    }
}
