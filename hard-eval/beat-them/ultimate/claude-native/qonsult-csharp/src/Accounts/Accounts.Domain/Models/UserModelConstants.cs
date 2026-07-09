// Validation limits for the User aggregate, shared by the aggregate invariants and the FluentValidation
// validators in the Application layer.
public static class UserModelConstants
{
    public const int UsernameMaxLength = 150;

    public const int UsernameMinLength = 1;

    public const int PasswordMinLength = 1;

    public const int EmailMaxLength = 254;
}
