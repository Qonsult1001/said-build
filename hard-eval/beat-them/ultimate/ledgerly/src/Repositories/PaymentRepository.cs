using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Ledgerly.Models;

namespace Ledgerly.Repositories;

/// <summary>In-memory persistence for <see cref="Payment"/> records.</summary>
public sealed class PaymentRepository
{
    private readonly ConcurrentDictionary<Guid, Payment> _store = new();

    public Payment Add(Payment entity)
    {
        _store[entity.Id] = entity;
        return entity;
    }

    public IReadOnlyList<Payment> List() =>
        _store.Values.OrderBy(x => x.CreatedAt).ToList();

    public Payment? Get(Guid id) =>
        _store.TryGetValue(id, out var entity) ? entity : null;
}
