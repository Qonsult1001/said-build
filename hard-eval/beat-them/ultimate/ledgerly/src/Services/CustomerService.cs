using System;
using System.Collections.Generic;
using Ledgerly.Models;
using Ledgerly.Repositories;

namespace Ledgerly.Services;

/// <summary>
/// Wires validation -> repository -> response for customers.
/// </summary>
public sealed class CustomerService
{
    private readonly CustomerRepository _repository;

    public CustomerService(CustomerRepository repository) => _repository = repository;

    /// <summary>Create: validate the request, persist to repository, return the response.</summary>
    public Customer Create(CreateCustomerRequest request)
    {
        // validate the request
        if (string.IsNullOrWhiteSpace(request.Name))
            throw new ArgumentException("Name is required.", nameof(request));
        if (string.IsNullOrWhiteSpace(request.Email) || !request.Email.Contains('@'))
            throw new ArgumentException("A valid Email is required.", nameof(request));

        // persist to repository
        var entity = new Customer
        {
            Id = Guid.NewGuid(),
            Name = request.Name.Trim(),
            Email = request.Email.Trim(),
            CreatedAt = DateTime.UtcNow,
        };
        _repository.Add(entity);

        // return the response
        return entity;
    }

    public IReadOnlyList<Customer> List() => _repository.List();

    public Customer? Get(Guid id) => _repository.Get(id);
}
