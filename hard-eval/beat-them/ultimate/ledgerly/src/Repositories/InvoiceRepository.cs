using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Ledgerly.Models;

namespace Ledgerly.Repositories;

/// <summary>In-memory persistence for <see cref="Invoice"/> records.</summary>
public sealed class InvoiceRepository
{
    private readonly ConcurrentDictionary<Guid, Invoice> _store = new();

    public Invoice Add(Invoice entity)
    {
        _store[entity.Id] = entity;
        return entity;
    }

    public IReadOnlyList<Invoice> List() =>
        _store.Values.OrderBy(x => x.CreatedAt).ToList();

    public Invoice? Get(Guid id) =>
        _store.TryGetValue(id, out var entity) ? entity : null;
}
