// Use-case contract for recording a lead.
public interface ICreateLeadService
{
    Task<Result<CreateLeadResponse>> Create(CreateLeadCommand command, CancellationToken cancellationToken = default);
}
