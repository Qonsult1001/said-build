using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Services;

namespace Ledgerly.Api;

/// <summary>REST-style handler for <see cref="Invoice"/> resources.</summary>
public sealed class InvoiceController
{
    private readonly InvoiceService _service;

    public InvoiceController(InvoiceService service) => _service = service;

    /// <summary>POST /invoices</summary>
    public Invoice Create(CreateInvoiceRequest request) => _service.Create(request);

    /// <summary>GET /invoices</summary>
    public IReadOnlyList<Invoice> List() => _service.List();

    /// <summary>GET /invoices/{id}</summary>
    public Invoice? Get(Guid id) => _service.Get(id);
}
