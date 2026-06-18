# C# Expert Pack — Skill Taxonomy (`csharp.said`)

**Goal:** make `csharp.said` a comprehensive, verified C# skill pack so a cheap model
(gpt-oss-20b) writes **expert-level, correctly-architected C#** — competing with Claude on
C#, sold as a per-language pack. Every skill is a **verified learning**: the correct idiom +
the non-obvious gotcha, gate-checked by `dotnet test` (real toolchain, no install).

**Source:** context7 → Microsoft Learn (`/websites/learn_microsoft_en-us_dotnet_csharp`
~14,727 snippets, `/dotnet/aspnetcore.docs` ~30k, `/dotnet/efcore`). The doc supplies the
behavior; Claude distills the standard + authors the gate; dotnet verifies.

**Pipeline per skill (proven):** context7 fetch → Claude curates {problem, learnings (idiom
+ gotcha), errors-to-avoid, reference} + a dotnet xunit gate that encodes the rule → verify
reference passes → store (dedup-keyed, `src:context7 lang:csharp area:<area>` tag) → A/B vs
cold 20b (keep only skills that genuinely lift, i.e. the model's edge — famous basics it
already knows add nothing).

**Status:** mechanism proven (2 skills in pack: gate_policy clean-arch 0/3→2/3; STJ). This
doc is the blueprint to scale to full coverage.

---

## Curriculum (areas → skills)

### 1. Language fundamentals & types
- value vs reference semantics; `struct` vs `class` choice; `readonly struct`
- `record` / `record struct` — value equality, `with` expressions, init-only
- nullable reference types — `?`, `!`, flow analysis, `[NotNull]`/`[MaybeNull]`
- pattern matching — `switch` expressions, relational/logical/list patterns, `is`
- generics — constraints, variance (`in`/`out`), generic math (`INumber<T>`)
- enums — `[Flags]`, parsing, `JsonStringEnumConverter` (have)
- tuples & deconstruction; `params`, optional/named args
- string handling — interpolation, `StringBuilder`, `string.Create`, raw strings

### 2. Async & concurrency
- `async`/`await` correctness — no `async void`, `ConfigureAwait`, no `.Result` deadlock
- `Task` vs `ValueTask`; `Task.WhenAll`/`WhenAny`
- `IAsyncEnumerable` + `await foreach` + `[EnumeratorCancellation]`
- `CancellationToken` threading; cooperative cancellation
- `Channel<T>` producer/consumer; `SemaphoreSlim` throttling
- thread-safety — `Interlocked`, `lock`, `ConcurrentDictionary`

### 3. LINQ & collections
- deferred vs immediate execution (the classic gotcha)
- `Aggregate`, `GroupBy`, `ToLookup`, `SelectMany`, `Zip`
- `IEnumerable` vs `IQueryable` (client vs server eval)
- collection expressions `[..]`; `Span<T>`/`Memory<T>` for zero-alloc

### 4. Clean architecture & design (the "how to structure" tier)
- pure domain functions, no I/O in the core (have: gate_policy)
- dependency inversion — depend on interfaces, DI registration lifetimes
  (Singleton/Scoped/Transient — the captive-dependency gotcha)
- CQRS / handler shape; Result<T> over exceptions for expected failures
- validation placement (domain invariants vs input validation)
- mapping (DTO ↔ domain) without leaking persistence types
- options pattern (`IOptions<T>`), configuration binding

### 5. ASP.NET Core
- minimal API endpoints; `Results.*`; route params; model binding
- middleware order; the exact `AllowAnonymous`/auth pipeline
- DI lifetimes in controllers; `IHttpClientFactory` (not `new HttpClient`)
- exception handling middleware; ProblemDetails

### 6. EF Core
- N+1 avoidance — `Include`/projection; split queries
- `AsNoTracking` for reads; change tracking semantics
- transactions, concurrency tokens, migrations basics
- value converters; owned types

### 7. Serialization & data
- System.Text.Json — camelCase + string enums (have); custom converters; source-gen
- exact-shape contracts; polymorphic serialization

### 8. Performance & memory
- `Span<T>`/`stackalloc`; pooling (`ArrayPool`); avoid LINQ in hot paths
- struct enumerators; `in` parameters; `ref struct`

### 9. Error handling & reliability
- exceptions vs Result; retry/backoff; idempotency
- `IDisposable`/`IAsyncDisposable`, `using` scopes, finalizer pitfalls

### 10. Testing idioms
- xunit `[Theory]`/`[InlineData]`; arrange-act-assert; testable seams

---

## How a skill qualifies for the pack

1. **Verifiable** — expressible as a dotnet xunit gate from the doc's stated behavior.
2. **At the model's edge** — A/B shows cold-fail (or unreliable) → warm-green. Skip
   anything the 20b already does cold (no lift = noise; measured: it knows CRC32, STJ,
   graphemes, basic generics).
3. **Non-obvious** — the learning captures the gotcha a naive attempt gets wrong
   (deferred-LINQ, captive-dependency, `async void`, N+1, precedence rules, etc.).

The pack's value = the union of the model's C# blind spots, each closed by a verified
standard. That is what makes the cheap model a C# expert.
