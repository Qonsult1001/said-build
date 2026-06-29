using System.Net.Http;
using System.Text.Json;

namespace Bench.HttpClient;

// SAME call shape as WeatherClient (80% skeleton identical); only the URL + parse slots differ.
// Two occurrences => harvest learns ONE "call/get external API" blueprint.
public class GeoClient
{
    private readonly string _apiKey;
    public GeoClient(string apiKey) { _apiKey = apiKey; }

    public async Task<GeoResult> Lookup(string place)
    {
        // [80%] build client
        using var client = new HttpClient();
        client.Timeout = TimeSpan.FromSeconds(10);
        // [80%] build request
        var url = $"https://api.example.com/geo?q={Uri.EscapeDataString(place)}&key={_apiKey}";
        using var msg = new HttpRequestMessage(HttpMethod.Get, url);
        msg.Headers.Add("Accept", "application/json");
        // [80%] send
        using var resp = await client.SendAsync(msg).ConfigureAwait(false);
        // [80%] guard status
        if (!resp.IsSuccessStatusCode) throw new ProviderError($"geo api {resp.StatusCode}");
        // [80%] parse body
        var body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
        using var doc = JsonDocument.Parse(body);
        var root = doc.RootElement;
        // [20%] map result (entity-specific)
        return new GeoResult(
            root.GetProperty("lat").GetDouble(),
            root.GetProperty("lon").GetDouble());
    }
}

public record GeoResult(double Lat, double Lon);
