using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Services;

namespace Ledgerly.Api;

/// <summary>REST-style handler for <see cref="Payment"/> resources.</summary>
public sealed class PaymentController
{
    private readonly PaymentService _service;

    public PaymentController(PaymentService service) => _service = service;

    /// <summary>POST /payments</summary>
    public Payment Create(CreatePaymentRequest request) => _service.Create(request);

    /// <summary>GET /payments</summary>
    public IReadOnlyList<Payment> List() => _service.List();

    /// <summary>GET /payments/{id}</summary>
    public Payment? Get(Guid id) => _service.Get(id);
}
