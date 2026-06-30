using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Repositories;

namespace Ledgerly.Services;

/// <summary>
/// Wires validation -> repository -> response for payments.
/// </summary>
public sealed class PaymentService
{
    private readonly PaymentRepository _repository;

    public PaymentService(PaymentRepository repository) => _repository = repository;

    /// <summary>Create: validate the request, persist to repository, return the response.</summary>
    public Payment Create(CreatePaymentRequest request)
    {
        // validate the request
        if (request.InvoiceId == Guid.Empty)
            throw new ArgumentException("InvoiceId is required.", nameof(request));
        if (request.Amount <= 0)
            throw new ArgumentException("Amount must be positive.", nameof(request));
        if (string.IsNullOrWhiteSpace(request.Method))
            throw new ArgumentException("Method is required.", nameof(request));

        // persist to repository
        var entity = new Payment
        {
            Id = Guid.NewGuid(),
            InvoiceId = request.InvoiceId,
            Amount = request.Amount,
            Method = request.Method.Trim(),
            CreatedAt = DateTime.UtcNow,
        };
        _repository.Add(entity);

        // return the response
        return entity;
    }

    public IReadOnlyList<Payment> List() => _repository.List();

    public Payment? Get(Guid id) => _repository.Get(id);
}
