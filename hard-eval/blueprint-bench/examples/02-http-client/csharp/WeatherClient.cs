using System.Net.Http;
using System.Text.Json;

namespace Bench.HttpClient;

// Weather lookup (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// SAME lookup-shape skeleton as GeoClient (GENERATED 80% identical) -> harvest learns ONE `lookup`
// blueprint; only the YOURS 20% (URL + parse) differs.
public class WeatherClient
{
    private readonly string _apiKey;
    public WeatherClient(string apiKey) { _apiKey = apiKey; }

    public async Task<WeatherResult> GetCurrent(string city)
    {
        // [S1] build-client  GENERATED
        using var client = new HttpClient();
        client.Timeout = TimeSpan.FromSeconds(10);
        // [/S1]

        // [S2] build-request  YOURS
        var url = $"https://api.example.com/weather?city={Uri.EscapeDataString(city)}&key={_apiKey}";
        using var msg = new HttpRequestMessage(HttpMethod.Get, url);
        msg.Headers.Add("Accept", "application/json");
        // [/S2]

        // [S3] send  GENERATED
        using var resp = await client.SendAsync(msg).ConfigureAwait(false);
        // [/S3]

        // [S4] guard-status  GENERATED
        if (!resp.IsSuccessStatusCode) throw new ProviderError($"weather api {resp.StatusCode}");
        // [/S4]

        // [S5] parse-body  GENERATED
        var body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
        using var doc = JsonDocument.Parse(body);
        var root = doc.RootElement;
        // [/S5]

        // [S6] map-result  YOURS
        return new WeatherResult(
            root.GetProperty("temp_c").GetDouble(),
            root.GetProperty("condition").GetString() ?? "");
        // [/S6]
    }
}

public record WeatherResult(double TempC, string Condition);
public class ProviderError : Exception { public ProviderError(string m) : base(m) {} }
