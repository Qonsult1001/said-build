using System.Net.Http;
using System.Text.Json;

namespace Bench.HttpClient;

// Geo lookup (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// SAME lookup-shape skeleton as WeatherClient (GENERATED 80% identical); only the YOURS 20% differs.
public class GeoClient
{
    private readonly string _apiKey;
    public GeoClient(string apiKey) { _apiKey = apiKey; }

    public async Task<GeoResult> Lookup(string place)
    {
        // [S1] build-client  GENERATED
        using var client = new HttpClient();
        client.Timeout = TimeSpan.FromSeconds(10);
        // [/S1]

        // [S2] build-request  YOURS
        var url = $"https://api.example.com/geo?q={Uri.EscapeDataString(place)}&key={_apiKey}";
        using var msg = new HttpRequestMessage(HttpMethod.Get, url);
        msg.Headers.Add("Accept", "application/json");
        // [/S2]

        // [S3] send  GENERATED
        using var resp = await client.SendAsync(msg).ConfigureAwait(false);
        // [/S3]

        // [S4] guard-status  GENERATED
        if (!resp.IsSuccessStatusCode) throw new ProviderError($"geo api {resp.StatusCode}");
        // [/S4]

        // [S5] parse-body  GENERATED
        var body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
        using var doc = JsonDocument.Parse(body);
        var root = doc.RootElement;
        // [/S5]

        // [S6] map-result  YOURS
        return new GeoResult(
            root.GetProperty("lat").GetDouble(),
            root.GetProperty("lon").GetDouble());
        // [/S6]
    }
}

public record GeoResult(double Lat, double Lon);
