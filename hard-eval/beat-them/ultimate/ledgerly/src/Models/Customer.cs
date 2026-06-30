using System;

namespace Ledgerly.Models;

/// <summary>
/// A customer who can be billed via invoices.
/// </summary>
public sealed class Customer
{
    public Guid Id { get; init; }
    public string Name { get; init; } = string.Empty;
    public string Email { get; init; } = string.Empty;
    public DateTime CreatedAt { get; init; }
}

/// <summary>Request payload for creating a <see cref="Customer"/>.</summary>
public sealed record CreateCustomerRequest(string Name, string Email);
