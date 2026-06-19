using System;
namespace Lib;
// DDD value object: immutable, self-validating, value equality.
public sealed record Email {
    public string Value { get; }
    public Email(string value){
        if (string.IsNullOrWhiteSpace(value) || !value.Contains('@'))
            throw new ArgumentException("invalid email", nameof(value));
        Value = value.Trim().ToLowerInvariant();
    }
}
