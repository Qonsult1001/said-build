namespace Lib;
// Vertical slice: ONE feature = its own request + handler + result, self-contained.
public record CreateOrder(string Sku, int Qty);
public record OrderCreated(string Sku, int Qty, decimal Total);
public class CreateOrderHandler {
    public OrderCreated Handle(CreateOrder cmd, decimal unitPrice)
        => new(cmd.Sku, cmd.Qty, cmd.Qty * unitPrice);
}
