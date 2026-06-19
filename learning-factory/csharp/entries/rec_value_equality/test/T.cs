using Xunit; using Lib;
public class RecT {
  [Fact] public void ValueEquality(){ Assert.Equal(new Money(5m,"USD"), new Money(5m,"USD")); }
  [Fact] public void NotEqualDifferentField(){ Assert.NotEqual(new Money(5m,"USD"), new Money(6m,"USD")); }
  [Fact] public void WithCopiesAndChanges(){ var a=new Money(5m,"USD"); var b=a.WithAmount(9m); Assert.Equal(9m,b.Amount); Assert.Equal("USD",b.Currency); Assert.Equal(5m,a.Amount); }
}
