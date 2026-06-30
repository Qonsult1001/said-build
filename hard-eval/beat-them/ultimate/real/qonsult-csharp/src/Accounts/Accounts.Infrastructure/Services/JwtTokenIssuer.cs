// Accounts.Infrastructure — JWT token issuer + BCrypt-style password hasher (impl of App ports).

using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using Microsoft.IdentityModel.Tokens;

public class JwtTokenIssuer(JwtSettings settings) : ITokenIssuer
{
    public string IssueToken(Guid accountId, string email)
    {
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(settings.SigningKey));
        var credentials = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);

        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, accountId.ToString()),
            new Claim(JwtRegisteredClaimNames.Email, email),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString())
        };

        var token = new JwtSecurityToken(
            issuer: settings.Issuer,
            audience: settings.Audience,
            claims: claims,
            expires: DateTime.UtcNow.AddMinutes(settings.ExpiryMinutes),
            signingCredentials: credentials);

        return new JwtSecurityTokenHandler().WriteToken(token);
    }
}

public class JwtSettings
{
    public string SigningKey { get; set; } = default!;
    public string Issuer { get; set; } = "qonsult";
    public string Audience { get; set; } = "qonsult-clients";
    public int ExpiryMinutes { get; set; } = 60;
}
