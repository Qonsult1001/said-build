using Xunit; using Lib;
public class VsT {
  [Fact] public void HandlerProducesResult(){ var r=new CreateOrderHandler().Handle(new CreateOrder("A",3), 10m); Assert.Equal(30m, r.Total); Assert.Equal("A", r.Sku); }
}
