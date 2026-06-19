# C# Expert Pack — Skill Taxonomy (`csharp.said`)

**Goal:** make `csharp.said` a **comprehensive, verified C# expert pack** so ANY model the
end user plugs in writes expert-level, correctly-architected C# — competing with Claude on
C#, sold as a per-language plugin. It is a GLOBAL plugin, not tuned to one model.

**Source:** context7 → Microsoft Learn (`/websites/learn_microsoft_en-us_dotnet_csharp`
~14.7k snippets, `/dotnet/aspnetcore.docs` ~30k, `/dotnet/efcore`, architecture guides).

## Build rules (corrected 2026-06-19)

1. **Store EVERYTHING — no model-edge filtering.** Curate + dotnet-gate + verify + store
   EVERY C# skill, whether or not any particular model already knows it. A weaker/other
   model the user plugs in may have different gaps; the plugin must cover the whole surface.
2. **Gate-only quality bar (no A/B in the build loop).** A skill qualifies if its reference
   implementation PASSES a `dotnet test` that encodes the documented behavior. The dotnet
   gate is the sole correctness check — no cold/warm model A/B needed to admit a skill.
   (Model A/B is an optional later product metric, never an admission filter.)
3. **Comprehensive.** Cover all C# sections below, not a sample.
4. **Architectures are a tagged GROUP.** Many C# architectures exist; each architecture
   skill carries an `arch:<style>` tag so recall keeps styles DISTINCT (a Hexagonal task
   never pulls a Clean frame). One pack now; splittable into sub-packs later (the pack
   rebuilds from entries) only if sold per-architecture.

**Frame tags:** `src:context7 lang:csharp area:<area>` plus `arch:<style>` for architecture
skills. **Pipeline per skill:** context7 fetch → Claude curates {problem, learnings (idiom +
gotcha), errors-to-avoid, reference} + a dotnet xunit gate → verify reference green → store
(dedup-keyed).

---

## Curriculum

### A. Language & type system
- value vs reference semantics; `struct` vs `class`; `readonly struct`; `ref struct`
- `record` / `record struct` — value equality, `with`, init-only, positional
- nullable reference types — `?`/`!`, flow analysis, `[NotNull]`/`[MaybeNull]`/`[NotNullWhen]`
- pattern matching — `switch` expr, relational/logical/list/property patterns, `is`
- generics — constraints, `in`/`out` variance, generic math (`INumber<T>`), generic methods
- enums — `[Flags]` bitwise, parsing, string-enum serialization
- tuples & deconstruction; `params`, optional/named args; local functions
- properties — init-only, `required`, expression-bodied, indexers
- strings — interpolation, `StringBuilder`, raw/verbatim, `string.Create`, spans over strings
- operators — overloading, implicit/explicit conversions, `IComparable`/`IEquatable`
- delegates, events, lambdas, closures (capture gotchas), `Func`/`Action`
- top-level statements, global usings, file-scoped namespaces, `nameof`, `with` on records

### B. Async & concurrency
- `async`/`await` correctness — never `async void`, `ConfigureAwait`, avoid `.Result`/`.Wait()` deadlock
- `Task` vs `ValueTask`; when each; `Task.WhenAll`/`WhenAny`; `Task.Run` misuse
- `IAsyncEnumerable<T>` + `await foreach` + `[EnumeratorCancellation]`
- `CancellationToken` threading + cooperative cancellation + `OperationCanceledException`
- `Channel<T>` producer/consumer; `SemaphoreSlim` throttling; `Parallel.ForEachAsync`
- thread-safety — `Interlocked`, `lock`, `ConcurrentDictionary`, `Lazy<T>`, immutability
- `IProgress<T>`, `TaskCompletionSource`, async streams cancellation

### C. LINQ & collections
- deferred vs immediate execution (the classic gotcha); multiple-enumeration
- `Aggregate`, `GroupBy`, `ToLookup`, `SelectMany`, `Zip`, `Chunk`, `DistinctBy`
- `IEnumerable` vs `IQueryable` (client vs server eval) — the EF translation boundary
- collection expressions `[..]`, spreads; `Span<T>`/`Memory<T>`/`ReadOnlySpan<T>`
- `Dictionary`/`HashSet` semantics, custom `IEqualityComparer`, `frozen`/`immutable` collections

### D. Architectures (TAGGED GROUP — `arch:<style>`)
Each style gets its own skills (structure, dependency direction, where logic lives, the
canonical gotcha). Cover at minimum:
- `arch:layered` — N-tier; dependency flows downward; no upward refs
- `arch:clean` — entities/use-cases/adapters; dependency rule points inward; pure core (have)
- `arch:onion` — domain at center; infrastructure at edges
- `arch:hexagonal` — ports & adapters; domain depends on port interfaces only
- `arch:vertical-slice` — feature folders; per-feature request/handler; minimal sharing
- `arch:cqrs` — command/query separation; handlers; (optional) separate read model
- `arch:ddd` — aggregates, entities, value objects, domain events, repositories
- `arch:mediator` — MediatR-style request/handler pipeline + behaviors
- `arch:mvc` — controllers/views/models separation; thin controllers
- `arch:mvvm` — (WPF/MAUI) view/viewmodel/binding, `INotifyPropertyChanged`
- `arch:event-driven` — events/handlers, message bus, eventual consistency
- `arch:microservices` — service boundaries, contracts, resilience seams
- cross-cutting: dependency inversion, DI lifetimes (Singleton/Scoped/Transient +
  captive-dependency gotcha), Result<T> vs exceptions, validation placement, DTO↔domain
  mapping (no persistence leakage), options pattern (`IOptions<T>`)

### E. ASP.NET Core
- minimal API endpoints; `Results.*`; route/query/body binding; `[FromServices]`
- middleware ORDER (auth before authz, exception handler first); `AllowAnonymous`
- DI lifetimes in controllers; `IHttpClientFactory` (never `new HttpClient`)
- model validation, ProblemDetails, exception-handling middleware
- filters, `IActionResult`, content negotiation; OpenAPI/Swagger
- auth — JWT, policies/roles, `[Authorize]`; CORS

### F. EF Core / data
- N+1 avoidance — `Include`, projection to DTO, split queries
- `AsNoTracking` for reads; change-tracking semantics; identity resolution
- transactions, optimistic concurrency tokens, migrations
- value converters, owned types, query filters, raw SQL safety (no string concat)

### G. Serialization & interop
- System.Text.Json — camelCase + string enums (have), custom converters, source-gen,
  polymorphic (`[JsonDerivedType]`), naming policies, `JsonSerializerOptions` caching
- exact-shape DTO contracts; `record` DTOs; ignore/required/default handling

### H. Performance & memory
- `Span<T>`/`stackalloc`/`ArrayPool`; avoid LINQ allocs in hot paths
- struct enumerators, `in`/`ref readonly` params, `ref struct`
- string allocation avoidance; `StringPool`; benchmarking mindset (BenchmarkDotNet)

### I. Error handling, reliability & resources
- exceptions vs Result<T> for expected failures; exception filters; custom exceptions
- `IDisposable`/`IAsyncDisposable`, `using`/`await using`, finalizer pitfalls, `SafeHandle`
- retry/backoff, idempotency, timeouts, circuit breaker (Polly seam)

### J. Testing
- xunit `[Fact]`/`[Theory]`/`[InlineData]`/`[MemberData]`; arrange-act-assert
- test seams (interfaces, fakes); `IClassFixture`; integration tests with `WebApplicationFactory`

### K. Tooling & modern C#
- `nullable enable`, analyzers, `EditorConfig`, warnings-as-errors mindset
- newest language features per version (primary constructors, collection expressions,
  `field` keyword, etc.) — versioned via the source doc

---

## Admission test (per skill)

A skill enters `csharp.said` iff: a `dotnet test` xunit gate encodes the documented
behavior AND the curated reference implementation makes it pass. That's the whole bar —
verified, model-agnostic, comprehensive. (Cold/warm model lift is a product metric we may
report later; it is NOT an admission filter.)
