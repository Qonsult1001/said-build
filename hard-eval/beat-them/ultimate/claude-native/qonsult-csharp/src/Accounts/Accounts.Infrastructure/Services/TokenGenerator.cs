using System.Security.Cryptography;

// Issues an opaque random auth token (matching the frontend's `Token {auth_token}` scheme). Token
// generation is an infrastructure concern.
public class TokenGenerator : ITokenGenerator
{
    public string Generate(User user)
        => Convert.ToHexString(RandomNumberGenerator.GetBytes(20)).ToLowerInvariant();
}
