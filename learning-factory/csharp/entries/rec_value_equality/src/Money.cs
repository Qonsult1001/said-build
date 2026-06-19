namespace Lib;
public record Money(decimal Amount, string Currency)
{
    public Money WithAmount(decimal amount) => this with { Amount = amount };
}
