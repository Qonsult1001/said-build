// Use-case contract for authenticating a user and issuing an auth token.
public interface ILoginService
{
    Task<Result<LoginResponse>> Login(LoginCommand command, CancellationToken cancellationToken = default);
}
