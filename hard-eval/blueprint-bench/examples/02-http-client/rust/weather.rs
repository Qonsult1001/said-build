// Weather lookup (rust/reqwest).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// Rendered cross-language from the recalled `lookup` (http-client) blueprint harvested from C#; the
// GENERATED sections ARE the reused 80% skeleton, the YOURS sections are the entity-specific 20% (URL + parse).
use reqwest::Client;
use std::time::Duration;

pub struct WeatherClient { api_key: String }

impl WeatherClient {
    pub fn new(api_key: impl Into<String>) -> Self { Self { api_key: api_key.into() } }

    pub async fn get_current(&self, city: &str) -> Result<WeatherResult, ProviderError> {
        // [S1] build-client  GENERATED
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        // [/S1]

        // [S2] build-request  YOURS
        let url = format!("https://api.example.com/weather?city={}&key={}",
            urlencoding::encode(city), self.api_key);
        // [/S2]

        // [S3] send  GENERATED
        let resp = client.get(&url).header("Accept", "application/json").send().await?;
        // [/S3]

        // [S4] guard-status  GENERATED
        if !resp.status().is_success() {
            return Err(ProviderError(format!("weather api {}", resp.status())));
        }
        // [/S4]

        // [S5] parse-body  GENERATED
        let body: serde_json::Value = resp.json().await?;
        // [/S5]

        // [S6] map-result  YOURS
        Ok(WeatherResult {
            temp_c: body["temp_c"].as_f64().unwrap_or(0.0),
            condition: body["condition"].as_str().unwrap_or("").to_string(),
        })
        // [/S6]
    }
}

pub struct WeatherResult { pub temp_c: f64, pub condition: String }
pub struct ProviderError(pub String);
impl From<reqwest::Error> for ProviderError { fn from(e: reqwest::Error) -> Self { ProviderError(e.to_string()) } }
