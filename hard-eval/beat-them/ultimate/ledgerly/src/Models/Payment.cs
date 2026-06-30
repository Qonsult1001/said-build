using System;

namespace Ledgerly.Models;

/// <summary>
/// A payment applied against an invoice.
/// </summary>
public sealed class Payment
{
    public Guid Id { get; init; }
    public Guid InvoiceId { get; init; }
    public decimal Amount { get; init; }
    public string Method { get; init; } = string.Empty;
    public DateTime CreatedAt { get; init; }
}

/// <summary>Request payload for creating a <see cref="Payment"/>.</summary>
public sealed record CreatePaymentRequest(Guid InvoiceId, decimal Amount, string Method);
