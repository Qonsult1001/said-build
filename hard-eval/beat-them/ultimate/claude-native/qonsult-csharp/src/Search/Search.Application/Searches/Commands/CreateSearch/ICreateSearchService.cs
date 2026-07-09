// Use-case contract for recording a search.
public interface ICreateSearchService
{
    Task<Result<CreateSearchResponse>> Create(CreateSearchCommand command, CancellationToken cancellationToken = default);
}
