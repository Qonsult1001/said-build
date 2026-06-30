using System;

namespace Ledgerly.Models;

/// <summary>
/// An invoice issued to a customer for an amount due by a date.
/// </summary>
public sealed class Invoice
{
    public Guid Id { get; init; }
    public string CustomerName { get; init; } = string.Empty;
    public decimal Amount { get; init; }
    public DateTime DueDate { get; init; }
    public DateTime CreatedAt { get; init; }
}

/// <summary>Request payload for creating an <see cref="Invoice"/>.</summary>
public sealed record CreateInvoiceRequest(string CustomerName, decimal Amount, DateTime DueDate);
