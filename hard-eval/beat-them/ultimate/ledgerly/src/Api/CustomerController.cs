using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Services;

namespace Ledgerly.Api;

/// <summary>REST-style handler for <see cref="Customer"/> resources.</summary>
public sealed class CustomerController
{
    private readonly CustomerService _service;

    public CustomerController(CustomerService service) => _service = service;

    /// <summary>POST /customers</summary>
    public Customer Create(CreateCustomerRequest request) => _service.Create(request);

    /// <summary>GET /customers</summary>
    public IReadOnlyList<Customer> List() => _service.List();

    /// <summary>GET /customers/{id}</summary>
    public Customer? Get(Guid id) => _service.Get(id);
}
