using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Ledgerly.Models;

namespace Ledgerly.Repositories;

/// <summary>In-memory persistence for <see cref="Customer"/> records.</summary>
public sealed class CustomerRepository
{
    private readonly ConcurrentDictionary<Guid, Customer> _store = new();

    public Customer Add(Customer entity)
    {
        _store[entity.Id] = entity;
        return entity;
    }

    public IReadOnlyList<Customer> List() =>
        _store.Values.OrderBy(x => x.CreatedAt).ToList();

    public Customer? Get(Guid id) =>
        _store.TryGetValue(id, out var entity) ? entity : null;
}
