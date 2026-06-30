using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Repositories;

namespace Ledgerly.Services;

/// <summary>
/// Wires validation -> repository -> response for invoices.
/// </summary>
public sealed class InvoiceService
{
    private readonly InvoiceRepository _repository;

    public InvoiceService(InvoiceRepository repository) => _repository = repository;

    /// <summary>Create: validate the request, persist to repository, return the response.</summary>
    public Invoice Create(CreateInvoiceRequest request)
    {
        // validate the request
        if (string.IsNullOrWhiteSpace(request.CustomerName))
            throw new ArgumentException("CustomerName is required.", nameof(request));
        if (request.Amount <= 0)
            throw new ArgumentException("Amount must be positive.", nameof(request));

        // persist to repository
        var entity = new Invoice
        {
            Id = Guid.NewGuid(),
            CustomerName = request.CustomerName.Trim(),
            Amount = request.Amount,
            DueDate = request.DueDate,
            CreatedAt = DateTime.UtcNow,
        };
        _repository.Add(entity);

        // return the response
        return entity;
    }

    public IReadOnlyList<Invoice> List() => _repository.List();

    public Invoice? Get(Guid id) => _repository.Get(id);
}
