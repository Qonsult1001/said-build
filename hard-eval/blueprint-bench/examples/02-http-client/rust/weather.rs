// RENDERED from the recalled `lookup<Entity>` (http-client) blueprint (harvested from WeatherClient/GeoClient).
// The [80%] skeleton (build client -> build request -> send -> guard status -> parse body) came from
// .said's recalled blueprint, rendered in rust/reqwest. Only the [20%] URL + parse slots were written.
use reqwest::Client;
use std::time::Duration;

pub struct WeatherClient { api_key: String }

impl WeatherClient {
    pub fn new(api_key: impl Into<String>) -> Self { Self { api_key: api_key.into() } }

    pub async fn get_current(&self, city: &str) -> Result<WeatherResult, ProviderError> {
        // [80%] build client            (blueprint: CreateClient, FromSeconds)
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        // [20%] build request (entity-specific URL)
        let url = format!("https://api.example.com/weather?city={}&key={}",
            urlencoding::encode(city), self.api_key);
        // [80%] send                    (blueprint: HttpRequestMessage, SendAsync)
        let resp = client.get(&url).header("Accept", "application/json").send().await?;
        // [80%] guard status            (blueprint: ProviderError)
        if !resp.status().is_success() {
            return Err(ProviderError(format!("weather api {}", resp.status())));
        }
        // [80%] parse body              (blueprint: TryGetProperty, GetString)
        let body: serde_json::Value = resp.json().await?;
        // [20%] map result (entity-specific)
        Ok(WeatherResult {
            temp_c: body["temp_c"].as_f64().unwrap_or(0.0),
            condition: body["condition"].as_str().unwrap_or("").to_string(),
        })
    }
}

pub struct WeatherResult { pub temp_c: f64, pub condition: String }
pub struct ProviderError(pub String);
impl From<reqwest::Error> for ProviderError { fn from(e: reqwest::Error) -> Self { ProviderError(e.to_string()) } }
