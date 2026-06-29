using System.Net.Http;
using System.Text.Json;

namespace Bench.HttpClient;

// HTTP-client-to-an-external-API example, entity = Weather. The "call" shape: build client -> build
// request -> send -> guard status -> parse body -> map result. The SAME shape appears in GeoClient
// (support>=2) so harvest learns ONE "call" blueprint; only the URL/parse slots differ.
public class WeatherClient
{
    private readonly string _apiKey;
    public WeatherClient(string apiKey) { _apiKey = apiKey; }

    public async Task<WeatherResult> GetCurrent(string city)
    {
        // [80%] build client
        using var client = new HttpClient();
        client.Timeout = TimeSpan.FromSeconds(10);
        // [80%] build request
        var url = $"https://api.example.com/weather?city={Uri.EscapeDataString(city)}&key={_apiKey}";
        using var msg = new HttpRequestMessage(HttpMethod.Get, url);
        msg.Headers.Add("Accept", "application/json");
        // [80%] send
        using var resp = await client.SendAsync(msg).ConfigureAwait(false);
        // [80%] guard status
        if (!resp.IsSuccessStatusCode) throw new ProviderError($"weather api {resp.StatusCode}");
        // [80%] parse body
        var body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
        using var doc = JsonDocument.Parse(body);
        var root = doc.RootElement;
        // [20%] map result (entity-specific)
        return new WeatherResult(
            root.GetProperty("temp_c").GetDouble(),
            root.GetProperty("condition").GetString() ?? "");
    }
}

public record WeatherResult(double TempC, string Condition);
public class ProviderError : Exception { public ProviderError(string m) : base(m) {} }
